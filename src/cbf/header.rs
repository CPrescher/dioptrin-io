//! Bounded scalar/loop CIF metadata before the admitted binary data item.
//! Original bytes remain in Metadata::header; delimiters and source order are
//! retained separately from decoded string values. No binary payload is parsed.
use super::Error;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct CifHeader {
    pub data_blocks: Vec<String>,
    pub fields: BTreeMap<String, String>,
    /// Original order, including duplicate assignments; fields uses the final
    /// occurrence. A duplicate is also an explicit diagnostic.
    pub entries: Vec<(String, String)>,
    pub loops: Vec<CifLoop>,
    /// Malformed/unsupported syntax stays explicit without invalidating counts.
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct CifLoop {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}
impl CifHeader {
    pub(crate) fn retained_bytes(&self) -> usize {
        let mut cost = 0usize;
        let mut charge = |bytes: usize| {
            cost = cost.saturating_add(bytes);
        };
        for block in &self.data_blocks {
            charge(block.len().saturating_add(32));
        }
        for (key, value) in &self.fields {
            charge(key.len().saturating_add(value.len()).saturating_add(128));
        }
        for (key, value) in &self.entries {
            charge(key.len().saturating_add(value.len()).saturating_add(64));
        }
        for group in &self.loops {
            charge(64);
            for key in &group.columns {
                charge(key.len().saturating_add(32));
            }
            for row in &group.rows {
                charge(32);
                for value in row {
                    charge(value.len().saturating_add(32));
                }
            }
        }
        for message in &self.diagnostics {
            charge(message.len().saturating_add(32));
        }
        cost
    }
}
enum LexError {
    Syntax(&'static str),
    Budget,
}
struct Token<'a> {
    text: &'a str,
    quoted: bool,
}
struct Tokens<'a> {
    source: &'a str,
    position: usize,
    cost: usize,
    budget: usize,
}
impl<'a> Tokens<'a> {
    fn read(&mut self, diagnostics: &mut Vec<String>) -> Result<Option<Token<'a>>, Error> {
        match self.scan() {
            Ok(value) => Ok(value),
            Err(LexError::Budget) => Err(Error("CIF retained metadata exceeds budget".into())),
            Err(LexError::Syntax(message)) => {
                self.position = self.source.len();
                diagnostics.push(message.into());
                Ok(None)
            }
        }
    }
    fn scan(&mut self) -> Result<Option<Token<'a>>, LexError> {
        let bytes = self.source.as_bytes();
        loop {
            while self.position < bytes.len() && bytes[self.position].is_ascii_whitespace() {
                self.position += 1;
            }
            if bytes.get(self.position) != Some(&b'#') {
                break;
            }
            while self.position < bytes.len() && bytes[self.position] != b'\n' {
                self.position += 1;
            }
        }
        if self.position == bytes.len() {
            return Ok(None);
        }
        let start = self.position;
        let quoted = b"'\"".contains(&bytes[start]);
        let text;
        if bytes[start] == b';' && (start == 0 || bytes[start - 1] == b'\n') {
            let begin = start + 1;
            let end = self.source[begin..]
                .find("\n;")
                .map(|n| begin + n)
                .ok_or(LexError::Syntax("unterminated CIF semicolon value"))?;
            text = &self.source[begin..end];
            self.position = end + 2;
        } else if quoted {
            let quote = bytes[start];
            self.position += 1;
            while self.position < bytes.len()
                && !(bytes[self.position] == quote
                    && (self.position + 1 == bytes.len()
                        || bytes[self.position + 1].is_ascii_whitespace()))
            {
                self.position += 1;
            }
            if self.position == bytes.len() {
                return Err(LexError::Syntax("unterminated CIF quoted value"));
            }
            text = &self.source[start + 1..self.position];
            self.position += 1;
        } else {
            while self.position < bytes.len() && !bytes[self.position].is_ascii_whitespace() {
                self.position += 1;
            }
            text = &self.source[start..self.position];
        }
        self.cost = text
            .len()
            .checked_mul(4)
            .and_then(|n| self.cost.checked_add(n))
            .and_then(|n| n.checked_add(160))
            .filter(|&n| n <= self.budget)
            .ok_or(LexError::Budget)?;
        Ok(Some(Token {
            text,
            quoted: quoted || bytes[start] == b';',
        }))
    }
}
fn prefix(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
}
fn control(token: &Token<'_>) -> bool {
    !token.quoted
        && (token.text.starts_with('_')
            || prefix(token.text, "data_")
            || token.text.eq_ignore_ascii_case("loop_")
            || token.text.eq_ignore_ascii_case("stop_")
            || prefix(token.text, "save_"))
}

pub(super) fn parse(bytes: &[u8], budget: usize) -> Result<CifHeader, Error> {
    let mut result = CifHeader::default();
    let Ok(source) = std::str::from_utf8(bytes) else {
        result
            .diagnostics
            .push("CIF text is not UTF-8; original bytes retained".into());
        return Ok(result);
    };
    let mut tokens = Tokens {
        source,
        position: 0,
        cost: 0,
        budget,
    };
    let mut pending = None;
    loop {
        let token = match pending.take() {
            Some(value) => value,
            None => tokens.read(&mut result.diagnostics)?,
        };
        let Some(token) = token else {
            break;
        };
        if token.text.eq_ignore_ascii_case("_array_data.data") && !token.quoted {
            break;
        }
        if prefix(token.text, "data_") && !token.quoted {
            result.data_blocks.push(token.text[5..].to_owned());
        } else if token.text.eq_ignore_ascii_case("loop_") && !token.quoted {
            let mut columns = Vec::new();
            let mut next = tokens.read(&mut result.diagnostics)?;
            while let Some(value) = next.as_ref() {
                if value.quoted || !value.text.starts_with('_') {
                    break;
                }
                columns.push(value.text.to_owned());
                next = tokens.read(&mut result.diagnostics)?;
            }
            if columns.is_empty() {
                result.diagnostics.push("CIF loop has no columns".into());
                break;
            }
            let mut rows = Vec::new();
            let mut row = Vec::new();
            while let Some(value) = next {
                if control(&value) {
                    pending = Some(Some(value));
                    break;
                }
                row.push(value.text.to_owned());
                if row.len() == columns.len() {
                    rows.push(std::mem::take(&mut row));
                }
                next = tokens.read(&mut result.diagnostics)?;
            }
            if !row.is_empty() {
                result.diagnostics.push("incomplete CIF loop row".into());
            }
            result.loops.push(CifLoop { columns, rows });
        } else if token.text.starts_with('_') && !token.quoted {
            let value = match tokens.read(&mut result.diagnostics)? {
                Some(value) if !control(&value) => value,
                value => {
                    pending = Some(value);
                    result
                        .diagnostics
                        .push(format!("CIF item {} has no value", token.text));
                    continue;
                }
            };
            let key = token.text.to_owned();
            if result
                .fields
                .insert(key.clone(), value.text.to_owned())
                .is_some()
            {
                result
                    .diagnostics
                    .push(format!("duplicate CIF item {key}; all entries retained"));
            }
            result.entries.push((key, value.text.to_owned()));
        } else if !token.text.eq_ignore_ascii_case("stop_") {
            result
                .diagnostics
                .push(format!("unsupported CIF token {}", token.text));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scalar_loop_quotes_duplicates_and_binary_boundary() {
        let source = b"DATA_example\n# ignored\n_name 'a # value'\n_multiline\n;first\nsecond\n;\nLOOP_\n_a\n_b\n1 \"two words\"\n3 4\nSTOP_\n_name final\n_ARRAY_DATA.DATA\n;\n";
        let meta = parse(source, 100_000).unwrap();
        assert_eq!(meta.data_blocks, ["example"]);
        assert_eq!(meta.fields["_name"], "final");
        assert_eq!(meta.entries[0].1, "a # value");
        assert_eq!(meta.fields["_multiline"], "first\nsecond");
        assert_eq!(meta.loops[0].columns, ["_a", "_b"]);
        assert_eq!(meta.loops[0].rows, [["1", "two words"], ["3", "4"]]);
        assert_eq!(meta.diagnostics.len(), 1);
        assert!(!meta.fields.contains_key("_array_data.data"));
    }
    #[test]
    fn malformed_optional_text_is_diagnostic_but_budget_is_enforced() {
        for source in [
            b"_name 'unfinished".as_slice(),
            b"loop_ _a _b 1 'unfinished",
            b"_name\n;unfinished",
            b"\xff",
        ] {
            assert!(!parse(source, 100_000).unwrap().diagnostics.is_empty());
        }
        assert!(parse(b"_name value", 1).is_err());
        let meta = parse(b"loop_ _a _b 1\n_next value", 100_000).unwrap();
        assert_eq!(meta.fields["_next"], "value");
        assert_eq!(meta.diagnostics, ["incomplete CIF loop row"]);
    }
}
