//! Strict, source-preserving CIF 1.1 syntax, separate from the detector's
//! deliberately permissive header projection. No scientific values are coerced.
use super::{Error, document::Provenance};
use crate::source::Snapshot;
use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    Cif11,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Bare,
    SingleQuoted,
    DoubleQuoted,
    TextField,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub span: Span,
    pub content: Span,
    pub style: Style,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Null {
    Unknown,
    Inapplicable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Value {
    pub token: Token,
    pub null: Option<Null>,
}
#[derive(Debug)]
pub struct Scalar {
    pub name: Token,
    pub value: Value,
    pub span: Span,
}
#[derive(Debug)]
pub struct Loop {
    pub columns: Vec<Token>,
    pub values: Vec<Value>,
    pub span: Span,
}
impl Loop {
    /// Flat values preserve row-major source order. Parser-created loops have
    /// at least one column and complete rows.
    pub fn rows(&self) -> impl Iterator<Item = &[Value]> {
        self.values.chunks(self.columns.len().max(1))
    }
}
#[derive(Debug)]
pub struct SaveFrame {
    pub name: Token,
    pub items: Vec<Item>,
    pub span: Span,
}
#[derive(Debug)]
pub enum Item {
    Scalar(Scalar),
    Loop(Loop),
    SaveFrame(SaveFrame),
}
#[derive(Debug)]
pub struct Block {
    pub name: Token,
    pub items: Vec<Item>,
    pub span: Span,
}

/// Application resource limits; CIF 1.1 character, line and name restrictions
/// are fixed grammar rules and are not relaxed by raising these limits.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source_bytes: usize,
    pub metadata_bytes: usize,
    pub tokens: usize,
    pub blocks: usize,
    pub items: usize,
    pub loop_columns: usize,
    pub values: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 16 * 1024 * 1024,
            metadata_bytes: 64 * 1024 * 1024,
            tokens: 1_000_000,
            blocks: 10_000,
            items: 100_000,
            loop_columns: 10_000,
            values: 1_000_000,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<(), Error> {
        if [
            self.source_bytes,
            self.metadata_bytes,
            self.tokens,
            self.blocks,
            self.items,
            self.loop_columns,
            self.values,
        ]
        .iter()
        .any(|v| *v == 0 || *v == usize::MAX)
        {
            return Err(Error(
                "CIF syntax limits must be positive and finite".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct Document {
    pub original: Vec<u8>,
    pub version: Version,
    pub provenance: Provenance,
    pub blocks: Vec<Block>,
}
impl Document {
    /// # Errors
    /// Unsupported version, malformed CIF 1.1 syntax or resource bounds.
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self, Error> {
        admit(bytes.len(), limits)?;
        let mut original = Vec::new();
        original
            .try_reserve_exact(bytes.len())
            .map_err(|e| Error(e.to_string()))?;
        original.extend_from_slice(bytes);
        Self::owned(original, limits, Provenance::Buffer)
    }
    /// Capture through EOF without seeking. The result is an owned snapshot;
    /// original stream/file changes after successful capture do not affect it.
    /// # Errors
    /// I/O, unsupported version, malformed syntax or resource bounds.
    pub fn from_reader(reader: impl Read, limits: Limits) -> Result<Self, Error> {
        Self::owned(capture(reader, limits)?, limits, Provenance::StreamCapture)
    }
    fn owned(original: Vec<u8>, limits: Limits, provenance: Provenance) -> Result<Self, Error> {
        admit(original.len(), limits)?;
        validate_source(&original)?;
        let source = std::str::from_utf8(&original).map_err(|e| Error(e.to_string()))?;
        let mut parser = Parser {
            source,
            position: 0,
            pending: None,
            limits,
            cost: original.len() + std::mem::size_of::<Self>(),
            tokens: 0,
            items: 0,
            values: 0,
        };
        let blocks = parser.blocks()?;
        Ok(Self {
            original,
            version: Version::Cif11,
            provenance,
            blocks,
        })
    }
    #[must_use]
    pub fn raw(&self, token: Token) -> Option<&[u8]> {
        self.original.get(token.span.start..token.span.end)
    }
    /// Content excludes quotes/semicolon delimiters and a block/frame's keyword.
    /// Original CR/LF bytes inside text fields remain unchanged.
    #[must_use]
    pub fn text(&self, token: Token) -> Option<&str> {
        self.original
            .get(token.content.start..token.content.end)
            .and_then(|s| std::str::from_utf8(s).ok())
    }
    #[must_use]
    pub fn block(&self, name: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| {
            self.text(b.name)
                .is_some_and(|s| s.eq_ignore_ascii_case(name))
        })
    }
    #[must_use]
    pub fn scalar<'a>(&self, items: &'a [Item], name: &str) -> Option<&'a Scalar> {
        items.iter().find_map(|item| match item {
            Item::Scalar(s)
                if self
                    .text(s.name)
                    .is_some_and(|s| s.eq_ignore_ascii_case(name)) =>
            {
                Some(s)
            }
            _ => None,
        })
    }
    /// Case-insensitive loop-column lookup in one innermost block/frame scope.
    #[must_use]
    pub fn loop_column<'a>(&self, items: &'a [Item], name: &str) -> Option<(&'a Loop, usize)> {
        items.iter().find_map(|item| match item {
            Item::Loop(group) => group
                .columns
                .iter()
                .position(|token| {
                    self.text(*token)
                        .is_some_and(|s| s.eq_ignore_ascii_case(name))
                })
                .map(|index| (group, index)),
            _ => None,
        })
    }
    #[must_use]
    pub fn save_frame<'a>(&self, block: &'a Block, name: &str) -> Option<&'a SaveFrame> {
        block.items.iter().find_map(|item| match item {
            Item::SaveFrame(s)
                if self
                    .text(s.name)
                    .is_some_and(|s| s.eq_ignore_ascii_case(name)) =>
            {
                Some(s)
            }
            _ => None,
        })
    }
}
fn admit(bytes: usize, limits: Limits) -> Result<(), Error> {
    limits.validate()?;
    if bytes > limits.source_bytes
        || bytes
            .checked_add(std::mem::size_of::<Document>())
            .is_none_or(|n| n > limits.metadata_bytes)
    {
        return Err(Error("CIF syntax source/metadata byte limit".into()));
    }
    Ok(())
}
fn capture(reader: impl Read, limits: Limits) -> Result<Vec<u8>, Error> {
    limits.validate()?;
    let bound = limits.source_bytes.min(limits.metadata_bytes);
    let length = u64::try_from(bound)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| Error("CIF syntax capture limit overflow".into()))?;
    let mut reader = reader.take(length);
    let mut source = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        if source.len().checked_add(n).is_none_or(|v| v > bound) {
            return Err(Error("CIF syntax capture source byte limit".into()));
        }
        source
            .try_reserve_exact(n)
            .map_err(|e| Error(e.to_string()))?;
        source.extend_from_slice(&chunk[..n]);
    }
    Ok(source)
}
fn validate_source(bytes: &[u8]) -> Result<(), Error> {
    let magic = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    if magic.starts_with(b"#\\#CIF_") {
        let end = magic.iter().position(|v| ws(*v)).unwrap_or(magic.len());
        if &magic[..end] != b"#\\#CIF_1.1" {
            return Err(Error(
                "unsupported CIF syntax version; only CIF 1.1 is implemented".into(),
            ));
        }
    }
    let mut line = 0usize;
    for (position, &byte) in bytes.iter().enumerate() {
        if !(ws(byte) || (32..=126).contains(&byte)) {
            return Err(Error(format!(
                "CIF 1.1 forbidden character at byte {position}"
            )));
        }
        if byte == b'\r' || byte == b'\n' {
            line = 0;
        } else {
            line += 1;
            if line > 2048 {
                return Err(Error(format!(
                    "CIF 1.1 line exceeds 2048 characters at byte {position}"
                )));
            }
        }
    }
    Ok(())
}
fn ws(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// Shared scanner. The legacy mode deliberately preserves the existing
/// detector/document projection's permissive whitespace and quote behavior.
pub(super) fn scan(
    source: &str,
    position: &mut usize,
    strict: bool,
) -> Result<Option<Token>, &'static str> {
    let bytes = source.as_bytes();
    let whitespace = |v: u8| {
        if strict {
            ws(v)
        } else {
            v.is_ascii_whitespace()
        }
    };
    loop {
        while *position < bytes.len() && whitespace(bytes[*position]) {
            *position += 1;
        }
        if bytes.get(*position) != Some(&b'#') {
            break;
        }
        while *position < bytes.len()
            && bytes[*position] != b'\n'
            && (!strict || bytes[*position] != b'\r')
        {
            *position += 1;
        }
    }
    if *position == bytes.len() {
        return Ok(None);
    }
    let start = *position;
    let mut content_start = start;
    let content_end;
    let style;
    let bol = start == 0 || bytes[start - 1] == b'\n' || (strict && bytes[start - 1] == b'\r');
    if bytes[start] == b';' && bol {
        style = Style::TextField;
        content_start = start + 1;
        let mut end = content_start;
        loop {
            if end == bytes.len() {
                return Err("unterminated CIF semicolon value");
            }
            if bytes[end] == b';'
                && (bytes[end - 1] == b'\n' || (strict && bytes[end - 1] == b'\r'))
            {
                break;
            }
            end += 1;
        }
        content_end = if strict && bytes[end - 1] == b'\n' && end >= 2 && bytes[end - 2] == b'\r' {
            end - 2
        } else {
            end - 1
        };
        *position = end + 1;
        if strict && bytes.get(*position).is_some_and(|v| !ws(*v)) {
            return Err("CIF text field closing delimiter requires whitespace");
        }
    } else if b"'\"".contains(&bytes[start]) {
        let quote = bytes[start];
        style = if quote == b'\'' {
            Style::SingleQuoted
        } else {
            Style::DoubleQuoted
        };
        content_start = start + 1;
        *position += 1;
        loop {
            if *position == bytes.len() {
                return Err("unterminated CIF quoted value");
            }
            if strict && matches!(bytes[*position], b'\r' | b'\n') {
                return Err("CIF 1.1 quoted value cannot cross a line");
            }
            if bytes[*position] == quote && bytes.get(*position + 1).is_none_or(|v| whitespace(*v))
            {
                break;
            }
            *position += 1;
        }
        content_end = *position;
        *position += 1;
    } else {
        style = Style::Bare;
        while *position < bytes.len() && !whitespace(bytes[*position]) {
            *position += 1;
        }
        content_end = *position;
    }
    Ok(Some(Token {
        span: Span {
            start,
            end: *position,
        },
        content: Span {
            start: content_start,
            end: content_end,
        },
        style,
    }))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Data,
    Save,
    EndSave,
    Loop,
    Tag,
    Reserved,
    Value,
}
struct Parser<'a> {
    source: &'a str,
    position: usize,
    pending: Option<Token>,
    limits: Limits,
    cost: usize,
    tokens: usize,
    items: usize,
    values: usize,
}
impl Parser<'_> {
    fn text(&self, t: Token) -> &str {
        &self.source[t.content.start..t.content.end]
    }
    fn kind(&self, t: Token) -> Kind {
        if t.style != Style::Bare {
            return Kind::Value;
        }
        let s = self.text(t);
        if s.starts_with('_') {
            Kind::Tag
        } else if s.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("data_")) {
            Kind::Data
        } else if s.eq_ignore_ascii_case("save_") {
            Kind::EndSave
        } else if s.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("save_")) {
            Kind::Save
        } else if s.eq_ignore_ascii_case("loop_") {
            Kind::Loop
        } else if s.eq_ignore_ascii_case("stop_") || s.eq_ignore_ascii_case("global_") {
            Kind::Reserved
        } else {
            Kind::Value
        }
    }
    fn charge(&mut self, n: usize) -> Result<(), Error> {
        self.cost = self
            .cost
            .checked_add(n)
            .filter(|n| *n <= self.limits.metadata_bytes)
            .ok_or_else(|| Error("CIF syntax retained metadata limit".into()))?;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Token>, Error> {
        if self.pending.is_some() {
            return Ok(self.pending.take());
        }
        let token = scan(self.source, &mut self.position, true)
            .map_err(|s| Error(format!("{s} at byte {}", self.position)))?;
        if token.is_some() {
            self.tokens = self
                .tokens
                .checked_add(1)
                .filter(|n| *n <= self.limits.tokens)
                .ok_or_else(|| Error("CIF syntax token limit".into()))?;
            self.charge(128)?;
        }
        Ok(token)
    }
    fn peek(&mut self) -> Result<Option<Token>, Error> {
        if self.pending.is_none() {
            self.pending = self.next()?;
        }
        Ok(self.pending)
    }
    fn name(
        &mut self,
        mut token: Token,
        prefix: usize,
        names: &mut BTreeSet<String>,
    ) -> Result<Token, Error> {
        token.content.start += prefix;
        let name = self.text(token);
        if name.is_empty() || name.len() > 75 || (prefix == 0 && name.len() < 2) {
            return Err(Error(format!(
                "CIF 1.1 invalid or overlong name at byte {}",
                token.span.start
            )));
        }
        let key = name.to_ascii_lowercase();
        self.charge(key.len().saturating_mul(2).saturating_add(128))?;
        if !names.insert(key) {
            return Err(Error(format!(
                "duplicate case-insensitive CIF name at byte {}",
                token.span.start
            )));
        }
        Ok(token)
    }
    fn value(&mut self, token: Token) -> Result<Value, Error> {
        if self.kind(token) != Kind::Value {
            return Err(Error(format!(
                "CIF item has no value at byte {}",
                token.span.start
            )));
        }
        let text = self.text(token);
        if token.style == Style::Bare && text.as_bytes().first().is_some_and(|v| b"$[]".contains(v))
        {
            return Err(Error(format!(
                "reserved initial character in CIF 1.1 value at byte {}",
                token.span.start
            )));
        }
        let null = if token.style != Style::Bare {
            None
        } else if text == "?" {
            Some(Null::Unknown)
        } else if text == "." {
            Some(Null::Inapplicable)
        } else {
            None
        };
        self.values = self
            .values
            .checked_add(1)
            .filter(|n| *n <= self.limits.values)
            .ok_or_else(|| Error("CIF syntax value limit".into()))?;
        self.charge(128)?;
        Ok(Value { token, null })
    }
    fn blocks(&mut self) -> Result<Vec<Block>, Error> {
        let mut blocks = Vec::new();
        let mut names = BTreeSet::new();
        while let Some(token) = self.next()? {
            if self.kind(token) != Kind::Data {
                return Err(Error(format!(
                    "CIF content outside a data block at byte {}",
                    token.span.start
                )));
            }
            if blocks.len() >= self.limits.blocks {
                return Err(Error("CIF syntax block limit".into()));
            }
            self.charge(256)?;
            let name = self.name(token, 5, &mut names)?;
            let (items, end) = self.scope(false)?;
            blocks.try_reserve(1).map_err(|e| Error(e.to_string()))?;
            blocks.push(Block {
                name,
                items,
                span: Span {
                    start: token.span.start,
                    end,
                },
            });
        }
        Ok(blocks)
    }
    fn scope(&mut self, frame: bool) -> Result<(Vec<Item>, usize), Error> {
        let mut items = Vec::new();
        let mut tags = BTreeSet::new();
        let mut frames = BTreeSet::new();
        loop {
            let Some(token) = self.peek()? else {
                if frame {
                    return Err(Error("unterminated CIF save frame".into()));
                }
                return Ok((items, self.source.len()));
            };
            let kind = self.kind(token);
            if kind == Kind::Data {
                if frame {
                    return Err(Error(
                        "CIF data block starts before save frame terminator".into(),
                    ));
                }
                return Ok((items, token.span.start));
            }
            self.next()?;
            if kind == Kind::EndSave {
                if !frame || items.is_empty() {
                    return Err(Error(
                        "CIF save frame terminator without a nonempty frame".into(),
                    ));
                }
                return Ok((items, token.span.end));
            }
            self.items = self
                .items
                .checked_add(1)
                .filter(|n| *n <= self.limits.items)
                .ok_or_else(|| Error("CIF syntax item limit".into()))?;
            self.charge(256)?;
            let item = match kind {
                Kind::Tag => {
                    let name = self.name(token, 0, &mut tags)?;
                    let next = self
                        .next()?
                        .ok_or_else(|| Error("CIF item has no value at EOF".into()))?;
                    let value = self.value(next)?;
                    Item::Scalar(Scalar {
                        name,
                        value,
                        span: Span {
                            start: token.span.start,
                            end: next.span.end,
                        },
                    })
                }
                Kind::Loop => {
                    let mut columns = Vec::new();
                    let mut values = Vec::new();
                    while let Some(next) = self.peek()? {
                        if self.kind(next) != Kind::Tag {
                            break;
                        }
                        self.next()?;
                        if columns.len() >= self.limits.loop_columns {
                            return Err(Error("CIF syntax loop column limit".into()));
                        }
                        let name = self.name(next, 0, &mut tags)?;
                        columns.try_reserve(1).map_err(|e| Error(e.to_string()))?;
                        columns.push(name);
                    }
                    if columns.is_empty() {
                        return Err(Error("CIF loop has no columns".into()));
                    }
                    let mut end = columns.last().unwrap().span.end;
                    while let Some(next) = self.peek()? {
                        if self.kind(next) != Kind::Value {
                            break;
                        }
                        self.next()?;
                        let value = self.value(next)?;
                        values.try_reserve(1).map_err(|e| Error(e.to_string()))?;
                        values.push(value);
                        end = next.span.end;
                    }
                    if values.is_empty() || values.len() % columns.len() != 0 {
                        return Err(Error("CIF loop requires nonempty complete rows".into()));
                    }
                    Item::Loop(Loop {
                        columns,
                        values,
                        span: Span {
                            start: token.span.start,
                            end,
                        },
                    })
                }
                Kind::Save if !frame => {
                    let name = self.name(token, 5, &mut frames)?;
                    let (items, end) = self.scope(true)?;
                    Item::SaveFrame(SaveFrame {
                        name,
                        items,
                        span: Span {
                            start: token.span.start,
                            end,
                        },
                    })
                }
                _ => {
                    return Err(Error(format!(
                        "reserved or unexpected CIF token at byte {}",
                        token.span.start
                    )));
                }
            };
            items.try_reserve(1).map_err(|e| Error(e.to_string()))?;
            items.push(item);
        }
    }
}

/// Checked immutable path snapshot. Metadata never survives source mutation as
/// a successful getter result; captured bytes remain available only when valid.
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    document: Document,
}
impl Reader {
    /// # Errors
    /// I/O, changed sources, unsupported versions, syntax or resource bounds.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if !std::fs::metadata(&snapshot.path)?.is_file() {
            return Err(Error(
                "CIF syntax path snapshots require regular files".into(),
            ));
        }
        if snapshot.bytes()
            > u64::try_from(limits.source_bytes.min(limits.metadata_bytes))
                .map_err(|e| Error(e.to_string()))?
        {
            return Err(Error("CIF syntax source byte limit".into()));
        }
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let document = Document::owned(
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
    /// An indexed file, original alias or open handle changed.
    pub fn metadata(&self) -> Result<&Document, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.document)
    }
    /// # Errors
    /// An indexed file, original alias or open handle changed.
    pub fn source_path(&self) -> Result<&Path, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.snapshot.path)
    }
}
