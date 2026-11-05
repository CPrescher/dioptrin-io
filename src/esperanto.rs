//! CrysAlis Esperanto: exact stored signed32 samples and bounded AGI bitfields.
//! AGI decoder adapted from the MIT FabIO implementation by Florian Plaswig and
//! Jérôme Kieffer, ESRF. See licenses/native-readers/FabIO-Esperanto-AGI-MIT.txt.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelBuffer, PixelType,
    binary::{copy_samples, resize_bytes},
    source::Snapshot,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Long32,
    AgiBitfield,
}
#[derive(Clone, Debug)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub frames: usize,
    pub source_pixel_type: PixelType,
    pub pixel_type: PixelType,
    pub encoding: Encoding,
    pub version: u32,
    pub header_lines: usize,
    /// Every original ASCII header byte, including spaces, CRLF and final CR/SUB.
    pub header: Vec<u8>,
    /// Original header assignments; unknown keys/values are retained.
    pub fields: BTreeMap<String, String>,
    pub data_bytes: usize,
    /// Original AGI row byte offsets, relative to the encoded data block.
    pub row_offsets: Vec<u32>,
    /// Optional zero fill to the next 1024-byte encoded-payload boundary.
    pub padding_bytes: usize,
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
    offset: u64,
    encoded_bytes: usize,
}
impl Reader {
    /// Open native signed32 stored pixels without padding/centering/resampling.
    /// # Errors
    /// Unsupported/malformed headers, shape/resources, unknown encoding or truncation.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        if limits.header_bytes < 256 {
            return Err(Error("Esperanto first header line exceeds limit".into()));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut first = [0u8; 256];
        file.read_exact(&mut first)?;
        if !first.is_ascii() || &first[254..] != b"\r\n" {
            return Err(Error("invalid Esperanto first header line".into()));
        }
        let text = std::str::from_utf8(&first[..254]).unwrap();
        let words: Vec<_> = text.split_ascii_whitespace().collect();
        if words.len() != 11
            || words[0..2] != ["ESPERANTO", "FORMAT"]
            || words[3..5] != ["CONSISTING", "OF"]
            || words[6..8] != ["LINES", "OF"]
            || words[8..11] != ["256", "BYTES", "EACH"]
        {
            return Err(Error("unsupported Esperanto header declaration".into()));
        }
        let version = words[2]
            .parse::<u32>()
            .map_err(|_| Error("invalid Esperanto version".into()))?;
        if version != 1 {
            return Err(Error("unsupported Esperanto version".into()));
        }
        let header_lines = words[5]
            .parse::<usize>()
            .map_err(|_| Error("invalid Esperanto header line count".into()))?;
        let header_bytes = header_lines
            .checked_mul(256)
            .filter(|&n| {
                header_lines >= 2 && n <= limits.header_bytes && n as u64 <= snapshot.bytes()
            })
            .ok_or_else(|| Error("Esperanto header size exceeds source/limit".into()))?;
        let header_cost = header_bytes
            .checked_mul(3)
            .and_then(|n| n.checked_add(header_lines * 128))
            .filter(|&n| n <= limits.metadata_bytes)
            .ok_or_else(|| Error("Esperanto header exceeds metadata budget".into()))?;
        let mut header = Vec::new();
        resize_bytes(&mut header, header_bytes)?;
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut header)?;
        if !header.is_ascii() {
            return Err(Error("Esperanto header must be ASCII".into()));
        }
        let mut fields = BTreeMap::new();
        fields.insert("ESPERANTO FORMAT".into(), words[2..].join(" "));
        for (i, line) in header.as_chunks::<256>().0.iter().enumerate().skip(1) {
            if &line[254..] != b"\r\n" && !(i == header_lines - 1 && &line[254..] == b"\r\x1a") {
                return Err(Error(format!("invalid Esperanto line delimiter {i}")));
            }
            let text = std::str::from_utf8(&line[..254]).unwrap().trim();
            if text.is_empty() {
                continue;
            }
            let (key, value) = text
                .split_once(char::is_whitespace)
                .map_or((text, ""), |(k, v)| (k, v.trim()));
            if fields.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(Error("duplicate Esperanto header key".into()));
            }
        }
        let image = fields
            .get("IMAGE")
            .ok_or_else(|| Error("missing Esperanto IMAGE declaration".into()))?;
        let image: Vec<_> = image.split_ascii_whitespace().collect();
        if image.len() != 5 {
            return Err(Error("invalid Esperanto IMAGE declaration".into()));
        }
        let dimension = |s: &str| {
            s.parse::<usize>()
                .map_err(|_| Error("invalid Esperanto image dimension/binning".into()))
        };
        let shape = [dimension(image[1])?, dimension(image[0])?];
        let _binning = [dimension(image[2])?, dimension(image[3])?];
        let encoding = match image[4].trim_matches('"') {
            "4BYTE_LONG" => Encoding::Long32,
            "AGI_BITFIELD" => Encoding::AgiBitfield,
            _ => return Err(Error("unsupported Esperanto encoding".into())),
        };
        let pixel_type = PixelType::I32;
        let count = limits.pixels(shape, pixel_type)?;
        let encoded_bytes = usize::try_from(snapshot.bytes() - header_bytes as u64)
            .ok()
            .filter(|&n| n > 0 && n <= limits.encoded_bytes)
            .ok_or_else(|| Error("Esperanto payload exceeds encoded-byte limit".into()))?;
        let (data_bytes, row_offsets, padding_bytes) = match encoding {
            Encoding::Long32 => {
                if count.checked_mul(4) != Some(encoded_bytes) {
                    return Err(Error(
                        "Esperanto Long32 source length disagrees with shape".into(),
                    ));
                }
                (encoded_bytes, Vec::new(), 0)
            }
            Encoding::AgiBitfield => {
                let mut size = [0u8; 4];
                file.read_exact(&mut size)?;
                let data_bytes = u32::from_le_bytes(size) as usize;
                shape[0]
                    .checked_mul(8)
                    .and_then(|n| header_cost.checked_add(n))
                    .filter(|&n| n <= limits.metadata_bytes)
                    .ok_or_else(|| Error("Esperanto row index exceeds metadata budget".into()))?;
                let index_bytes = shape[0] * 4;
                let expected = data_bytes
                    .checked_add(4)
                    .and_then(|n| n.checked_add(index_bytes))
                    .filter(|&n| data_bytes > 0 && n <= encoded_bytes)
                    .ok_or_else(|| {
                        Error("Esperanto AGI data/index lengths exceed source".into())
                    })?;
                let padding = encoded_bytes - expected;
                if padding > 0 && (padding >= 1024 || !encoded_bytes.is_multiple_of(1024)) {
                    return Err(Error("unexpected Esperanto AGI trailing bytes".into()));
                }
                file.seek(SeekFrom::Start(header_bytes as u64 + 4 + data_bytes as u64))?;
                let mut raw = Vec::new();
                resize_bytes(&mut raw, index_bytes)?;
                file.read_exact(&mut raw)?;
                let mut offsets = Vec::new();
                offsets
                    .try_reserve_exact(shape[0])
                    .map_err(|e| Error(e.to_string()))?;
                for value in raw.as_chunks::<4>().0 {
                    offsets.push(u32::from_le_bytes(*value));
                }
                if offsets[0] != 0
                    || offsets.iter().any(|&n| n as usize >= data_bytes)
                    || offsets.windows(2).any(|p| p[0] >= p[1])
                {
                    return Err(Error("invalid Esperanto AGI row offsets".into()));
                }
                let mut tail = Vec::new();
                resize_bytes(&mut tail, padding)?;
                file.read_exact(&mut tail)?;
                if tail.iter().any(|&b| b != 0) {
                    return Err(Error("nonzero Esperanto AGI padding".into()));
                }
                (data_bytes, offsets, padding)
            }
        };
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            offset: header_bytes as u64,
            encoded_bytes,
            metadata: Metadata {
                shape,
                frames: 1,
                source_pixel_type: PixelType::I32,
                pixel_type,
                encoding,
                version,
                header_lines,
                header,
                fields,
                data_bytes,
                row_offsets,
                padding_bytes,
            },
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
    /// Read exactly one stored-order image with native signed32 reconstruction.
    /// # Errors
    /// Invalid index, changed source, malformed AGI row/escapes, truncation or allocation.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        if index != 0 {
            return Err(Error("Esperanto frame outside source".into()));
        }
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(self.offset))?;
        resize_bytes(&mut buffer.encoded, self.encoded_bytes)?;
        file.read_exact(&mut buffer.encoded)?;
        let shape = self.metadata.shape;
        buffer
            .pixels
            .resize(self.metadata.pixel_type, shape[0] * shape[1])
            .map_err(Error)?;
        if self.metadata.encoding == Encoding::Long32 {
            copy_samples(&buffer.encoded, &mut buffer.pixels, true)?;
        } else {
            let data = &buffer.encoded[4..4 + self.metadata.data_bytes];
            match &mut buffer.pixels {
                PixelBuffer::I32(output) => {
                    for (r, row) in output.chunks_exact_mut(shape[1]).enumerate() {
                        let mut position = 0;
                        let mut value = 0i32;
                        decode_row(
                            &data[self.metadata.row_offsets[r] as usize
                                ..self
                                    .metadata
                                    .row_offsets
                                    .get(r + 1)
                                    .map_or(self.metadata.data_bytes, |&n| n as usize)],
                            shape[1],
                            |difference| {
                                value = value.wrapping_add(difference);
                                row[position] = value;
                                position += 1;
                                Ok(())
                            },
                        )?;
                    }
                }
                _ => unreachable!(),
            }
        }
        self.snapshot.check_handle(&file).map_err(Error)?;
        buffer.shape = Some(shape);
        Ok(())
    }
}
struct Bytes<'a> {
    data: &'a [u8],
    position: usize,
}
impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self
            .position
            .checked_add(n)
            .filter(|&end| end <= self.data.len())
            .ok_or_else(|| Error("truncated Esperanto AGI field/escape".into()))?;
        let bytes = &self.data[self.position..end];
        self.position = end;
        Ok(bytes)
    }
    fn escaped(&mut self) -> Result<i32, Error> {
        match self.take(1)?[0] {
            254 => Ok(i32::from(i16::from_le_bytes(
                self.take(2)?.try_into().unwrap(),
            ))),
            255 => Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap())),
            value => Ok(i32::from(value) - 127),
        }
    }
}
fn decode_row(
    data: &[u8],
    columns: usize,
    mut sample: impl FnMut(i32) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut input = Bytes { data, position: 0 };
    sample(input.escaped()?)?;
    for _ in 0..(columns - 1) / 16 {
        let lengths = input.take(1)?[0];
        let widths = [lengths & 15, lengths >> 4];
        if widths.iter().any(|&n| !(1..=8).contains(&n)) {
            return Err(Error("unsupported Esperanto AGI field width".into()));
        }
        let mut fields = [[0u8; 8]; 2];
        for (field, &width) in fields.iter_mut().zip(&widths) {
            field[..width as usize].copy_from_slice(input.take(width as usize)?);
        }
        for (field, &width) in fields.iter().zip(&widths) {
            let packed = u64::from_le_bytes(*field);
            let mask = (1u64 << width) - 1;
            let bias = (1i32 << (width - 1)) - 1;
            for i in 0..8 {
                let code = ((packed >> (i * u32::from(width))) & mask) as u8;
                let value = if width == 8 && code >= 254 {
                    if code == 254 {
                        i32::from(i16::from_le_bytes(input.take(2)?.try_into().unwrap()))
                    } else {
                        i32::from_le_bytes(input.take(4)?.try_into().unwrap())
                    }
                } else {
                    i32::from(code) - bias
                };
                sample(value)?;
            }
        }
    }
    for _ in 0..(columns - 1) % 16 {
        sample(input.escaped()?)?;
    }
    if input.position != data.len() {
        return Err(Error(
            "Esperanto AGI row length disagrees with index".into(),
        ));
    }
    Ok(())
}
