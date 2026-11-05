//! JPEG 2000 component samples through jpeg2k and BSD-licensed OpenJPEG.
//! No display conversion, precision scaling, palette expansion or ICC transform
//! is applied. Component precision and signedness remain explicit metadata.
pub use crate::raster::{Error, FrameBuffer, Limits};
use crate::{PixelBuffer, PixelType, source::Snapshot};
use std::{fs::File, io::Read, path::Path};

#[derive(Clone, Debug)]
pub struct BoxMetadata {
    pub kind: [u8; 4],
    pub payload: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub channels: usize,
    pub pixel_type: PixelType,
    pub precision: Vec<u8>,
    pub signed: Vec<bool>,
    pub source_origin: [u32; 2],
    /// The default COD wavelet only. Reversible coding can still be rate limited.
    pub default_reversible_wavelet: Option<bool>,
    /// Some(true) for a default irreversible wavelet; otherwise unknown.
    pub lossy: Option<bool>,
    pub encoded_bytes: u64,
    /// Original top-level JP2 boxes other than the codestream, including jp2h.
    pub boxes: Vec<BoxMetadata>,
    /// Original codestream bytes up to the first tile-part marker.
    pub codestream_header: Vec<u8>,
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
    /// Admit bounded full-resolution, equally sampled components before decoding.
    /// # Errors
    /// Malformed containers, subsampling/palettes, precision above 31 bits,
    /// resource bounds, or source changes.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let bytes = read_encoded(&mut file, limits)?;
        let metadata = admit(&bytes, limits)?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            metadata,
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
    /// Decode native integer component samples in stored row order, HWC layout.
    /// # Errors
    /// Source changes, truncated/corrupt streams, backend or sample range failures.
    pub fn read_frame_into(&self, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        let bytes = read_encoded(&mut file, self.limits)?;
        let image = jpeg2000_backend::Image::from_bytes_with(
            &bytes,
            jpeg2000_backend::DecodeParameters::new().strict(true),
        )
        .map_err(|e| Error(e.to_string()))?;
        let components = image.components();
        let metadata = &self.metadata;
        if components.len() != metadata.channels {
            return Err(Error(
                "JPEG 2000 component count changed during decode".into(),
            ));
        }
        for (index, component) in components.iter().enumerate() {
            if [component.height() as usize, component.width() as usize] != metadata.shape
                || component.precision() != u32::from(metadata.precision[index])
                || component.is_signed() != metadata.signed[index]
            {
                return Err(Error(
                    "JPEG 2000 decoded component layout disagrees with SIZ".into(),
                ));
            }
        }
        let count = self.limits.samples(
            metadata.shape,
            metadata.channels,
            metadata.pixel_type.bytes(),
        )?;
        macro_rules! collect_samples {
            ($ty:ty,$variant:ident) => {{
                let mut result = Vec::with_capacity(count);
                for index in 0..metadata.shape[0] * metadata.shape[1] {
                    for component in components {
                        result.push(<$ty>::try_from(component.data()[index]).map_err(|_| {
                            Error("JPEG 2000 sample exceeds declared native container".into())
                        })?);
                    }
                }
                PixelBuffer::$variant(result)
            }};
        }
        buffer.pixels = match metadata.pixel_type {
            PixelType::U8 => collect_samples!(u8, U8),
            PixelType::U16 => collect_samples!(u16, U16),
            PixelType::U32 => collect_samples!(u32, U32),
            PixelType::I8 => collect_samples!(i8, I8),
            PixelType::I16 => collect_samples!(i16, I16),
            PixelType::I32 => collect_samples!(i32, I32),
            _ => return Err(Error("unsupported JPEG 2000 output type".into())),
        };
        self.snapshot.check_handle(&file).map_err(Error)?;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        buffer.shape = Some(metadata.shape);
        buffer.channels = metadata.channels;
        Ok(())
    }
}
fn read_encoded(file: &mut File, limits: Limits) -> Result<Vec<u8>, Error> {
    let len = usize::try_from(file.metadata()?.len())
        .map_err(|_| Error("JPEG 2000 encoded size overflow".into()))?;
    if len > limits.encoded_bytes {
        return Err(Error("JPEG 2000 encoded file exceeds limit".into()));
    }
    let mut bytes = Vec::with_capacity(len);
    file.take(len as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() != len {
        return Err(Error("JPEG 2000 encoded size changed".into()));
    }
    Ok(bytes)
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes(b.try_into().expect("four-byte validated field"))
}
fn take_box(bytes: &[u8], start: usize) -> Result<([u8; 4], usize, usize), Error> {
    let head = bytes
        .get(start..start.saturating_add(8))
        .ok_or_else(|| Error("truncated JPEG 2000 box".into()))?;
    let kind = head[4..8].try_into().expect("four-byte field");
    let (len, header) = match be32(&head[..4]) {
        0 => (bytes.len() - start, 8),
        1 => {
            let ext = bytes
                .get(start + 8..start + 16)
                .ok_or_else(|| Error("truncated extended JP2 box".into()))?;
            (
                usize::try_from(u64::from_be_bytes(
                    ext.try_into().expect("eight-byte field"),
                ))
                .map_err(|_| Error("JP2 box size overflow".into()))?,
                16,
            )
        }
        n => (n as usize, 8),
    };
    let end = start
        .checked_add(len)
        .filter(|&end| len >= header && end <= bytes.len())
        .ok_or_else(|| Error("invalid JP2 box bounds".into()))?;
    Ok((kind, start + header, end))
}
fn admit(bytes: &[u8], limits: Limits) -> Result<Metadata, Error> {
    let mut boxes = Vec::new();
    let mut used = 0usize;
    let codestream = if bytes.starts_with(&[0xff, 0x4f]) {
        bytes
    } else {
        if !bytes.starts_with(&[0, 0, 0, 12, b'j', b'P', b' ', b' ', 13, 10, 135, 10]) {
            return Err(Error("invalid JPEG 2000 signature".into()));
        }
        let mut cursor = 0;
        let mut stream = None;
        while cursor < bytes.len() {
            let (kind, start, end) = take_box(bytes, cursor)?;
            if &kind == b"jp2c" {
                if stream.replace(&bytes[start..end]).is_some() {
                    return Err(Error("multiple JP2 codestreams unsupported".into()));
                }
            } else {
                used = used
                    .checked_add(end - cursor + 64)
                    .filter(|&n| n <= limits.metadata_bytes)
                    .ok_or_else(|| Error("JP2 metadata exceeds limit".into()))?;
                if &kind == b"ftyp" && (end - start < 8 || &bytes[start..start + 4] != b"jp2 ") {
                    return Err(Error(
                        "JPEG 2000 container brand unsupported; require single-image JP2".into(),
                    ));
                }
                if &kind == b"jp2h" {
                    let mut child = start;
                    while child < end {
                        let (kind, _, next) = take_box(&bytes[..end], child)?;
                        if &kind == b"cdef" {
                            return Err(Error(
                                "JP2 channel definitions (cdef) unsupported: preserve original component order".into(),
                            ));
                        }
                        if [&b"pclr"[..], &b"cmap"[..]].contains(&kind.as_slice()) {
                            return Err(Error("JP2 palette/component mapping unsupported".into()));
                        }
                        child = next;
                    }
                }
                boxes.push(BoxMetadata {
                    kind,
                    payload: bytes[start..end].to_vec(),
                });
            }
            cursor = end;
        }
        stream.ok_or_else(|| Error("JP2 lacks codestream".into()))?
    };
    if !codestream.starts_with(&[0xff, 0x4f]) || !codestream.ends_with(&[0xff, 0xd9]) {
        return Err(Error("JPEG 2000 lacks SOC/EOC markers".into()));
    }
    let mut cursor = 2usize;
    let mut siz = None;
    let mut wavelet = None;
    while cursor < codestream.len() {
        let marker = codestream
            .get(cursor..cursor + 2)
            .ok_or_else(|| Error("truncated JPEG 2000 marker".into()))?;
        if marker == [0xff, 0x90] {
            break;
        }
        if marker[0] != 0xff {
            return Err(Error("invalid JPEG 2000 marker".into()));
        }
        let length = codestream
            .get(cursor + 2..cursor + 4)
            .ok_or_else(|| Error("truncated JPEG 2000 marker length".into()))?;
        let len = usize::from(u16::from_be_bytes(
            length.try_into().expect("two-byte field"),
        ));
        if len < 2 {
            return Err(Error("invalid JPEG 2000 marker length".into()));
        }
        let end = cursor
            .checked_add(2 + len)
            .filter(|&end| end <= codestream.len())
            .ok_or_else(|| Error("JPEG 2000 marker exceeds codestream".into()))?;
        if used
            .checked_add(end)
            .is_none_or(|n| n > limits.metadata_bytes)
        {
            return Err(Error("JPEG 2000 header exceeds metadata limit".into()));
        }
        let payload = &codestream[cursor + 4..end];
        match marker[1] {
            0x51 => {
                if siz.is_some() {
                    return Err(Error("duplicate JPEG 2000 SIZ".into()));
                }
                siz = Some(parse_siz(payload, limits)?);
            }
            0x52 => {
                if payload.len() < 10 || payload[5] > 32 || payload[9] > 1 {
                    return Err(Error("invalid/unsupported JPEG 2000 COD".into()));
                }
                wavelet = Some(payload[9] == 1);
            }
            _ => {}
        }
        cursor = end;
    }
    let (shape, precision, signed, origin) =
        siz.ok_or_else(|| Error("JPEG 2000 lacks SIZ".into()))?;
    if codestream.get(cursor..cursor + 2) != Some(&[0xff, 0x90]) {
        return Err(Error("JPEG 2000 lacks tile data".into()));
    }
    let channels = precision.len();
    let bits = precision
        .iter()
        .zip(&signed)
        .map(|(&p, &s)| p + u8::from(!s && signed.contains(&true)))
        .max()
        .unwrap_or(0);
    let pixel_type = match (signed.contains(&true), bits) {
        (false, 1..=8) => PixelType::U8,
        (false, 9..=16) => PixelType::U16,
        (false, 17..=31) => PixelType::U32,
        (true, 1..=8) => PixelType::I8,
        (true, 9..=16) => PixelType::I16,
        (true, 17..=32) => PixelType::I32,
        _ => return Err(Error("JPEG 2000 component type unsupported".into())),
    };
    limits.samples(shape, channels, pixel_type.bytes())?;
    Ok(Metadata {
        shape,
        channels,
        pixel_type,
        precision,
        signed,
        source_origin: origin,
        default_reversible_wavelet: wavelet,
        lossy: wavelet.filter(|&v| !v).map(|_| true),
        encoded_bytes: bytes.len() as u64,
        boxes,
        codestream_header: codestream[..cursor].to_vec(),
    })
}
type Siz = ([usize; 2], Vec<u8>, Vec<bool>, [u32; 2]);
fn parse_siz(p: &[u8], limits: Limits) -> Result<Siz, Error> {
    if p.len() < 36 {
        return Err(Error("truncated JPEG 2000 SIZ".into()));
    }
    let channels = usize::from(u16::from_be_bytes([p[34], p[35]]));
    if channels == 0 || channels > 256 || p.len() != 36 + 3 * channels {
        return Err(Error("invalid JPEG 2000 component table".into()));
    }
    let x = be32(&p[2..6]);
    let y = be32(&p[6..10]);
    let ox = be32(&p[10..14]);
    let oy = be32(&p[14..18]);
    let tw = be32(&p[18..22]);
    let th = be32(&p[22..26]);
    let tx = be32(&p[26..30]);
    let ty = be32(&p[30..34]);
    if x <= ox
        || y <= oy
        || tw == 0
        || th == 0
        || tx > ox
        || ty > oy
        || ox - tx >= tw
        || oy - ty >= th
    {
        return Err(Error("invalid JPEG 2000 image/tile geometry".into()));
    }
    let tiles =
        u64::from(x - tx).div_ceil(u64::from(tw)) * u64::from(y - ty).div_ceil(u64::from(th));
    if tiles > 65536 {
        return Err(Error("JPEG 2000 tile count exceeds admission limit".into()));
    }
    let shape = [(y - oy) as usize, (x - ox) as usize];
    let pixels = shape[0]
        .checked_mul(shape[1])
        .and_then(|v| v.checked_mul(channels))
        .ok_or_else(|| Error("JPEG 2000 sample count overflow".into()))?;
    if pixels / channels > u32::MAX as usize
        || pixels
            .checked_mul(4)
            .is_none_or(|n| n > limits.decoder_bytes)
    {
        return Err(Error(
            "JPEG 2000 component payload exceeds decoder limit".into(),
        ));
    }
    let mut precision = Vec::with_capacity(channels);
    let mut signed = Vec::with_capacity(channels);
    for c in p[36..].as_chunks::<3>().0 {
        let bits = (c[0] & 0x7f) + 1;
        if bits > 31 || c[1] != 1 || c[2] != 1 {
            return Err(Error(
                "JPEG 2000 subsampling or precision above 31 bits unsupported".into(),
            ));
        }
        precision.push(bits);
        signed.push(c[0] & 0x80 != 0);
    }
    Ok((shape, precision, signed, [oy, ox]))
}
