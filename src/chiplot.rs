//! Source-preserving CHIPLOT/FIT2D text profiles. Three opaque text headers,
//! a declared point/curve count, and stored X plus every Y column. Numerical
//! lexemes are authoritative; float/grid conversions are explicit derived views.
pub use crate::binary::{Error, Limits};
use crate::source::Snapshot;
use std::{fs::File, io::Read, ops::Range, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    Buffer,
    StreamCapture,
    PathSnapshot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Number {
    I64(i64),
    U64(u64),
    IntegerText,
    Decimal,
    NonFinite,
}
#[derive(Clone, Debug)]
pub struct Sample {
    pub offset: usize,
    pub lexeme: String,
    pub number: Number,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatPolicy {
    RequireExactInteger,
    AllowRoundedInteger,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exactness {
    ExactInteger,
    RoundedInteger,
    UnverifiedIntegerApproximation,
    DecimalApproximation,
    NonFinite,
}
#[derive(Clone, Copy, Debug)]
pub struct FloatView {
    pub value: f64,
    pub exactness: Exactness,
}
impl Sample {
    /// Explicit approximation of a text scalar. Integer rounding requires caller
    /// policy, including integers beyond the native signed/unsigned64 ranges.
    /// Decimal conversion remains labelled approximate; overflow/underflow fails.
    pub fn f64_view(&self, policy: FloatPolicy) -> Result<FloatView, Error> {
        if number(&self.lexeme)? != self.number {
            return Err(Error(
                "profile sample kind/value no longer matches its lexical source".into(),
            ));
        }
        let value = if self.number == Number::NonFinite {
            let negative = self.lexeme.starts_with('-');
            if self.lexeme.to_ascii_lowercase().contains("nan") {
                f64::NAN.copysign(if negative { -1.0 } else { 1.0 })
            } else if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        } else {
            self.lexeme
                .parse::<f64>()
                .map_err(|_| Error("unsupported float64 profile conversion".into()))?
        };
        let exactness = match self.number {
            Number::I64(integer)
                if value.is_finite()
                    && value.fract() == 0.0
                    && value as i128 == i128::from(integer) =>
            {
                Exactness::ExactInteger
            }
            Number::U64(integer)
                if value.is_finite()
                    && value.fract() == 0.0
                    && value as u128 == u128::from(integer) =>
            {
                Exactness::ExactInteger
            }
            Number::I64(_) | Number::U64(_) => {
                if policy == FloatPolicy::RequireExactInteger {
                    return Err(Error(
                        "profile integer is not proven exactly representable in float64".into(),
                    ));
                }
                Exactness::RoundedInteger
            }
            Number::IntegerText => {
                if policy == FloatPolicy::RequireExactInteger {
                    return Err(Error(
                        "profile integer exactness exceeds native integer range".into(),
                    ));
                }
                Exactness::UnverifiedIntegerApproximation
            }
            Number::Decimal => Exactness::DecimalApproximation,
            Number::NonFinite => Exactness::NonFinite,
        };
        if self.number != Number::NonFinite {
            let mantissa = self.lexeme.split(['e', 'E']).next().unwrap();
            if !value.is_finite()
                || value == 0.0 && mantissa.bytes().any(|b| matches!(b, b'1'..=b'9'))
            {
                return Err(Error(
                    "profile float64 conversion overflows or underflows; original lexeme retained"
                        .into(),
                ));
            }
        }
        Ok(FloatView { value, exactness })
    }
}
#[derive(Clone, Debug)]
pub struct Row {
    pub line: usize,
    pub samples: Vec<Sample>,
}
#[derive(Clone, Debug)]
pub struct Comment {
    pub line: usize,
    pub offset: usize,
    pub text: Vec<u8>,
}
#[derive(Debug)]
pub struct Profile {
    pub original: Vec<u8>,
    /// Original header spans without CR/LF. Source octets need not be UTF8.
    pub headers: [Range<usize>; 3],
    pub declaration: Range<usize>,
    pub declared_points: u64,
    pub curves: usize,
    pub rows: Vec<Row>,
    pub comments: Vec<Comment>,
    pub diagnostics: Vec<String>,
    pub provenance: Provenance,
}
#[derive(Debug)]
pub struct GridView {
    pub points: usize,
    pub first: Option<f64>,
    pub last: Option<f64>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub endpoint_average_step: Option<f64>,
    pub uniform_step: Option<f64>,
    pub tolerance: f64,
    pub approximate_decimals: bool,
    pub rounded_integers: bool,
    pub diagnostics: Vec<String>,
}
impl Profile {
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self, Error> {
        admit_source(bytes.len(), limits)?;
        let mut original = Vec::new();
        original
            .try_reserve_exact(bytes.len())
            .map_err(|e| Error(e.to_string()))?;
        original.extend_from_slice(bytes);
        Self::parse_owned(original, limits, Provenance::Buffer)
    }
    pub fn from_reader(reader: impl Read, limits: Limits) -> Result<Self, Error> {
        Self::parse_owned(capture(reader, limits)?, limits, Provenance::StreamCapture)
    }
    pub fn header(&self, index: usize) -> Result<&[u8], Error> {
        self.headers
            .get(index)
            .and_then(|range| self.original.get(range.clone()))
            .ok_or_else(|| Error("profile header index/range invalid".into()))
    }
    pub fn header_text(&self, index: usize) -> Result<&str, Error> {
        std::str::from_utf8(self.header(index)?).map_err(|e| Error(e.to_string()))
    }
    /// Derived float64 assessment only. No stored X/Y is replaced, sorted or
    /// interpolated. Caller specifies absolute tolerance and integer policy.
    pub fn grid_view(&self, tolerance: f64, policy: FloatPolicy) -> Result<GridView, Error> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(Error(
                "profile grid tolerance must be finite and nonnegative".into(),
            ));
        }
        let columns = self
            .curves
            .checked_add(1)
            .filter(|&n| n > 1)
            .ok_or_else(|| Error("profile grid curve count invalid".into()))?;
        if u64::try_from(self.rows.len()).ok() != Some(self.declared_points)
            || self.rows.iter().any(|row| row.samples.len() != columns)
        {
            return Err(Error("profile grid row shape/count changed".into()));
        }
        let mut result = GridView {
            points: self.rows.len(),
            first: None,
            last: None,
            minimum: None,
            maximum: None,
            endpoint_average_step: None,
            uniform_step: None,
            tolerance,
            approximate_decimals: false,
            rounded_integers: false,
            diagnostics: Vec::new(),
        };
        for row in &self.rows {
            let view = row
                .samples
                .first()
                .ok_or_else(|| Error("profile grid row has no X".into()))?
                .f64_view(policy)?;
            if !view.value.is_finite() {
                return Err(Error(
                    "nonfinite stored X prevents finite grid projection".into(),
                ));
            }
            result.approximate_decimals |= view.exactness == Exactness::DecimalApproximation;
            result.rounded_integers |= matches!(
                view.exactness,
                Exactness::RoundedInteger | Exactness::UnverifiedIntegerApproximation
            );
            result.first.get_or_insert(view.value);
            result.last = Some(view.value);
            result.minimum = Some(result.minimum.map_or(view.value, |v| v.min(view.value)));
            result.maximum = Some(result.maximum.map_or(view.value, |v| v.max(view.value)));
        }
        if self.rows.len() < 2 {
            result
                .diagnostics
                .push("fewer than two stored points do not define a grid step".into());
            return Ok(result);
        }
        let first = result.first.unwrap();
        let step = (result.last.unwrap() - first) / ((self.rows.len() - 1) as f64);
        if !step.is_finite() {
            return Err(Error(
                "derived profile endpoint step overflows float64".into(),
            ));
        }
        result.endpoint_average_step = Some(step);
        let mut uniform = true;
        for (index, row) in self.rows.iter().enumerate() {
            let projected = first + index as f64 * step;
            let actual = row
                .samples
                .first()
                .ok_or_else(|| Error("profile grid row has no X".into()))?
                .f64_view(policy)?
                .value;
            if !projected.is_finite() || (actual - projected).abs() > tolerance {
                uniform = false;
            }
        }
        if uniform {
            result.uniform_step = Some(step);
        } else {
            result.diagnostics.push("stored X values are nonuniform under the caller's float64 tolerance; endpoint average is not a uniform step".into());
        }
        if step < 0.0 {
            result.diagnostics.push(
                "stored X order is descending; signed step retained and Y order unchanged".into(),
            );
        }
        Ok(result)
    }
    fn parse_owned(
        original: Vec<u8>,
        limits: Limits,
        provenance: Provenance,
    ) -> Result<Self, Error> {
        admit_source(original.len(), limits)?;
        let mut budget = original
            .len()
            .checked_add(std::mem::size_of::<Self>())
            .filter(|&n| n <= limits.metadata_bytes)
            .ok_or_else(|| Error("CHIPLOT aggregate metadata limit".into()))?;
        let mut lines = original.split_inclusive(|b| *b == b'\n');
        let mut offset = 0;
        let mut headers = std::array::from_fn(|_| 0..0);
        for header in &mut headers {
            let line = lines
                .next()
                .ok_or_else(|| Error("truncated CHIPLOT title/axis header".into()))?;
            *header = offset..offset + content_len(line);
            offset += line.len();
        }
        let declaration_line = lines
            .next()
            .ok_or_else(|| Error("missing CHIPLOT declared point count".into()))?;
        let declaration = offset..offset + content_len(declaration_line);
        let mut comments = Vec::new();
        let declared_content = uncomment(
            declaration_line,
            4,
            offset,
            &mut comments,
            &mut budget,
            limits,
        )?;
        let count_tokens = token_spans(declared_content, 2)?;
        if !(1..=2).contains(&count_tokens.len()) {
            return Err(Error("CHIPLOT declaration requires point count and optional curve count; uncertainty/other flags unsupported".into()));
        }
        let integer = |range: &Range<usize>| -> Result<u64, Error> {
            let text = std::str::from_utf8(&declared_content[range.clone()])
                .map_err(|e| Error(e.to_string()))?;
            if !text.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error("invalid unsigned CHIPLOT declaration".into()));
            }
            text.parse::<u64>()
                .map_err(|_| Error("CHIPLOT declared count overflow".into()))
        };
        let declared_points = integer(&count_tokens[0])?;
        let points = usize::try_from(declared_points)
            .ok()
            .filter(|&n| n <= limits.frames)
            .ok_or_else(|| Error("CHIPLOT point limit".into()))?;
        let curves = if count_tokens.len() == 2 {
            usize::try_from(integer(&count_tokens[1])?)
                .map_err(|_| Error("CHIPLOT curve count overflow".into()))?
        } else {
            1
        };
        if curves == 0 {
            return Err(Error("CHIPLOT requires at least one stored curve".into()));
        }
        let columns = curves
            .checked_add(1)
            .ok_or_else(|| Error("CHIPLOT column count overflow".into()))?;
        columns
            .checked_mul(std::mem::size_of::<Sample>())
            .filter(|&n| n <= limits.frame_bytes)
            .ok_or_else(|| Error("CHIPLOT curve byte limit".into()))?;
        let samples = points
            .checked_mul(columns)
            .ok_or_else(|| Error("CHIPLOT sample count overflow".into()))?;
        samples
            .checked_mul(std::mem::size_of::<Sample>())
            .filter(|&n| n <= limits.frame_bytes)
            .ok_or_else(|| Error("CHIPLOT sample byte limit".into()))?;
        offset += declaration_line.len();
        if offset > limits.header_bytes {
            return Err(Error("CHIPLOT header byte limit".into()));
        }
        let allocation = points
            .checked_mul(std::mem::size_of::<Row>())
            .and_then(|n| n.checked_add(samples.checked_mul(std::mem::size_of::<Sample>())?))
            .and_then(|n| {
                n.checked_add(if points > 0 {
                    columns.checked_mul(2 * std::mem::size_of::<Range<usize>>())?
                } else {
                    0
                })
            })
            .ok_or_else(|| Error("CHIPLOT allocation size overflow".into()))?;
        charge(&mut budget, allocation, limits)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(points)
            .map_err(|e| Error(e.to_string()))?;
        let mut diagnostics = Vec::new();
        let mut nonfinite = false;
        for (index, line) in lines.enumerate() {
            let content = uncomment(line, index + 5, offset, &mut comments, &mut budget, limits)?;
            if rows.len() >= points && content.iter().any(|b| !b.is_ascii_whitespace()) {
                return Err(Error(
                    "CHIPLOT rows exceed declared count; extra datasets/trailers unsupported"
                        .into(),
                ));
            }
            let spans = token_spans(content, columns)?;
            if !spans.is_empty() {
                if rows.len() >= points {
                    return Err(Error(
                        "CHIPLOT rows exceed declared count; extra datasets/trailers unsupported"
                            .into(),
                    ));
                }
                if spans.len() != columns {
                    return Err(Error(format!(
                        "CHIPLOT row {} has {} fields; expected {columns}",
                        index + 5,
                        spans.len()
                    )));
                }
                let mut values = Vec::new();
                values
                    .try_reserve_exact(columns)
                    .map_err(|e| Error(e.to_string()))?;
                for span in spans {
                    let lexeme = std::str::from_utf8(&content[span.clone()])
                        .map_err(|_| Error("CHIPLOT numeric lexeme is not ASCII".into()))?;
                    let number = number(lexeme)?;
                    nonfinite |= number == Number::NonFinite;
                    charge(&mut budget, lexeme.len(), limits)?;
                    values.push(Sample {
                        offset: offset + span.start,
                        lexeme: lexeme.into(),
                        number,
                    });
                }
                rows.push(Row {
                    line: index + 5,
                    samples: values,
                });
            }
            offset += line.len();
        }
        if rows.len() != points {
            return Err(Error(
                "truncated CHIPLOT rows: declared point count is incomplete".into(),
            ));
        }
        if nonfinite {
            let message = "nonfinite text scalar extension retained; no finite numerical/grid interpretation implied";
            charge(
                &mut budget,
                message.len() + std::mem::size_of::<String>(),
                limits,
            )?;
            diagnostics.push(message.into());
        }
        Ok(Self {
            original,
            headers,
            declaration,
            declared_points,
            curves,
            rows,
            comments,
            diagnostics,
            provenance,
        })
    }
}
fn number(text: &str) -> Result<Number, Error> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if ["nan", "inf", "infinity"]
        .iter()
        .any(|v| unsigned.eq_ignore_ascii_case(v))
    {
        return Ok(Number::NonFinite);
    }
    let b = unsigned.as_bytes();
    let mut i = 0;
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    let mut real = false;
    if b.get(i) == Some(&b'.') {
        real = true;
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return Err(Error(format!(
            "unsupported CHIPLOT numeric lexeme {:.64}",
            text
        )));
    }
    if b.get(i).is_some_and(|b| matches!(b, b'e' | b'E')) {
        real = true;
        i += 1;
        if b.get(i).is_some_and(|b| matches!(b, b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return Err(Error("CHIPLOT exponent has no digits".into()));
        }
    }
    if i != b.len() {
        return Err(Error(format!(
            "unsupported CHIPLOT numeric lexeme {:.64}; uncertainties/D-exponents not silently converted",
            text
        )));
    }
    if real {
        Ok(Number::Decimal)
    } else if let Ok(value) = text.parse::<i64>() {
        Ok(Number::I64(value))
    } else if let Ok(value) = text.parse::<u64>() {
        Ok(Number::U64(value))
    } else {
        Ok(Number::IntegerText)
    }
}
fn content_len(line: &[u8]) -> usize {
    let n = line.len() - usize::from(line.ends_with(b"\n"));
    n - usize::from(line[..n].ends_with(b"\r"))
}
fn uncomment<'a>(
    line: &'a [u8],
    number: usize,
    offset: usize,
    comments: &mut Vec<Comment>,
    budget: &mut usize,
    limits: Limits,
) -> Result<&'a [u8], Error> {
    let line = &line[..content_len(line)];
    if let Some(i) = line.iter().position(|b| *b == b'#') {
        charge(
            budget,
            2 * std::mem::size_of::<Comment>() + line.len() - i,
            limits,
        )?;
        comments.push(Comment {
            line: number,
            offset: offset + i,
            text: line[i..].to_vec(),
        });
        Ok(&line[..i])
    } else {
        Ok(line)
    }
}
fn token_spans(bytes: &[u8], maximum: usize) -> Result<Vec<Range<usize>>, Error> {
    let mut spans = Vec::new();
    let mut i = 0;
    let mut comma = false;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i] == b',' {
            if spans.is_empty() || comma {
                return Err(Error("empty CHIPLOT comma field".into()));
            }
            comma = true;
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b',' {
            i += 1;
        }
        if spans.len() >= maximum {
            return Err(Error("CHIPLOT row/declaration has too many fields".into()));
        }
        spans.push(start..i);
        comma = false;
    }
    if comma {
        return Err(Error("trailing empty CHIPLOT comma field".into()));
    }
    Ok(spans)
}
fn charge(used: &mut usize, add: usize, limits: Limits) -> Result<(), Error> {
    *used = used
        .checked_add(add)
        .filter(|&n| n <= limits.metadata_bytes)
        .ok_or_else(|| Error("CHIPLOT retained metadata limit".into()))?;
    Ok(())
}
fn admit_source(bytes: usize, limits: Limits) -> Result<(), Error> {
    limits.validate()?;
    if bytes > limits.encoded_bytes.min(limits.metadata_bytes) {
        return Err(Error("CHIPLOT source byte limit".into()));
    }
    Ok(())
}
fn capture(reader: impl Read, limits: Limits) -> Result<Vec<u8>, Error> {
    limits.validate()?;
    let bound = limits.encoded_bytes.min(limits.metadata_bytes);
    let count = u64::try_from(bound)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| Error("CHIPLOT capture limit overflow".into()))?;
    let mut reader = reader.take(count);
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
            .ok_or_else(|| Error("CHIPLOT capture byte limit".into()))?;
        bytes
            .try_reserve_exact(n)
            .map_err(|e| Error(e.to_string()))?;
        bytes.extend_from_slice(&chunk[..n]);
    }
    Ok(bytes)
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    profile: Profile,
}
impl Reader {
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if !std::fs::metadata(&snapshot.path)?.is_file() {
            return Err(Error("CHIPLOT path snapshots require regular files".into()));
        }
        if snapshot.bytes() > limits.encoded_bytes.min(limits.metadata_bytes) as u64 {
            return Err(Error("CHIPLOT source byte limit".into()));
        }
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let profile = Profile::parse_owned(
            capture(&mut file, limits)?,
            limits,
            Provenance::PathSnapshot,
        )?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            profile,
        })
    }
    pub fn metadata(&self) -> Result<&Profile, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.profile)
    }
    pub fn source_path(&self) -> Result<&Path, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.snapshot.path)
    }
}
