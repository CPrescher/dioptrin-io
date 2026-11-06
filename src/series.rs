//! Frozen ordered file series over the existing wrapped readers.
//!
//! The opener is an immutable configuration callback, not a decoder fallback.
//! It must always open the supplied path with the same format, interpretation,
//! link policy and resource limits. Interior mutable configuration is outside
//! this contract. Only one reader is opened at a time; decoded files are never
//! retained by the series. Counts, dtype, channels and orientation belong to
//! each file's reader and are not homogenized across the series.
use crate::{source::Snapshot, wrapped};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Bounds for the manifest itself. Backend and transport limits remain part
/// of the deterministic opener's configuration.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub files: usize,
    pub frames: usize,
    pub metadata_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            files: 10_000,
            frames: 100_000,
            metadata_bytes: 16 * 1024 * 1024,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<(), wrapped::Error> {
        if [self.files, self.frames, self.metadata_bytes]
            .iter()
            .any(|v| *v == 0 || *v == usize::MAX)
        {
            return Err(wrapped::Error(
                "series limits must be positive and finite".into(),
            ));
        }
        Ok(())
    }
    fn charge(self, used: &mut usize, bytes: usize) -> Result<(), wrapped::Error> {
        *used = used
            .checked_add(bytes)
            .filter(|v| *v <= self.metadata_bytes)
            .ok_or_else(|| wrapped::Error("series manifest exceeds metadata limit".into()))?;
        Ok(())
    }
}

/// An explicit filename template. `digits` is a minimum width (0 means no
/// padding); `suffix` includes any format and compression extensions. No codec
/// or numeric detector extension is guessed from a filename.
#[derive(Clone, Debug)]
pub struct Numbered {
    pub stem: PathBuf,
    pub suffix: OsString,
    pub first: u64,
    pub step: u64,
    pub digits: usize,
}
impl Numbered {
    fn admitted_path_bytes(
        &self,
        limits: Limits,
        origin: Option<&Path>,
    ) -> Result<(), wrapped::Error> {
        let bytes = self
            .stem
            .as_os_str()
            .len()
            .checked_add(self.suffix.len())
            .and_then(|v| v.checked_add(20))
            .and_then(|v| {
                v.checked_add(origin.map_or(0, |p| p.as_os_str().len().saturating_add(1)))
            })
            .ok_or_else(|| wrapped::Error("numbered series path length overflow".into()))?;
        charge_path_bytes(limits, &mut 0, bytes)
    }
    /// Parse a UTF-8 numbered basename, retaining its directory and suffix.
    /// A final `.gz`/`.bz2` and a nonnumeric format extension are excluded from
    /// number selection; a numeric extension (Bruker `image.0123`) is retained.
    /// The last digit run in the remaining stem is selected. Explicit templates
    /// remain available for other filename conventions or non-UTF-8 basenames.
    /// # Errors
    /// No numbered basename, overflowing u64, invalid step or padding width.
    pub fn from_seed(path: impl AsRef<Path>, step: u64) -> Result<Self, wrapped::Error> {
        let path = path.as_ref();
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| wrapped::Error("numbered seed requires a UTF-8 filename".into()))?;
        let mut stem_end = name.len();
        if let Some((head, extension)) = name.rsplit_once('.')
            && (extension.eq_ignore_ascii_case("gz") || extension.eq_ignore_ascii_case("bz2"))
        {
            stem_end = head.len();
        }
        if let Some((head, extension)) = name[..stem_end].rsplit_once('.')
            && !head.is_empty()
            && !extension.is_empty()
            && !extension.bytes().all(|v| v.is_ascii_digit())
        {
            stem_end = head.len();
        }
        let end = name.as_bytes()[..stem_end]
            .iter()
            .rposition(u8::is_ascii_digit)
            .map(|v| v + 1)
            .ok_or_else(|| wrapped::Error("seed filename contains no series number".into()))?;
        let start = name.as_bytes()[..end]
            .iter()
            .rposition(|v| !v.is_ascii_digit())
            .map_or(0, |v| v + 1);
        let first = name[start..end]
            .parse()
            .map_err(|_| wrapped::Error("seed number exceeds u64".into()))?;
        let result = Self {
            stem: path.with_file_name(&name[..start]),
            suffix: name[end..].into(),
            first,
            step,
            digits: end - start,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), wrapped::Error> {
        if self.step == 0 || self.digits > 20 {
            return Err(wrapped::Error(
                "numbered series requires nonzero step and at most 20 padding digits".into(),
            ));
        }
        Ok(())
    }
    /// Construct one filename without checking whether it exists.
    /// # Errors
    /// Invalid padding/step or path length arithmetic overflow.
    pub fn path(&self, number: u64) -> Result<PathBuf, wrapped::Error> {
        self.validate()?;
        self.stem
            .as_os_str()
            .len()
            .checked_add(self.suffix.len())
            .and_then(|v| v.checked_add(20))
            .ok_or_else(|| wrapped::Error("numbered series path length overflow".into()))?;
        let mut path = self.stem.clone().into_os_string();
        path.push(format!("{number:0width$}", width = self.digits));
        path.push(&self.suffix);
        Ok(path.into())
    }
    /// An inclusive numeric range, preserving caller order and step. A final
    /// number beyond `last` is never included. Every resulting path must exist
    /// when subsequently passed to `Series::open`.
    /// # Errors
    /// Invalid range, arithmetic overflow or manifest bounds.
    pub fn through(&self, last: u64, limits: Limits) -> Result<Vec<PathBuf>, wrapped::Error> {
        self.validate()?;
        limits.validate()?;
        self.admitted_path_bytes(limits, None)?;
        let count = last
            .checked_sub(self.first)
            .and_then(|v| (v / self.step).checked_add(1))
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| *v <= limits.files)
            .ok_or_else(|| {
                wrapped::Error("numbered series range exceeds file limit or is invalid".into())
            })?;
        // Admit the minimum record footprint before allocating the path vector.
        if count > limits.metadata_bytes / 512 {
            return Err(wrapped::Error(
                "numbered series exceeds metadata limit".into(),
            ));
        }
        let mut paths = Vec::new();
        paths
            .try_reserve_exact(count)
            .map_err(|e| wrapped::Error(e.to_string()))?;
        let mut used = 0;
        let mut number = self.first;
        for n in 0..count {
            let path = self.path(number)?;
            charge_path(limits, &mut used, &path)?;
            paths.push(path);
            if n + 1 < count {
                number = number
                    .checked_add(self.step)
                    .ok_or_else(|| wrapped::Error("numbered series arithmetic overflow".into()))?;
            }
        }
        Ok(paths)
    }
    /// Freeze consecutive existing filenames until the first absent directory
    /// entry. Broken symlinks and other I/O failures are errors, not terminators.
    /// A bound reached while another file exists is an error, never truncation.
    /// # Errors
    /// Invalid template, source I/O failure, number overflow or manifest bounds.
    pub fn until_missing(&self, limits: Limits) -> Result<Manifest, wrapped::Error> {
        self.validate()?;
        limits.validate()?;
        let origin = std::env::current_dir()?;
        self.admitted_path_bytes(
            limits,
            (!self.stem.is_absolute()).then_some(origin.as_path()),
        )?;
        let mut paths = Vec::new();
        let mut used = 0;
        let mut number = self.first;
        loop {
            let path = self.path(number)?;
            let path = if path.is_absolute() {
                path
            } else {
                origin.join(path)
            };
            charge_path(limits, &mut used, &path)?;
            match std::fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Manifest {
                        paths,
                        stopped_at: Some(path),
                    });
                }
                Err(e) => return Err(e.into()),
                Ok(_) => {}
            }
            if !std::fs::metadata(&path)?.is_file() {
                return Err(wrapped::Error(
                    "series paths must resolve to regular files".into(),
                ));
            }
            if paths.len() >= limits.files {
                return Err(wrapped::Error(
                    "consecutive series exceeds file limit before first missing path".into(),
                ));
            }
            paths
                .try_reserve(1)
                .map_err(|e| wrapped::Error(e.to_string()))?;
            paths.push(path);
            number = number.checked_add(self.step).ok_or_else(|| {
                wrapped::Error(
                    "numbered series arithmetic overflow before first missing path".into(),
                )
            })?;
        }
    }
}

/// Frozen filenames and the reason a first-missing traversal stopped. Files
/// created later do not extend this manifest; build a new one explicitly.
#[derive(Clone, Debug)]
pub struct Manifest {
    pub paths: Vec<PathBuf>,
    pub stopped_at: Option<PathBuf>,
}

/// One file's exact prefix frame range in the global series.
#[derive(Debug)]
pub struct File {
    pub path: PathBuf,
    pub first_frame: usize,
    pub frames: usize,
}

/// Global-to-file mapping. Indices are zero based.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location<'a> {
    pub global_frame: usize,
    pub file_index: usize,
    pub local_frame: usize,
    pub path: &'a Path,
}

struct Tracked {
    sources: Vec<Snapshot>,
    bindings: Vec<crate::SourceBinding>,
}

/// An immutable source manifest plus a deterministic reader configuration.
pub struct Series<O> {
    files: Vec<File>,
    tracked: Vec<Tracked>,
    frames: usize,
    opener: O,
    valid: bool,
    current: Option<(usize, wrapped::Reader)>,
}

fn charge_path(limits: Limits, used: &mut usize, path: &Path) -> Result<(), wrapped::Error> {
    charge_path_bytes(limits, used, path.as_os_str().len())
}
fn charge_path_bytes(
    limits: Limits,
    used: &mut usize,
    length: usize,
) -> Result<(), wrapped::Error> {
    // Snapshot retains its canonical path and both sides of its binding. Use
    // conservative path/record accounting, also shared by filename generation.
    let bytes = length
        .checked_mul(4)
        .and_then(|v| v.checked_add(512))
        .ok_or_else(|| wrapped::Error("series path metadata overflow".into()))?;
    limits.charge(used, bytes)
}

impl<O> Series<O>
where
    O: Fn(&Path) -> Result<wrapped::Reader, wrapped::Error>,
{
    /// Index every explicit path, dropping its reader before opening the next.
    /// An empty manifest is valid. Missing interior paths always fail.
    /// # Errors
    /// Manifest bounds, missing/changed sources, zero-frame files or opener errors.
    pub fn open<I, P>(paths: I, opener: O, limits: Limits) -> Result<Self, wrapped::Error>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        limits.validate()?;
        let origin = std::env::current_dir()?;
        let mut series = Self {
            files: Vec::new(),
            tracked: Vec::new(),
            frames: 0,
            opener,
            valid: true,
            current: None,
        };
        let mut used = 0;
        for path in paths {
            if series.files.len() >= limits.files {
                return Err(wrapped::Error("series exceeds file limit".into()));
            }
            // Absolute lexical path preserves the original symlink binding and
            // makes later changes of the process working directory irrelevant.
            let path = path.as_ref();
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                origin.join(path)
            };
            charge_path(limits, &mut used, &path)?;
            let primary = Snapshot::open(&path).map_err(wrapped::Error)?;
            if !std::fs::metadata(&path)?.is_file() {
                return Err(wrapped::Error(
                    "series paths must resolve to regular files".into(),
                ));
            }
            let reader = (series.opener)(&path)?;
            reader.check_sources()?;
            if reader.frames() == 0 {
                return Err(wrapped::Error("series file contains no frames".into()));
            }
            let frames = series
                .frames
                .checked_add(reader.frames())
                .filter(|v| *v <= limits.frames)
                .ok_or_else(|| wrapped::Error("series exceeds cumulative frame limit".into()))?;
            let paths = reader.source_paths();
            if paths.first().is_none_or(|p| **p != primary.path) {
                return Err(wrapped::Error(
                    "series opener substituted a different source".into(),
                ));
            }
            let mut sources = Vec::new();
            for source in paths {
                charge_path(limits, &mut used, source)?;
                sources
                    .try_reserve(1)
                    .map_err(|e| wrapped::Error(e.to_string()))?;
                sources.push(Snapshot::open(source).map_err(wrapped::Error)?);
            }
            let bindings = reader.source_bindings()?;
            for binding in &bindings {
                charge_path(limits, &mut used, &binding.requested)?;
                charge_path(limits, &mut used, &binding.resolved)?;
                binding.check().map_err(wrapped::Error)?;
            }
            primary.check().map_err(wrapped::Error)?;
            reader.check_sources()?;
            series
                .files
                .try_reserve(1)
                .map_err(|e| wrapped::Error(e.to_string()))?;
            series
                .tracked
                .try_reserve(1)
                .map_err(|e| wrapped::Error(e.to_string()))?;
            series.files.push(File {
                path,
                first_frame: series.frames,
                frames: reader.frames(),
            });
            series.tracked.push(Tracked { sources, bindings });
            series.frames = frames;
        }
        series.check_all()?;
        Ok(series)
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.frames
    }
    #[must_use]
    pub fn files(&self) -> &[File] {
        &self.files
    }
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.valid
    }
    #[must_use]
    pub fn locate(&self, global_frame: usize) -> Option<Location<'_>> {
        if global_frame >= self.frames {
            return None;
        }
        let file_index = self
            .files
            .partition_point(|f| f.first_frame <= global_frame)
            .checked_sub(1)?;
        let file = &self.files[file_index];
        Some(Location {
            global_frame,
            file_index,
            local_frame: global_frame - file.first_frame,
            path: &file.path,
        })
    }
    fn check_all(&self) -> Result<(), wrapped::Error> {
        if !self.valid {
            return Err(wrapped::Error(
                "series is invalid; construct a new source manifest".into(),
            ));
        }
        for (file, tracked) in self.files.iter().zip(&self.tracked) {
            if std::fs::canonicalize(&file.path)? != tracked.sources[0].path {
                return Err(wrapped::Error("series path binding changed".into()));
            }
            for source in &tracked.sources {
                source.check().map_err(wrapped::Error)?;
            }
            for binding in &tracked.bindings {
                binding.check().map_err(wrapped::Error)?;
            }
        }
        Ok(())
    }
    fn reopen(&self, file: usize) -> Result<wrapped::Reader, wrapped::Error> {
        self.check_all()?;
        let reader = (self.opener)(&self.files[file].path)?;
        reader.check_sources()?;
        let sources = reader.source_paths();
        if reader.frames() != self.files[file].frames
            || sources.len() != self.tracked[file].sources.len()
            || sources
                .iter()
                .zip(&self.tracked[file].sources)
                .any(|(a, b)| **a != b.path)
            || reader.source_bindings()? != self.tracked[file].bindings
        {
            return Err(wrapped::Error(
                "reopened series frame count or source graph differs from indexed manifest".into(),
            ));
        }
        self.check_all()?;
        Ok(reader)
    }
    /// Read exactly one file-local frame into the caller's reusable buffer.
    /// Every error clears visible output. Source/open/decoding failures invalidate
    /// the series permanently; an out-of-range index only invalidates output.
    /// # Errors
    /// Index bounds, changed sources, changed reopening contract or backend errors.
    pub fn read_frame_into(
        &mut self,
        index: usize,
        output: &mut wrapped::FrameBuffer,
    ) -> Result<(), wrapped::Error> {
        self.current = None;
        output.invalidate();
        let Some(location) = self.locate(index) else {
            return Err(wrapped::Error("series frame index outside manifest".into()));
        };
        let (file, local) = (location.file_index, location.local_frame);
        let result = (|| {
            let mut reader = self.reopen(file)?;
            reader.read_frame_into(local, output)?;
            reader.check_sources()?;
            if reader.source_bindings()? != self.tracked[file].bindings {
                return Err(wrapped::Error(
                    "series source graph changed during frame read".into(),
                ));
            }
            self.check_all()?;
            Ok(reader)
        })();
        match result {
            Err(e) => {
                output.invalidate();
                self.valid = false;
                Err(e)
            }
            Ok(reader) => {
                self.current = Some((index, reader));
                Ok(())
            }
        }
    }
    /// Location of the last successful frame read. Any failed frame read clears
    /// it, including a bounds error.
    #[must_use]
    pub fn current_location(&self) -> Option<Location<'_>> {
        self.current
            .as_ref()
            .and_then(|(index, _)| self.locate(*index))
    }
    /// Borrow metadata from the single retained reader after checking every
    /// indexed source. A failed check clears the current reader permanently.
    /// # Errors
    /// An indexed source, binding or retained reader changed.
    pub fn current_metadata(&mut self) -> Result<Option<wrapped::Metadata<'_>>, wrapped::Error> {
        if let Err(e) = self.check_current() {
            self.valid = false;
            self.current = None;
            return Err(e);
        }
        Ok(self.borrow_current_metadata())
    }
    fn check_current(&self) -> Result<(), wrapped::Error> {
        self.check_all()?;
        if let Some((_, reader)) = &self.current {
            reader.check_sources()?;
        }
        Ok(())
    }
    /// Borrow checked metadata without reopening or changing navigation state.
    /// A validation error leaves the logical last-successful location intact;
    /// the next frame read will invalidate the series and its output.
    /// # Errors
    /// An indexed source, binding or retained reader changed.
    pub fn current_metadata_ref(&self) -> Result<Option<wrapped::Metadata<'_>>, wrapped::Error> {
        self.check_current()?;
        Ok(self.borrow_current_metadata())
    }
    fn borrow_current_metadata(&self) -> Option<wrapped::Metadata<'_>> {
        self.current.as_ref().and_then(|(index, reader)| {
            self.locate(*index)
                .and_then(|location| reader.metadata(location.local_frame))
        })
    }
    /// Inspect source metadata using the same checked reopening contract. The
    /// callback's result cannot borrow a dropped per-file reader.
    /// # Errors
    /// Index bounds, changed source graph or backend/opener failure.
    pub fn with_metadata<T>(
        &mut self,
        index: usize,
        inspect: impl FnOnce(wrapped::Metadata<'_>) -> T,
    ) -> Result<T, wrapped::Error> {
        self.current = None;
        let location = self
            .locate(index)
            .ok_or_else(|| wrapped::Error("series frame index outside manifest".into()))?;
        let (file, local) = (location.file_index, location.local_frame);
        let result = (|| {
            let reader = self.reopen(file)?;
            let metadata = reader
                .metadata(local)
                .ok_or_else(|| wrapped::Error("series frame metadata missing".into()))?;
            let result = inspect(metadata);
            reader.check_sources()?;
            if reader.source_bindings()? != self.tracked[file].bindings {
                return Err(wrapped::Error(
                    "series source graph changed during metadata inspection".into(),
                ));
            }
            self.check_all()?;
            Ok(result)
        })();
        if result.is_err() {
            self.valid = false;
            self.current = None;
        }
        result
    }
}

impl<O> crate::navigation::FrameSource for Series<O>
where
    O: Fn(&Path) -> Result<wrapped::Reader, wrapped::Error>,
{
    fn frames(&self) -> usize {
        self.frames()
    }
    fn read_frame_into(
        &mut self,
        index: usize,
        output: &mut wrapped::FrameBuffer,
    ) -> Result<(), wrapped::Error> {
        self.read_frame_into(index, output)
    }
}
