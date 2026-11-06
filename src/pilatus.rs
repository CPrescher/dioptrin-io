//! Shared bounded Pilatus acquisition metadata for TIFF and CBF.
//! Field schema follows MIT FabIO cbfimage.PilatusHeader, ESRF. Retained notice:
//! licenses/native-readers/FabIO-2026.6.0-copyright.txt. Original text is retained.
use std::collections::BTreeMap;
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Integer(i64),
    Float(f64),
    Sequence(Vec<Value>),
    Invalid(String),
}
#[derive(Debug, Clone)]
pub struct Header {
    /// Original ImageDescription text, including comments the field parser does
    /// not interpret. Original TIFF tag values are also retained by image metadata.
    pub description: String,
    /// Typed acquisition values, without applying corrections or changing units.
    pub typed_fields: BTreeMap<String, Value>,
    /// Last assignment to each key, as in the pinned reference. Units remain text.
    pub fields: BTreeMap<String, String>,
    /// Every interpreted assignment, retaining duplicate keys and source order.
    pub entries: Vec<(String, String)>,
    pub unparsed_lines: Vec<String>,
}
// Python str.splitlines boundaries, borrowed without building a line index.
// CRLF is one delimiter; a trailing delimiter does not invent another line.
#[derive(Clone)]
struct SourceLines<'a> {
    remaining: &'a str,
}
impl<'a> SourceLines<'a> {
    fn new(source: &'a str) -> Self {
        Self { remaining: source }
    }
}
impl<'a> Iterator for SourceLines<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }
        for (index, character) in self.remaining.char_indices() {
            if matches!(
                character,
                '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}'
                    ..='\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}'
            ) {
                let mut next = index + character.len_utf8();
                if character == '\r' && self.remaining.as_bytes().get(next) == Some(&b'\n') {
                    next += 1;
                }
                let line = &self.remaining[..index];
                self.remaining = &self.remaining[next..];
                return Some(line);
            }
        }
        let line = self.remaining;
        self.remaining = "";
        Some(line)
    }
}
fn source_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}
fn words(line: &str) -> impl Iterator<Item = &str> {
    line.split(source_space).filter(|word| !word.is_empty())
}
fn cleaned_source(description: &str) -> CleanedLines {
    let mut source = SourceLines::new(description).peekable();
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut diagnostics = Vec::new();
    while let Some(line) = source.next() {
        current.extend(
            line.chars()
                .map(|c| if "()#:=,".contains(c) { ' ' } else { c }),
        );
        if current.ends_with('\\') && source.peek().is_some() {
            current.pop();
            continue;
        }
        if current.ends_with('\\') {
            diagnostics.push("terminal Pilatus continuation retained in original and cleaned view; pinned cleaner indexes beyond its source".into());
        }
        lines.push(std::mem::take(&mut current));
    }
    if lines.iter().any(|line| words(line).count() == 1) {
        diagnostics.push("single-token cleaned line cannot satisfy all indexed keyword positions in the pinned constructor".into());
    }
    CleanedLines { lines, diagnostics }
}
fn parse_text(description: &str) -> Header {
    let mut header = Header {
        description: description.into(),
        typed_fields: BTreeMap::new(),
        fields: BTreeMap::new(),
        entries: Vec::new(),
        unparsed_lines: Vec::new(),
    };
    for line in SourceLines::new(description) {
        let original = line.trim_matches(|c: char| source_space(c) || c == '\0');
        if original.is_empty() {
            continue;
        }
        let Some(text) = original.strip_prefix("# ") else {
            header.unparsed_lines.push(line.into());
            continue;
        };
        let text = text.trim_matches(source_space);
        let Some(position) = text.find(|c: char| source_space(c) || [',', ':', '='].contains(&c))
        else {
            header.unparsed_lines.push(line.into());
            continue;
        };
        let key = text[..position].trim();
        let value = text[position..]
            .trim_start_matches(|c: char| source_space(c) || [',', ':', '='].contains(&c));
        if key.is_empty() {
            header.unparsed_lines.push(line.into());
            continue;
        }
        header.entries.push((key.into(), value.into()));
        header.fields.insert(key.into(), value.into());
    }
    header
}

impl Header {
    /// Parse acquisition text without performing any detector correction.
    /// # Errors
    /// The conservative retained-output estimate exceeds max_metadata_bytes.
    pub fn parse(description: &str, max_metadata_bytes: usize) -> Result<Self, String> {
        let lines = SourceLines::new(description).count();
        let cost = description
            .len()
            .checked_mul(8)
            .and_then(|n| n.checked_add(lines.checked_mul(512)?))
            .filter(|&n| n <= max_metadata_bytes)
            .ok_or("Pilatus text exceeds metadata budget")?;
        let _ = cost;
        let mut result = parse_text(description);
        let cleaned = cleaned_source(description);
        for line in &cleaned.lines {
            let word = |index| words(line).nth(index);
            let Some(key) = word(0) else {
                continue;
            };
            let number = |index: usize| {
                word(index)
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(Value::Float)
            };
            let text = |index: usize| word(index).map(|value| Value::Text(value.into()));
            let value = if word(1) == Some("sensor") {
                match (text(0), number(3)) {
                    (Some(a), Some(b)) => Some(("sensor", Value::Sequence(vec![a, b]))),
                    _ => None,
                }
            } else if key == "Detector" {
                let mut value = String::new();
                for part in words(line).skip(1) {
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    value.push_str(part);
                }
                Some((key, Value::Text(value)))
            } else if ["Pixel_size", "Beam_xy", "Energy_range", "Gain_setting"].contains(&key) {
                let second = if key == "Pixel_size" { 4 } else { 2 };
                let values = if key == "Gain_setting" {
                    (text(1), text(second))
                } else {
                    (number(1), number(second))
                };
                match values {
                    (Some(a), Some(b)) => Some((key, Value::Sequence(vec![a, b]))),
                    _ => None,
                }
            } else if ["Count_cutoff", "N_excluded_pixels", "N_oscillations"].contains(&key) {
                word(1)
                    .and_then(|value| value.parse().ok())
                    .map(|n| (key, Value::Integer(n)))
            } else if [
                "Excluded_pixels",
                "Flat_field",
                "Trim_file",
                "Image_path",
                "Oscillation_axis",
            ]
            .contains(&key)
            {
                text(1).map(|value| (key, value))
            } else if [
                "Exposure_time",
                "Exposure_period",
                "Tau",
                "Threshold_setting",
                "Wavelength",
                "Detector_distance",
                "Detector_Voffset",
                "Flux",
                "Filter_transmission",
                "Start_angle",
                "Angle_increment",
                "Detector_2theta",
                "Polarization",
                "Alpha",
                "Kappa",
                "Phi",
                "Phi_increment",
                "Chi",
                "Chi_increment",
                "Omega",
                "Omega_increment",
                "Start_position",
                "Position_increment",
                "Shutter_time",
            ]
            .contains(&key)
            {
                number(1).map(|value| (key, value))
            } else {
                continue;
            };
            if let Some((key, value)) = value {
                result.typed_fields.insert(key.into(), value);
            } else {
                result
                    .typed_fields
                    .insert(key.into(), Value::Invalid(line.clone()));
            }
        }
        Ok(result)
    }
}

#[derive(Debug, Clone)]
pub struct CleanedLines {
    pub lines: Vec<String>,
    pub diagnostics: Vec<String>,
}
/// Explicit derived delimiter/continuation view; raw text remains in Document.
/// Python-compatible borrowed line counting admits its output before allocation.
pub fn clean_string(description: &str, max_metadata_bytes: usize) -> Result<CleanedLines, String> {
    description
        .len()
        .checked_mul(4)
        .and_then(|n| n.checked_add(SourceLines::new(description).count().checked_mul(512)?))
        .filter(|&n| n <= max_metadata_bytes)
        .ok_or("Pilatus cleaned view exceeds metadata bound")?;
    Ok(cleaned_source(description))
}
pub const CONVENTION: &str = "PILATUS_1.2";
pub use crate::binary::{Error, Limits};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    Buffer,
    StreamCapture,
    PathSnapshot,
}
/// Original text plus the existing acquisition projection. Unknown fields,
/// units, repetitions and source order remain in Header and the raw bytes.
#[derive(Debug)]
pub struct Document {
    pub original: Vec<u8>,
    pub header: Header,
    pub convention: String,
    pub diagnostics: Vec<String>,
    pub provenance: Provenance,
}
impl Header {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.typed_fields.get(key)
    }
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.typed_fields.contains_key(key)
    }
}
impl Document {
    pub fn cleaned_lines(&self, max_metadata_bytes: usize) -> Result<CleanedLines, Error> {
        clean_string(&self.header.description, max_metadata_bytes).map_err(Error)
    }
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self, Error> {
        Self::from_bytes_with_convention(bytes, CONVENTION, limits)
    }
    pub fn from_bytes_with_convention(
        bytes: &[u8],
        convention: &str,
        limits: Limits,
    ) -> Result<Self, Error> {
        admit(bytes.len(), limits)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bytes.len())
            .map_err(|e| Error(e.to_string()))?;
        owned.extend_from_slice(bytes);
        Self::parse_owned(owned, convention, limits, Provenance::Buffer)
    }
    pub fn from_reader(reader: impl std::io::Read, limits: Limits) -> Result<Self, Error> {
        Self::parse_owned(
            capture(reader, limits)?,
            CONVENTION,
            limits,
            Provenance::StreamCapture,
        )
    }
    fn parse_owned(
        original: Vec<u8>,
        convention: &str,
        limits: Limits,
        provenance: Provenance,
    ) -> Result<Self, Error> {
        let remaining = admit(original.len(), limits)?;
        if convention != CONVENTION {
            return Err(Error(
                "unsupported Pilatus header convention; no default substitution".into(),
            ));
        }
        let text = std::str::from_utf8(&original).map_err(|_| {
            Error("Pilatus acquisition text must be UTF8; no lossy conversion".into())
        })?;
        let header = Header::parse(text, remaining).map_err(Error)?;
        let mut diagnostics = clean_string(text, remaining).map_err(Error)?.diagnostics;
        for (key, value) in &header.typed_fields {
            if matches!(value, Value::Invalid(_)) {
                diagnostics.push(format!(
                    "recognized Pilatus field {key} is malformed; lexical source retained"
                ));
            }
        }
        let extra = diagnostics
            .iter()
            .try_fold(0usize, |n, message| {
                n.checked_add(message.len() + std::mem::size_of::<String>())
            })
            .ok_or_else(|| Error("Pilatus diagnostic size overflow".into()))?;
        // Header::parse uses a conservative aggregate estimate. Reserve the
        // separately retained diagnostics as well, with no parser fallback.
        let lines = SourceLines::new(text).count();
        let total = text
            .len()
            .checked_mul(8)
            .and_then(|n| n.checked_add(lines.checked_mul(512)?))
            .and_then(|n| n.checked_add(extra))
            .ok_or_else(|| Error("Pilatus aggregate metadata overflow".into()))?;
        if total > remaining {
            return Err(Error("Pilatus aggregate metadata limit".into()));
        }
        Ok(Self {
            original,
            header,
            convention: convention.into(),
            diagnostics,
            provenance,
        })
    }
}
fn admit(bytes: usize, limits: Limits) -> Result<usize, Error> {
    limits.validate()?;
    if bytes > limits.encoded_bytes.min(limits.header_bytes) {
        return Err(Error("Pilatus source/header limit".into()));
    }
    limits
        .metadata_bytes
        .checked_sub(bytes)
        .and_then(|n| n.checked_sub(std::mem::size_of::<Document>()))
        .filter(|&n| n > 0)
        .ok_or_else(|| Error("Pilatus aggregate metadata limit".into()))
}
fn capture(reader: impl std::io::Read, limits: Limits) -> Result<Vec<u8>, Error> {
    use std::io::Read;
    limits.validate()?;
    let bound = limits
        .encoded_bytes
        .min(limits.header_bytes)
        .min(limits.metadata_bytes);
    let n = u64::try_from(bound)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| Error("Pilatus capture bound overflow".into()))?;
    let mut reader = reader.take(n);
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        bytes
            .len()
            .checked_add(n)
            .filter(|&n| n <= bound)
            .ok_or_else(|| Error("Pilatus capture byte limit".into()))?;
        bytes
            .try_reserve_exact(n)
            .map_err(|e| Error(e.to_string()))?;
        bytes.extend_from_slice(&chunk[..n]);
    }
    Ok(bytes)
}
pub struct Reader {
    snapshot: crate::source::Snapshot,
    file: std::fs::File,
    document: Document,
}
impl Reader {
    pub fn open(path: impl AsRef<std::path::Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = crate::source::Snapshot::open(path).map_err(Error)?;
        if !std::fs::metadata(&snapshot.path)?.is_file() {
            return Err(Error("Pilatus header requires regular file".into()));
        }
        if snapshot.bytes()
            > limits
                .encoded_bytes
                .min(limits.header_bytes)
                .min(limits.metadata_bytes) as u64
        {
            return Err(Error("Pilatus source byte limit".into()));
        }
        let mut file = std::fs::File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let document = Document::parse_owned(
            capture(&mut file, limits)?,
            CONVENTION,
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
    pub fn metadata(&self) -> Result<&Document, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.document)
    }
    pub fn source_path(&self) -> Result<&std::path::Path, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.snapshot.path)
    }
}
