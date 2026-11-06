//! Bounded legacy labeled ASCII tables. This is a table API, not detector frames.
//! Original bytes, label order and numeric lexemes survive float64 interpretation.
pub use crate::binary::Error;
use crate::source::Snapshot;
use std::{fs::File, io::Read, ops::Range, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Ascii,
    /// Reserved explicit selection. The pinned legacy C reader has no binary
    /// implementation or defined endian/ABI schema, so this always errors.
    Binary,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    PathSnapshot,
    Buffer,
    Stream,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source_bytes: usize,
    pub line_bytes: usize,
    pub rows: usize,
    pub columns: usize,
    pub value_bytes: usize,
    pub metadata_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 16 * 1024 * 1024,
            line_bytes: 64 * 1024,
            rows: 1_000_000,
            columns: 4096,
            value_bytes: 128 * 1024 * 1024,
            metadata_bytes: 64 * 1024 * 1024,
        }
    }
}
impl Limits {
    fn validate(self, format: Format) -> Result<(), Error> {
        if format != Format::Ascii {
            return Err(Error("legacy columnfile binary schema is not implemented by the pinned reference; select ASCII explicitly".into()));
        }
        if [
            self.source_bytes,
            self.line_bytes,
            self.rows,
            self.columns,
            self.value_bytes,
            self.metadata_bytes,
        ]
        .iter()
        .any(|&n| n == 0 || n == usize::MAX)
        {
            return Err(Error(
                "columnfile limits must be positive and finite".into(),
            ));
        }
        Ok(())
    }
    fn charge(self, used: &mut usize, bytes: usize) -> Result<(), Error> {
        *used = used
            .checked_add(bytes)
            .filter(|&n| n <= self.metadata_bytes)
            .ok_or_else(|| Error("columnfile aggregate metadata limit".into()))?;
        Ok(())
    }
}
/// An owned immutable interpretation. Samples are row-major float64, as in the
/// legacy ASCII format; textual integers are not reinterpreted as integer arrays.
/// All numeric lexemes and opaque byte labels remain recoverable from `original`.
pub struct Table {
    original: Vec<u8>,
    labels: Vec<Range<usize>>,
    comments: Vec<Range<usize>>,
    lexemes: Vec<Range<usize>>,
    values: Vec<f64>,
    shape: [usize; 2],
    provenance: Provenance,
}
impl Table {
    /// # Errors
    /// Unsupported binary selection, malformed labels/rows/numbers or bounds.
    pub fn from_bytes(bytes: &[u8], format: Format, limits: Limits) -> Result<Self, Error> {
        limits.validate(format)?;
        if bytes.len() > limits.source_bytes.min(limits.metadata_bytes) {
            return Err(Error("columnfile source byte limit".into()));
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bytes.len())
            .map_err(|e| Error(e.to_string()))?;
        owned.extend_from_slice(bytes);
        Self::parse(owned, limits, Provenance::Buffer)
    }
    /// Capture current position through EOF without seeking or retaining caller
    /// input. Errors do not rewind; oversize detection consumes at most limit+1.
    /// No filename, external origin or compression is inferred.
    /// # Errors
    /// Invalid options, I/O, malformed table or resource limits.
    pub fn from_reader(reader: impl Read, format: Format, limits: Limits) -> Result<Self, Error> {
        limits.validate(format)?;
        Self::parse(capture(reader, limits)?, limits, Provenance::Stream)
    }
    #[must_use]
    pub fn shape(&self) -> [usize; 2] {
        self.shape
    }
    #[must_use]
    pub fn values(&self) -> &[f64] {
        &self.values
    }
    pub fn labels(&self) -> impl ExactSizeIterator<Item = &[u8]> {
        self.labels.iter().map(|r| &self.original[r.clone()])
    }
    /// Complete original # lines, including the label row and line endings.
    pub fn comments(&self) -> impl ExactSizeIterator<Item = &[u8]> {
        self.comments.iter().map(|r| &self.original[r.clone()])
    }
    /// One original token per value, in row-major source order.
    pub fn lexemes(&self) -> impl ExactSizeIterator<Item = &[u8]> {
        self.lexemes.iter().map(|r| &self.original[r.clone()])
    }
    /// For signed/unsigned decimal integer lexemes, whether the float64 decoded
    /// view represents that integer exactly. Other numeric spellings return None
    /// (no exactness claim). The authoritative lexemes remain unchanged.
    pub fn integer_exactness(&self) -> impl ExactSizeIterator<Item = Option<bool>> {
        self.lexemes().zip(&self.values).map(|(token, &value)| {
            let (negative, digits) = match token.first() {
                Some(b'-') => (true, &token[1..]),
                Some(b'+') => (false, &token[1..]),
                _ => (false, token),
            };
            if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
                return None;
            }
            let first = digits.iter().position(|&b| b != b'0');
            let digits = first.map_or(&b"0"[..], |i| &digits[i..]);
            if !value.is_finite() {
                return Some(false);
            }
            let rendered = format!("{value:.0}");
            let rendered = rendered.as_bytes();
            let rendered = if rendered.first() == Some(&b'-') {
                &rendered[1..]
            } else {
                rendered
            };
            Some(rendered == digits && (first.is_none() || negative == value.is_sign_negative()))
        })
    }
    #[must_use]
    pub fn original(&self) -> &[u8] {
        &self.original
    }
    #[must_use]
    pub fn provenance(&self) -> Provenance {
        self.provenance
    }
    fn parse(original: Vec<u8>, limits: Limits, provenance: Provenance) -> Result<Self, Error> {
        if original.first() != Some(&b'#') || original.contains(&0) {
            return Err(Error(
                "columnfile requires a # label row and no NUL bytes".into(),
            ));
        }
        let mut used = 0;
        limits.charge(&mut used, original.len())?;
        limits.charge(&mut used, std::mem::size_of::<Self>())?;
        let mut labels = Vec::new();
        let mut comments = Vec::new();
        let mut lexemes = Vec::new();
        let mut values = Vec::new();
        let mut offset = 0;
        let mut rows = 0usize;
        for (number, line) in original.split_inclusive(|&b| b == b'\n').enumerate() {
            if line.len() > limits.line_bytes {
                return Err(Error("columnfile line byte limit".into()));
            }
            let end = offset + line.len();
            let first = line.iter().position(|b| !b.is_ascii_whitespace());
            if number == 0 || first.is_some_and(|i| line[i] == b'#') {
                limits.charge(&mut used, std::mem::size_of::<Range<usize>>())?;
                comments
                    .try_reserve_exact(1)
                    .map_err(|e| Error(e.to_string()))?;
                comments.push(offset..end);
                if number == 0 {
                    for span in spans(&original, offset + 1, end) {
                        if labels.len() >= limits.columns {
                            return Err(Error("columnfile column limit".into()));
                        }
                        limits.charge(&mut used, std::mem::size_of::<Range<usize>>())?;
                        labels
                            .try_reserve_exact(1)
                            .map_err(|e| Error(e.to_string()))?;
                        labels.push(span);
                    }
                    if labels.is_empty() {
                        return Err(Error("columnfile has no column labels".into()));
                    }
                }
            } else {
                if rows >= limits.rows {
                    return Err(Error("columnfile row limit".into()));
                }
                let mut columns = 0;
                for span in spans(&original, offset, end) {
                    if columns >= labels.len() {
                        return Err(Error("columnfile row has extra values".into()));
                    }
                    let value = std::str::from_utf8(&original[span.clone()])
                        .ok()
                        .and_then(|s| s.parse::<f64>().ok())
                        .ok_or_else(|| Error("columnfile invalid float64 token".into()))?;
                    values
                        .len()
                        .checked_add(1)
                        .and_then(|n| n.checked_mul(8))
                        .filter(|&n| n <= limits.value_bytes)
                        .ok_or_else(|| Error("columnfile decoded value byte limit".into()))?;
                    limits.charge(&mut used, std::mem::size_of::<Range<usize>>())?;
                    lexemes
                        .try_reserve_exact(1)
                        .map_err(|e| Error(e.to_string()))?;
                    values
                        .try_reserve_exact(1)
                        .map_err(|e| Error(e.to_string()))?;
                    lexemes.push(span);
                    values.push(value);
                    columns += 1;
                }
                if columns != labels.len() {
                    return Err(Error("columnfile row has missing values".into()));
                }
                rows = rows
                    .checked_add(1)
                    .ok_or_else(|| Error("columnfile row count overflow".into()))?;
            }
            offset = end;
        }
        let shape = [rows, labels.len()];
        Ok(Self {
            original,
            labels,
            comments,
            lexemes,
            values,
            shape,
            provenance,
        })
    }
}
fn spans(bytes: &[u8], mut offset: usize, end: usize) -> impl Iterator<Item = Range<usize>> + '_ {
    std::iter::from_fn(move || {
        while offset < end && bytes[offset].is_ascii_whitespace() {
            offset += 1;
        }
        let start = offset;
        while offset < end && !bytes[offset].is_ascii_whitespace() {
            offset += 1;
        }
        (start < offset).then_some(start..offset)
    })
}
fn capture(reader: impl Read, limits: Limits) -> Result<Vec<u8>, Error> {
    let bound = limits.source_bytes.min(limits.metadata_bytes);
    let count = u64::try_from(bound)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| Error("columnfile capture limit overflow".into()))?;
    let mut reader = reader.take(count);
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        if bytes.len().checked_add(n).is_none_or(|n| n > bound) {
            return Err(Error("columnfile source byte limit".into()));
        }
        bytes
            .try_reserve_exact(n)
            .map_err(|e| Error(e.to_string()))?;
        bytes.extend_from_slice(&chunk[..n]);
    }
    Ok(bytes)
}
/// A bounded decoded table snapshot. Path accesses recheck both the caller's
/// path binding and the original open handle. Reopen after changes; no refresh.
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    table: Table,
}
impl Reader {
    /// # Errors
    /// Nonregular/changed source, malformed table, unsupported format or bounds.
    pub fn open(path: impl AsRef<Path>, format: Format, limits: Limits) -> Result<Self, Error> {
        limits.validate(format)?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if !std::fs::metadata(&snapshot.path)?.is_file() {
            return Err(Error("columnfile requires a regular source file".into()));
        }
        if snapshot.bytes() > limits.source_bytes.min(limits.metadata_bytes) as u64 {
            return Err(Error("columnfile source byte limit".into()));
        }
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let table = Table::parse(
            capture(&mut file, limits)?,
            limits,
            Provenance::PathSnapshot,
        )?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            table,
        })
    }
    /// # Errors
    /// Original path, source or handle changed since capture.
    pub fn table(&self) -> Result<&Table, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.table)
    }
    /// # Errors
    /// Original path, source or handle changed since capture.
    pub fn source_path(&self) -> Result<&Path, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.snapshot.path)
    }
}
