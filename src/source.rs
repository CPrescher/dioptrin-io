//! Metadata identity checks for explicitly immutable source snapshots.
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{
    fs::Metadata,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    bytes: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl Stamp {
    fn new(meta: Metadata) -> Result<Self, String> {
        Ok(Self {
            bytes: meta.len(),
            modified: meta.modified().map_err(|e| e.to_string())?,
            created: meta.created().ok(),
            #[cfg(unix)]
            identity: (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()),
        })
    }
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
        let stamp = Stamp::new(std::fs::metadata(&path).map_err(|e| e.to_string())?)?;
        Ok(Self {
            path,
            stamp,
            binding,
        })
    }
    #[cfg(any(feature = "hdf5", feature = "cbf", feature = "edf", feature = "native"))]
    pub fn bytes(&self) -> u64 {
        self.stamp.bytes
    }
    pub fn check(&self) -> Result<(), String> {
        self.binding.check()?;
        let now = Stamp::new(std::fs::metadata(&self.path).map_err(|e| e.to_string())?)?;
        if now != self.stamp {
            return Err("source changed; reopen the immutable snapshot".into());
        }
        Ok(())
    }
    pub fn check_handle(&self, file: &std::fs::File) -> Result<(), String> {
        if Stamp::new(file.metadata().map_err(|e| e.to_string())?)? != self.stamp {
            return Err("source handle changed while opening snapshot".into());
        }
        self.check()
    }
}
