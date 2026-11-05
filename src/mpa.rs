//! Multiwire MPA ASCII source counts and named sections. Original spectra and
//! coincidence maps are indexed separately; no channel sums are implicit.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelBuffer, PixelType,
    detector_common::{Source, sum},
};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    path::Path,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    AsciiF64,
}
#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub coincidence_map: bool,
    pub shape: [usize; 2],
    pub count: usize,
    pub offset: u64,
    pub encoded_bytes: usize,
    pub header: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    /// ASCII has no source scalar width or byte order. F64 is the declared
    /// numeric interpretation; integer tokens beyond exact f64 range fail.
    pub source_type: Option<PixelType>,
    pub encoding: Encoding,
    pub header: Vec<u8>,
    pub fields: BTreeMap<String, String>,
    pub sections: Vec<Section>,
}
pub struct Reader {
    source: Source,
    metadata: Metadata,
    maps: Vec<usize>,
}
impl Reader {
    /// Index bounded ASCII DATA spectra and CDAT maps. Non-ASCII/binary files,
    /// external headers and ambiguous section declarations fail explicitly.
    /// # Errors
    /// Unsupported layout, truncation, numeric precision loss or resource bounds.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let source = Source::open(path)?;
        let mut input = BufReader::new(source.file()?);
        let mut header = Vec::new();
        let mut fields = BTreeMap::new();
        let mut sections: Vec<Section> = Vec::new();
        let mut prefix = String::new();
        let mut line = Vec::new();
        let mut current = None;
        let mut consumed = 0usize;
        let mut maps = Vec::new();
        let mut shape = None;
        let mut retained = 0usize;
        let mut position = 0u64;
        loop {
            let line_start = position;
            line.clear();
            let n = Read::by_ref(&mut input)
                .take((limits.header_bytes as u64).saturating_add(1))
                .read_until(b'\n', &mut line)?;
            position = position
                .checked_add(n as u64)
                .ok_or_else(|| Error("MPA text offset overflow".into()))?;
            if n == 0 {
                break;
            }
            if n > limits.header_bytes {
                return Err(Error("MPA line exceeds header/text limit".into()));
            }
            if !line.is_ascii() {
                return Err(Error("unsupported non-ASCII/binary MPA source".into()));
            }
            let text = std::str::from_utf8(&line)
                .map_err(|_| Error("invalid MPA text".into()))?
                .trim();
            if text.starts_with("[DATA") || text.starts_with("[CDAT") {
                if let Some(index) = current {
                    finish(&mut sections, index, consumed, line_start, limits)?;
                }
                if sections.len() >= limits.frames {
                    return Err(Error("MPA section count exceeds frame limit".into()));
                }
                if shape.is_none() {
                    if fields.get("mpafmt").map(String::as_str) != Some("asc")
                        || fields.get("fmt").is_some_and(|v| v != "asc")
                        || fields.get("sephead").is_some_and(|v| v != "0")
                    {
                        return Err(Error(
                            "unsupported MPA binary/external-header layout".into(),
                        ));
                    }
                    let dimension = |key: &str| {
                        fields
                            .get(key)
                            .and_then(|v| v.parse::<usize>().ok())
                            .ok_or_else(|| Error(format!("invalid MPA {key}")))
                    };
                    let dims = [dimension("ADC1_range")?, dimension("ADC2_range")?];
                    limits.pixels(dims, PixelType::F64)?;
                    shape = Some(dims);
                }
                let body = text
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .ok_or_else(|| Error("invalid MPA data section".into()))?;
                let (name, count) = body
                    .split_once(',')
                    .ok_or_else(|| Error("missing MPA section count".into()))?;
                let count = count
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| Error("invalid MPA section count".into()))?;
                let name = name.trim();
                let coincidence_map = name.starts_with("CDAT");
                let number = if coincidence_map {
                    &name[4..]
                } else {
                    name.strip_prefix("DATA")
                        .ok_or_else(|| Error("invalid MPA section name".into()))?
                };
                if number.is_empty()
                    || !number.bytes().all(|b| b.is_ascii_digit())
                    || sections.iter().any(|s| s.name == name)
                {
                    return Err(Error("invalid/duplicate MPA section name".into()));
                }
                let dims = if coincidence_map {
                    shape.unwrap()
                } else {
                    [1, count]
                };
                let expected = limits.pixels(dims, PixelType::F64)?;
                if count != expected {
                    return Err(Error("MPA CDAT count disagrees with ADC shape".into()));
                }
                if coincidence_map {
                    maps.push(sections.len());
                }
                retained = sum(retained, line.len())?;
                sections.push(Section {
                    name: name.to_owned(),
                    coincidence_map,
                    shape: dims,
                    count,
                    offset: position,
                    encoded_bytes: 0,
                    header: line.clone(),
                });
                current = Some(sections.len() - 1);
                consumed = 0;
            } else if let Some(index) = current {
                if !text.is_empty() {
                    numeric(text)?;
                    consumed = sum(consumed, 1)?;
                    if consumed > sections[index].count {
                        return Err(Error("extra MPA counts before next section".into()));
                    }
                }
            } else {
                if sum(header.len(), line.len())? > limits.header_bytes {
                    return Err(Error("MPA source header exceeds limit".into()));
                }
                header.extend_from_slice(&line);
                retained = sum(retained, line.len())?;
                if let Some(rest) = text.strip_prefix('[') {
                    let (name, _) = rest
                        .split_once(']')
                        .ok_or_else(|| Error("invalid MPA metadata section".into()))?;
                    if name.is_empty() {
                        return Err(Error("empty MPA metadata section".into()));
                    }
                    prefix = name.to_owned();
                } else if let Some((key, value)) = text.split_once('=') {
                    let name = if prefix.is_empty() {
                        key.trim().to_owned()
                    } else {
                        format!("{}_{}", prefix, key.trim())
                    };
                    if key.trim().is_empty()
                        || fields.insert(name, value.trim().to_owned()).is_some()
                    {
                        return Err(Error("duplicate/empty MPA header field".into()));
                    }
                }
            }
            if retained
                .saturating_mul(3)
                .saturating_add(fields.len().saturating_mul(128))
                .saturating_add(sections.len().saturating_mul(256))
                > limits.metadata_bytes
            {
                return Err(Error("MPA retained metadata exceeds limit".into()));
            }
            if let Some(index) = current
                && position.saturating_sub(sections[index].offset) > limits.encoded_bytes as u64
            {
                return Err(Error("MPA section exceeds encoded limit".into()));
            }
        }
        if let Some(index) = current {
            finish(&mut sections, index, consumed, source.bytes(), limits)?;
        }
        if maps.is_empty() {
            return Err(Error("missing MPA CDAT coincidence map".into()));
        }
        source.check()?;
        Ok(Self {
            source,
            maps,
            metadata: Metadata {
                shape: shape.unwrap(),
                pixel_type: PixelType::F64,
                source_type: None,
                encoding: Encoding::AsciiF64,
                header,
                fields,
                sections,
            },
        })
    }
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.maps.len()
    }
    /// Read one CDAT frame without summing or omitting the separately exposed DATA spectra.
    /// # Errors
    /// Invalid index, mutation, malformed text or precision loss.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        let section = *self
            .maps
            .get(index)
            .ok_or_else(|| Error("MPA frame index out of range".into()))?;
        self.read_section_into(section, buffer)
    }
    /// Read any original DATA/CDAT section by metadata index.
    /// # Errors
    /// Invalid index, mutation, malformed text, bounds or precision loss.
    pub fn read_section_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        let section = self
            .metadata
            .sections
            .get(index)
            .ok_or_else(|| Error("MPA section index outside source".into()))?;
        self.source
            .read(section.offset, section.encoded_bytes, &mut buffer.encoded)?;
        buffer
            .pixels
            .resize(PixelType::F64, section.count)
            .map_err(Error)?;
        let PixelBuffer::F64(out) = &mut buffer.pixels else {
            unreachable!()
        };
        let text = std::str::from_utf8(&buffer.encoded)
            .map_err(|_| Error("invalid MPA numeric text".into()))?;
        let mut lines = text.lines().map(str::trim).filter(|s| !s.is_empty());
        for value in out {
            *value = numeric(
                lines
                    .next()
                    .ok_or_else(|| Error("truncated MPA counts".into()))?,
            )?;
        }
        if lines.next().is_some() {
            return Err(Error("extra MPA numeric lines".into()));
        }
        self.source.check()?;
        buffer.shape = Some(section.shape);
        Ok(())
    }
}
fn finish(
    sections: &mut [Section],
    index: usize,
    consumed: usize,
    end: u64,
    limits: Limits,
) -> Result<(), Error> {
    let section = &mut sections[index];
    if consumed != section.count {
        return Err(Error("MPA section count mismatch/truncation".into()));
    }
    section.encoded_bytes = usize::try_from(
        end.checked_sub(section.offset)
            .ok_or_else(|| Error("invalid MPA section offset".into()))?,
    )
    .map_err(|_| Error("MPA section exceeds platform".into()))?;
    if section.encoded_bytes > limits.encoded_bytes {
        return Err(Error("MPA encoded section exceeds limit".into()));
    }
    Ok(())
}
fn numeric(text: &str) -> Result<f64, Error> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        let integer = text
            .parse::<i128>()
            .map_err(|_| Error("MPA integer exceeds exact ASCII/f64 admission".into()))?;
        if !(-(1i128 << 53)..=(1i128 << 53)).contains(&integer) {
            return Err(Error(
                "MPA integer exceeds exact f64 range; explicit wider integer reader required"
                    .into(),
            ));
        }
    }
    text.parse::<f64>()
        .map_err(|_| Error("invalid MPA ASCII numeric count".into()))
}
