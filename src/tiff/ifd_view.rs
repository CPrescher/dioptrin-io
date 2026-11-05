//! Bounded read-only metadata adaptation for the mature TIFF sample decoder.
//! Only explicit WhiteIsZero/palette photometric values are presented as gray;
//! source samples and all exposed source metadata remain unchanged.
use super::{Error, Limits, OpaqueTag, TagValue};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::Arc,
};
#[derive(Clone, Debug)]
pub(super) struct OriginalPhoto {
    pub value: u16,
    pub typed: TagValue,
}
pub(super) type ChunkWindows = Arc<[(u64, u64)]>;
pub(super) struct FileView {
    file: File,
    patches: BTreeMap<u64, [u8; 2]>,
    samples: Option<(ChunkWindows, Option<u64>)>,
}
pub(super) struct Admission {
    pub file: FileView,
    pub photos: BTreeMap<u64, OriginalPhoto>,
    pub opaque: BTreeMap<u64, BTreeMap<u16, OpaqueTag>>,
    pub retained: usize,
}
impl Admission {
    pub fn open(mut file: File, limits: Limits) -> Result<Self, Error> {
        let length = file.metadata().map_err(io)?.len();
        let mut header = [0; 8];
        file.read_exact(&mut header).map_err(io)?;
        let little = match &header[..2] {
            b"II" => true,
            b"MM" => false,
            _ => return Err(Error("invalid TIFF byte order".into())),
        };
        let word = |b: [u8; 2]| {
            if little {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            }
        };
        let number = |bytes: &[u8]| {
            bytes.iter().enumerate().fold(0u64, |n, (i, &byte)| {
                n | (u64::from(byte) << (8 * if little { i } else { bytes.len() - 1 - i }))
            })
        };
        let (count_bytes, stride, offset_bytes, mut next) = match word([header[2], header[3]]) {
            42 => (2u64, 12u64, 4u64, number(&header[4..8])),
            43 => {
                if word([header[4], header[5]]) != 8 || header[6..8] != [0, 0] {
                    return Err(Error("unsupported BigTIFF offset layout".into()));
                }
                let mut first = [0; 8];
                file.read_exact(&mut first).map_err(io)?;
                (8, 20, 8, number(&first))
            }
            _ => return Err(Error("invalid TIFF version".into())),
        };
        let mut visited = BTreeSet::new();
        let mut patches = BTreeMap::new();
        let mut photos = BTreeMap::new();
        let mut opaque: BTreeMap<u64, BTreeMap<u16, OpaqueTag>> = BTreeMap::new();
        let mut payloads = Vec::new();
        let mut tables: BTreeMap<u64, u64> = BTreeMap::new();
        let mut retained = 0usize;
        while next != 0 {
            if visited.len() >= limits.frames || !visited.insert(next) {
                return Err(Error("TIFF primary IFD cycle/frame limit".into()));
            }
            retained = retained
                .checked_add(96)
                .filter(|&n| n <= limits.metadata_bytes)
                .ok_or_else(|| Error("TIFF IFD adapter exceeds metadata budget".into()))?;
            let mut count = [0; 8];
            read_at(&mut file, next, &mut count[..count_bytes as usize])?;
            let count = number(&count[..count_bytes as usize]);
            let entries = next
                .checked_add(count_bytes)
                .ok_or_else(|| Error("TIFF IFD offset overflow".into()))?;
            let end = count
                .checked_mul(stride)
                .and_then(|n| entries.checked_add(n))
                .filter(|&n| n.checked_add(offset_bytes).is_some_and(|end| end <= length))
                .ok_or_else(|| Error("TIFF IFD table exceeds source bounds".into()))?;
            let table_end = end + offset_bytes;
            if tables
                .range(..table_end)
                .next_back()
                .is_some_and(|(_, &stop)| stop > next)
            {
                return Err(Error("overlapping TIFF IFD tables".into()));
            }
            tables.insert(next, table_end);
            if count > limits.metadata_bytes as u64 / 24 {
                return Err(Error("TIFF IFD entry count exceeds metadata budget".into()));
            }
            let mut photo_seen = false;
            let mut description_seen = false;
            for index in 0..count {
                let pos = entries + index * stride;
                let mut entry = [0; 20];
                read_at(&mut file, pos, &mut entry[..stride as usize])?;
                let tag = word([entry[0], entry[1]]);
                let ty = word([entry[2], entry[3]]);
                let (count_end, value_pos) = if count_bytes == 2 {
                    (8usize, 8u64)
                } else {
                    (12usize, 12u64)
                };
                let n = number(&entry[4..count_end]);
                let unit = match ty {
                    1 | 2 | 6 | 7 => 1,
                    3 | 8 => 2,
                    4 | 9 | 11 | 13 => 4,
                    5 | 10 | 12 | 16 | 17 | 18 => 8,
                    _ => 0,
                };
                let size = n
                    .checked_mul(unit)
                    .ok_or_else(|| Error("TIFF tag byte count overflow".into()))?;
                if unit != 0 && size > offset_bytes {
                    let start = number(&entry[value_pos as usize..stride as usize]);
                    let stop = start
                        .checked_add(size)
                        .filter(|&end| end <= length)
                        .ok_or_else(|| Error("TIFF tag payload exceeds source bounds".into()))?;
                    if payloads.len() >= limits.metadata_bytes / 24 {
                        return Err(Error(
                            "TIFF tag-range admission exceeds metadata budget".into(),
                        ));
                    }
                    payloads.push((start, stop));
                }
                if tag == 270 {
                    if description_seen {
                        return Err(Error("duplicate TIFF ImageDescription".into()));
                    }
                    description_seen = true;
                    if ty == 2 && n >= 1 && n <= offset_bytes && {
                        let value = &entry[value_pos as usize..value_pos as usize + n as usize];
                        !value.is_ascii() || !value.ends_with(&[0])
                    } {
                        retained = retained
                            .checked_add(256)
                            .filter(|&n| n <= limits.metadata_bytes)
                            .ok_or_else(|| Error("opaque TIFF metadata exceeds budget".into()))?;
                        // Declare undefined bytes only to the backend. Exposed
                        // metadata retains the original ASCII descriptor below.
                        patches.insert(
                            pos + 2,
                            if little {
                                7u16.to_le_bytes()
                            } else {
                                7u16.to_be_bytes()
                            },
                        );
                        opaque.entry(next).or_default().insert(tag,OpaqueTag {field_type:ty,count:n,ifd_entry:entry[..stride as usize].to_vec(),value_bytes:entry[value_pos as usize..value_pos as usize+n as usize].to_vec(),reason:"declared inline ASCII ImageDescription is non-ASCII or lacks its required NUL; retained opaque without text/offset repair".into()});
                    }
                }
                if tag == 262 {
                    if photo_seen {
                        return Err(Error("duplicate TIFF PhotometricInterpretation".into()));
                    }
                    photo_seen = true;
                    if ty == 3 && n == 1 {
                        let value =
                            word([entry[value_pos as usize], entry[value_pos as usize + 1]]);
                        if [0, 3, 32803].contains(&value) {
                            patches.insert(
                                pos + value_pos,
                                if little {
                                    1u16.to_le_bytes()
                                } else {
                                    1u16.to_be_bytes()
                                },
                            );
                            photos.insert(
                                next,
                                OriginalPhoto {
                                    value,
                                    typed: TagValue::Short(value),
                                },
                            );
                        }
                    }
                }
            }
            let mut offset = [0; 8];
            read_at(&mut file, end, &mut offset[..offset_bytes as usize])?;
            next = number(&offset[..offset_bytes as usize]);
        }
        // Coalesce source tag intervals once; patch admission must not scan
        // every payload for every IFD in a metadata-heavy source.
        coalesce_ranges(&mut payloads);
        for &offset in patches.keys() {
            if overlaps_ranges(&payloads, offset, offset + 2) {
                return Err(Error(
                    "TIFF adapted tag overlaps source metadata payload".into(),
                ));
            }
        }
        file.seek(SeekFrom::Start(0)).map_err(io)?;
        Ok(Self {
            file: FileView {
                file,
                patches,
                samples: None,
            },
            photos,
            opaque,
            retained,
        })
    }
}
impl FileView {
    /// Prevent an intentionally aliased pixel window from observing adapted
    /// metadata bytes as if they were original detector samples.
    pub fn check_chunks(&self, offsets: &[u64], lengths: &[u64]) -> Result<ChunkWindows, Error> {
        if offsets.len() != lengths.len() {
            return Err(Error("TIFF chunk table length mismatch".into()));
        }
        let source_len = self.file.metadata().map_err(io)?.len();
        let mut windows = Vec::with_capacity(offsets.len());
        for (&start, &length) in offsets.iter().zip(lengths) {
            let end = start
                .checked_add(length)
                .filter(|&end| length != 0 && end <= source_len)
                .ok_or_else(|| Error("TIFF chunk exceeds source bounds".into()))?;
            windows.push((start, end));
            if self
                .patches
                .range(start.saturating_sub(1)..end)
                .next()
                .is_some()
            {
                return Err(Error(
                    "TIFF sample chunk overlaps adapted source metadata".into(),
                ));
            }
        }
        windows.sort_unstable();
        Ok(windows.into())
    }
    /// Metadata reads may use adapted bytes; sample reads are strictly bounded
    /// by the selected original declared chunk, for every backend codec.
    pub fn begin_samples(&mut self, windows: ChunkWindows) {
        self.samples = Some((windows, None));
    }
    pub fn end_samples(&mut self) {
        self.samples = None;
    }
}
fn io(error: std::io::Error) -> Error {
    Error(error.to_string())
}
fn read_at(file: &mut File, offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
    file.seek(SeekFrom::Start(offset)).map_err(io)?;
    file.read_exact(bytes).map_err(io)
}
impl Read for FileView {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let start = self.file.stream_position()?;
        let limit = if let Some((_, end)) = &self.samples {
            let end =
                end.ok_or_else(|| std::io::Error::other("TIFF sample read without chunk seek"))?;
            usize::try_from(end.saturating_sub(start))
                .unwrap_or(usize::MAX)
                .min(buffer.len())
        } else {
            buffer.len()
        };
        let count = self.file.read(&mut buffer[..limit])?;
        let end = start + count as u64;
        for (&offset, patch) in self.patches.range(start.saturating_sub(1)..end) {
            for (index, &value) in patch.iter().enumerate() {
                let pos = offset + index as u64;
                if pos >= start && pos < end {
                    buffer[(pos - start) as usize] = value;
                }
            }
        }
        Ok(count)
    }
}
impl Seek for FileView {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        if let Some((windows, end)) = &mut self.samples {
            let SeekFrom::Start(start) = pos else {
                return Err(std::io::Error::other("unsupported TIFF sample-phase seek"));
            };
            let index = windows.partition_point(|&(offset, _)| offset < start);
            *end = Some(
                windows
                    .get(index)
                    .filter(|&&(offset, _)| offset == start)
                    .ok_or_else(|| {
                        std::io::Error::other("TIFF seek outside admitted sample chunks")
                    })?
                    .1,
            );
        }
        self.file.seek(pos)
    }
}

fn coalesce_ranges(ranges: &mut Vec<(u64, u64)>) {
    ranges.sort_unstable();
    let mut kept = 0;
    for index in 0..ranges.len() {
        let (start, end) = ranges[index];
        if kept > 0 && start <= ranges[kept - 1].1 {
            ranges[kept - 1].1 = ranges[kept - 1].1.max(end);
        } else {
            ranges[kept] = (start, end);
            kept += 1;
        }
    }
    ranges.truncate(kept);
}
fn overlaps_ranges(ranges: &[(u64, u64)], start: u64, end: u64) -> bool {
    let count = ranges.partition_point(|&(left, _)| left < end);
    count
        .checked_sub(1)
        .is_some_and(|index| ranges[index].1 > start)
}
#[cfg(test)]
mod interval_tests {
    use super::*;
    #[test]
    fn sorted_admission_agrees_with_independent_source_interval_scan() {
        let mut seed = 0x12345678u64;
        let mut original = Vec::new();
        for _ in 0..500 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let start = (seed >> 32) % 10000;
            original.push((start, start + 1 + (seed % 100)));
        }
        let mut indexed = original.clone();
        coalesce_ranges(&mut indexed);
        for start in (0..10200).step_by(3) {
            let end = start + 2;
            assert_eq!(
                overlaps_ranges(&indexed, start, end),
                original
                    .iter()
                    .any(|&(left, right)| left < end && right > start)
            );
        }
        assert!(!overlaps_ranges(&[(2, 4), (6, 8)], 4, 6));
        assert!(overlaps_ranges(&[(2, 4), (6, 8)], 3, 6));
    }
}
