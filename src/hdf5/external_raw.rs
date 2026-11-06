//! Explicit immutable raw-external storage. HDF5 never opens payload filenames.
use super::{
    Error, FilenameBinding, Limits, PixelBuffer, PixelType, Storage, fill, storage_allocated,
};
use crate::hdf5::sources::Sources;
use hdf5_backend::{Dataset, dataset::Layout};
use hdf5_sys::h5p::{H5Pget_external, H5Pget_external_count};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Copy, Debug, Default)]
pub struct ExternalRawOptions<'a> {
    /// Every stored filename needs one exact, caller-approved local binding.
    pub filenames: &'a [FilenameBinding],
}
#[derive(Clone, Debug)]
pub struct ExternalRawWindow {
    pub stored_filename: String,
    pub source_file: PathBuf,
    pub file_offset: u64,
    /// None is the HDF5 unlimited final reservation, not an unbounded read.
    pub declared_bytes: Option<u64>,
    pub logical_offset: u64,
    /// Frozen bytes used by this dataset, independently of future reservation.
    pub effective_bytes: u64,
}
#[derive(Clone, Debug)]
pub struct ExternalRawMetadata {
    pub windows: Vec<ExternalRawWindow>,
    pub frozen_bytes: u64,
}
pub(super) struct Raw {
    windows: Vec<(usize, ExternalRawWindow)>,
}
pub(super) struct Bindings<'a> {
    options: ExternalRawOptions<'a>,
    used: BTreeSet<&'a str>,
}
impl<'a> Bindings<'a> {
    pub fn new(options: ExternalRawOptions<'a>, limits: Limits) -> Result<Self, Error> {
        if options.filenames.len() > limits.source_files {
            return Err(Error("raw external filename binding limit".into()));
        }
        let mut names = BTreeSet::new();
        let mut bytes = options
            .filenames
            .len()
            .checked_mul(128)
            .filter(|&n| n <= limits.metadata_bytes)
            .ok_or_else(|| Error("raw external binding metadata limit".into()))?;
        for binding in options.filenames {
            bytes = bytes
                .checked_add(binding.stored_filename.len())
                .and_then(|n| n.checked_add(binding.actual_file.as_os_str().len()))
                .ok_or_else(|| Error("raw external binding size overflow".into()))?;
            if binding.stored_filename.is_empty()
                || binding.stored_filename.len() > limits.text_bytes
                || binding.stored_filename.contains('\0')
                || !names.insert(binding.stored_filename.as_str())
                || bytes > limits.metadata_bytes
            {
                return Err(Error(
                    "invalid/duplicate/oversized raw external filename binding".into(),
                ));
            }
        }
        Ok(Self {
            options,
            used: BTreeSet::new(),
        })
    }
    pub fn finish(&self) -> Result<(), Error> {
        if !self.used.is_empty() && self.used.len() != self.options.filenames.len() {
            return Err(Error("unused raw external filename binding".into()));
        }
        Ok(())
    }
}
pub(super) fn admit(
    dataset: &Dataset,
    sources: &mut Sources,
    bindings: &mut Bindings<'_>,
    limits: Limits,
    ty: PixelType,
    bytes: u64,
) -> Result<(Raw, ExternalRawMetadata, Storage), Error> {
    if dataset.layout() != Layout::Contiguous || !dataset.filters().is_empty() {
        return Err(Error(
            "raw external storage requires unfiltered contiguous HDF5 layout".into(),
        ));
    }
    let plist = dataset.dcpl()?;
    let count = hdf5_backend::sync::sync(|| unsafe { H5Pget_external_count(plist.id()) });
    let count =
        usize::try_from(count).map_err(|_| Error("cannot inspect raw external windows".into()))?;
    if count == 0 || count > limits.group_members {
        return Err(Error("raw external window count limit".into()));
    }
    sources.charge(
        count
            .checked_mul(256)
            .ok_or_else(|| Error("raw external metadata overflow".into()))?,
    )?;
    let mut windows = Vec::new();
    let mut logical = 0u64;
    for index in 0..count {
        let (name, offset, declared) = hdf5_backend::sync::sync(|| {
            // One additional byte distinguishes a complete bounded name from
            // a native truncated/NUL-terminated prefix at the caller's limit.
            let bound = limits
                .text_bytes
                .min(limits.metadata_bytes)
                .checked_add(2)
                .ok_or_else(|| Error("raw filename bound overflow".into()))?;
            let mut name = vec![0xffu8; bound];
            let mut offset = 0;
            let mut size = 0;
            // SAFETY: live property list and bounded writable name/offset/size.
            if unsafe {
                H5Pget_external(
                    plist.id(),
                    index as u32,
                    bound,
                    name.as_mut_ptr().cast(),
                    &mut offset,
                    &mut size,
                )
            } < 0
            {
                return Err(Error("cannot inspect raw external window".into()));
            }
            let end = name
                .iter()
                .position(|&v| v == 0)
                .filter(|&n| n <= limits.text_bytes.min(limits.metadata_bytes))
                .ok_or_else(|| Error("raw external filename exceeds text bound".into()))?;
            name.truncate(end);
            let name = String::from_utf8(name)
                .map_err(|_| Error("non-UTF8 raw external filename unsupported".into()))?;
            let offset =
                u64::try_from(offset).map_err(|_| Error("negative raw external offset".into()))?;
            Ok((name, offset, (size != u64::MAX).then_some(size)))
        })?;
        if declared.is_none() && index + 1 != count {
            return Err(Error(
                "unlimited raw external reservation must be final".into(),
            ));
        }
        let binding = bindings
            .options
            .filenames
            .iter()
            .find(|b| b.stored_filename == name)
            .ok_or_else(|| {
                Error(format!(
                    "raw external source requires explicit filename binding: {name}"
                ))
            })?;
        bindings.used.insert(&binding.stored_filename);
        let source = sources.add_raw(&binding.actual_file)?;
        let effective = declared
            .unwrap_or(bytes.saturating_sub(logical))
            .min(bytes.saturating_sub(logical));
        let end = offset
            .checked_add(effective)
            .ok_or_else(|| Error("raw external source window overflow".into()))?;
        if end > sources.raw_bytes(source) {
            return Err(Error(
                "raw external file does not contain the complete frozen source window".into(),
            ));
        }
        sources.charge(
            name.len()
                .saturating_mul(2)
                .saturating_add(sources.raw_path(source).as_os_str().len().saturating_mul(2)),
        )?;
        let window = ExternalRawWindow {
            stored_filename: name,
            source_file: sources.raw_path(source).to_path_buf(),
            file_offset: offset,
            declared_bytes: declared,
            logical_offset: logical,
            effective_bytes: effective,
        };
        logical = logical
            .checked_add(effective)
            .ok_or_else(|| Error("raw external logical byte overflow".into()))?;
        windows.push((source, window));
    }
    if logical != bytes {
        return Err(Error(
            "raw external reservations do not cover every dataset sample".into(),
        ));
    }
    let metadata = ExternalRawMetadata {
        windows: windows.iter().map(|(_, w)| w.clone()).collect(),
        frozen_bytes: bytes,
    };
    let storage = Storage {
        filters: Vec::new(),
        fill_value: fill(dataset, ty)?,
        storage_allocated: storage_allocated(dataset)?,
        fill_time: plist.get_fill_time()?,
    };
    Ok((Raw { windows }, metadata, storage))
}
impl Raw {
    pub fn read(
        &self,
        sources: &Sources,
        offset: u64,
        pixels: &mut PixelBuffer,
        little: bool,
    ) -> Result<(), Error> {
        let component = pixels.pixel_type().component_bytes();
        let destination = pixels.bytes_mut();
        let end = offset
            .checked_add(destination.len() as u64)
            .ok_or_else(|| Error("raw frame window overflow".into()))?;
        let mut copied = 0usize;
        for (source, window) in &self.windows {
            let begin = offset.max(window.logical_offset);
            let stop = end.min(window.logical_offset + window.effective_bytes);
            if begin >= stop {
                continue;
            }
            let n = usize::try_from(stop - begin)
                .map_err(|_| Error("raw frame size overflow".into()))?;
            let file_offset = window.file_offset + (begin - window.logical_offset);
            read_exact_at(
                sources.raw_file(*source),
                &mut destination[copied..copied + n],
                file_offset,
            )?;
            copied += n;
        }
        if copied != destination.len() {
            return Err(Error("incomplete raw external frame".into()));
        }
        if little != cfg!(target_endian = "little") {
            for value in destination.chunks_exact_mut(component) {
                value.reverse();
            }
        }
        Ok(())
    }
}
fn read_exact_at(
    file: &std::fs::File,
    mut target: &mut [u8],
    mut offset: u64,
) -> Result<(), Error> {
    while !target.is_empty() {
        #[cfg(unix)]
        let n = {
            use std::os::unix::fs::FileExt;
            file.read_at(target, offset)?
        };
        #[cfg(windows)]
        let n = {
            use std::os::windows::fs::FileExt;
            file.seek_read(target, offset)?
        };
        #[cfg(not(any(unix, windows)))]
        let n = {
            use std::io::{Read, Seek, SeekFrom};
            let mut f = file.try_clone()?;
            f.seek(SeekFrom::Start(offset))?;
            f.read(target)?
        };
        if n == 0 {
            return Err(Error("truncated raw external source".into()));
        }
        offset += n as u64;
        target = &mut target[n..];
    }
    Ok(())
}
