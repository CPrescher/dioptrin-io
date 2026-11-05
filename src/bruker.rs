//! Bruker format 86/100 counts, with storage overflow/underflow expansion.
//! LINEAR calibration is retained as metadata and never applied implicitly.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelBuffer, PixelType,
    binary::copy_samples,
    detector_common::{Source, product, sum},
};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V86,
    V100,
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub version: Version,
    pub shape: [usize; 2],
    pub storage_type: PixelType,
    pub pixel_type: PixelType,
    pub little_endian: bool,
    pub header: Vec<u8>,
    /// Repeated free-text records are joined with LF. Original records survive
    /// byte-for-byte in header; structural duplicates fail explicitly.
    pub fields: BTreeMap<String, String>,
    pub linear: Option<[f64; 2]>,
    pub baseline: i32,
    pub overflow_counts: [usize; 3],
    pub underflow_bytes: usize,
    /// Original bounded bytes after the declared image/tables (including padding).
    pub trailer: Vec<u8>,
}
pub struct Reader {
    source: Source,
    metadata: Metadata,
    offset: u64,
    bytes: usize,
    main: usize,
    table_sizes: [usize; 3],
}
impl Reader {
    /// Open exactly one immutable Bruker frame. Numbered series are metadata,
    /// not neighboring files selected automatically.
    /// # Errors
    /// Invalid headers/types, truncated tables, shape/resource admission or mutation.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let source = Source::open(path)?;
        if limits.header_bytes < 2560 {
            return Err(Error("Bruker minimum header exceeds limit".into()));
        }
        let mut header = Vec::new();
        source.read(0, 2560, &mut header)?;
        let prefix = parse_header(&header)?;
        let blocks = integer(&prefix, "HDRBLKS")?;
        let header_size = product(blocks, 512)?;
        if blocks < 5 || blocks % 5 != 0 || header_size > limits.header_bytes {
            return Err(Error("invalid/oversized Bruker HDRBLKS".into()));
        }
        source.read(0, header_size, &mut header)?;
        let fields = parse_header(&header)?;
        let version = match fields.get("FORMAT").map(String::as_str).unwrap_or("86") {
            "86" => Version::V86,
            "100" => Version::V100,
            _ => return Err(Error("unsupported Bruker format version".into())),
        };
        for key in ["WORDORD", "LONGORD"] {
            if fields.get(key).is_some_and(|v| v.trim() != "0") {
                return Err(Error(format!(
                    "unsupported Bruker {key}; only little-endian storage is admitted"
                )));
            }
        }
        if let Some(compression) = fields.get("COMPRES")
            && !matches!(
                compression.trim(),
                "NONE" | "LINEAR" | "None" | "none" | "0" | ""
            )
        {
            return Err(Error(format!(
                "unsupported Bruker compression {compression}"
            )));
        }
        let shape = [integer(&fields, "NROWS")?, integer(&fields, "NCOLS")?];
        let bpp = numbers(&fields, "NPIXELB")?;
        let width = usize::try_from(
            *bpp.first()
                .ok_or_else(|| Error("missing Bruker NPIXELB".into()))?,
        )
        .map_err(|_| Error("negative Bruker sample width".into()))?;
        let storage_type = match (version, width) {
            (Version::V86, 1) | (Version::V100, 1) => PixelType::U8,
            (Version::V86, 2) | (Version::V100, 2) => PixelType::U16,
            (Version::V86, 4) => PixelType::U32,
            (Version::V100, 4) => PixelType::I32,
            _ => return Err(Error("unsupported Bruker stored sample width".into())),
        };
        let nover = numbers(&fields, "NOVERFL")?;
        let (counts, underflow_bytes, baseline) = match version {
            Version::V86 => {
                if nover.len() != 1 || nover[0] < 0 {
                    return Err(Error("invalid Bruker86 NOVERFL".into()));
                }
                (
                    [
                        0,
                        0,
                        usize::try_from(nover[0])
                            .map_err(|_| Error("Bruker overflow count exceeds platform".into()))?,
                    ],
                    0,
                    0,
                )
            }
            Version::V100 => {
                if nover.len() != 3 || nover[0] < -1 || nover[1] < 0 || nover[2] < 0 {
                    return Err(Error("invalid Bruker100 NOVERFL".into()));
                }
                let under = if nover[0] > 0 {
                    usize::try_from(
                        *bpp.get(1)
                            .ok_or_else(|| Error("missing Bruker underflow width".into()))?,
                    )
                    .map_err(|_| Error("invalid underflow width".into()))?
                } else {
                    0
                };
                if under != 0 && ![1, 2, 4].contains(&under) {
                    return Err(Error("unsupported Bruker underflow width".into()));
                }
                let base = if nover[0] == -1 {
                    0
                } else {
                    let nexp = numbers(&fields, "NEXP")?;
                    i32::try_from(
                        *nexp
                            .get(2)
                            .ok_or_else(|| Error("missing Bruker100 baseline in NEXP".into()))?,
                    )
                    .map_err(|_| Error("Bruker baseline exceeds signed32".into()))?
                };
                let mut counts = [0; 3];
                for (out, &n) in counts.iter_mut().zip(&nover) {
                    *out = usize::try_from(n.max(0))
                        .map_err(|_| Error("Bruker overflow count exceeds platform".into()))?;
                }
                (counts, under, base)
            }
        };
        let pixel_type = if version == Version::V100 {
            PixelType::I32
        } else if counts[2] > 0 {
            PixelType::U32
        } else {
            storage_type
        };
        let count = limits.pixels(shape, pixel_type)?;
        if counts.iter().any(|&n| n > count) {
            return Err(Error("Bruker table count exceeds number of pixels".into()));
        }
        let main = product(count, width)?;
        let table_sizes = if version == Version::V86 {
            [0, 0, product(counts[2], 16)?]
        } else {
            [
                padded(product(counts[0], underflow_bytes)?)?,
                padded(product(counts[1], 2)?)?,
                padded(product(counts[2], 4)?)?,
            ]
        };
        let bytes = table_sizes.iter().try_fold(main, |n, &v| sum(n, v))?;
        if bytes > limits.encoded_bytes {
            return Err(Error("Bruker encoded payload exceeds limit".into()));
        }
        let end = (header_size as u64)
            .checked_add(bytes as u64)
            .filter(|&n| n <= source.bytes())
            .ok_or_else(|| Error("truncated Bruker image/tables".into()))?;
        let remaining = usize::try_from(source.bytes() - end)
            .map_err(|_| Error("Bruker trailer exceeds platform".into()))?;
        if remaining > limits.header_bytes {
            return Err(Error("Bruker trailer exceeds limit".into()));
        }
        let mut trailer = Vec::new();
        source.read(end, remaining, &mut trailer)?;
        let retained = header
            .len()
            .saturating_mul(3)
            .saturating_add(fields.len().saturating_mul(128))
            .saturating_add(trailer.len());
        if retained > limits.metadata_bytes {
            return Err(Error("Bruker retained metadata exceeds limit".into()));
        }
        let linear = fields
            .get("LINEAR")
            .map(|s| {
                let values = s
                    .split_whitespace()
                    .map(str::parse::<f64>)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| Error("invalid Bruker LINEAR metadata".into()))?;
                if values.len() != 2 || values.iter().any(|n| !n.is_finite()) {
                    return Err(Error("invalid Bruker LINEAR metadata".into()));
                }
                Ok([values[0], values[1]])
            })
            .transpose()?;
        source.check()?;
        Ok(Self {
            source,
            metadata: Metadata {
                version,
                shape,
                storage_type,
                pixel_type,
                little_endian: true,
                header,
                fields,
                linear,
                baseline,
                overflow_counts: counts,
                underflow_bytes,
                trailer,
            },
            offset: header_size as u64,
            bytes,
            main,
            table_sizes,
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
    /// Expand only the declared storage tables. LINEAR is not a correction
    /// performed here; consumers can explicitly interpret that metadata.
    /// # Errors
    /// Mutation, malformed table markers/positions, duplicate entries or count overflow.
    pub fn read_frame_into(&self, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.source
            .read(self.offset, self.bytes, &mut buffer.encoded)?;
        let meta = &self.metadata;
        let count = meta.shape[0] * meta.shape[1];
        buffer
            .pixels
            .resize(meta.pixel_type, count)
            .map_err(Error)?;
        if meta.version == Version::V86 {
            self.decode86(buffer)?;
        } else {
            self.decode100(buffer)?;
        }
        self.source.check()?;
        buffer.shape = Some(meta.shape);
        Ok(())
    }
    fn decode86(&self, buffer: &mut FrameBuffer) -> Result<(), Error> {
        let meta = &self.metadata;
        if meta.overflow_counts[2] == 0 {
            return copy_samples(&buffer.encoded[..self.main], &mut buffer.pixels, true);
        }
        let PixelBuffer::U32(out) = &mut buffer.pixels else {
            unreachable!()
        };
        read_base(&buffer.encoded[..self.main], meta.storage_type, |i, v| {
            out[i] = v as u32;
            Ok(())
        })?;
        let mut positions = Vec::new();
        positions
            .try_reserve_exact(out.len())
            .map_err(|e| Error(e.to_string()))?;
        positions.resize(out.len(), false);
        for record in buffer.encoded[self.main..].as_chunks::<16>().0.iter() {
            let intensity = unsigned_ascii(&record[..9])?;
            let position = unsigned_ascii(&record[9..])?;
            let slot = out
                .get_mut(position)
                .ok_or_else(|| Error("Bruker86 overflow position outside image".into()))?;
            if positions[position] {
                return Err(Error("duplicate Bruker86 overflow position".into()));
            }
            positions[position] = true;
            let marker = match meta.storage_type {
                PixelType::U8 => 255,
                PixelType::U16 => 65535,
                PixelType::U32 => u32::MAX,
                _ => unreachable!(),
            };
            if *slot != marker {
                return Err(Error(
                    "Bruker86 overflow does not reference a stored sentinel".into(),
                ));
            }
            *slot = u32::try_from(intensity)
                .map_err(|_| Error("Bruker86 intensity exceeds u32".into()))?;
        }
        Ok(())
    }
    fn decode100(&self, buffer: &mut FrameBuffer) -> Result<(), Error> {
        let meta = &self.metadata;
        let PixelBuffer::I32(out) = &mut buffer.pixels else {
            unreachable!()
        };
        read_base(&buffer.encoded[..self.main], meta.storage_type, |i, v| {
            out[i] = i32::try_from(v)
                .map_err(|_| Error("Bruker100 base sample exceeds signed32".into()))?;
            Ok(())
        })?;
        let offsets = [
            self.main,
            sum(self.main, self.table_sizes[0])?,
            sum(sum(self.main, self.table_sizes[0])?, self.table_sizes[1])?,
        ];
        if meta.storage_type == PixelType::U8 && meta.overflow_counts[1] > 0 {
            let mut table = buffer.encoded[offsets[1]..offsets[1] + meta.overflow_counts[1] * 2]
                .as_chunks::<2>()
                .0
                .iter();
            for value in out.iter_mut().filter(|v| **v == 255) {
                *value = i32::from(u16::from_le_bytes(
                    *table
                        .next()
                        .ok_or_else(|| Error("short Bruker100 overflow1 table".into()))?,
                ));
            }
            if table.next().is_some() {
                return Err(Error("unused Bruker100 overflow1 entries".into()));
            }
        } else if meta.overflow_counts[1] > 0 {
            return Err(Error(
                "Bruker100 overflow1 table incompatible with stored type".into(),
            ));
        }
        if meta.storage_type != PixelType::I32 && meta.overflow_counts[2] > 0 {
            let mut table = buffer.encoded[offsets[2]..offsets[2] + meta.overflow_counts[2] * 4]
                .as_chunks::<4>()
                .0
                .iter();
            for value in out.iter_mut().filter(|v| **v == 65535) {
                *value = i32::from_le_bytes(
                    *table
                        .next()
                        .ok_or_else(|| Error("short Bruker100 overflow2 table".into()))?,
                );
            }
            if table.next().is_some() {
                return Err(Error("unused Bruker100 overflow2 entries".into()));
            }
        } else if meta.overflow_counts[2] > 0 {
            return Err(Error(
                "Bruker100 overflow2 table incompatible with stored type".into(),
            ));
        }
        let mut under = buffer.encoded
            [offsets[0]..offsets[0] + meta.overflow_counts[0] * meta.underflow_bytes]
            .chunks_exact(meta.underflow_bytes.max(1));
        for value in out.iter_mut() {
            if meta.overflow_counts[0] > 0 && *value == 0 {
                let bytes = under
                    .next()
                    .ok_or_else(|| Error("short Bruker100 underflow table".into()))?;
                *value = match meta.underflow_bytes {
                    1 => i32::from(bytes[0] as i8),
                    2 => i32::from(i16::from_le_bytes(bytes.try_into().unwrap())),
                    4 => i32::from_le_bytes(bytes.try_into().unwrap()),
                    _ => unreachable!(),
                };
            } else {
                *value = value
                    .checked_add(meta.baseline)
                    .ok_or_else(|| Error("Bruker100 baseline expansion exceeds signed32".into()))?;
            }
        }
        if under.next().is_some() {
            return Err(Error("unused Bruker100 underflow entries".into()));
        }
        Ok(())
    }
}
fn padded(bytes: usize) -> Result<usize, Error> {
    bytes
        .checked_add(15)
        .map(|n| n / 16 * 16)
        .ok_or_else(|| Error("Bruker table padding overflow".into()))
}
fn unsigned_ascii(bytes: &[u8]) -> Result<usize, Error> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| Error("invalid Bruker ASCII overflow entry".into()))
}
fn parse_header(header: &[u8]) -> Result<BTreeMap<String, String>, Error> {
    if !header.is_ascii() {
        return Err(Error("non-ASCII Bruker header".into()));
    }
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    for record in header.chunks(80) {
        if let Some(colon) = record.iter().position(|&b| b == b':') {
            let key = std::str::from_utf8(&record[..colon]).unwrap().trim();
            let value = std::str::from_utf8(&record[colon + 1..]).unwrap().trim();
            if key.is_empty() {
                return Err(Error("empty Bruker header key".into()));
            }
            if let Some(previous) = fields.get_mut(key) {
                if [
                    "FORMAT", "HDRBLKS", "NROWS", "NCOLS", "NPIXELB", "NOVERFL", "NEXP", "WORDORD",
                    "LONGORD", "LINEAR",
                ]
                .contains(&key)
                {
                    return Err(Error(format!("duplicate Bruker structural field {key}")));
                }
                previous.push('\n');
                previous.push_str(value);
            } else {
                fields.insert(key.into(), value.into());
            }
        }
    }
    Ok(fields)
}
fn numbers(fields: &BTreeMap<String, String>, key: &str) -> Result<Vec<i64>, Error> {
    fields
        .get(key)
        .ok_or_else(|| Error(format!("missing Bruker {key}")))?
        .split_whitespace()
        .map(|s| {
            s.parse::<i64>()
                .map_err(|_| Error(format!("invalid Bruker {key}")))
        })
        .collect()
}
fn integer(fields: &BTreeMap<String, String>, key: &str) -> Result<usize, Error> {
    usize::try_from(
        *numbers(fields, key)?
            .first()
            .ok_or_else(|| Error(format!("missing Bruker {key} value")))?,
    )
    .map_err(|_| Error(format!("negative Bruker {key}")))
}
fn read_base(
    input: &[u8],
    ty: PixelType,
    mut put: impl FnMut(usize, i64) -> Result<(), Error>,
) -> Result<(), Error> {
    for (i, bytes) in input.chunks_exact(ty.bytes()).enumerate() {
        put(
            i,
            match ty {
                PixelType::U8 => i64::from(bytes[0]),
                PixelType::U16 => i64::from(u16::from_le_bytes(bytes.try_into().unwrap())),
                PixelType::U32 => i64::from(u32::from_le_bytes(bytes.try_into().unwrap())),
                PixelType::I32 => i64::from(i32::from_le_bytes(bytes.try_into().unwrap())),
                _ => return Err(Error("unsupported Bruker stored sample type".into())),
            },
        )?;
    }
    Ok(())
}
