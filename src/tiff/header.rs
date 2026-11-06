//! Header-only indexing through the existing TIFF IFD/value reader. No sample
//! codec is selected and no pixel buffer is allocated or declared available.
use super::{Admission, Error, Limits, OpaqueTag, Options, TagValue};
use crate::source::Snapshot;
use std::{collections::BTreeMap, fs::File, io::BufReader, path::Path};
use tiff_backend::decoder::{DirectoryReader, Limits as BackendLimits};

#[derive(Debug, Clone)]
pub struct Page {
    pub ifd_offset: u64,
    pub tags: BTreeMap<u16, TagValue>,
    pub opaque_tags: BTreeMap<u16, OpaqueTag>,
}
pub struct Reader {
    snapshot: Snapshot,
    identity: File,
    pages: Vec<Page>,
}
impl Reader {
    /// Index original primary IFDs independently of pixel codec/layout support.
    /// This verifies metadata, not sample availability or image completeness.
    /// # Errors
    /// Invalid/cyclic/out-of-bounds metadata, budget exhaustion or source change.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        if limits.frames == 0 || limits.metadata_bytes == 0 {
            return Err(Error("TIFF header limits must be positive".into()));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let file = File::open(&snapshot.path).map_err(|e| Error(e.to_string()))?;
        let identity = file.try_clone().map_err(|e| Error(e.to_string()))?;
        snapshot.check_handle(&identity).map_err(Error)?;
        // Reuse the existing source-range/cycle admission and opaque-tag rules.
        let Admission {
            file,
            photos,
            mut opaque,
            retained,
            entry_counts,
            ..
        } = Admission::open(file, limits, Options::default())?;
        let mut backend = BackendLimits::default();
        backend.ifd_value_size = limits.metadata_bytes;
        let mut reader = DirectoryReader::new(BufReader::new(file), backend)?;
        let mut pages = Vec::new();
        let mut retained = retained;
        while let Some((pointer, directory)) = reader.next_directory()? {
            if directory.iter().count() as u64 != entry_counts[&pointer.0] {
                return Err(Error(
                    "unsupported TIFF metadata field type or duplicate tag ID".into(),
                ));
            }
            if pages.len() >= limits.frames {
                return Err(Error("TIFF header page limit".into()));
            }
            for (_, entry) in directory.iter() {
                retained = entry
                    .count()
                    .checked_mul(40)
                    .and_then(|n| usize::try_from(n).ok())
                    .and_then(|n| n.checked_add(128))
                    .and_then(|n| retained.checked_add(n))
                    .filter(|&n| n <= limits.metadata_bytes)
                    .ok_or_else(|| Error("TIFF header metadata budget".into()))?;
            }
            let mut tags: BTreeMap<_, _> = reader
                .directory_tags(&directory)
                .tag_iter()
                .map(|value| value.map(|(tag, value)| (tag.to_u16(), value)))
                .collect::<Result<_, _>>()?;
            if let Some(photo) = photos.get(&pointer.0) {
                tags.insert(262, photo.typed.clone());
            }
            let opaque_tags = opaque.remove(&pointer.0).unwrap_or_default();
            for tag in opaque_tags.keys() {
                tags.remove(tag);
            }
            pages.push(Page {
                ifd_offset: pointer.0,
                tags,
                opaque_tags,
            });
        }
        snapshot.check_handle(&identity).map_err(Error)?;
        Ok(Self {
            snapshot,
            identity,
            pages,
        })
    }
    /// Return frozen metadata only while the original source is unchanged.
    /// # Errors
    /// Original source replaced, truncated or modified after indexing.
    pub fn pages(&self) -> Result<&[Page], Error> {
        self.snapshot.check_handle(&self.identity).map_err(Error)?;
        Ok(&self.pages)
    }
}
