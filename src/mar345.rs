//! Bounded MAR345/MAR555 PCK V1 images with original unsigned counts/headers.
//! PCK decoding/predictor adapted from MIT FabIO ext/mar345_IO.pyx, ESRF.
//! The older LGPL ccp4_pack implementation is not incorporated.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{PixelBuffer, PixelType, binary::resize_bytes, source::Snapshot};
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
const HEADER_BYTES: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverflowLayout {
    PaddedEight,
    Compact,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overflow {
    /// Original one-based flat source index.
    pub one_based_index: u32,
    /// Original unsigned32 count bits, independent of the endian source word.
    pub value: u32,
}
#[derive(Clone, Debug)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    /// Packed image base counts before unsigned32 high-value replacements.
    pub base_pixel_type: PixelType,
    pub little_endian: bool,
    pub pck_version: u8,
    pub overflow_layout: OverflowLayout,
    pub header: Vec<u8>,
    pub binary_words: [i32; 16],
    pub text_start: usize,
    /// Reversible Latin-1 source assignments, with repeated fields retained below.
    pub fields: BTreeMap<String, String>,
    pub entries: Vec<(String, String)>,
    pub overflow_records: Vec<Overflow>,
    /// Complete original table, including unused record padding.
    pub overflow_bytes: Vec<u8>,
    /// Original newline-delimited PCK dimensions, including both delimiters.
    pub marker: Vec<u8>,
    pub encoded_bytes: usize,
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
    offset: u64,
}
impl Reader {
    /// Open one frozen MAR345 or MAR555 image. PCK V2/spiral and unknown layouts
    /// fail explicitly; no fallback or whitespace trimming changes binary pixels.
    /// # Errors
    /// Malformed/unsupported headers, shape/overflow/marker bounds, limits or mutation.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        if limits.header_bytes < HEADER_BYTES || limits.metadata_bytes < HEADER_BYTES * 4 {
            return Err(Error("MAR345 header exceeds limits".into()));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut header = Vec::new();
        resize_bytes(&mut header, HEADER_BYTES)?;
        file.read_exact(&mut header)?;
        let magic: [u8; 4] = header[..4].try_into().unwrap();
        let little = if i32::from_le_bytes(magic) == 1234 {
            true
        } else if i32::from_be_bytes(magic) == 1234 {
            false
        } else {
            return Err(Error("invalid MAR345 byte-order marker".into()));
        };
        let mut words = [0i32; 16];
        for (word, raw) in words.iter_mut().zip(header[..64].as_chunks::<4>().0) {
            *word = if little {
                i32::from_le_bytes(*raw)
            } else {
                i32::from_be_bytes(*raw)
            };
        }
        let positive = |v: i32| {
            usize::try_from(v)
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| Error("invalid MAR345 dimensions/pixel count".into()))
        };
        let width = positive(words[1])?;
        let count = positive(words[5])?;
        if width < 2 || !count.is_multiple_of(width) || count / width < 2 {
            return Err(Error(
                "unsupported or inconsistent MAR345 predictor shape".into(),
            ));
        }
        let shape = [count / width, width];
        limits.pixels(shape, PixelType::U32)?;
        let high = usize::try_from(words[2])
            .ok()
            .filter(|&n| n <= count)
            .ok_or_else(|| Error("invalid MAR345 high-count record count".into()))?;
        if words[3] != 1 {
            return Err(Error(format!(
                "unsupported MAR345 storage format {} (PCK required)",
                words[3]
            )));
        }
        if ![0, 1].contains(&words[4]) {
            return Err(Error("unsupported MAR345 collection mode".into()));
        }
        let compact = high
            .checked_mul(8)
            .ok_or_else(|| Error("MAR345 overflow table size overflow".into()))?;
        let padded = high
            .div_ceil(8)
            .checked_mul(64)
            .ok_or_else(|| Error("MAR345 overflow table size overflow".into()))?;
        let mut metadata_used = HEADER_BYTES
            .checked_mul(4)
            .and_then(|n| n.checked_add(padded))
            .and_then(|n| n.checked_add(high.checked_mul(64)?))
            .filter(|&n| n <= limits.metadata_bytes)
            .ok_or_else(|| Error("MAR345 overflow/header metadata exceeds budget".into()))?;
        let text_start = header[64..256]
            .windows(12)
            .position(|s| s == b"mar research")
            .map(|n| n + 64)
            .ok_or_else(|| Error("MAR345 text header signature missing".into()))?;
        let mut fields = BTreeMap::new();
        let mut entries = Vec::new();
        let mut end_header = false;
        for line in header[text_start..].split(|&b| b == b'\n') {
            let text: String = line.iter().map(|&b| char::from(b)).collect();
            let text = text.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0');
            if text == "END OF HEADER" {
                end_header = true;
                break;
            }
            if text.is_empty() || text == "mar research" {
                continue;
            }
            if let Some((key, value)) = text.split_once(char::is_whitespace) {
                metadata_used = metadata_used
                    .checked_add(
                        key.len()
                            .saturating_add(value.len())
                            .saturating_mul(2)
                            .saturating_add(128),
                    )
                    .filter(|&n| n <= limits.metadata_bytes)
                    .ok_or_else(|| Error("MAR345 text entries exceed metadata budget".into()))?;
                let value = value.trim().to_owned();
                entries.push((key.to_owned(), value.clone()));
                fields.insert(key.to_owned(), value);
            }
        }
        if !end_header {
            return Err(Error("MAR345 text header lacks terminator".into()));
        }
        if let Some(value) = fields.get("HIGH")
            && value.parse::<usize>().ok() != Some(high)
        {
            return Err(Error("MAR345 binary/text HIGH counts disagree".into()));
        }
        if let Some(value) = fields.get("FORMAT") {
            let parts: Vec<_> = value.split_ascii_whitespace().collect();
            if parts.len() != 3
                || parts[0].parse::<usize>().ok() != Some(width)
                || !["PCK", "MAR345", "PCK345"].contains(&parts[1])
                || parts[2].parse::<usize>().ok() != Some(count)
            {
                return Err(Error("MAR345 binary/text FORMAT disagree".into()));
            }
        }
        let mut candidates = Vec::new();
        for (bytes, layout) in [
            (padded, OverflowLayout::PaddedEight),
            (compact, OverflowLayout::Compact),
        ] {
            if layout == OverflowLayout::Compact && compact == padded {
                continue;
            }
            let start = HEADER_BYTES as u64 + bytes as u64;
            if let Some(marker) = marker_at(&mut file, start, snapshot.bytes())? {
                candidates.push((bytes, layout, marker));
            }
        }
        if candidates.len() != 1 {
            return Err(Error(
                "MAR345 overflow/PCK framing is missing or ambiguous".into(),
            ));
        }
        let (table_bytes, layout, (version, marker_shape, marker)) = candidates.pop().unwrap();
        if version != 1 {
            return Err(Error("MAR345 PCK V2 unsupported; no V1 fallback".into()));
        }
        if marker_shape != shape {
            return Err(Error("MAR345 binary/PCK dimensions disagree".into()));
        }
        if HEADER_BYTES
            .checked_add(marker.len())
            .is_none_or(|n| n > limits.header_bytes)
        {
            return Err(Error("MAR345 PCK marker exceeds header budget".into()));
        }
        let offset = HEADER_BYTES as u64 + table_bytes as u64 + marker.len() as u64;
        let encoded_bytes = usize::try_from(
            snapshot
                .bytes()
                .checked_sub(offset)
                .ok_or_else(|| Error("truncated MAR345 PCK data".into()))?,
        )
        .ok()
        .filter(|&n| n > 0 && n <= limits.encoded_bytes)
        .ok_or_else(|| Error("MAR345 encoded data empty or exceeds limit".into()))?;
        let mut overflow_bytes = Vec::new();
        resize_bytes(&mut overflow_bytes, table_bytes)?;
        file.seek(SeekFrom::Start(HEADER_BYTES as u64))?;
        file.read_exact(&mut overflow_bytes)?;
        let mut overflow_records = Vec::new();
        overflow_records
            .try_reserve_exact(high)
            .map_err(|e| Error(e.to_string()))?;
        let mut seen = HashSet::new();
        seen.try_reserve(high).map_err(|e| Error(e.to_string()))?;
        for record in overflow_bytes[..compact].as_chunks::<8>().0 {
            let read = |b: [u8; 4]| {
                if little {
                    u32::from_le_bytes(b)
                } else {
                    u32::from_be_bytes(b)
                }
            };
            let index = read(record[..4].try_into().unwrap());
            let value = read(record[4..].try_into().unwrap());
            if index == 0 || index as usize > count || !seen.insert(index) {
                return Err(Error(
                    "MAR345 overflow index outside image or duplicated".into(),
                ));
            }
            overflow_records.push(Overflow {
                one_based_index: index,
                value,
            });
        }
        // The manufacturer's format requires zero-filled unused pairs. They
        // are not extra declared counts and must never silently override pixels.
        if overflow_bytes[compact..].iter().any(|&byte| byte != 0) {
            return Err(Error("MAR345 unused overflow pairs must be zero".into()));
        }
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            offset,
            metadata: Metadata {
                shape,
                pixel_type: PixelType::U32,
                base_pixel_type: PixelType::U16,
                little_endian: little,
                pck_version: 1,
                overflow_layout: layout,
                header,
                binary_words: words,
                text_start,
                fields,
                entries,
                overflow_records,
                overflow_bytes,
                marker,
                encoded_bytes,
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
    /// Decode original row-major counts, with modulo16 predictor arithmetic and
    /// exact unsigned32 overflow values. No orientation/calibration is applied.
    /// # Errors
    /// Bad frame, changed/truncated source, malformed blocks/padding or allocation.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        if index != 0 {
            return Err(Error("MAR345 frame outside source".into()));
        }
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(self.offset))?;
        resize_bytes(&mut buffer.encoded, self.metadata.encoded_bytes)?;
        file.read_exact(&mut buffer.encoded)?;
        let shape = self.metadata.shape;
        buffer
            .pixels
            .resize(PixelType::U32, shape[0] * shape[1])
            .map_err(Error)?;
        let PixelBuffer::U32(output) = &mut buffer.pixels else {
            unreachable!()
        };
        decode_v1(&buffer.encoded, shape[1], output)?;
        for record in &self.metadata.overflow_records {
            output[record.one_based_index as usize - 1] = record.value;
        }
        self.snapshot.check_handle(&file).map_err(Error)?;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        buffer.shape = Some(shape);
        Ok(())
    }
}
type Marker = (u8, [usize; 2], Vec<u8>);
fn marker_at(file: &mut File, offset: u64, source_bytes: u64) -> Result<Option<Marker>, Error> {
    if offset >= source_bytes {
        return Ok(None);
    }
    let len = (source_bytes - offset).min(128) as usize;
    let mut bytes = vec![0u8; len];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut bytes)?;
    let start = if bytes.starts_with(b"\r\n") {
        2
    } else if bytes.starts_with(b"\n") {
        1
    } else {
        return Ok(None);
    };
    let Some(end) = bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|n| n + start)
    else {
        return Ok(None);
    };
    let Ok(line) = std::str::from_utf8(&bytes[start..end]) else {
        return Ok(None);
    };
    let line = line.trim_end_matches('\r');
    let (version, dimensions) = if let Some(s) = line.strip_prefix("CCP4 packed image, X: ") {
        (1, s)
    } else if let Some(s) = line.strip_prefix("CCP4 packed image V2, X: ") {
        (2, s)
    } else {
        return Ok(None);
    };
    let Some((width, height)) = dimensions.split_once(", Y: ") else {
        return Ok(None);
    };
    let (Ok(width), Ok(height)) = (
        width.trim().parse::<usize>(),
        height.trim().parse::<usize>(),
    ) else {
        return Ok(None);
    };
    bytes.truncate(end + 1);
    Ok(Some((version, [height, width], bytes)))
}
struct Bits<'a> {
    data: &'a [u8],
    position: usize,
}
impl Bits<'_> {
    fn take(&mut self, width: u8) -> Result<u32, Error> {
        if width == 0 {
            return Ok(0);
        }
        let offset = self.position % 8;
        let start = self.position / 8;
        let bytes = (offset + usize::from(width)).div_ceil(8);
        let end = start
            .checked_add(bytes)
            .ok_or_else(|| Error("MAR345 bit position overflow".into()))?;
        let source = self
            .data
            .get(start..end)
            .ok_or_else(|| Error("truncated MAR345 PCK field/block".into()))?;
        let mut word = 0u64;
        for (n, &byte) in source.iter().enumerate() {
            word |= u64::from(byte) << (8 * n);
        }
        self.position = self
            .position
            .checked_add(usize::from(width))
            .ok_or_else(|| Error("MAR345 bit position overflow".into()))?;
        Ok(((word >> offset) & ((1u64 << width) - 1)) as u32)
    }
}
fn decode_v1(encoded: &[u8], width: usize, output: &mut [u32]) -> Result<(), Error> {
    let mut bits = Bits {
        data: encoded,
        position: 0,
    };
    let mut index = 0usize;
    while index < output.len() {
        let header = bits.take(6)?;
        let count = 1usize << (header & 7);
        let size = [0u8, 4, 5, 6, 7, 8, 16, 32][(header >> 3) as usize];
        if count > output.len() - index {
            return Err(Error(
                "MAR345 PCK block exceeds declared pixel count".into(),
            ));
        }
        for _ in 0..count {
            let raw = bits.take(size)?;
            let difference = if size == 0 {
                0
            } else {
                ((raw << (32 - size)) as i32) >> (32 - size)
            };
            let prediction = if index == 0 {
                0
            } else if index <= width {
                i32::from(output[index - 1] as i16)
            } else {
                // This flattening at row boundaries and C-style truncation are
                // properties of the original PCK predictor, not display geometry.
                (i32::from(output[index - 1] as i16)
                    + i32::from(output[index - width - 1] as i16)
                    + i32::from(output[index - width] as i16)
                    + i32::from(output[index - width + 1] as i16)
                    + 2)
                    / 4
            };
            output[index] = u32::from((i64::from(difference) + i64::from(prediction)) as u16);
            index += 1;
        }
    }
    let used = bits.position.div_ceil(8);
    if used != encoded.len()
        || (!bits.position.is_multiple_of(8) && encoded[used - 1] >> (bits.position % 8) != 0)
    {
        return Err(Error(
            "MAR345 PCK has trailing bytes/nonzero padding bits".into(),
        ));
    }
    Ok(())
}
