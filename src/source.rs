//! Metadata identity checks for explicitly immutable source snapshots.
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{
    fs::{File, Metadata},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    bytes: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
    #[cfg(windows)]
    identity: (u64, [u8; 16]),
}
impl Stamp {
    fn new(meta: Metadata, #[cfg(windows)] file: &File) -> Result<Self, String> {
        Ok(Self {
            bytes: meta.len(),
            modified: meta.modified().map_err(|e| e.to_string())?,
            created: meta.created().ok(),
            #[cfg(unix)]
            identity: (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()),
            #[cfg(windows)]
            identity: windows_identity(file)?,
        })
    }
    pub(crate) fn from_path(path: &Path) -> Result<Self, String> {
        #[cfg(windows)]
        {
            if !std::fs::metadata(path)
                .map_err(|e| e.to_string())?
                .is_file()
            {
                return Err("source snapshots require regular files".into());
            }
            let file = File::open(path).map_err(|e| e.to_string())?;
            Self::from_file(&file)
        }
        #[cfg(not(windows))]
        {
            Self::new(std::fs::metadata(path).map_err(|e| e.to_string())?)
        }
    }
    fn from_file(file: &File) -> Result<Self, String> {
        Self::new(
            file.metadata().map_err(|e| e.to_string())?,
            #[cfg(windows)]
            file,
        )
    }
}
#[cfg(windows)]
fn windows_identity(file: &File) -> Result<(u64, [u8; 16]), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    let mut info = FILE_ID_INFO::default();
    // SAFETY: the live File owns this handle; the exact writable FILE_ID_INFO
    // buffer and size match FileIdInfo. Volume +128-bit ID distinguish replaced
    // files even when timestamps/length collide, including on ReFS.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok((info.VolumeSerialNumber, info.FileId.Identifier))
}
/// An explicitly selected path and its resolved target at indexing time.
/// Checking both sides detects symlink retargeting even if the old target survives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceBinding {
    pub requested: PathBuf,
    pub resolved: PathBuf,
}
impl SourceBinding {
    /// # Errors
    /// Missing target, invalid current directory or filesystem error.
    pub fn capture(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let requested = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(path)
        };
        let resolved = std::fs::canonicalize(&requested).map_err(|e| e.to_string())?;
        Ok(Self {
            requested,
            resolved,
        })
    }
    /// # Errors
    /// The supplied path disappeared or now resolves to another source.
    pub fn check(&self) -> Result<(), String> {
        let resolved = std::fs::canonicalize(&self.requested).map_err(|e| e.to_string())?;
        if resolved != self.resolved {
            return Err("source path binding changed; reopen the immutable snapshot".into());
        }
        Ok(())
    }
}
#[derive(Debug)]
pub(crate) struct Snapshot {
    pub path: PathBuf,
    stamp: Stamp,
    pub binding: SourceBinding,
}
impl Snapshot {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let binding = SourceBinding::capture(path)?;
        let path = binding.resolved.clone();
        let stamp = Stamp::from_path(&path)?;
        Ok(Self {
            path,
            stamp,
            binding,
        })
    }
    #[cfg(any(
        feature = "hdf5",
        feature = "cbf",
        feature = "edf",
        feature = "native",
        feature = "tiff"
    ))]
    pub fn bytes(&self) -> u64 {
        self.stamp.bytes
    }
    pub fn check(&self) -> Result<(), String> {
        self.binding.check()?;
        let now = Stamp::from_path(&self.path)?;
        if now != self.stamp {
            return Err("source changed; reopen the immutable snapshot".into());
        }
        Ok(())
    }
    pub fn check_handle(&self, file: &std::fs::File) -> Result<(), String> {
        if Stamp::from_file(file)? != self.stamp {
            return Err("source handle changed while opening snapshot".into());
        }
        self.check()
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
    use windows_sys::Win32::{Foundation::FILETIME, Storage::FileSystem::SetFileTime};

    #[test]
    fn replacement_with_identical_size_and_timestamps_invalidates_snapshot_and_cache_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        let other = dir.path().join("replacement");
        std::fs::write(&path, b"original").unwrap();
        let snapshot = Snapshot::open(&path).unwrap();
        let original_handle = File::open(&path).unwrap();
        let original = original_handle.metadata().unwrap();
        let time = |v: u64| FILETIME {
            dwLowDateTime: v as u32,
            dwHighDateTime: (v >> 32) as u32,
        };
        std::fs::write(&other, b"replaced").unwrap();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&other)
            .unwrap();
        let created = time(original.creation_time());
        let modified = time(original.last_write_time());
        // SAFETY: writable live handle; both input FILETIMEs remain valid for
        // the call. A null access-time pointer intentionally leaves it alone.
        assert_ne!(
            unsafe { SetFileTime(file.as_raw_handle(), &created, std::ptr::null(), &modified) },
            0
        );
        drop(file);
        let replacement = Stamp::from_path(&other).unwrap();
        assert_eq!(replacement.bytes, snapshot.stamp.bytes);
        assert_eq!(replacement.modified, snapshot.stamp.modified);
        assert_eq!(replacement.created, snapshot.stamp.created);
        assert_ne!(replacement.identity, snapshot.stamp.identity);
        std::fs::rename(&other, &path).unwrap();
        assert!(snapshot.check().is_err());
        assert!(snapshot.check_handle(&original_handle).is_err());
        // The decoded-count cache uses this same stamp, so it must invalidate
        // rather than serve a cached frame from the replaced file.
        assert_ne!(Stamp::from_path(&path).unwrap(), snapshot.stamp);
    }
}
