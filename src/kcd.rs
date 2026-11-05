//! Nonius KappaCCD fixed-prefix files. Independent readouts are preserved by
//! default; summing them is a separate explicit interpretation.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelBuffer, PixelType,
    binary::copy_samples,
    detector_common::{Source, product, sum},
};
use std::{collections::BTreeMap, path::Path};
const PREFIX: usize = 4100;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Interpretation {
    #[default]
    StoredReadouts,
    SumReadouts,
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub storage_type: PixelType,
    pub pixel_type: PixelType,
    pub little_endian: bool,
    pub readouts: usize,
    pub interpretation: Interpretation,
    pub header: Vec<u8>,
    pub fields: BTreeMap<String, String>,
    pub payload_offset: u64,
}
pub struct Reader {
    source: Source,
    metadata: Metadata,
    bytes: usize,
}
impl Reader {
    /// Open the admitted fixed 4100-byte prefix layout, preserving each readout.
    /// # Errors
    /// Unsupported dtype/order/layout, duplicate fields, bounds or source mutation.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        Self::open_with_interpretation(path, limits, Interpretation::StoredReadouts)
    }
    /// Select an explicit output policy. Summation is checked signed32 arithmetic.
    /// # Errors
    /// Unsupported layouts, invalid counts/limits, truncation or source mutation.
    pub fn open_with_interpretation(
        path: impl AsRef<Path>,
        limits: Limits,
        interpretation: Interpretation,
    ) -> Result<Self, Error> {
        limits.validate()?;
        let source = Source::open(path)?;
        if PREFIX > limits.header_bytes {
            return Err(Error("KCD prefix exceeds header limit".into()));
        }
        let mut header = Vec::new();
        source.read(0, PREFIX, &mut header)?;
        if !header.is_ascii() {
            return Err(Error("non-ASCII KCD header".into()));
        }
        let text = std::str::from_utf8(&header).map_err(|_| Error("invalid KCD header".into()))?;
        let mut lines = text.lines();
        let first = lines
            .next()
            .ok_or_else(|| Error("empty KCD header".into()))?;
        if !first.starts_with("No") || first.len() > 100 {
            return Err(Error("unsupported KCD first record".into()));
        }
        let mut fields = BTreeMap::new();
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = if line == "Binned mode" {
                ("Mode", "Binned")
            } else {
                line.split_once('=')
                    .ok_or_else(|| Error("unknown KCD header record/padding".into()))?
            };
            if key.trim().is_empty()
                || fields
                    .insert(key.trim().to_owned(), value.trim().to_owned())
                    .is_some()
            {
                return Err(Error("duplicate/empty KCD field".into()));
            }
        }
        if fields.get("Data type").map(String::as_str) != Some("u16") {
            return Err(Error("unsupported/missing KCD Data type".into()));
        }
        if fields
            .get("ByteOrder")
            .is_some_and(|v| !matches!(v.as_str(), "little" | "LittleEndian" | "LowByteFirst"))
        {
            return Err(Error("unsupported KCD byte order".into()));
        }
        let number = |key: &str| {
            fields
                .get(key)
                .and_then(|v| v.parse::<usize>().ok())
                .ok_or_else(|| Error(format!("invalid KCD {key}")))
        };
        let shape = [number("Y dimension")?, number("X dimension")?];
        let readouts = number("Number of readouts")?;
        if readouts == 0 || readouts > limits.frames {
            return Err(Error("KCD readout count exceeds limits".into()));
        }
        let pixel_type = if interpretation == Interpretation::StoredReadouts {
            PixelType::U16
        } else {
            PixelType::I32
        };
        let count = limits.pixels(shape, pixel_type)?;
        let bytes = product(count, 2)?;
        if bytes > limits.encoded_bytes {
            return Err(Error("KCD readout exceeds encoded limit".into()));
        }
        let expected = sum(PREFIX, product(bytes, readouts)?)?;
        if source.bytes() != expected as u64 {
            return Err(Error(
                "unsupported/truncated KCD layout: expected 4100-byte prefix and exact readouts"
                    .into(),
            ));
        }
        if header
            .len()
            .saturating_mul(3)
            .saturating_add(fields.len().saturating_mul(128))
            > limits.metadata_bytes
        {
            return Err(Error("KCD retained metadata exceeds limit".into()));
        }
        source.check()?;
        Ok(Self {
            source,
            bytes,
            metadata: Metadata {
                shape,
                storage_type: PixelType::U16,
                pixel_type,
                little_endian: true,
                readouts,
                interpretation,
                header,
                fields,
                payload_offset: PREFIX as u64,
            },
        })
    }
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        if self.metadata.interpretation == Interpretation::StoredReadouts {
            self.metadata.readouts
        } else {
            1
        }
    }
    /// Read one stored readout or the explicitly selected sum. No correction,
    /// normalization, adjacent-file search or orientation change is performed.
    /// # Errors
    /// Invalid index, source mutation, truncation or signed32 sum overflow.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        if index >= self.frames() {
            return Err(Error("KCD frame index out of range".into()));
        }
        let meta = &self.metadata;
        let count = meta.shape[0] * meta.shape[1];
        buffer
            .pixels
            .resize(meta.pixel_type, count)
            .map_err(Error)?;
        if meta.interpretation == Interpretation::StoredReadouts {
            self.source.read(
                meta.payload_offset + (index * self.bytes) as u64,
                self.bytes,
                &mut buffer.encoded,
            )?;
            copy_samples(&buffer.encoded, &mut buffer.pixels, true)?;
        } else {
            let PixelBuffer::I32(out) = &mut buffer.pixels else {
                unreachable!()
            };
            out.fill(0);
            for readout in 0..meta.readouts {
                self.source.read(
                    meta.payload_offset + (readout * self.bytes) as u64,
                    self.bytes,
                    &mut buffer.encoded,
                )?;
                for (value, bytes) in out.iter_mut().zip(buffer.encoded.as_chunks::<2>().0.iter()) {
                    *value = value
                        .checked_add(i32::from(u16::from_le_bytes(*bytes)))
                        .ok_or_else(|| Error("explicit KCD SumReadouts exceeds signed32".into()))?;
                }
            }
        }
        self.source.check()?;
        buffer.shape = Some(meta.shape);
        Ok(())
    }
}
