//! Native JPEG decoding via libjpeg-turbo, with jpeg-decoder for lossless precision.
//! Decoded values retain JPEG's lossy provenance; EXIF orientation, ICC and
//! density metadata are retained without applying display transforms.
pub use crate::raster::{Error, FrameBuffer, Limits};
use crate::{PixelBuffer, PixelType, source::Snapshot};
use jpeg_backend::{Decoder, PixelFormat};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};
#[derive(Clone, Debug)]
pub struct Segment {
    pub marker: u8,
    pub payload: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub channels: usize,
    pub pixel_type: PixelType,
    pub precision: u8,
    pub sof_marker: u8,
    pub lossy: bool,
    /// Original low-bit point transform for every lossless SOS, in source order.
    /// Empty for DCT JPEG; a nonzero transform discards source low bits.
    pub lossless_point_transforms: Vec<u8>,
    pub encoded_bytes: u64,
    /// Original marker segments before the first scan, including JFIF/Adobe,
    /// EXIF, ICC, comments and quantization/Huffman tables.
    pub segments: Vec<Segment>,
    pub decoded_color_space: String,
}
impl Metadata {
    #[must_use]
    pub fn sample_shape(&self) -> Vec<usize> {
        if self.channels == 1 {
            self.shape.to_vec()
        } else {
            vec![self.shape[0], self.shape[1], self.channels]
        }
    }
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
    limits: Limits,
}
impl Reader {
    /// Open one immutable JPEG with explicit encoded/decoded/metadata bounds.
    /// # Errors
    /// Invalid marker structure, unsupported coding/precision, bounds or source changes.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let encoded = file.metadata()?.len();
        if encoded > limits.encoded_bytes as u64 {
            return Err(Error("JPEG encoded file exceeds limit".into()));
        }
        if encoded < 4 {
            return Err(Error("truncated JPEG".into()));
        }
        file.seek(SeekFrom::End(-2))?;
        let mut eoi = [0; 2];
        file.read_exact(&mut eoi)?;
        if eoi != [0xff, 0xd9] {
            return Err(Error(
                "JPEG requires terminal EOI; trailing bytes unsupported".into(),
            ));
        }
        let mut source = BufReader::new(File::open(&snapshot.path)?).take(encoded);
        let (segments, precision, sof) = header(&mut source, limits.metadata_bytes)?;
        if precision < 8 {
            return Err(Error(
                "JPEG source precision below 8 bits is unsupported".into(),
            ));
        }
        let frame = &segments
            .iter()
            .find(|segment| segment.marker == sof)
            .expect("validated SOF")
            .payload;
        // jpeg-decoder's info() assumes one, three or four components. Reject
        // all other legal/invalid counts before entering that backend API.
        if ![1, 3, 4].contains(&frame[5]) || frame.len() != 6 + 3 * usize::from(frame[5]) {
            return Err(Error("unsupported JPEG frame component table".into()));
        }
        let lossless = matches!(sof, 0xc3 | 0xc7 | 0xcb | 0xcf);
        let point_transforms = if lossless {
            let scan = &segments.last().expect("validated SOS").payload;
            lossless_scans(&mut source, frame, scan, precision)?
        } else {
            dct_end(&mut source, limits.metadata_bytes)?;
            Vec::new()
        };
        let mut decoder = Decoder::new(BufReader::new(File::open(&snapshot.path)?));
        decoder.set_max_decoding_buffer_size(limits.decoder_bytes);
        decoder.read_info().map_err(|e| Error(e.to_string()))?;
        let info = decoder
            .info()
            .ok_or_else(|| Error("JPEG lacks image information".into()))?;
        let (channels, pixel_type, color) = match info.pixel_format {
            PixelFormat::L8 => (1, PixelType::U8, "Gray"),
            PixelFormat::L16 => (1, PixelType::U16, "Gray"),
            PixelFormat::RGB24 => (3, PixelType::U8, "RGB"),
            PixelFormat::CMYK32 => (4, PixelType::U8, "CMYK"),
        };
        if precision > 8 && (channels != 1 || !matches!(sof, 0xc3 | 0xc7 | 0xcb | 0xcf)) {
            return Err(Error(
                "JPEG precision above 8 bits requires single-channel lossless coding".into(),
            ));
        }
        let shape = [usize::from(info.height), usize::from(info.width)];
        limits.samples(shape, channels, pixel_type.bytes())?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            metadata: Metadata {
                shape,
                channels,
                pixel_type,
                precision,
                sof_marker: sof,
                lossy: !lossless || point_transforms.iter().any(|&point| point != 0),
                lossless_point_transforms: point_transforms,
                encoded_bytes: encoded,
                segments,
                decoded_color_space: color.into(),
            },
            limits,
        })
    }
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        1
    }
    /// Decode original stored orientation to native channel samples.
    /// # Errors
    /// Changed source, corrupt/truncated stream, backend or output-size failures.
    pub fn read_frame_into(&self, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        let file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        let mut decoder = Decoder::new(BufReader::new(file));
        decoder.set_max_decoding_buffer_size(self.limits.decoder_bytes);
        let bytes = if self.metadata.precision == 8 {
            let expected = self.metadata.shape[0] * self.metadata.shape[1] * self.metadata.channels;
            if expected > self.limits.decoder_bytes {
                return Err(Error("JPEG decoded payload exceeds decoder limit".into()));
            }
            let mut encoded = Vec::with_capacity(self.metadata.encoded_bytes as usize);
            let mut file = File::open(&self.snapshot.path)?;
            self.snapshot.check_handle(&file).map_err(Error)?;
            file.by_ref()
                .take((self.limits.encoded_bytes as u64).saturating_add(1))
                .read_to_end(&mut encoded)?;
            if encoded.len() as u64 != self.metadata.encoded_bytes {
                return Err(Error("JPEG encoded size changed".into()));
            }
            let mut turbo =
                turbojpeg_backend::Decompressor::new().map_err(|e| Error(e.to_string()))?;
            turbo
                .set_scan_limit(1000)
                .map_err(|e| Error(e.to_string()))?;
            let format = match self.metadata.channels {
                1 => turbojpeg_backend::PixelFormat::GRAY,
                3 => turbojpeg_backend::PixelFormat::RGB,
                4 => turbojpeg_backend::PixelFormat::CMYK,
                _ => return Err(Error("unsupported JPEG channel count".into())),
            };
            let mut pixels = vec![0; expected];
            turbo
                .decompress(
                    &encoded,
                    turbojpeg_backend::Image {
                        pixels: pixels.as_mut_slice(),
                        width: self.metadata.shape[1],
                        pitch: self.metadata.shape[1] * self.metadata.channels,
                        height: self.metadata.shape[0],
                        format,
                    },
                )
                .map_err(|e| Error(e.to_string()))?;
            pixels
        } else {
            decoder.decode().map_err(|e| Error(e.to_string()))?
        };
        let expected = self.metadata.shape[0]
            * self.metadata.shape[1]
            * self.metadata.channels
            * self.metadata.pixel_type.bytes();
        if bytes.len() != expected {
            return Err(Error("JPEG decoded sample count mismatch".into()));
        }
        buffer.pixels = if self.metadata.pixel_type == PixelType::U16 {
            PixelBuffer::U16(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_ne_bytes([b[0], b[1]]))
                    .collect(),
            )
        } else {
            PixelBuffer::U8(bytes)
        };
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        buffer.shape = Some(self.metadata.shape);
        buffer.channels = self.metadata.channels;
        Ok(())
    }
}
fn header(input: &mut impl Read, limit: usize) -> Result<(Vec<Segment>, u8, u8), Error> {
    let mut magic = [0; 2];
    input.read_exact(&mut magic)?;
    if magic != [0xff, 0xd8] {
        return Err(Error("invalid JPEG SOI marker".into()));
    }
    let mut segments = Vec::new();
    let mut bytes = 2usize;
    let mut coding = None;
    loop {
        let mut marker = [0; 2];
        input.read_exact(&mut marker)?;
        if marker[0] != 0xff {
            return Err(Error("invalid JPEG marker boundary".into()));
        }
        while marker[1] == 0xff {
            input.read_exact(&mut marker[1..])?;
            bytes = bytes.saturating_add(1);
            if bytes > limit {
                return Err(Error("JPEG header exceeds metadata limit".into()));
            }
        }
        if [0, 0xd8, 0xd9, 0x01].contains(&marker[1]) || (0xd0..=0xd7).contains(&marker[1]) {
            return Err(Error("unexpected JPEG marker before scan".into()));
        }
        let mut size = [0; 2];
        input.read_exact(&mut size)?;
        let size = usize::from(u16::from_be_bytes(size));
        if size < 2 {
            return Err(Error("invalid JPEG segment length".into()));
        }
        bytes = bytes
            .checked_add(size + 66)
            .filter(|&n| n <= limit)
            .ok_or_else(|| Error("JPEG header exceeds metadata limit".into()))?;
        let mut payload = vec![0; size - 2];
        input.read_exact(&mut payload)?;
        if matches!(marker[1],0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
            if payload.len() < 6 || coding.is_some() {
                return Err(Error("invalid/duplicate JPEG frame header".into()));
            }
            coding = Some((payload[0], marker[1]));
        }
        let scan = marker[1] == 0xda;
        segments.push(Segment {
            marker: marker[1],
            payload,
        });
        if scan {
            break;
        }
    }
    let (precision, sof) = coding.ok_or_else(|| Error("JPEG has no frame header".into()))?;
    Ok((segments, precision, sof))
}

/// Shared bounded DCT envelope inspection for TIFF's original JPEG containers.
/// Native codecs still own entropy decoding and reject invalid coding parameters.
#[cfg(feature = "tiff")]
pub(crate) fn inspect_dct_bytes(data: &[u8], limit: usize) -> Result<(Vec<Segment>, usize), Error> {
    let mut input = std::io::Cursor::new(data);
    let (segments, precision, sof) = header(&mut input, limit)?;
    if precision != 8 || !matches!(sof, 0xc0..=0xc2) {
        return Err(Error(
            "TIFF JPEG components require 8-bit DCT coding".into(),
        ));
    }
    let entropy = usize::try_from(input.position())
        .map_err(|_| Error("JPEG header position overflow".into()))?;
    dct_end(&mut input, limit)?;
    Ok((segments, entropy))
}

/// Inspect every lossless scan without decoding entropy data. The entropy marker
/// escape/restart rules identify SOS boundaries; native codecs decode samples.
fn lossless_scans(
    input: &mut impl Read,
    frame: &[u8],
    first_scan: &[u8],
    precision: u8,
) -> Result<Vec<u8>, Error> {
    let channels = usize::from(frame[5]);
    if channels == 0 || channels > 4 || frame.len() != 6 + channels * 3 {
        return Err(Error(
            "unsupported lossless JPEG frame component table".into(),
        ));
    }
    let ids: Vec<_> = frame[6..].as_chunks::<3>().0.iter().map(|c| c[0]).collect();
    if ids.iter().enumerate().any(|(i, id)| ids[..i].contains(id)) {
        return Err(Error("duplicate lossless JPEG frame component".into()));
    }
    let mut seen = Vec::with_capacity(channels);
    let mut transforms = Vec::with_capacity(channels);
    let mut scan = first_scan.to_vec();
    loop {
        let count = usize::from(
            *scan
                .first()
                .ok_or_else(|| Error("empty lossless JPEG SOS".into()))?,
        );
        if count == 0 || count > channels || scan.len() != 4 + count * 2 {
            return Err(Error("invalid lossless JPEG SOS component table".into()));
        }
        let predictor = scan[scan.len() - 3];
        let spectral_end = scan[scan.len() - 2];
        let transform = scan[scan.len() - 1];
        if !(1..=7).contains(&predictor)
            || spectral_end != 0
            || transform >> 4 != 0
            || transform >= precision
        {
            return Err(Error("invalid lossless JPEG scan parameters".into()));
        }
        for component in scan[1..1 + 2 * count].as_chunks::<2>().0 {
            if !ids.contains(&component[0]) || seen.contains(&component[0]) {
                return Err(Error(
                    "unknown/repeated lossless JPEG scan component".into(),
                ));
            }
            seen.push(component[0]);
        }
        transforms.push(transform);
        // Skip compressed entropy bytes, escaped FF00, and standalone restart
        // markers. Every other marker ends the current scan.
        let mut marker = loop {
            if byte(input)? != 0xff {
                continue;
            }
            let mut next = byte(input)?;
            while next == 0xff {
                next = byte(input)?;
            }
            if next == 0 || (0xd0..=0xd7).contains(&next) {
                continue;
            }
            break next;
        };
        loop {
            if marker == 0xd9 {
                if seen.len() != channels {
                    return Err(Error("incomplete lossless JPEG component scans".into()));
                }
                let mut extra = [0];
                if input.read(&mut extra)? != 0 {
                    return Err(Error("bytes after lossless JPEG EOI unsupported".into()));
                }
                return Ok(transforms);
            }
            if !matches!(marker, 0xda | 0xc4 | 0xcc | 0xdd | 0xfe | 0xe0..=0xef) {
                return Err(Error(
                    "unsupported marker between lossless JPEG scans".into(),
                ));
            }
            let len = usize::from(u16::from_be_bytes([byte(input)?, byte(input)?]));
            if len < 2 {
                return Err(Error("invalid lossless JPEG segment length".into()));
            }
            let mut payload = vec![0; len - 2];
            input.read_exact(&mut payload)?;
            if marker == 0xda {
                scan = payload;
                break;
            }
            if byte(input)? != 0xff {
                return Err(Error("invalid lossless JPEG inter-scan boundary".into()));
            }
            marker = byte(input)?;
            while marker == 0xff {
                marker = byte(input)?;
            }
        }
    }
}
fn byte(input: &mut impl Read) -> Result<u8, Error> {
    let mut value = [0];
    input.read_exact(&mut value)?;
    Ok(value[0])
}

// Validate the first EOI rather than only the last two file bytes. Entropy byte
// stuffing/restarts are skipped; length-bearing table/scan records stay bounded.
fn dct_end(input: &mut impl Read, limit: usize) -> Result<(), Error> {
    let mut entropy = true;
    let mut headers = 0usize;
    let mut scans = 1usize;
    loop {
        let marker = if entropy {
            loop {
                if byte(input)? != 0xff {
                    continue;
                }
                let mut next = byte(input)?;
                while next == 0xff {
                    next = byte(input)?;
                }
                if next == 0 || (0xd0..=0xd7).contains(&next) {
                    continue;
                }
                break next;
            }
        } else {
            if byte(input)? != 0xff {
                return Err(Error("invalid JPEG inter-scan boundary".into()));
            }
            let mut next = byte(input)?;
            while next == 0xff {
                next = byte(input)?;
            }
            next
        };
        if marker == 0xd9 {
            let mut extra = [0];
            if input.read(&mut extra)? != 0 {
                return Err(Error("bytes after JPEG EOI unsupported".into()));
            }
            return Ok(());
        }
        if !matches!(
            marker,
            0xda | 0xc4 | 0xcc | 0xdb | 0xdd | 0xfe | 0xe0..=0xef
        ) {
            return Err(Error("unsupported marker between JPEG scans".into()));
        }
        let len = usize::from(u16::from_be_bytes([byte(input)?, byte(input)?]));
        if len < 2 {
            return Err(Error("invalid JPEG inter-scan segment length".into()));
        }
        headers = headers
            .checked_add(len + 2)
            .filter(|&n| n <= limit)
            .ok_or_else(|| Error("JPEG inter-scan metadata exceeds limit".into()))?;
        let mut remaining = len - 2;
        let mut scratch = [0u8; 4096];
        while remaining != 0 {
            let n = remaining.min(scratch.len());
            input.read_exact(&mut scratch[..n])?;
            remaining -= n;
        }
        entropy = marker == 0xda;
        if entropy {
            scans += 1;
            if scans > 1000 {
                return Err(Error("JPEG exceeds scan limit".into()));
            }
        }
    }
}
