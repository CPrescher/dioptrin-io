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
#[derive(Debug)]
pub(crate) struct Snapshot {
    pub path: PathBuf,
    stamp: Stamp,
}
impl Snapshot {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        let stamp = Stamp::new(std::fs::metadata(&path).map_err(|e| e.to_string())?)?;
        Ok(Self { path, stamp })
    }
    #[cfg(any(feature = "hdf5", feature = "cbf", feature = "edf"))]
    pub fn bytes(&self) -> u64 {
        self.stamp.bytes
    }
    pub fn check(&self) -> Result<(), String> {
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
