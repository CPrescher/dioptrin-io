//! Oxford Sapphire/KM4CCD frames with raw int32, TY1 or row-indexed TY5.
mod ty5;
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelBuffer, PixelType,
    binary::copy_samples,
    detector_common::{Source, product, sum},
};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Ty1,
    Ty5,
}
#[derive(Debug, Clone, PartialEq)]
pub enum HeaderValue {
    Unsigned(u64),
    Signed(i64),
    Float(f64),
    Bytes(Vec<u8>),
}
#[derive(Debug, Clone)]
pub struct Section {
    pub name: &'static str,
    pub offset: usize,
    pub bytes: usize,
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    pub source_type: Option<PixelType>,
    pub little_endian: bool,
    pub compression: Compression,
    pub exception16: usize,
    pub exception32: usize,
    /// Original TY5 little-endian byte offsets into the inline difference stream.
    /// Empty for raw/TY1 storage.
    pub row_offsets: Vec<u32>,
    pub header_version: String,
    pub header: Vec<u8>,
    pub sections: Vec<Section>,
    /// Original stored acquisition scalars. Angles in motor steps/radians and
    /// statistical variance remain source values, not derived degrees/std.
    pub fields: BTreeMap<String, HeaderValue>,
    pub trailer: Vec<u8>,
}
pub struct Reader {
    source: Source,
    metadata: Metadata,
    offset: u64,
    bytes: usize,
    count: usize,
}
impl Reader {
    /// Index one frozen Oxford image. Stored row order, negative values and
    /// reserved counts survive; no correction/normalization is applied.
    /// # Errors
    /// Bounds, unsupported version/layout, malformed exception counts or truncation.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let source = Source::open(path)?;
        if limits.header_bytes < 256 {
            return Err(Error("Oxford ASCII header exceeds limit".into()));
        }
        let mut prefix = Vec::new();
        source.read(0, 256, &mut prefix)?;
        let lines: Vec<_> = prefix.split(|&b| b == b'\n').collect();
        if lines.len() < 6 || !lines[0].starts_with(b"OD ") {
            return Err(Error("invalid Oxford image magic/header".into()));
        }
        let header_version = std::str::from_utf8(lines[0])
            .map_err(|_| Error("non-UTF8 Oxford version".into()))?
            .trim()
            .to_owned();
        let version = header_version
            .split_whitespace()
            .last()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite())
            .ok_or_else(|| Error("invalid Oxford header version".into()))?;
        if !(3.0..5.0).contains(&version) {
            return Err(Error("unsupported Oxford binary header version".into()));
        }
        let compression = match lines[1].get(12..15).map(trim_bytes) {
            Some(b"TY1") => Compression::Ty1,
            Some(b"NO") => Compression::None,
            Some(b"TY5") => Compression::Ty5,
            _ => return Err(Error("unsupported Oxford compression selector".into())),
        };
        let supplements = number_at(lines[4], 12, 19)?;
        if supplements != 0 {
            return Err(Error(
                "unsupported Oxford supplementary frame/header layout".into(),
            ));
        }
        let width = number_at(lines[2], 3, 7)?;
        let height = number_at(lines[2], 11, 15)?;
        let exception16 = number_at(lines[2], 19, 26)?;
        let exception32 = number_at(lines[2], 30, 37)?;
        let header_size = number_at(lines[3], 8, 15)?;
        let sizes = [
            number_at(lines[3], 19, 26)?,
            number_at(lines[3], 30, 37)?,
            number_at(lines[3], 41, 48)?,
            number_at(lines[3], 52, 59)?,
            number_at(lines[3], 63, lines[3].len())?,
        ];
        if header_size < 256 || header_size > limits.header_bytes {
            return Err(Error("invalid/oversized Oxford header size".into()));
        }
        let binary = sizes.iter().try_fold(0usize, |n, &s| sum(n, s))?;
        let ascii = if version < 4.0 {
            header_size
                .checked_sub(binary)
                .ok_or_else(|| Error("Oxford sections exceed header size".into()))?
        } else {
            256
        };
        if ascii < 256 || sum(ascii, binary)? > header_size {
            return Err(Error("Oxford sections exceed header size".into()));
        }
        let count = limits.pixels(
            [height, width],
            if compression == Compression::Ty1 {
                PixelType::I8
            } else {
                PixelType::I32
            },
        )?;
        if exception16 > count || exception32 > count || sum(exception16, exception32)? > count {
            return Err(Error("Oxford exception counts exceed image".into()));
        }
        if compression == Compression::None && (exception16 != 0 || exception32 != 0) {
            return Err(Error(
                "uncompressed Oxford frame declares exception arrays".into(),
            ));
        }
        let bytes = if compression != Compression::None {
            sum(
                sum(count, product(exception16, 2)?)?,
                product(exception32, 4)?,
            )?
        } else {
            product(count, 4)?
        };
        let bytes = if compression == Compression::Ty5 {
            sum(bytes, product(height, 4)?)?
        } else {
            bytes
        };
        if bytes > limits.encoded_bytes {
            return Err(Error("Oxford encoded payload exceeds limit".into()));
        }
        let end = (header_size as u64)
            .checked_add(bytes as u64)
            .filter(|&n| n <= source.bytes())
            .ok_or_else(|| {
                Error(if compression == Compression::Ty5 {
                    "truncated Oxford TY5 image/row offsets".into()
                } else {
                    "truncated Oxford image/exception arrays".into()
                })
            })?;
        let trailing = usize::try_from(source.bytes() - end)
            .map_err(|_| Error("Oxford trailer size overflow".into()))?;
        if trailing > limits.header_bytes {
            return Err(Error("Oxford trailer exceeds metadata limit".into()));
        }
        let mut header = Vec::new();
        source.read(0, header_size, &mut header)?;
        let mut trailer = Vec::new();
        source.read(end, trailing, &mut trailer)?;
        let mut offset = ascii;
        let mut sections = Vec::new();
        for (name, bytes) in ["General", "Special", "KM4", "Statistics", "History"]
            .into_iter()
            .zip(sizes)
        {
            sections.push(Section {
                name,
                offset,
                bytes,
            });
            offset = sum(offset, bytes)?;
        }
        let mut fields = BTreeMap::new();
        for (key, offset) in [
            ("Binning x", 0),
            ("Binning y", 2),
            ("Detector size x", 22),
            ("Detector size y", 24),
            ("Pixels x", 26),
            ("Pixels y", 28),
        ] {
            unsigned16(&header, &sections[0], offset, key, &mut fields)?;
        }
        unsigned32(&header, &sections[0], 36, "Number of pixels", &mut fields)?;
        for (key, offset) in [
            ("Gain", 56),
            ("Exposure time seconds", 480),
            ("Overflow time seconds", 488),
            ("Pixel size x mm", 568),
            ("Pixel size y mm", 576),
        ] {
            float64(&header, &sections[1], offset, key, &mut fields)?;
        }
        for (key, offset) in [
            ("Overflow threshold", 472),
            ("Monitor raw1", 528),
            ("Monitor raw2", 532),
            ("Monitor overflow1", 536),
            ("Monitor overflow2", 540),
            ("Unwarping", 544),
            ("Detector type code", 548),
        ] {
            signed32(&header, &sections[1], offset, key, &mut fields)?;
        }
        for (key, offset) in [
            ("Beam rotation e2 degrees", 552),
            ("Beam rotation e3 degrees", 560),
            ("Wavelength alpha1", 568),
            ("Wavelength alpha2", 576),
            ("Wavelength alpha", 584),
            ("Wavelength beta", 592),
            ("Detector tilt e1 degrees", 640),
            ("Detector tilt e2 degrees", 648),
            ("Detector tilt e3 degrees", 656),
            ("Beam center x", 664),
            ("Beam center y", 672),
            ("Alpha angle degrees", 680),
            ("Beta angle degrees", 688),
            ("Distance mm", 712),
        ] {
            float64(&header, &sections[2], offset, key, &mut fields)?;
        }
        for (label, offset) in [("Start", 284), ("End", 324), ("Zero correction", 512)] {
            for (axis, delta) in ["Omega", "Theta", "Kappa", "Phi", "Unused"]
                .into_iter()
                .zip([0, 4, 8, 12, 16])
            {
                signed32(
                    &header,
                    &sections[2],
                    offset + delta,
                    &format!("{axis} {label} motor steps"),
                    &mut fields,
                )?;
            }
        }
        for (axis, delta) in ["Omega", "Theta", "Kappa", "Phi", "Unused"]
            .into_iter()
            .zip([0, 8, 16, 24, 32])
        {
            float64(
                &header,
                &sections[2],
                368 + delta,
                &format!("{axis} radians per motor step"),
                &mut fields,
            )?;
        }
        for (key, offset) in [("Stat minimum", 0), ("Stat maximum", 4)] {
            signed32(&header, &sections[3], offset, key, &mut fields)?;
        }
        for (key, offset) in [
            ("Stat average", 24),
            ("Stat variance", 32),
            ("Stat skewness", 40),
        ] {
            float64(&header, &sections[3], offset, key, &mut fields)?;
        }
        fields.insert(
            "Spatial correction file".into(),
            HeaderValue::Bytes(section_bytes(&header, &sections[2], 26, 246)?.to_vec()),
        );
        fields.insert(
            "Spatial correction date".into(),
            HeaderValue::Bytes(section_bytes(&header, &sections[2], 0, 26)?.to_vec()),
        );
        fields.insert(
            "Flood field image".into(),
            HeaderValue::Bytes(section_bytes(&header, &sections[4], 99, 27)?.to_vec()),
        );
        let row_offset_bytes = if compression == Compression::Ty5 {
            product(height, 4)?
        } else {
            0
        };
        if header
            .len()
            .saturating_mul(2)
            .saturating_add(fields.len().saturating_mul(160))
            .saturating_add(trailer.len())
            .saturating_add(row_offset_bytes)
            > limits.metadata_bytes
        {
            return Err(Error("Oxford retained metadata exceeds limit".into()));
        }
        let mut row_offsets = Vec::new();
        let pixel_type = if compression == Compression::Ty5 {
            let mut encoded = Vec::new();
            source.read(header_size as u64, bytes, &mut encoded)?;
            ty5::decode(
                &encoded,
                [height, width],
                [exception16, exception32],
                |_, _| Ok(()),
            )?;
            row_offsets = encoded[bytes - row_offset_bytes..]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&v| u32::from_le_bytes(v))
                .collect();
            PixelType::I32
        } else if compression == Compression::None {
            PixelType::I32
        } else {
            let mut encoded = Vec::new();
            source.read(header_size as u64, bytes, &mut encoded)?;
            let (min, max) = ty1(&encoded, count, exception16, exception32, |_, _| Ok(()))?;
            if i8::try_from(min).is_ok() && i8::try_from(max).is_ok() {
                PixelType::I8
            } else if i16::try_from(min).is_ok() && i16::try_from(max).is_ok() {
                PixelType::I16
            } else if i32::try_from(min).is_ok() && i32::try_from(max).is_ok() {
                PixelType::I32
            } else {
                PixelType::I64
            }
        };
        limits.pixels([height, width], pixel_type)?;
        source.check()?;
        Ok(Self {
            source,
            metadata: Metadata {
                shape: [height, width],
                pixel_type,
                source_type: if compression == Compression::None {
                    Some(PixelType::I32)
                } else {
                    None
                },
                little_endian: true,
                compression,
                exception16,
                exception32,
                row_offsets,
                header_version,
                header,
                sections,
                fields,
                trailer,
            },
            offset: header_size as u64,
            bytes,
            count,
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
    /// Read exact native signed samples, using both extrema for type admission.
    /// # Errors
    /// Mutation, truncated/excess exceptions, cumulative or declared-type overflow.
    pub fn read_frame_into(&self, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.source
            .read(self.offset, self.bytes, &mut buffer.encoded)?;
        let meta = &self.metadata;
        buffer
            .pixels
            .resize(meta.pixel_type, self.count)
            .map_err(Error)?;
        if meta.compression == Compression::None {
            copy_samples(&buffer.encoded, &mut buffer.pixels, true)?;
        } else if meta.compression == Compression::Ty5 {
            let PixelBuffer::I32(output) = &mut buffer.pixels else {
                return Err(Error("Oxford TY5 requires signed32 output".into()));
            };
            ty5::decode(
                &buffer.encoded,
                meta.shape,
                [meta.exception16, meta.exception32],
                |i, value| {
                    output[i] = value;
                    Ok(())
                },
            )?;
        } else {
            ty1(
                &buffer.encoded,
                self.count,
                meta.exception16,
                meta.exception32,
                |i, v| {
                    macro_rules! put {($($variant:ident,$t:ty);*)=>{match &mut buffer.pixels {$(PixelBuffer::$variant(out)=>out[i]=<$t>::try_from(v).map_err(|_|Error("Oxford decoded value exceeds admitted type".into()))?),*,_=>return Err(Error("Oxford TY1 requires signed output".into()))}};}
                    put!(I8,i8;I16,i16;I32,i32;I64,i64);
                    Ok(())
                },
            )?;
        }
        self.source.check()?;
        buffer.shape = Some(meta.shape);
        Ok(())
    }
}
fn trim_bytes(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}
fn number_at(line: &[u8], start: usize, end: usize) -> Result<usize, Error> {
    let bytes = line
        .get(start..end)
        .ok_or_else(|| Error("short Oxford ASCII header field".into()))?;
    std::str::from_utf8(trim_bytes(bytes))
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Error("invalid Oxford ASCII integer field".into()))
}
fn section_bytes<'a>(
    header: &'a [u8],
    section: &Section,
    offset: usize,
    bytes: usize,
) -> Result<&'a [u8], Error> {
    if sum(offset, bytes)? > section.bytes {
        return Err(Error(format!(
            "short Oxford {} header section",
            section.name
        )));
    }
    header
        .get(sum(section.offset, offset)?..sum(sum(section.offset, offset)?, bytes)?)
        .ok_or_else(|| Error("truncated Oxford binary section".into()))
}
fn unsigned16(
    h: &[u8],
    s: &Section,
    o: usize,
    k: &str,
    f: &mut BTreeMap<String, HeaderValue>,
) -> Result<(), Error> {
    f.insert(
        k.into(),
        HeaderValue::Unsigned(
            u16::from_le_bytes(section_bytes(h, s, o, 2)?.try_into().unwrap()).into(),
        ),
    );
    Ok(())
}
fn unsigned32(
    h: &[u8],
    s: &Section,
    o: usize,
    k: &str,
    f: &mut BTreeMap<String, HeaderValue>,
) -> Result<(), Error> {
    f.insert(
        k.into(),
        HeaderValue::Unsigned(
            u32::from_le_bytes(section_bytes(h, s, o, 4)?.try_into().unwrap()).into(),
        ),
    );
    Ok(())
}
fn signed32(
    h: &[u8],
    s: &Section,
    o: usize,
    k: &str,
    f: &mut BTreeMap<String, HeaderValue>,
) -> Result<(), Error> {
    f.insert(
        k.into(),
        HeaderValue::Signed(
            i32::from_le_bytes(section_bytes(h, s, o, 4)?.try_into().unwrap()).into(),
        ),
    );
    Ok(())
}
fn float64(
    h: &[u8],
    s: &Section,
    o: usize,
    k: &str,
    f: &mut BTreeMap<String, HeaderValue>,
) -> Result<(), Error> {
    f.insert(
        k.into(),
        HeaderValue::Float(f64::from_le_bytes(
            section_bytes(h, s, o, 8)?.try_into().unwrap(),
        )),
    );
    Ok(())
}
fn ty1(
    input: &[u8],
    count: usize,
    n16: usize,
    n32: usize,
    mut put: impl FnMut(usize, i64) -> Result<(), Error>,
) -> Result<(i64, i64), Error> {
    let offset32 = sum(count, product(n16, 2)?)?;
    if input.len() != sum(offset32, product(n32, 4)?)? {
        return Err(Error("Oxford TY1 payload length mismatch".into()));
    }
    let mut words16 = input[count..offset32].as_chunks::<2>().0.iter();
    let mut words32 = input[offset32..].as_chunks::<4>().0.iter();
    let mut value = 0i64;
    let mut min = i64::MAX;
    let mut max = i64::MIN;
    for (i, &byte) in input[..count].iter().enumerate() {
        let delta = match byte {
            254 => i64::from(i16::from_le_bytes(*words16.next().ok_or_else(|| {
                Error("short Oxford TY1 16-bit exception table".into())
            })?)),
            255 => i64::from(i32::from_le_bytes(*words32.next().ok_or_else(|| {
                Error("short Oxford TY1 32-bit exception table".into())
            })?)),
            _ => i64::from(byte) - 127,
        };
        value = value
            .checked_add(delta)
            .ok_or_else(|| Error("Oxford TY1 cumulative count overflow".into()))?;
        min = min.min(value);
        max = max.max(value);
        put(i, value)?;
    }
    if words16.next().is_some() || words32.next().is_some() {
        return Err(Error("unused Oxford TY1 exception entries".into()));
    }
    Ok((min, max))
}
