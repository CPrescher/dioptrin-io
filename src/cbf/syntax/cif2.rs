//! CIF2 lexical/composite rules and explicit logical-string views. The shared
//! block/item parser and the CIF1/detector scanners retain their own policies.
use super::*;
use caseless::Caseless;
use unicode_normalization::UnicodeNormalization;

pub(super) fn canonical_key(name: &str, limit: usize) -> Result<String, Error> {
    let length = name
        .chars()
        .nfd()
        .default_case_fold()
        .nfd()
        .try_fold(0usize, |n, c| {
            n.checked_add(c.len_utf8()).filter(|v| *v <= limit)
        })
        .ok_or_else(|| Error("CIF canonical name exceeds metadata limit".into()))?;
    let mut key = String::new();
    key.try_reserve_exact(length)
        .map_err(|e| Error(e.to_string()))?;
    for c in name.chars().nfd().default_case_fold().nfd() {
        key.push(c);
    }
    Ok(key)
}

impl Document {
    /// Case-sensitive logical-string key lookup. Duplicate matching keys remain
    /// preserved as entries and produce an ambiguity error instead of overwrite.
    /// # Errors
    /// Invalid/non-table arena index, invalid key spans, ambiguous key or view bounds.
    pub fn table_get(&self, value: Value, key: &str) -> Result<Option<Value>, Error> {
        let Some(Composite::Table(entries)) = self.composite(value) else {
            return Err(Error(
                "CIF table lookup requires a valid table value".into(),
            ));
        };
        let mut found = None;
        for entry in entries {
            if self.logical_text(entry.key)?.text == key {
                if found.is_some() {
                    return Err(Error("ambiguous duplicate CIF2 table key".into()));
                }
                found = Some(entry.value);
            }
        }
        Ok(found)
    }
}
pub(super) fn validate_source(bytes: &[u8]) -> Result<(Version, usize), Error> {
    let source = std::str::from_utf8(bytes)
        .map_err(|e| Error(format!("CIF2 requires well-formed UTF-8: {e}")))?;
    let bom = usize::from(bytes.starts_with(b"\xef\xbb\xbf")) * 3;
    let marker = "#\\#CIF_2.0";
    let heading = &source[bom..];
    let end = heading.find(['\r', '\n']).unwrap_or(heading.len());
    if !heading[..end]
        .strip_prefix(marker)
        .is_some_and(|v| v.bytes().all(|c| matches!(c, b' ' | b'\t')))
    {
        return Err(Error(
            "CIF2 version heading must end after optional spaces/tabs".into(),
        ));
    }
    let mut line = 0usize;
    for (position, c) in source.char_indices() {
        if position == 0 && c == '\u{feff}' {
            continue;
        }
        let n = u32::from(c);
        let allowed = matches!(n,9|10|13|32..=126|160..=0xd7ff|0xe000..=0x10fffd)
            && !(0xfdd0..=0xfdef).contains(&n)
            && n & 0xffff < 0xfffe
            && n != 0xfeff;
        if !allowed {
            return Err(Error(format!(
                "CIF2 forbidden Unicode character at byte {position}"
            )));
        }
        if matches!(c, '\r' | '\n') {
            line = 0;
        } else {
            line += 1;
            if line > 2048 {
                return Err(Error(format!(
                    "CIF2 line exceeds 2048 Unicode characters at byte {position}"
                )));
            }
        }
    }
    let mut position = bom + end;
    if bytes.get(position) == Some(&b'\r') {
        position += 1;
    }
    if bytes.get(position) == Some(&b'\n') {
        position += 1;
    }
    Ok((Version::Cif20, position))
}

pub(super) fn scan(source: &str, position: &mut usize) -> Result<Option<Token>, &'static str> {
    let bytes = source.as_bytes();
    loop {
        while *position < bytes.len() && ws(bytes[*position]) {
            *position += 1;
        }
        if bytes.get(*position) != Some(&b'#') {
            break;
        }
        while *position < bytes.len() && !matches!(bytes[*position], b'\r' | b'\n') {
            *position += 1;
        }
    }
    if *position == bytes.len() {
        return Ok(None);
    }
    let start = *position;
    let first = bytes[start];
    if first == b';' && (start == 0 || matches!(bytes[start - 1], b'\r' | b'\n')) {
        return scan_core(source, position, true, true);
    }
    let style;
    let content = if b"'\"".contains(&first) {
        let triple = bytes.get(start..start + 3).is_some_and(|s| s == [first; 3]);
        let width = if triple { 3 } else { 1 };
        style = match (first, triple) {
            (b'\'', false) => Style::SingleQuoted,
            (b'"', false) => Style::DoubleQuoted,
            (b'\'', true) => Style::TripleSingleQuoted,
            _ => Style::TripleDoubleQuoted,
        };
        let begin = start + width;
        let mut end = begin;
        loop {
            if end == bytes.len() {
                return Err("unterminated CIF2 quoted value");
            }
            if !triple && matches!(bytes[end], b'\r' | b'\n') {
                return Err("CIF2 single-quoted value cannot cross a line");
            }
            if bytes
                .get(end..end + width)
                .is_some_and(|s| s.iter().all(|v| *v == first))
            {
                break;
            }
            end += 1;
        }
        *position = end + width;
        Span { start: begin, end }
    } else if b"[]{}".contains(&first) {
        style = match first {
            b'[' => Style::List,
            b'{' => Style::Table,
            _ => Style::Delimiter,
        };
        *position += 1;
        Span {
            start,
            end: *position,
        }
    } else {
        style = Style::Bare;
        let named = first == b'_'
            || source[start..].get(..5).is_some_and(|s| {
                s.eq_ignore_ascii_case("data_") || s.eq_ignore_ascii_case("save_")
            });
        while *position < bytes.len()
            && !ws(bytes[*position])
            && (named || !b"[]{}".contains(&bytes[*position]))
        {
            *position += 1;
        }
        Span {
            start,
            end: *position,
        }
    };
    Ok(Some(Token {
        span: Span {
            start,
            end: *position,
        },
        content,
        style,
    }))
}

impl Parser<'_> {
    pub(super) fn value2(&mut self, mut token: Token, depth: usize) -> Result<Value, Error> {
        if self.kind(token) != Kind::Value {
            return Err(Error(format!(
                "CIF2 expected a data value at byte {}",
                token.span.start
            )));
        }
        self.values = self
            .values
            .checked_add(1)
            .filter(|v| *v <= self.limits.values)
            .ok_or_else(|| Error("CIF syntax value limit".into()))?;
        self.charge(256)?;
        if matches!(token.style, Style::List | Style::Table) {
            if depth >= self.limits.nesting {
                return Err(Error("CIF2 composite nesting limit".into()));
            }
            let mut list = Vec::new();
            let mut table = Vec::new();
            let mut previous = None;
            let closer = if token.style == Style::List {
                b"]".as_slice()
            } else {
                b"}".as_slice()
            };
            loop {
                let next = self
                    .next()?
                    .ok_or_else(|| Error("unterminated CIF2 composite value".into()))?;
                if next.style == Style::Delimiter && self.text(next).as_bytes() == closer {
                    token.span.end = next.span.end;
                    token.content = Span {
                        start: token.span.start + 1,
                        end: next.span.start,
                    };
                    break;
                }
                if let Some(previous) = previous {
                    self.space(previous, next)?;
                }
                if token.style == Style::List {
                    let child = self.value2(next, depth + 1)?;
                    previous = Some(child.token.span.end);
                    list.try_reserve(1).map_err(|e| Error(e.to_string()))?;
                    list.push(child);
                } else {
                    if !matches!(
                        next.style,
                        Style::SingleQuoted
                            | Style::DoubleQuoted
                            | Style::TripleSingleQuoted
                            | Style::TripleDoubleQuoted
                    ) {
                        return Err(Error(
                            "CIF2 table keys require quoted or triple-quoted strings".into(),
                        ));
                    }
                    if self.source.as_bytes().get(self.position) != Some(&b':') {
                        return Err(Error(
                            "CIF2 table key requires an immediately adjacent colon".into(),
                        ));
                    }
                    self.position += 1;
                    let after_colon = self.position;
                    let child = self
                        .next()?
                        .ok_or_else(|| Error("CIF2 table entry has no value".into()))?;
                    // Only a text field supplies its own leading EOL. A
                    // comment otherwise needs whitespace before it here.
                    if self.source.as_bytes().get(after_colon) == Some(&b'#')
                        && child.style != Style::TextField
                    {
                        return Err(Error(
                            "CIF2 table value comment requires preceding whitespace".into(),
                        ));
                    }
                    let child = self.value2(child, depth + 1)?;
                    previous = Some(child.token.span.end);
                    self.charge(256)?;
                    table.try_reserve(1).map_err(|e| Error(e.to_string()))?;
                    table.push(TableEntry {
                        key: next,
                        value: child,
                    });
                }
            }
            self.charge(256)?;
            self.composites
                .try_reserve(1)
                .map_err(|e| Error(e.to_string()))?;
            let index = self.composites.len();
            self.composites.push(if token.style == Style::List {
                Composite::List(list)
            } else {
                Composite::Table(table)
            });
            return Ok(Value {
                token,
                null: None,
                composite: Some(index),
            });
        }
        let text = self.text(token);
        if token.style == Style::Bare
            && text
                .as_bytes()
                .first()
                .is_some_and(|b| b"$[]{}".contains(b))
        {
            return Err(Error("reserved initial character in CIF2 value".into()));
        }
        let null = if token.style != Style::Bare {
            None
        } else {
            match text {
                "?" => Some(Null::Unknown),
                "." => Some(Null::Inapplicable),
                _ => None,
            }
        };
        Ok(Value {
            token,
            null,
            composite: None,
        })
    }
}

pub(super) fn logical_text(
    doc: &Document,
    token: Token,
    limit: usize,
) -> Result<LogicalText, Error> {
    if matches!(token.style, Style::List | Style::Table | Style::Delimiter) {
        return Err(Error("CIF logical text requires a string token".into()));
    }
    if token.content.start < token.span.start
        || token.content.end > token.span.end
        || token.content.start > token.content.end
        || doc.raw(token).is_none()
    {
        return Err(Error("invalid CIF token span for logical text".into()));
    }
    let raw = doc
        .text(token)
        .ok_or_else(|| Error("invalid UTF-8/content span for CIF logical text".into()))?;
    if raw
        .len()
        .checked_mul(4)
        .and_then(|v| v.checked_add(256))
        .is_none_or(|v| v > limit)
    {
        return Err(Error(
            "CIF logical text view exceeds working byte limit".into(),
        ));
    }
    let mut text = String::new();
    text.try_reserve_exact(raw.len())
        .map_err(|e| Error(e.to_string()))?;
    if doc.version == Version::Cif11 {
        text.push_str(raw);
        return Ok(LogicalText {
            text,
            normalized_eol: false,
            prefix: None,
            folded: false,
        });
    }
    let mut chars = raw.chars().peekable();
    let mut normalized_eol = false;
    while let Some(c) = chars.next() {
        if c == '\r' {
            normalized_eol = true;
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            text.push('\n');
        } else {
            text.push(c);
        }
    }
    let mut prefix = None;
    let mut folded = false;
    if token.style == Style::TextField {
        let first = text.split('\n').next().unwrap_or("");
        if let Some(slash) = first.find('\\').filter(|n| *n > 0) {
            let candidate = &first[..slash];
            let tail = &first[slash..];
            let slashes = tail.bytes().take_while(|c| *c == b'\\').count();
            if !candidate.starts_with(';')
                && matches!(slashes, 1 | 2)
                && tail[slashes..].bytes().all(|c| matches!(c, b' ' | b'\t'))
                && text
                    .split('\n')
                    .skip(1)
                    .all(|line| line.starts_with(candidate))
            {
                let mut decoded = String::new();
                decoded
                    .try_reserve_exact(text.len())
                    .map_err(|e| Error(e.to_string()))?;
                if slashes == 2 {
                    decoded.push_str(&tail[1..]);
                }
                let mut first_output = slashes != 2;
                for line in text.split('\n').skip(1) {
                    if !first_output {
                        decoded.push('\n');
                    }
                    decoded.push_str(&line[candidate.len()..]);
                    first_output = false;
                }
                prefix = Some(candidate.to_owned());
                text = decoded;
            }
        }
        if fold_end(&text, 0).is_some() {
            folded = true;
            let mut decoded = String::new();
            decoded
                .try_reserve_exact(text.len())
                .map_err(|e| Error(e.to_string()))?;
            let mut cursor = 0;
            while let Some(offset) = text[cursor..].find('\\') {
                let slash = cursor + offset;
                decoded.push_str(&text[cursor..slash]);
                if let Some(end) = fold_end(&text, slash) {
                    cursor = end;
                } else {
                    decoded.push('\\');
                    cursor = slash + 1;
                }
            }
            decoded.push_str(&text[cursor..]);
            text = decoded;
        }
    }
    Ok(LogicalText {
        text,
        normalized_eol,
        prefix,
        folded,
    })
}
fn fold_end(text: &str, start: usize) -> Option<usize> {
    if text.as_bytes().get(start) != Some(&b'\\') {
        return None;
    }
    let mut end = start + 1;
    while text
        .as_bytes()
        .get(end)
        .is_some_and(|c| matches!(c, b' ' | b'\t'))
    {
        end += 1;
    }
    if end == text.len() {
        Some(end)
    } else if text.as_bytes()[end] == b'\n' {
        Some(end + 1)
    } else {
        None
    }
}
