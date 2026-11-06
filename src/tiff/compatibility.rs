//! Explicit, bounded interpretations of legacy TIFF chunk metadata.
use super::{Error, Limits};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
};
use tiff_backend::decoder::{ChunkLayoutOverride, ChunkType};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UncompressedWindows {
    #[default]
    Declared,
    FromShape,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TileTables {
    #[default]
    Declared,
    FromStripTags,
}
/// Caller-selected interpretation of ambiguous table-only old-JPEG tile resets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OjpegTiles {
    /// Require geometry and restart interpretation established by source declarations.
    #[default]
    Declared,
    /// The caller asserts that every tile is an independently reset JPEG chunk.
    IndependentResetTiles,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    pub uncompressed_windows: UncompressedWindows,
    pub tile_tables: TileTables,
    pub ojpeg_tiles: OjpegTiles,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub tag: u16,
    pub field_type: u16,
    pub count: u64,
    pub entry_offset: u64,
    /// Original complete classic/BigTIFF descriptor, including literal value/offset.
    pub ifd_entry: Vec<u8>,
    pub payload_offset: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    UncompressedWindowsFromShape,
    TileTablesFromStripTags,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    Strips,
    Tiles,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub offset: u64,
    pub bytes: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interpretation {
    pub source_ifd_offset: u64,
    pub source_entries: Vec<SourceEntry>,
    pub operations: Vec<Operation>,
    pub kind: ChunkKind,
    pub rows_per_strip: u32,
    pub windows: Vec<Window>,
}
impl Interpretation {
    pub(super) fn backend(&self) -> ChunkLayoutOverride {
        ChunkLayoutOverride {
            kind: if self.kind == ChunkKind::Tiles {
                ChunkType::Tile
            } else {
                ChunkType::Strip
            },
            offsets: self.windows.iter().map(|w| w.offset).collect(),
            byte_counts: self.windows.iter().map(|w| w.bytes).collect(),
            rows_per_strip: self.rows_per_strip,
            strict_lzw: true,
        }
    }
    pub(super) fn retained_bytes(&self) -> Option<usize> {
        self.source_entries
            .len()
            .checked_mul(128)?
            .checked_add(self.windows.len().checked_mul(64)?)?
            .checked_add(256)
    }
}
fn io(e: std::io::Error) -> Error {
    Error(e.to_string())
}
fn number(bytes: &[u8], little: bool) -> u64 {
    bytes.iter().enumerate().fold(0, |v, (i, &b)| {
        v | (u64::from(b) << (8 * if little { i } else { bytes.len() - 1 - i }))
    })
}
fn values(
    file: &mut File,
    source: &BTreeMap<u16, &SourceEntry>,
    tag: u16,
    little: bool,
    limits: Limits,
) -> Result<Option<Vec<u64>>, Error> {
    let Some(entry) = source.get(&tag) else {
        return Ok(None);
    };
    let unit = match entry.field_type {
        1 => 1,
        3 => 2,
        4 => 4,
        16 => 8,
        _ => {
            return Err(Error(format!(
                "compatibility tag{tag} requires original unsigned integer type"
            )));
        }
    };
    if entry
        .count
        .checked_mul(16)
        .is_none_or(|n| n > limits.metadata_bytes as u64 / 4)
    {
        return Err(Error(
            "TIFF compatibility integer values exceed metadata scratch budget".into(),
        ));
    }
    let size = entry
        .count
        .checked_mul(unit)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|&n| n <= limits.metadata_bytes / 4)
        .ok_or_else(|| Error("TIFF compatibility tag exceeds metadata budget".into()))?;
    let mut raw = vec![0; size];
    if let Some(offset) = entry.payload_offset {
        file.seek(SeekFrom::Start(offset)).map_err(io)?;
        file.read_exact(&mut raw).map_err(io)?;
    } else {
        let start = entry.ifd_entry.len() - if entry.ifd_entry.len() == 12 { 4 } else { 8 };
        raw.copy_from_slice(&entry.ifd_entry[start..start + size]);
    }
    Ok(Some(
        raw.chunks_exact(unit as usize)
            .map(|b| number(b, little))
            .collect(),
    ))
}
fn scalar(
    file: &mut File,
    source: &BTreeMap<u16, &SourceEntry>,
    tag: u16,
    little: bool,
    limits: Limits,
    default: Option<u64>,
) -> Result<u64, Error> {
    match values(file, source, tag, little, limits)? {
        Some(v) if v.len() == 1 => Ok(v[0]),
        None => default.ok_or_else(|| Error(format!("missing TIFF compatibility tag{tag}"))),
        _ => Err(Error(format!("TIFF compatibility tag{tag} must be scalar"))),
    }
}
pub(super) fn propose(
    file: &mut File,
    ifd: u64,
    entries: Vec<SourceEntry>,
    little: bool,
    options: Options,
    limits: Limits,
) -> Result<Option<Interpretation>, Error> {
    let source: BTreeMap<_, _> = entries.iter().map(|e| (e.tag, e)).collect();
    if source.len() != entries.len() {
        return Err(Error(
            "duplicate TIFF source entry under compatibility policy".into(),
        ));
    }
    let compression = scalar(file, &source, 259, little, limits, Some(1))?;
    let mixed = options.tile_tables == TileTables::FromStripTags
        && source.contains_key(&322)
        && source.contains_key(&323)
        && source.contains_key(&273)
        && source.contains_key(&279)
        && !source.contains_key(&324)
        && !source.contains_key(&325);
    let compute =
        options.uncompressed_windows == UncompressedWindows::FromShape && compression == 1;
    if !mixed && !compute {
        return Ok(None);
    }
    let width = scalar(file, &source, 256, little, limits, None)?;
    let height = scalar(file, &source, 257, little, limits, None)?;
    let samples = scalar(file, &source, 277, little, limits, Some(1))?;
    let planar = scalar(file, &source, 284, little, limits, Some(1))?;
    if width == 0
        || height == 0
        || width > u64::from(u32::MAX)
        || height > u64::from(u32::MAX)
        || samples == 0
        || ![1, 2].contains(&planar)
    {
        return Err(Error(
            "invalid TIFF compatibility source shape/planes".into(),
        ));
    }
    let planes = if planar == 2 { samples } else { 1 };
    let source_rows = scalar(file, &source, 278, little, limits, Some(height))?;
    if source_rows == 0 || source_rows > u64::from(u32::MAX) {
        return Err(Error("invalid TIFF compatibility RowsPerStrip".into()));
    }
    let rows = source_rows.min(height);
    let has_tiles = source.contains_key(&324) || source.contains_key(&325);
    let tiled = mixed || has_tiles;
    let (chunk_width, chunk_height) = if tiled {
        (
            scalar(file, &source, 322, little, limits, None)?,
            scalar(file, &source, 323, little, limits, None)?,
        )
    } else {
        (width, rows)
    };
    if chunk_width == 0
        || chunk_height == 0
        || chunk_width > u64::from(u32::MAX)
        || chunk_height > u64::from(u32::MAX)
    {
        return Err(Error("invalid TIFF compatibility chunk geometry".into()));
    }
    let across = width.div_ceil(chunk_width);
    let down = height.div_ceil(chunk_height);
    let count = across
        .checked_mul(down)
        .and_then(|n| n.checked_mul(planes))
        .and_then(|n| usize::try_from(n).ok())
        .filter(|&n| n > 0 && n <= limits.metadata_bytes / 64)
        .ok_or_else(|| Error("TIFF compatibility chunk count exceeds budget".into()))?;
    let offset_tag = if tiled && !mixed { 324 } else { 273 };
    let count_tag = if tiled && !mixed { 325 } else { 279 };
    let offsets = values(file, &source, offset_tag, little, limits)?
        .ok_or_else(|| Error("missing TIFF compatibility source offsets".into()))?;
    let declared = values(file, &source, count_tag, little, limits)?;
    if offsets.len() != count || declared.as_ref().is_some_and(|n| n.len() != count) {
        return Err(Error(
            "ambiguous TIFF compatibility chunk tables/geometry".into(),
        ));
    }
    if mixed
        && height
            .div_ceil(rows)
            .checked_mul(planes)
            .is_none_or(|strips| strips == count as u64)
    {
        return Err(Error(
            "TIFF strip/tile reinterpretation is ambiguous".into(),
        ));
    }
    let mut operations = Vec::new();
    if mixed {
        operations.push(Operation::TileTablesFromStripTags);
    }
    let counts = if compute {
        let bits = values(file, &source, 258, little, limits)?.unwrap_or_else(|| vec![1]);
        if bits.is_empty() || bits.iter().any(|&b| b == 0 || b != bits[0]) {
            return Err(Error(
                "TIFF compatibility needs uniform source sample bits".into(),
            ));
        }
        let stored = if planar == 2 { 1 } else { samples };
        let row = chunk_width
            .checked_mul(bits[0])
            .and_then(|n| n.checked_mul(stored))
            .and_then(|n| n.checked_add(7))
            .map(|n| n / 8)
            .ok_or_else(|| Error("TIFF compatibility source row overflow".into()))?;
        let mut required = Vec::new();
        required
            .try_reserve_exact(count)
            .map_err(|e| Error(e.to_string()))?;
        for index in 0..count {
            let y = ((index as u64 / across) % down) * chunk_height;
            let lines = if tiled {
                chunk_height
            } else {
                chunk_height.min(height - y)
            };
            let bytes = row
                .checked_mul(lines)
                .filter(|&n| n > 0 && n <= limits.intermediate_bytes as u64)
                .ok_or_else(|| Error("computed TIFF window exceeds intermediate budget".into()))?;
            required.push(bytes);
        }
        if declared
            .as_ref()
            .is_none_or(|v| v.iter().zip(&required).any(|(a, b)| a < b))
        {
            operations.push(Operation::UncompressedWindowsFromShape);
            required
        } else {
            declared.unwrap()
        }
    } else {
        declared.ok_or_else(|| Error("missing TIFF compatibility bytecounts".into()))?
    };
    if operations.is_empty() {
        return Ok(None);
    }
    let windows = offsets
        .into_iter()
        .zip(counts)
        .map(|(offset, bytes)| Window { offset, bytes })
        .collect();
    Ok(Some(Interpretation {
        source_ifd_offset: ifd,
        source_entries: entries,
        operations,
        kind: if tiled {
            ChunkKind::Tiles
        } else {
            ChunkKind::Strips
        },
        rows_per_strip: rows as u32,
        windows,
    }))
}
/// Recovery windows must be complete, mutually disjoint and disjoint from all
/// original IFD/header/tag payload bytes, including the adapter's metadata views.
pub(super) fn validate_windows(
    value: &Interpretation,
    length: u64,
    metadata: &[(u64, u64)],
) -> Result<(), Error> {
    let mut ranges = Vec::new();
    for window in &value.windows {
        let end = window
            .offset
            .checked_add(window.bytes)
            .filter(|&n| window.bytes > 0 && n <= length)
            .ok_or_else(|| Error("effective TIFF sample window exceeds source bounds".into()))?;
        let index = metadata.partition_point(|&(a, _)| a < end);
        if index > 0 && metadata[index - 1].1 > window.offset {
            return Err(Error(
                "effective TIFF sample window overlaps source metadata".into(),
            ));
        }
        ranges.push((window.offset, end));
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|v| v[0].1 > v[1].0) {
        return Err(Error("effective TIFF sample windows overlap".into()));
    }
    Ok(())
}
