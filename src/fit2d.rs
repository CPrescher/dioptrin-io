//! FIT2D binary records, scalar arrays and packed31-bit masks. Stored array
//! orientation and floating bits survive without display flips or correction.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelBuffer, PixelType,
    detector_common::{Source, product, sum},
};
use std::{collections::BTreeMap, path::Path};
#[derive(Debug, Clone, PartialEq)]
pub enum HeaderValue {
    Text(String),
    Integer(u32),
    Float(f32),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Scalar,
    Packed31,
}
#[derive(Debug, Clone)]
pub struct ArrayMetadata {
    pub name: String,
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    pub little_endian: bool,
    pub encoding: Encoding,
    pub offset: u64,
    pub encoded_bytes: usize,
    pub blocks: usize,
    /// Scalar storage keeps the first128 values in each physical record. Mask
    /// storage keeps the low31 bits of every32-bit word, least significant first.
    pub values_per_block: usize,
    pub record: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    pub little_endian: bool,
    pub block_bytes: usize,
    /// Original non-array records in source order; array records survive in arrays.
    pub header: Vec<u8>,
    pub fields: BTreeMap<String, HeaderValue>,
    pub arrays: Vec<ArrayMetadata>,
}
pub struct Reader {
    source: Source,
    metadata: Metadata,
    data: usize,
}
impl Reader {
    /// Index the complete immutable record stream, including named mask/axis arrays.
    /// # Errors
    /// Unsupported records, malformed lengths, duplicates, bounds or truncation.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let source = Source::open(path)?;
        if limits.header_bytes < 512 {
            return Err(Error("FIT2D record exceeds header limit".into()));
        }
        let mut first = Vec::new();
        source.read(0, 512, &mut first)?;
        if !first.starts_with(b"\\$FFF_START:") {
            return Err(Error("invalid FIT2D magic".into()));
        }
        // The initial string record has no payload; the second endian record
        // identifies an admitted physical record size in 512-byte units.
        let mut block_bytes = None;
        let mut probe = Vec::new();
        for blocks in 1..16 {
            let offset = product(blocks, 512)?;
            if offset > limits.header_bytes || offset as u64 + 512 > source.bytes() {
                break;
            }
            source.read(offset as u64, 512, &mut probe)?;
            if probe.starts_with(b"\\$BIG_ENDIAN:") {
                block_bytes = Some(offset);
                break;
            }
        }
        let block_bytes = block_bytes
            .ok_or_else(|| Error("unsupported FIT2D record size/endian prefix".into()))?;
        let mut position = 0u64;
        let mut header = Vec::new();
        let mut fields = BTreeMap::new();
        let mut arrays: Vec<ArrayMetadata> = Vec::new();
        let mut little = None;
        let mut ended = false;
        let mut retained = 0usize;
        while position < source.bytes() {
            let mut record = Vec::new();
            source.read(position, block_bytes, &mut record)?;
            if record.first() != Some(&b'\\') {
                return Err(Error("invalid FIT2D record start".into()));
            }
            let colon = record
                .iter()
                .position(|&v| v == b':')
                .ok_or_else(|| Error("missing FIT2D record delimiter".into()))?;
            let name = std::str::from_utf8(&record[1..colon])
                .map_err(|_| Error("non-ASCII FIT2D record key".into()))?
                .to_owned();
            if name.is_empty()
                || fields.contains_key(&name)
                || arrays.iter().any(|a| a.name == name)
            {
                return Err(Error("duplicate/empty FIT2D record key".into()));
            }
            let line = &record[colon + 1..];
            let blocks = hex(line, 0)? as usize;
            let kind = *line
                .get(8)
                .ok_or_else(|| Error("short FIT2D record".into()))?;
            position = position
                .checked_add(block_bytes as u64)
                .ok_or_else(|| Error("FIT2D offset overflow".into()))?;
            retained = sum(retained, record.len())?;
            if retained > limits.header_bytes {
                return Err(Error("FIT2D retained records exceed header limit".into()));
            }
            if kind == b'a' {
                if blocks == 0 || arrays.len() >= limits.frames {
                    return Err(Error("invalid/oversized FIT2D array count".into()));
                }
                let little_endian =
                    little.ok_or_else(|| Error("FIT2D endian must precede arrays".into()))?;
                let shape = [hex(line, 34)? as usize, hex(line, 26)? as usize];
                if hex(line, 18)? != 2 {
                    return Err(Error("unsupported FIT2D array rank".into()));
                }
                let (pixel_type, encoding, values_per_block) = match line.get(9) {
                    Some(b'i') => (PixelType::I32, Encoding::Scalar, 128),
                    Some(b'r') => (PixelType::F32, Encoding::Scalar, 128),
                    Some(b'l') => (
                        PixelType::U8,
                        Encoding::Packed31,
                        product(block_bytes / 4, 31)?,
                    ),
                    _ => return Err(Error("unsupported FIT2D array type".into())),
                };
                let expected_bits = if encoding == Encoding::Packed31 {
                    1
                } else {
                    32
                };
                if hex(line, 10)? != expected_bits {
                    return Err(Error("unsupported FIT2D declared sample width".into()));
                }
                let count = limits.pixels(shape, pixel_type)?;
                let expected_blocks = count
                    .checked_add(values_per_block - 1)
                    .ok_or_else(|| Error("FIT2D array size overflow".into()))?
                    / values_per_block;
                if blocks != expected_blocks {
                    return Err(Error("FIT2D block count disagrees with shape".into()));
                }
                let encoded_bytes = product(blocks, block_bytes)?;
                if encoded_bytes > limits.encoded_bytes {
                    return Err(Error("FIT2D array exceeds encoded limit".into()));
                }
                let end = position
                    .checked_add(encoded_bytes as u64)
                    .filter(|&p| p <= source.bytes())
                    .ok_or_else(|| Error("truncated FIT2D array".into()))?;
                arrays.push(ArrayMetadata {
                    name,
                    shape,
                    pixel_type,
                    little_endian,
                    encoding,
                    offset: position,
                    encoded_bytes,
                    blocks,
                    values_per_block,
                    record,
                });
                position = end;
            } else {
                if blocks != 0 {
                    return Err(Error("unsupported FIT2D non-array payload".into()));
                }
                let value = match kind {
                    b's' => {
                        let len = hex(line, 9)? as usize;
                        let end = sum(17, len)?;
                        HeaderValue::Text(
                            std::str::from_utf8(
                                line.get(17..end)
                                    .ok_or_else(|| Error("FIT2D string outside record".into()))?,
                            )
                            .map_err(|_| Error("invalid FIT2D text".into()))?
                            .to_owned(),
                        )
                    }
                    b'i' => HeaderValue::Integer(hex(line, 9)?),
                    b'r' => HeaderValue::Float(f32::from_bits(hex(line, 9)?)),
                    _ => return Err(Error("unsupported FIT2D scalar type".into())),
                };
                if name == "$BIG_ENDIAN" {
                    little = match &value {
                        HeaderValue::Text(s) if s == "FALSE" => Some(true),
                        HeaderValue::Text(s) if s == "TRUE" => Some(false),
                        _ => return Err(Error("invalid FIT2D byte order".into())),
                    };
                }
                if name == "$FFF_START" && value != HeaderValue::Text("FIT2D Data File".into()) {
                    return Err(Error("invalid FIT2D start record".into()));
                }
                if name == "$FFF_END" {
                    ended = true;
                    if position != source.bytes() {
                        return Err(Error("trailing/stacked FIT2D records".into()));
                    }
                }
                fields.insert(name, value);
                header.extend_from_slice(&record);
            }
            let estimate = retained
                .saturating_mul(3)
                .saturating_add(fields.len().saturating_mul(160))
                .saturating_add(arrays.len().saturating_mul(256));
            if estimate > limits.metadata_bytes {
                return Err(Error("FIT2D retained metadata exceeds limit".into()));
            }
        }
        if !ended {
            return Err(Error("missing FIT2D end record".into()));
        }
        let data = arrays
            .iter()
            .position(|a| a.name == "data_array")
            .ok_or_else(|| Error("missing FIT2D data_array".into()))?;
        let shape = arrays[data].shape;
        let pixel_type = arrays[data].pixel_type;
        let little_endian = arrays[data].little_endian;
        source.check()?;
        Ok(Self {
            source,
            data,
            metadata: Metadata {
                shape,
                pixel_type,
                little_endian,
                block_bytes,
                header,
                fields,
                arrays,
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
    /// Read the primary data_array with exact stored row order.
    /// # Errors
    /// Source mutation, truncation or allocation failure.
    pub fn read_frame_into(&self, buffer: &mut FrameBuffer) -> Result<(), Error> {
        self.read_array_into(self.data, buffer)
    }
    /// Read a named mask/axis/data array by metadata index; no array is combined.
    /// # Errors
    /// Invalid index, mutation, truncation or allocation failure.
    pub fn read_array_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        let array = self
            .metadata
            .arrays
            .get(index)
            .ok_or_else(|| Error("FIT2D array index outside source".into()))?;
        self.source
            .read(array.offset, array.encoded_bytes, &mut buffer.encoded)?;
        let count = array.shape[0] * array.shape[1];
        buffer
            .pixels
            .resize(array.pixel_type, count)
            .map_err(Error)?;
        if array.encoding == Encoding::Packed31 {
            let PixelBuffer::U8(out) = &mut buffer.pixels else {
                unreachable!()
            };
            for (i, slot) in out.iter_mut().enumerate() {
                let p = (i / 31) * 4;
                let bytes = buffer.encoded[p..p + 4].try_into().unwrap();
                let word = if array.little_endian {
                    u32::from_le_bytes(bytes)
                } else {
                    u32::from_be_bytes(bytes)
                };
                *slot = ((word >> (i % 31)) & 1) as u8;
            }
        } else {
            let output = buffer.pixels.bytes_mut();
            for (index, slot) in output.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let offset = (index / 128) * self.metadata.block_bytes + (index % 128) * 4;
                slot.copy_from_slice(&buffer.encoded[offset..offset + 4]);
                if array.little_endian != cfg!(target_endian = "little") {
                    slot.reverse();
                }
            }
        }
        self.source.check()?;
        buffer.shape = Some(array.shape);
        Ok(())
    }
}
fn hex(line: &[u8], offset: usize) -> Result<u32, Error> {
    let bytes = line
        .get(offset..offset + 8)
        .ok_or_else(|| Error("short FIT2D hexadecimal field".into()))?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error("non-ASCII FIT2D hexadecimal field".into()))?;
    if !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error("invalid FIT2D hexadecimal field".into()));
    }
    u32::from_str_radix(text, 16).map_err(|_| Error("invalid FIT2D hexadecimal integer".into()))
}
