//! Standalone, bounded CIF scalar/loop projection using the detector's existing
//! header parser. Original bytes and all parser diagnostics remain authoritative;
//! this is not a claim of complete CIF1/CIF2 grammar or binary-item support.
use super::{CifHeader, Error, Limits, header};
use crate::source::Snapshot;
use std::{fs::File, io::Read, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    HeaderProjection,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    Buffer,
    StreamCapture,
    PathSnapshot,
}
#[derive(Debug)]
pub struct Document {
    pub original: Vec<u8>,
    pub cif: CifHeader,
    pub coverage: Coverage,
    pub coverage_warning: &'static str,
    pub provenance: Provenance,
}
impl Document {
    /// Preserve the complete source and expose the existing bounded scalar/loop
    /// parser's result. Syntax/UTF8 diagnostics indicate partial interpretation;
    /// they never imply that unavailable text values or image counts exist.
    /// # Errors
    /// Encoded/header/aggregate metadata limits or allocation failure.
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self, Error> {
        admit(bytes.len(), limits)?;
        let mut original = Vec::new();
        original
            .try_reserve_exact(bytes.len())
            .map_err(|e| Error(e.to_string()))?;
        original.extend_from_slice(bytes);
        Self::parse_owned(original, limits, Provenance::Buffer)
    }
    /// Capture a nonseekable source under explicit byte limits. This owned
    /// snapshot does not monitor an original file behind the stream.
    /// # Errors
    /// I/O failure, limits, allocation or parser resource limits.
    pub fn from_reader(reader: impl Read, limits: Limits) -> Result<Self, Error> {
        Self::parse_owned(capture(reader, limits)?, limits, Provenance::StreamCapture)
    }
    fn parse_owned(
        original: Vec<u8>,
        limits: Limits,
        provenance: Provenance,
    ) -> Result<Self, Error> {
        let budget = admit(original.len(), limits)?;
        let mut cif = header::parse(&original, budget)?;
        if cif.data_blocks.len() > 1 {
            cif.diagnostics.push("multiple CIF data blocks retained as a flat header projection; per-block field/loop scopes are not reconstructed".into());
        }
        if cif.retained_bytes() > budget {
            return Err(Error(
                "CIF document retained metadata exceeds budget".into(),
            ));
        }
        Ok(Self {
            original,
            cif,
            coverage: Coverage::HeaderProjection,
            coverage_warning: "Scalar/loop header projection only: reserved _array_data.data bodies and full CIF grammar are not interpreted; original source bytes remain authoritative",
            provenance,
        })
    }
}
fn admit(bytes: usize, limits: Limits) -> Result<usize, Error> {
    limits.validate()?;
    if bytes > limits.encoded_bytes.min(limits.header_bytes) {
        return Err(Error("CIF document source/header byte limit".into()));
    }
    limits
        .metadata_bytes
        .checked_sub(bytes)
        .and_then(|n| n.checked_sub(std::mem::size_of::<Document>()))
        .filter(|&n| n > 0)
        .ok_or_else(|| Error("CIF document aggregate metadata limit".into()))
}
fn capture(reader: impl Read, limits: Limits) -> Result<Vec<u8>, Error> {
    limits.validate()?;
    let bound = limits
        .encoded_bytes
        .min(limits.header_bytes)
        .min(limits.metadata_bytes);
    let length = u64::try_from(bound)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| Error("CIF capture limit overflow".into()))?;
    let mut reader = reader.take(length);
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        bytes
            .len()
            .checked_add(count)
            .filter(|&n| n <= bound)
            .ok_or_else(|| Error("CIF document capture byte limit".into()))?;
        bytes
            .try_reserve_exact(count)
            .map_err(|e| Error(e.to_string()))?;
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(bytes)
}
/// Immutable source identity plus the captured document. Every metadata access
/// checks replacement, growth and the original open handle again.
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    document: Document,
}
impl Reader {
    /// # Errors
    /// Source changes, I/O, byte/metadata limits or parser resource limits.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let bound = limits
            .encoded_bytes
            .min(limits.header_bytes)
            .min(limits.metadata_bytes);
        if snapshot.bytes() > bound as u64 {
            return Err(Error("CIF document source byte limit".into()));
        }
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let document = Document::parse_owned(
            capture(&mut file, limits)?,
            limits,
            Provenance::PathSnapshot,
        )?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            document,
        })
    }
    /// # Errors
    /// The indexed file or its open handle changed.
    pub fn metadata(&self) -> Result<&Document, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.document)
    }
    /// # Errors
    /// The indexed file or its open handle changed.
    pub fn source_path(&self) -> Result<&Path, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.snapshot.path)
    }
}
