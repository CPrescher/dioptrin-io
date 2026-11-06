//! Explicit metadata-only catalog of the primary and SubIFD directory graph.
//! A catalog verifies directory metadata, not the availability of pixel data.
use super::{Admission, Error, Limits, OpaqueTag, Options, TagValue, compatibility::SourceEntry};
use crate::source::Snapshot;
use std::{collections::BTreeMap, fs::File, io::BufReader, path::Path};
use tiff_backend::{
    decoder::{DirectoryReader, Limits as BackendLimits},
    tags::IfdPointer,
};

#[derive(Clone, Debug)]
pub struct Directory {
    pub ifd_offset: u64,
    /// Zero means there is no NextIFD edge. This edge is retained for child
    /// chains too; it does not promote a child to the primary page chain.
    pub next_ifd: u64,
    /// Original ordered SubIFDs slots, including null slots and shared aliases.
    pub subifd_offsets: Vec<u64>,
    pub subifd_entry: Option<SourceEntry>,
    pub primary_index: Option<usize>,
    pub tags: BTreeMap<u16, TagValue>,
    pub opaque_tags: BTreeMap<u16, OpaqueTag>,
}
pub struct Catalog {
    snapshot: Snapshot,
    identity: File,
    primary_offsets: Vec<u64>,
    directories: Vec<Directory>,
}
impl Catalog {
    /// Traverse only source-declared NextIFD/SubIFDs edges. `limits.frames`
    /// bounds all unique directories, including unsupported-image directories.
    /// Shared aliases are retained; cycles and overlapping tables are rejected.
    /// # Errors
    /// Invalid metadata, source/resource bounds or a changed source.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        if limits.frames == 0 || limits.metadata_bytes == 0 {
            return Err(Error("TIFF catalog limits must be positive".into()));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if !std::fs::metadata(&snapshot.path)
            .map_err(|e| Error(e.to_string()))?
            .is_file()
        {
            return Err(Error("TIFF catalogs require a regular source file".into()));
        }
        let file = File::open(&snapshot.path).map_err(|e| Error(e.to_string()))?;
        let identity = file.try_clone().map_err(|e| Error(e.to_string()))?;
        snapshot.check_handle(&identity).map_err(Error)?;
        let Admission {
            file,
            first_ifd,
            links,
            entry_counts,
            photos,
            mut opaque,
            mut retained,
            ..
        } = Admission::graph(file, limits, Options::default())?;
        let mut native = BackendLimits::default();
        native.ifd_value_size = limits.metadata_bytes;
        let mut backend = DirectoryReader::new(BufReader::new(file), native)?;
        let mut primary_offsets = Vec::new();
        let mut offset = first_ifd;
        while offset != 0 {
            primary_offsets.push(offset);
            offset = links[&offset].next;
        }
        let primary_indices: BTreeMap<_, _> = primary_offsets
            .iter()
            .enumerate()
            .map(|(i, &p)| (p, i))
            .collect();
        let mut directories = Vec::new();
        directories
            .try_reserve_exact(links.len())
            .map_err(|e| Error(e.to_string()))?;
        for (offset, link) in links {
            let directory = backend.read_directory(IfdPointer(offset))?;
            if directory.iter().count() as u64 != entry_counts[&offset] {
                return Err(Error(
                    "unsupported TIFF metadata field type or duplicate tag ID".into(),
                ));
            }
            for (_, entry) in directory.iter() {
                retained = entry
                    .count()
                    .checked_mul(40)
                    .and_then(|n| usize::try_from(n).ok())
                    .and_then(|n| n.checked_add(128))
                    .and_then(|n| retained.checked_add(n))
                    .filter(|&n| n <= limits.metadata_bytes)
                    .ok_or_else(|| Error("TIFF catalog metadata budget".into()))?;
            }
            let mut tags: BTreeMap<_, _> = backend
                .directory_tags(&directory)
                .tag_iter()
                .map(|value| value.map(|(tag, value)| (tag.to_u16(), value)))
                .collect::<Result<_, _>>()?;
            if let Some(photo) = photos.get(&offset) {
                tags.insert(262, photo.typed.clone());
            }
            let opaque_tags = opaque.remove(&offset).unwrap_or_default();
            for tag in opaque_tags.keys() {
                tags.remove(tag);
            }
            directories.push(Directory {
                ifd_offset: offset,
                next_ifd: link.next,
                subifd_offsets: link.children,
                subifd_entry: link.subifd_entry,
                primary_index: primary_indices.get(&offset).copied(),
                tags,
                opaque_tags,
            });
        }
        snapshot.check_handle(&identity).map_err(Error)?;
        Ok(Self {
            snapshot,
            identity,
            primary_offsets,
            directories,
        })
    }
    /// Checked source-order primary chain, without promoted SubIFDs.
    /// # Errors
    /// The original source has changed.
    pub fn primary_offsets(&self) -> Result<&[u64], Error> {
        self.snapshot.check_handle(&self.identity).map_err(Error)?;
        Ok(&self.primary_offsets)
    }
    /// Checked catalog nodes sorted by source IFD offset.
    /// # Errors
    /// The original source has changed.
    pub fn directories(&self) -> Result<&[Directory], Error> {
        self.snapshot.check_handle(&self.identity).map_err(Error)?;
        Ok(&self.directories)
    }
}
