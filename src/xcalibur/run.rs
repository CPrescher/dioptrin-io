//! Xcalibur `.run` descriptions and their counted 88-byte sweeps.
//!
//! Field schema adapted from MIT FabIO xcaliburimage.py (ESRF, Jérôme Kieffer);
//! permission notice: licenses/native-readers/FabIO-Xcalibur-MIT.txt. Unknown
//! fields, all floating-point bits, fixed text octets and trailers are retained.
use super::{Error, Limits, Text};
use crate::source::Snapshot;
use std::{fs::File, io::Read, path::Path};

pub const HEADER_BYTES: usize = 528;
pub const SWEEP_BYTES: usize = 88;

/// What remains immutable: an owned byte copy, a captured stream, or a checked
/// file snapshot. Buffer/stream values do not claim to monitor an original file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    Buffer,
    StreamCapture,
    PathSnapshot,
}

#[derive(Debug)]
pub struct Sweep {
    pub scan_index: u16,
    pub scan_type: u16,
    pub unknown1: u32,
    pub omega: f64,
    pub theta: f64,
    pub kappa: f64,
    pub phi: f64,
    pub start: f64,
    pub end: f64,
    pub width: f64,
    pub unknown2: f64,
    pub unknown: [u16; 4],
    pub exposure: f64,
    pub original: [u8; SWEEP_BYTES],
}
impl Sweep {
    /// Read exactly one source record; reserved fields and non-finite float bits
    /// remain source values. Extra bytes must be handled by the containing run.
    /// # Errors
    /// The record is not exactly 88 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let original: [u8; SWEEP_BYTES] = bytes
            .try_into()
            .map_err(|_| Error("Xcalibur sweep must be exactly 88 bytes".into()))?;
        let u16_at = |offset| u16::from_le_bytes(original[offset..offset + 2].try_into().unwrap());
        let f64_at = |offset| f64::from_le_bytes(original[offset..offset + 8].try_into().unwrap());
        Ok(Self {
            scan_index: u16_at(0),
            scan_type: u16_at(2),
            unknown1: u32::from_le_bytes(original[4..8].try_into().unwrap()),
            omega: f64_at(8),
            theta: f64_at(16),
            kappa: f64_at(24),
            phi: f64_at(32),
            start: f64_at(40),
            end: f64_at(48),
            width: f64_at(56),
            unknown2: f64_at(64),
            unknown: [u16_at(72), u16_at(74), u16_at(76), u16_at(78)],
            exposure: f64_at(80),
            original,
        })
    }
}

#[derive(Debug)]
pub struct RunDescription {
    pub prefix: Text,
    pub folder: Text,
    /// Original unsigned 64-bit declaration, independently checked before any
    /// allocation or sweep iteration; never truncated to a narrower integer.
    pub declared_sweeps: u64,
    pub unknown: f64,
    pub sweeps: Vec<Sweep>,
    pub parsed_bytes: usize,
    pub original: Vec<u8>,
    pub provenance: Provenance,
}
impl RunDescription {
    /// Copy and parse a bounded immutable byte buffer, without text conversion.
    /// # Errors
    /// Truncated counted records, count/arithmetic overflow or resource limits.
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self, Error> {
        Self::admit(bytes, limits)?;
        let mut original = Vec::new();
        original
            .try_reserve_exact(bytes.len())
            .map_err(|e| Error(e.to_string()))?;
        original.extend_from_slice(bytes);
        Self::parse_owned(original, limits, Provenance::Buffer)
    }
    /// Capture at most the encoded/metadata byte budget plus one detection byte
    /// from any `Read`, including a nonseekable source. Later changes to that
    /// original stream are outside the owned capture's contract.
    /// # Errors
    /// I/O failures, byte/record limits or truncated counted source records.
    pub fn from_reader(reader: impl Read, limits: Limits) -> Result<Self, Error> {
        let bytes = capture(reader, limits)?;
        Self::parse_owned(bytes, limits, Provenance::StreamCapture)
    }
    #[must_use]
    pub fn trailing_bytes(&self) -> &[u8] {
        &self.original[self.parsed_bytes..]
    }
    fn admit(bytes: &[u8], limits: Limits) -> Result<(usize, usize), Error> {
        limits.validate()?;
        if HEADER_BYTES > limits.header_bytes
            || bytes.len() > limits.encoded_bytes.min(limits.metadata_bytes)
        {
            return Err(Error("Xcalibur run byte/header limit".into()));
        }
        if bytes.len() < HEADER_BYTES {
            return Err(Error("truncated Xcalibur run header".into()));
        }
        let count = u64::from_le_bytes(bytes[512..520].try_into().unwrap());
        let count = usize::try_from(count)
            .ok()
            .filter(|&count| count <= limits.frames)
            .ok_or_else(|| Error("Xcalibur run sweep count limit".into()))?;
        let parsed_bytes = count
            .checked_mul(SWEEP_BYTES)
            .and_then(|n| n.checked_add(HEADER_BYTES))
            .ok_or_else(|| Error("Xcalibur run counted size overflow".into()))?;
        if parsed_bytes > bytes.len() {
            return Err(Error("truncated Xcalibur run counted sweeps".into()));
        }
        count
            .checked_mul(std::mem::size_of::<Sweep>())
            .and_then(|n| n.checked_add(bytes.len()))
            .and_then(|n| n.checked_add(std::mem::size_of::<Self>()))
            .filter(|&n| n <= limits.metadata_bytes)
            .ok_or_else(|| Error("Xcalibur run aggregate metadata limit".into()))?;
        Ok((count, parsed_bytes))
    }
    fn parse_owned(bytes: Vec<u8>, limits: Limits, provenance: Provenance) -> Result<Self, Error> {
        let (count, parsed_bytes) = Self::admit(&bytes, limits)?;
        let mut sweeps = Vec::new();
        sweeps
            .try_reserve_exact(count)
            .map_err(|e| Error(e.to_string()))?;
        for record in bytes[HEADER_BYTES..parsed_bytes]
            .as_chunks::<SWEEP_BYTES>()
            .0
        {
            sweeps.push(Sweep::from_bytes(record)?);
        }
        Ok(Self {
            prefix: Text(bytes[..256].try_into().unwrap()),
            folder: Text(bytes[256..512].try_into().unwrap()),
            declared_sweeps: u64::from_le_bytes(bytes[512..520].try_into().unwrap()),
            unknown: f64::from_le_bytes(bytes[520..528].try_into().unwrap()),
            sweeps,
            parsed_bytes,
            original: bytes,
            provenance,
        })
    }
}
fn capture(reader: impl Read, limits: Limits) -> Result<Vec<u8>, Error> {
    limits.validate()?;
    let bound = limits.encoded_bytes.min(limits.metadata_bytes);
    let count = u64::try_from(bound)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| Error("Xcalibur run capture limit overflow".into()))?;
    let mut bytes = Vec::new();
    let mut reader = reader.take(count);
    let mut chunk = [0u8; 8192];
    loop {
        let length = reader.read(&mut chunk)?;
        if length == 0 {
            break;
        }
        bytes
            .len()
            .checked_add(length)
            .filter(|&n| n <= bound)
            .ok_or_else(|| Error("Xcalibur run capture byte limit".into()))?;
        bytes
            .try_reserve_exact(length)
            .map_err(|e| Error(e.to_string()))?;
        bytes.extend_from_slice(&chunk[..length]);
    }
    Ok(bytes)
}

/// Path reader holding an immutable source identity and the original handle.
/// Every metadata access checks replacement, growth and handle identity again.
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: RunDescription,
}
impl Reader {
    /// # Errors
    /// Source mutation, malformed/truncated records, I/O or resource limits.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if snapshot.bytes() > limits.encoded_bytes.min(limits.metadata_bytes) as u64 {
            return Err(Error("Xcalibur run source byte limit".into()));
        }
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let bytes = capture(&mut file, limits)?;
        let metadata = RunDescription::parse_owned(bytes, limits, Provenance::PathSnapshot)?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            metadata,
        })
    }
    /// # Errors
    /// The original source or open handle changed after indexing.
    pub fn metadata(&self) -> Result<&RunDescription, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.metadata)
    }
    /// # Errors
    /// The original source or open handle changed after indexing.
    pub fn source_path(&self) -> Result<&Path, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.snapshot.path)
    }
}
