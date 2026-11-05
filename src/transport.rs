//! Shared immutable outer-file decompression, bounded by disk/output admission.
use crate::{binary::Error, source::Snapshot};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub encoded_bytes: u64,
    pub source_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            encoded_bytes: 2 * 1024 * 1024 * 1024,
            source_bytes: 8 * 1024 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Bzip2,
}
pub(crate) struct PreparedSource {
    snapshot: Snapshot,
    handle: File,
    staged: Option<tempfile::NamedTempFile>,
    staged_snapshot: Option<Snapshot>,
    bytes: u64,
    compression: Compression,
}
impl PreparedSource {
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        if limits.encoded_bytes == 0 || limits.source_bytes == 0 || limits.source_bytes == u64::MAX
        {
            return Err(Error("transport limits must be positive and finite".into()));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if !std::fs::metadata(&snapshot.path)?.is_file() {
            return Err(Error("transport snapshots require regular files".into()));
        }
        let mut handle = File::open(&snapshot.path)?;
        snapshot.check_handle(&handle).map_err(Error)?;
        let mut prefix = [0; 3];
        let n = snapshot.bytes().min(3) as usize;
        handle.read_exact(&mut prefix[..n])?;
        handle.seek(SeekFrom::Start(0))?;
        let compression = if n >= 2 && prefix[..2] == [31, 139] {
            Compression::Gzip
        } else if n == 3 && prefix == *b"BZh" {
            Compression::Bzip2
        } else {
            Compression::None
        };
        let mut staged = None;
        let bytes;
        if compression != Compression::None {
            if snapshot.bytes() > limits.encoded_bytes {
                return Err(Error("wrapped source exceeds encoded file limit".into()));
            }
            let mut temp = tempfile::NamedTempFile::new()?;
            let decoder: Box<dyn Read + '_> = match compression {
                Compression::Gzip => Box::new(flate2::read::MultiGzDecoder::new(&mut handle)),
                Compression::Bzip2 => Box::new(bzip2::read::MultiBzDecoder::new(&mut handle)),
                Compression::None => unreachable!(),
            };
            bytes = std::io::copy(
                &mut decoder.take(limits.source_bytes + 1),
                temp.as_file_mut(),
            )?;
            if bytes > limits.source_bytes {
                return Err(Error("wrapper output exceeds staged source limit".into()));
            }
            staged = Some(temp);
        } else {
            bytes = snapshot.bytes();
            if bytes > limits.source_bytes {
                return Err(Error("source exceeds file size limit".into()));
            }
        }
        let staged_snapshot = staged
            .as_ref()
            .map(|f| Snapshot::open(f.path()).map_err(Error))
            .transpose()?;
        snapshot.check_handle(&handle).map_err(Error)?;
        Ok(Self {
            snapshot,
            handle,
            staged,
            staged_snapshot,
            bytes,
            compression,
        })
    }
    pub fn path(&self) -> &Path {
        self.staged
            .as_ref()
            .map_or(&self.snapshot.path, |f| f.path())
    }
    pub fn original_path(&self) -> &Path {
        &self.snapshot.path
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn compression(&self) -> Compression {
        self.compression
    }
    pub fn check_input_handle(&self, file: &File) -> Result<(), Error> {
        self.staged_snapshot
            .as_ref()
            .unwrap_or(&self.snapshot)
            .check_handle(file)
            .map_err(Error)?;
        self.check()
    }
    pub fn check(&self) -> Result<(), Error> {
        self.snapshot.check_handle(&self.handle).map_err(Error)?;
        if let (Some(snapshot), Some(file)) = (&self.staged_snapshot, &self.staged) {
            snapshot.check_handle(file.as_file()).map_err(Error)?;
        }
        Ok(())
    }
}
