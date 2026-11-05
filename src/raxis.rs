//! Rigaku R-AXIS stored words and explicit photomultiplier interpretation.
//! The field layout is adapted from the pinned MIT FabIO R-AXIS reader by
//! Brian R. Pauw; its notice is retained in licenses/native-readers.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelBuffer, PixelType,
    binary::copy_samples,
    detector_common::{Source, product},
};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ByteOrder {
    Little,
    #[default]
    Big,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interpretation {
    /// Original u16 words, including the high-bit range flag.
    #[default]
    Stored,
    /// Explicit FabIO/Rigaku flag interpretation: low 15 bits times the
    /// header's f32 photomultiplier ratio for flagged samples, truncated to u32.
    /// This is never selected automatically or retried after failure.
    PhotomultiplierExpanded,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub byte_order: ByteOrder,
    pub interpretation: Interpretation,
}
#[derive(Debug, Clone, PartialEq)]
pub enum HeaderValue {
    Bytes(Vec<u8>),
    Integer(i32),
    Float(f32),
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub storage_type: PixelType,
    pub pixel_type: PixelType,
    pub little_endian: bool,
    pub interpretation: Interpretation,
    pub has_range_flags: bool,
    pub photomultiplier_ratio: f32,
    pub payload_offset: u64,
    pub record_bytes: usize,
    /// Entire original prefix before EOF-relative image data, not just 1400 bytes.
    pub header: Vec<u8>,
    pub fields: BTreeMap<String, HeaderValue>,
}
pub struct Reader {
    source: Source,
    metadata: Metadata,
    bytes: usize,
}
impl Reader {
    /// Open stored u16 words in the specified/default big byte order. This does
    /// not reinterpret intensity flags or apply scan-direction transformations.
    /// # Errors
    /// Unsupported/malformed headers, bounds, truncation or source changes.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        Self::open_with_options(path, limits, Options::default())
    }
    /// Open with explicit byte order and range-flag interpretation policy.
    /// # Errors
    /// Invalid bounds, unknown header, or nonfinite/negative/overflowing expanded counts.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        limits: Limits,
        options: Options,
    ) -> Result<Self, Error> {
        limits.validate()?;
        let source = Source::open(path)?;
        if limits.header_bytes < 1400 {
            return Err(Error("R-AXIS header exceeds limit".into()));
        }
        let mut prefix = Vec::new();
        source.read(0, 1400, &mut prefix)?;
        if !prefix.starts_with(b"RAXIS") && !prefix.starts_with(b"R-AXIS") {
            return Err(Error("invalid R-AXIS instrument magic".into()));
        }
        let little = options.byte_order == ByteOrder::Little;
        let signed = |offset| i32_from(&prefix[offset..offset + 4], little);
        let shape = [
            usize::try_from(signed(772)).map_err(|_| Error("negative R-AXIS height".into()))?,
            usize::try_from(signed(768)).map_err(|_| Error("negative R-AXIS width".into()))?,
        ];
        let count = limits.pixels(shape, PixelType::U16)?;
        let bytes = product(count, 2)?;
        if bytes > limits.encoded_bytes {
            return Err(Error("R-AXIS encoded frame exceeds limit".into()));
        }
        let offset = source
            .bytes()
            .checked_sub(bytes as u64)
            .filter(|&n| n >= 1400)
            .ok_or_else(|| Error("truncated R-AXIS header/frame".into()))?;
        let record_bytes = usize::try_from(signed(784))
            .map_err(|_| Error("negative R-AXIS record length".into()))?;
        if record_bytes < 1400 || offset != record_bytes as u64 {
            return Err(Error(
                "unsupported/truncated R-AXIS record layout: prefix must be one declared record"
                    .into(),
            ));
        }
        let header_size =
            usize::try_from(offset).map_err(|_| Error("R-AXIS prefix exceeds platform".into()))?;
        if header_size > limits.header_bytes
            || header_size.saturating_mul(2) > limits.metadata_bytes
        {
            return Err(Error("R-AXIS prefix exceeds metadata/header limit".into()));
        }
        let mut header = Vec::new();
        source.read(0, header_size, &mut header)?;
        let ratio = f32_from(&prefix[800..804], little);
        let mut encoded = Vec::new();
        source.read(offset, bytes, &mut encoded)?;
        let has_flags = encoded
            .as_chunks::<2>()
            .0
            .iter()
            .any(|b| word(b, little) & 0x8000 != 0);
        let expanded =
            options.interpretation == Interpretation::PhotomultiplierExpanded && has_flags;
        let pixel_type = if expanded {
            limits.pixels(shape, PixelType::U32)?;
            PixelType::U32
        } else {
            PixelType::U16
        };
        if expanded {
            for sample in encoded
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| word(b, little))
                .filter(|v| v & 0x8000 != 0)
            {
                expand(sample, ratio)?;
            }
        }
        let mut fields = BTreeMap::new();
        let mut position = 0usize;
        for &(name, kind) in FIELDS {
            let (value, size) = match kind {
                Field::Bytes(size) => (
                    HeaderValue::Bytes(prefix[position..position + size].to_vec()),
                    size,
                ),
                Field::Integer => (
                    HeaderValue::Integer(i32_from(&prefix[position..position + 4], little)),
                    4,
                ),
                Field::Float => (
                    HeaderValue::Float(f32_from(&prefix[position..position + 4], little)),
                    4,
                ),
            };
            fields.insert(name.to_owned(), value);
            position += size;
        }
        fields.insert(
            "Header Leftovers".into(),
            HeaderValue::Bytes(prefix[position..1400].to_vec()),
        );
        if header_size
            .saturating_mul(2)
            .saturating_add(fields.len().saturating_mul(192))
            > limits.metadata_bytes
        {
            return Err(Error("R-AXIS retained metadata exceeds limit".into()));
        }
        source.check()?;
        Ok(Self {
            source,
            metadata: Metadata {
                shape,
                storage_type: PixelType::U16,
                pixel_type,
                little_endian: little,
                interpretation: options.interpretation,
                has_range_flags: has_flags,
                photomultiplier_ratio: ratio,
                payload_offset: offset,
                record_bytes,
                header,
                fields,
            },
            bytes,
        })
    }
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        1
    }
    /// Read the selected explicit representation. Stored words remain available
    /// by opening a separate Stored reader; no flags or scan directions are lost.
    /// # Errors
    /// Mutation, malformed/truncated samples or range expansion overflow.
    pub fn read_frame_into(&self, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        let meta = &self.metadata;
        self.source
            .read(meta.payload_offset, self.bytes, &mut buffer.encoded)?;
        buffer
            .pixels
            .resize(meta.pixel_type, meta.shape[0] * meta.shape[1])
            .map_err(Error)?;
        if meta.pixel_type == PixelType::U16 {
            copy_samples(&buffer.encoded, &mut buffer.pixels, meta.little_endian)?;
        } else {
            let PixelBuffer::U32(out) = &mut buffer.pixels else {
                unreachable!()
            };
            for (sample, bytes) in out.iter_mut().zip(buffer.encoded.as_chunks::<2>().0.iter()) {
                let stored = word(bytes, meta.little_endian);
                *sample = if stored & 0x8000 != 0 {
                    expand(stored, meta.photomultiplier_ratio)?
                } else {
                    u32::from(stored)
                };
            }
        }
        self.source.check()?;
        buffer.shape = Some(meta.shape);
        Ok(())
    }
}
fn expand(stored: u16, ratio: f32) -> Result<u32, Error> {
    if !ratio.is_finite() || ratio < 0.0 {
        return Err(Error(
            "invalid R-AXIS photomultiplier ratio for explicit expansion".into(),
        ));
    }
    let value = f64::from(stored & 0x7fff) * f64::from(ratio);
    if value > f64::from(u32::MAX) {
        return Err(Error(
            "R-AXIS photomultiplier interpretation exceeds u32".into(),
        ));
    }
    Ok(value as u32)
}
fn word(bytes: &[u8], little: bool) -> u16 {
    let bytes = bytes.try_into().unwrap();
    if little {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    }
}
fn i32_from(bytes: &[u8], little: bool) -> i32 {
    let bytes = bytes.try_into().unwrap();
    if little {
        i32::from_le_bytes(bytes)
    } else {
        i32::from_be_bytes(bytes)
    }
}
fn f32_from(bytes: &[u8], little: bool) -> f32 {
    let bytes = bytes.try_into().unwrap();
    if little {
        f32::from_le_bytes(bytes)
    } else {
        f32::from_be_bytes(bytes)
    }
}
#[derive(Clone, Copy)]
enum Field {
    Bytes(usize),
    Integer,
    Float,
}
const FIELDS: &[(&str, Field)] = &[
    ("InstrumentType", Field::Bytes(10)),
    ("Version", Field::Bytes(10)),
    ("Crystal Name", Field::Bytes(20)),
    ("Crystal System", Field::Bytes(12)),
    ("A", Field::Float),
    ("B", Field::Float),
    ("C", Field::Float),
    ("Alpha", Field::Float),
    ("Beta", Field::Float),
    ("Gamma", Field::Float),
    ("Space Group", Field::Bytes(12)),
    ("Mosaicity", Field::Float),
    ("Memo", Field::Bytes(80)),
    ("Date", Field::Bytes(12)),
    ("Reserved Space 1", Field::Bytes(84)),
    ("User", Field::Bytes(20)),
    ("Xray Target", Field::Bytes(4)),
    ("Wavelength", Field::Float),
    ("Monochromator", Field::Bytes(20)),
    ("Monochromator 2theta", Field::Float),
    ("Collimator", Field::Bytes(20)),
    ("Filter", Field::Bytes(4)),
    ("Crystal-to-detector Distance", Field::Float),
    ("Generator Voltage", Field::Float),
    ("Generator Current", Field::Float),
    ("Focus", Field::Bytes(12)),
    ("Xray Memo", Field::Bytes(80)),
    ("IP shape", Field::Integer),
    ("Oscillation Type", Field::Float),
    ("Reserved Space 2", Field::Bytes(56)),
    ("Crystal Mount (spindle axis)", Field::Bytes(4)),
    ("Crystal Mount (beam axis)", Field::Bytes(4)),
    ("Phi Datum", Field::Float),
    ("Phi Oscillation Start", Field::Float),
    ("Phi Oscillation Stop", Field::Float),
    ("Frame Number", Field::Integer),
    ("Exposure Time", Field::Float),
    ("Direct beam X position", Field::Float),
    ("Direct beam Y position", Field::Float),
    ("Omega Angle", Field::Float),
    ("Chi Angle", Field::Float),
    ("2Theta Angle", Field::Float),
    ("Mu Angle", Field::Float),
    ("Image Template", Field::Bytes(204)),
    ("X Pixels", Field::Integer),
    ("Y Pixels", Field::Integer),
    ("X Pixel Length", Field::Float),
    ("Y Pixel Length", Field::Float),
    ("Record Length", Field::Integer),
    ("Total", Field::Integer),
    ("Starting Line", Field::Integer),
    ("IP Number", Field::Integer),
    ("Photomultiplier Ratio", Field::Float),
    ("Fade Time (to start of read)", Field::Float),
    ("Fade Time (to end of read)", Field::Float),
    ("Host Type/Endian", Field::Bytes(10)),
    ("IP Type", Field::Bytes(10)),
    ("Horizontal Scan", Field::Integer),
    ("Vertical Scan", Field::Integer),
    ("Front/Back Scan", Field::Integer),
    ("Pixel Shift (RAXIS V)", Field::Float),
    ("Even/Odd Intensity Ratio (RAXIS V)", Field::Float),
    ("Magic number", Field::Integer),
    ("Number of Axes", Field::Integer),
    ("Goniometer Vector ax.1.1", Field::Float),
    ("Goniometer Vector ax.1.2", Field::Float),
    ("Goniometer Vector ax.1.3", Field::Float),
    ("Goniometer Vector ax.2.1", Field::Float),
    ("Goniometer Vector ax.2.2", Field::Float),
    ("Goniometer Vector ax.2.3", Field::Float),
    ("Goniometer Vector ax.3.1", Field::Float),
    ("Goniometer Vector ax.3.2", Field::Float),
    ("Goniometer Vector ax.3.3", Field::Float),
    ("Goniometer Vector ax.4.1", Field::Float),
    ("Goniometer Vector ax.4.2", Field::Float),
    ("Goniometer Vector ax.4.3", Field::Float),
    ("Goniometer Vector ax.5.1", Field::Float),
    ("Goniometer Vector ax.5.2", Field::Float),
    ("Goniometer Vector ax.5.3", Field::Float),
    ("Goniometer Start ax.1", Field::Float),
    ("Goniometer Start ax.2", Field::Float),
    ("Goniometer Start ax.3", Field::Float),
    ("Goniometer Start ax.4", Field::Float),
    ("Goniometer Start ax.5", Field::Float),
    ("Goniometer End ax.1", Field::Float),
    ("Goniometer End ax.2", Field::Float),
    ("Goniometer End ax.3", Field::Float),
    ("Goniometer End ax.4", Field::Float),
    ("Goniometer End ax.5", Field::Float),
    ("Goniometer Offset ax.1", Field::Float),
    ("Goniometer Offset ax.2", Field::Float),
    ("Goniometer Offset ax.3", Field::Float),
    ("Goniometer Offset ax.4", Field::Float),
    ("Goniometer Offset ax.5", Field::Float),
    ("Goniometer Scan Axis", Field::Integer),
    ("Axes Names", Field::Bytes(40)),
    ("file", Field::Bytes(16)),
    ("cmnt", Field::Bytes(20)),
    ("smpl", Field::Bytes(20)),
    ("iext", Field::Integer),
    ("reso", Field::Integer),
    ("save", Field::Integer),
    ("dint", Field::Integer),
    ("byte", Field::Integer),
    ("init", Field::Integer),
    ("ipus", Field::Integer),
    ("dexp", Field::Integer),
    ("expn", Field::Integer),
    ("posx", Field::Bytes(20)),
    ("posy", Field::Bytes(20)),
    ("xray", Field::Integer),
];
