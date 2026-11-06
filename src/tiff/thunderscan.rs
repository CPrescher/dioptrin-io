//! Original libtiff ThunderScan rows with explicit source availability.
//! A failed native row is discarded, including libtiff's internal zero fill.
use super::{Error, Limits, Metadata, TagValue};
use crate::{PixelBuffer, source::Snapshot};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSpan {
    pub start_row: usize,
    pub rows: usize,
    pub sample_offset: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingRows {
    pub start_row: usize,
    pub rows: usize,
    pub strip: usize,
    pub reason: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrity {
    Complete,
    InputExhausted,
    InvalidRow,
    TrailingCodedData,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripStatus {
    pub offset: u64,
    pub declared_bytes: usize,
    pub consumed_bytes: usize,
    pub requested_rows: usize,
    pub complete_rows: usize,
    pub integrity: Integrity,
}
/// Concatenated validated source rows only. Shape is the declared source shape;
/// spans map the shorter pixel vector into it. Missing rows have no samples.
#[derive(Debug, Default)]
pub struct AvailableFrame {
    shape: Option<[usize; 2]>,
    pixels: PixelBuffer,
    available: Vec<RowSpan>,
    missing: Vec<MissingRows>,
    strips: Vec<StripStatus>,
}
impl AvailableFrame {
    #[must_use]
    pub fn shape(&self) -> Option<[usize; 2]> {
        self.shape
    }
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        self.shape.map(|_| &self.pixels)
    }
    #[must_use]
    pub fn available_rows(&self) -> Option<&[RowSpan]> {
        self.shape.map(|_| self.available.as_slice())
    }
    #[must_use]
    pub fn missing_rows(&self) -> Option<&[MissingRows]> {
        self.shape.map(|_| self.missing.as_slice())
    }
    #[must_use]
    pub fn strips(&self) -> Option<&[StripStatus]> {
        self.shape.map(|_| self.strips.as_slice())
    }
    #[must_use]
    pub fn complete(&self) -> bool {
        self.shape.is_some()
            && self.missing.is_empty()
            && self
                .strips
                .iter()
                .all(|s| s.integrity == Integrity::Complete)
    }
    pub fn invalidate(&mut self) {
        self.shape = None;
    }
    pub(super) fn into_pixels(mut self) -> PixelBuffer {
        std::mem::take(&mut self.pixels)
    }
}
unsafe extern "C" {
    fn dioptrin_thunderscan_row(
        input: *const u8,
        input_len: usize,
        width: u32,
        output: *mut u8,
        output_len: usize,
        consumed: *mut usize,
    ) -> std::ffi::c_int;
}
fn entries(page: &Metadata, tag: u16) -> Result<&[TagValue], Error> {
    match page.tags.get(&tag) {
        Some(TagValue::List(v)) => Ok(v),
        Some(v) => Ok(std::slice::from_ref(v)),
        None => Err(Error(format!("missing ThunderScan tag{tag}"))),
    }
}
fn io(e: std::io::Error) -> Error {
    Error(e.to_string())
}
pub(super) fn decode(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limits: Limits,
    frame: &mut AvailableFrame,
) -> Result<(), Error> {
    frame.invalidate();
    if page.compression != 32809 {
        return Err(Error(
            "available-row API requires ThunderScan source".into(),
        ));
    }
    snapshot.check_handle(identity).map_err(Error)?;
    let offsets = entries(page, 273)?;
    let lengths = entries(page, 279)?;
    let rows = page
        .tags
        .get(&278)
        .map(|v| v.clone().into_u64())
        .transpose()?
        .unwrap_or(page.shape[0] as u64)
        .min(page.shape[0] as u64) as usize;
    if rows == 0 || offsets.len() != page.shape[0].div_ceil(rows) || offsets.len() != lengths.len()
    {
        return Err(Error("invalid ThunderScan strip geometry".into()));
    }
    let width = page.shape[1];
    let native_width = u32::try_from(width)
        .ok()
        .filter(|_| width <= isize::MAX as usize - 64)
        .ok_or_else(|| Error("ThunderScan width exceeds native signed arithmetic bound".into()))?;
    let packed_width = width.div_ceil(2);
    if packed_width >= limits.intermediate_bytes {
        return Err(Error(
            "ThunderScan row scratch exceeds intermediate limit".into(),
        ));
    }
    if !matches!(frame.pixels, PixelBuffer::U8(_)) {
        frame.pixels = PixelBuffer::U8(Vec::new());
    }
    let PixelBuffer::U8(pixels) = &mut frame.pixels else {
        unreachable!()
    };
    pixels.clear();
    pixels
        .try_reserve_exact(page.shape[0] * width)
        .map_err(|e| Error(e.to_string()))?;
    frame.available.clear();
    frame.missing.clear();
    frame.strips.clear();
    let mut source = File::open(&snapshot.path).map_err(io)?;
    snapshot.check_handle(&source).map_err(Error)?;
    let file_len = source.metadata().map_err(io)?.len();
    let mut encoded = Vec::new();
    let mut packed = Vec::new();
    for (strip, (offset, length)) in offsets.iter().zip(lengths).enumerate() {
        let offset = offset.clone().into_u64()?;
        let length = usize::try_from(length.clone().into_u64()?)
            .map_err(|_| Error("ThunderScan strip length overflow".into()))?;
        if length
            .checked_add(packed_width)
            .is_none_or(|n| n > limits.intermediate_bytes)
            || length > isize::MAX as usize
        {
            return Err(Error(
                "ThunderScan scratch exceeds intermediate limit".into(),
            ));
        }
        if offset
            .checked_add(length as u64)
            .is_none_or(|n| n > file_len)
        {
            return Err(Error(
                "ThunderScan declared window exceeds source bounds".into(),
            ));
        }
        source.seek(SeekFrom::Start(offset)).map_err(io)?;
        encoded.clear();
        encoded
            .try_reserve_exact(length)
            .map_err(|e| Error(e.to_string()))?;
        encoded.resize(length, 0);
        source.read_exact(&mut encoded).map_err(io)?;
        packed.clear();
        packed
            .try_reserve_exact(packed_width)
            .map_err(|e| Error(e.to_string()))?;
        packed.resize(packed_width, 0);
        let start_row = strip * rows;
        let requested = rows.min(page.shape[0] - start_row);
        let mut consumed = 0usize;
        let mut complete = 0usize;
        let mut integrity = Integrity::Complete;
        let sample_offset = pixels.len();
        for _ in 0..requested {
            let mut used = 0usize;
            // Source metadata admitted one 4-bit plane and positive u32 width; both
            // bounded slices remain alive. Native output is exactly ceil(width/2).
            let success = unsafe {
                dioptrin_thunderscan_row(
                    encoded[consumed..].as_ptr(),
                    encoded.len() - consumed,
                    native_width,
                    packed.as_mut_ptr(),
                    packed.len(),
                    &mut used,
                )
            };
            if success == -1 {
                return Err(Error(
                    "ThunderScan native row admission contract violated".into(),
                ));
            }
            if used > encoded.len() - consumed {
                return Err(Error(
                    "ThunderScan native consumed-byte contract violated".into(),
                ));
            }
            consumed += used;
            if success != 1 {
                integrity = if success == 0 && consumed == encoded.len() {
                    Integrity::InputExhausted
                } else {
                    Integrity::InvalidRow
                };
                break;
            }
            for x in 0..width {
                pixels.push((packed[x / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 15);
            }
            complete += 1;
        }
        if complete > 0 {
            if let Some(previous) = frame.available.last_mut().filter(|p| {
                p.start_row + p.rows == start_row
                    && p.sample_offset + p.rows * width == sample_offset
            }) {
                previous.rows += complete;
            } else {
                frame.available.push(RowSpan {
                    start_row,
                    rows: complete,
                    sample_offset,
                });
            }
        }
        if complete < requested {
            frame.missing.push(MissingRows {start_row:start_row+complete,rows:requested-complete,strip,reason:format!("native ThunderScan row validation failed: {integrity:?}; failed-row padding discarded")});
        } else if encoded[consumed..]
            .iter()
            .all(|&b| [0, 0x6a, 0xa4].contains(&b))
        {
            consumed = encoded.len();
        } else {
            integrity = Integrity::TrailingCodedData;
        }
        frame.strips.push(StripStatus {
            offset,
            declared_bytes: length,
            consumed_bytes: consumed,
            requested_rows: requested,
            complete_rows: complete,
            integrity,
        });
    }
    snapshot.check_handle(&source).map_err(Error)?;
    snapshot.check_handle(identity).map_err(Error)?;
    frame.shape = Some(page.shape);
    Ok(())
}
