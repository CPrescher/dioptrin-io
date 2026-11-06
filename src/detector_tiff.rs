//! Detector acquisition metadata layered on the existing TIFF count reader.
//! Geometry/origin/correction flags are retained, never applied to samples.
use crate::{source::Snapshot, tiff};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
pub use tiff::{Error, FrameBuffer, Limits};
mod mar_schema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pilatus,
    MarCcd,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Unsigned(u32),
    Signed(i32),
    UnsignedArray(Vec<u32>),
    SignedArray(Vec<i32>),
    ShortArray(Vec<u16>),
    Bytes(Vec<u8>),
}
pub use crate::pilatus::Header as PilatusHeader;
#[derive(Debug, Clone)]
pub struct MarHeader {
    pub little_endian: bool,
    pub bytes: Vec<u8>,
    /// Original published field names, including array dimensions, and unscaled
    /// native integer/byte values. Reserved bytes are retained too.
    pub fields: BTreeMap<String, Value>,
}
#[derive(Debug, Clone)]
pub enum Header {
    Pilatus(PilatusHeader),
    MarCcd(MarHeader),
}
pub struct Metadata<'a> {
    pub image: &'a tiff::Metadata,
    pub header: &'a Header,
}
pub struct Reader {
    reader: tiff::Reader,
    headers: Vec<Header>,
    format: Format,
}
impl Reader {
    /// Open explicitly selected detector TIFF. No detector-type retry occurs.
    /// # Errors
    /// Missing/malformed acquisition header, invalid TIFF, metadata limits or
    /// mutation. All original primary TIFF page contracts still apply.
    pub fn open(path: impl AsRef<Path>, format: Format, limits: Limits) -> Result<Self, Error> {
        let snapshot = Snapshot::open(path.as_ref()).map_err(Error)?;
        let mut headers = Vec::new();
        let mut used = 0usize;
        let reader = match format {
            Format::Pilatus => {
                let reader = tiff::Reader::open(&snapshot.path, limits)?;
                for index in 0..reader.frames() {
                    let description = match reader.metadata(index).unwrap().tags.get(&270) {
                        Some(tiff::TagValue::Ascii(s)) => s,
                        _ => {
                            return Err(Error(
                                "Pilatus TIFF requires an ASCII ImageDescription tag".into(),
                            ));
                        }
                    };
                    let header =
                        PilatusHeader::parse(description, limits.metadata_bytes).map_err(Error)?;
                    used = used
                        .checked_add(description.len().saturating_mul(8))
                        .and_then(|n| {
                            n.checked_add(description.lines().count().saturating_mul(512))
                        })
                        .ok_or_else(|| Error("Pilatus header storage overflow".into()))?;
                    if used >= limits.metadata_bytes {
                        return Err(Error("Pilatus headers exceed metadata budget".into()));
                    }
                    headers.push(Header::Pilatus(header));
                }
                // Account for both TIFF IFD metadata and semantic header storage.
                drop(reader);
                tiff::Reader::open(
                    &snapshot.path,
                    Limits {
                        metadata_bytes: limits.metadata_bytes - used,
                        ..limits
                    },
                )?
            }
            Format::MarCcd => {
                if limits.metadata_bytes < 32 * 1024 {
                    return Err(Error(
                        "MAR acquisition header exceeds metadata budget".into(),
                    ));
                }
                let mut file = File::open(&snapshot.path).map_err(|e| Error(e.to_string()))?;
                snapshot.check_handle(&file).map_err(Error)?;
                file.seek(SeekFrom::Start(1024))
                    .map_err(|e| Error(e.to_string()))?;
                let mut bytes = vec![0u8; 3072];
                file.read_exact(&mut bytes)
                    .map_err(|e| Error(e.to_string()))?;
                headers.push(Header::MarCcd(parse_mar(bytes)?));
                snapshot.check_handle(&file).map_err(Error)?;
                tiff::Reader::open(
                    &snapshot.path,
                    Limits {
                        metadata_bytes: limits.metadata_bytes - 32 * 1024,
                        ..limits
                    },
                )?
            }
        };
        snapshot.check().map_err(Error)?;
        Ok(Self {
            reader,
            headers,
            format,
        })
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.reader.frames()
    }
    #[must_use]
    pub fn metadata(&self, index: usize) -> Option<Metadata<'_>> {
        let image = self.reader.metadata(index)?;
        let header = self.headers.get(if self.format == Format::MarCcd {
            0
        } else {
            index
        })?;
        Some(Metadata { image, header })
    }
    /// Decode the unchanged source pixels through the existing TIFF reader.
    /// # Errors
    /// Same source mutation, index and payload failures as the underlying reader.
    pub fn read_frame_into(&mut self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        self.reader.read_frame_into(index, buffer)
    }
}
fn parse_mar(bytes: Vec<u8>) -> Result<MarHeader, Error> {
    if bytes.len() != 3072 || !bytes[4..20].starts_with(b"MMX\0") {
        return Err(Error(
            "unsupported MAR CCD acquisition-header signature/length".into(),
        ));
    }
    let word: [u8; 4] = bytes[28..32].try_into().unwrap();
    let little = if u32::from_le_bytes(word) == 1234 {
        true
    } else if u32::from_be_bytes(word) == 4321 {
        false
    } else {
        return Err(Error(
            "unsupported MAR acquisition-header byte-order marker".into(),
        ));
    };
    let u32_at = |pos: usize| {
        let b = bytes[pos..pos + 4].try_into().unwrap();
        if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    if u32_at(36) != 3072 {
        return Err(Error("unsupported MAR acquisition-header size".into()));
    }
    let mut fields = BTreeMap::new();
    for &(name, offset, kind, count) in mar_schema::FIELDS {
        let value = match (kind, count) {
            (b'c', _) => Value::Bytes(bytes[offset..offset + count].to_vec()),
            (b'I', 1) => Value::Unsigned(u32_at(offset)),
            (b'i', 1) => Value::Signed(u32_at(offset) as i32),
            (b'I', _) => Value::UnsignedArray((0..count).map(|n| u32_at(offset + n * 4)).collect()),
            (b'i', _) => {
                Value::SignedArray((0..count).map(|n| u32_at(offset + n * 4) as i32).collect())
            }
            (b'H', _) => Value::ShortArray(
                (0..count)
                    .map(|n| {
                        let b = bytes[offset + n * 2..offset + n * 2 + 2]
                            .try_into()
                            .unwrap();
                        if little {
                            u16::from_le_bytes(b)
                        } else {
                            u16::from_be_bytes(b)
                        }
                    })
                    .collect(),
            ),
            _ => return Err(Error("invalid internal MAR header schema".into())),
        };
        fields.insert(name.into(), value);
    }
    Ok(MarHeader {
        little_endian: little,
        bytes,
        fields,
    })
}
