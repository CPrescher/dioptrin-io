//! File/scratch primitives shared by the independent legacy detector readers.
use crate::{
    binary::{Error, resize_bytes},
    source::Snapshot,
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
pub(crate) struct Source {
    snapshot: Snapshot,
    file: File,
}
impl Source {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self { snapshot, file })
    }
    pub fn bytes(&self) -> u64 {
        self.snapshot.bytes()
    }
    pub fn check(&self) -> Result<(), Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)
    }
    pub fn read(&self, offset: u64, len: usize, output: &mut Vec<u8>) -> Result<(), Error> {
        self.check()?;
        if offset
            .checked_add(len as u64)
            .is_none_or(|n| n > self.bytes())
        {
            return Err(Error("truncated detector source".into()));
        }
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(offset))?;
        resize_bytes(output, len)?;
        file.read_exact(output)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        Ok(())
    }
}
pub(crate) fn product(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_mul(b)
        .ok_or_else(|| Error("detector payload size overflow".into()))
}
#[cfg(any(feature = "bruker", feature = "oxford"))]
pub(crate) fn sum(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_add(b)
        .ok_or_else(|| Error("detector payload offset overflow".into()))
}
