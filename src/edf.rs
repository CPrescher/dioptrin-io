//! Strict EDF inline detector frames with stored orientation and native scalars.
//! Supports concatenated 2D frames, ordinary endian types and bounded internal
//! gzip/zlib/bzip2. External payloads/general headers and outer compression are
//! explicitly unsupported in this first implementation.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelType,
    binary::{copy_samples, resize_bytes},
    source::Snapshot,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};
#[derive(Debug, Clone)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    pub little_endian: bool,
    pub compression: String,
    /// Original padded header bytes, retained without interpreting geometry.
    pub header: Vec<u8>,
    /// Case-normalized EDF keys; raw spelling/ordering remains in `header`.
    pub fields: BTreeMap<String, String>,
}
struct Page {
    metadata: Metadata,
    offset: u64,
    bytes: usize,
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    pages: Vec<Page>,
}
impl Reader {
    /// Index an immutable inline EDF scan. All primary frames are checked before
    /// pixel allocation; headers are not silently defaulted or repaired.
    /// # Errors
    /// Unsupported type/compression/external layout, malformed headers or limits.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut input = BufReader::new(file.try_clone()?);
        let mut pages = Vec::new();
        let mut offset = 0;
        let mut total = 0_usize;
        while offset < snapshot.bytes() {
            if pages.len() >= limits.frames {
                return Err(Error("EDF exceeds frame limit".into()));
            }
            input.seek(SeekFrom::Start(offset))?;
            let mut header = Vec::new();
            loop {
                if header.len() >= limits.header_bytes {
                    return Err(Error(
                        "EDF header exceeds limit or lacks closing brace".into(),
                    ));
                }
                let mut b = [0];
                input.read_exact(&mut b)?;
                header.push(b[0]);
                if b[0] == b'}' {
                    break;
                }
            }
            let text = std::str::from_utf8(&header)
                .map_err(|_| Error("EDF headers must be UTF-8/ASCII".into()))?;
            let body = text
                .trim_start()
                .strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .ok_or_else(|| {
                    Error("invalid EDF header braces; outer compression is unsupported".into())
                })?;
            let mut fields = BTreeMap::new();
            for entry in body.split(';') {
                let entry = entry.trim();
                if entry.is_empty() {
                    continue;
                }
                let (k, v) = entry
                    .split_once('=')
                    .ok_or_else(|| Error("invalid EDF header assignment".into()))?;
                if fields
                    .insert(k.trim().to_ascii_uppercase(), v.trim().to_owned())
                    .is_some()
                {
                    return Err(Error("duplicate EDF header field".into()));
                }
            }
            let field = |k: &str| {
                fields
                    .get(k)
                    .map(String::as_str)
                    .ok_or_else(|| Error(format!("missing EDF field {k}")))
            };
            let number = |k: &str| {
                field(k)?
                    .parse::<usize>()
                    .map_err(|_| Error(format!("invalid EDF field {k}")))
            };
            if fields.contains_key("EDF_BINARYFILENAME") {
                return Err(Error(
                    "external EDF payloads require separate source validation".into(),
                ));
            }
            if fields
                .keys()
                .filter(|k| k.starts_with("DIM_"))
                .any(|k| k != "DIM_1" && k != "DIM_2" && fields[k] != "1")
            {
                return Err(Error("EDF volume dimensions are unsupported".into()));
            }
            let shape = [number("DIM_2")?, number("DIM_1")?];
            let pixel_type = edf_type(field("DATATYPE")?)?;
            let count = limits.pixels(shape, pixel_type)?;
            let little_endian = match field("BYTEORDER")?.to_ascii_lowercase().as_str() {
                "lowbytefirst" => true,
                "highbytefirst" => false,
                _ => return Err(Error("unsupported EDF byte order".into())),
            };
            let compression = fields
                .get("COMPRESSION")
                .map(|s| s.to_ascii_uppercase())
                .unwrap_or_else(|| "NONE".into());
            if ![
                "NONE",
                "NO",
                "NO COMPRESSION",
                "GZIP",
                "ZLIB",
                "Z",
                "BZ2",
                "BZIP2",
            ]
            .contains(&compression.as_str())
            {
                return Err(Error(format!("unsupported EDF compression {compression}")));
            }
            let bytes = if fields.contains_key("EDF_BINARYSIZE") {
                number("EDF_BINARYSIZE")?
            } else {
                number("SIZE")?
            };
            let plain = ["NONE", "NO", "NO COMPRESSION"].contains(&compression.as_str());
            if bytes == 0
                || bytes > limits.encoded_bytes
                || (plain && bytes != count * pixel_type.bytes())
            {
                return Err(Error("invalid EDF encoded payload size".into()));
            }
            let header_size = if fields.contains_key("EDF_HEADERSIZE") {
                number("EDF_HEADERSIZE")?
            } else if fields.contains_key("HEADERSIZE") {
                number("HEADERSIZE")?
            } else {
                (header.len() + 1).div_ceil(512) * 512
            };
            if header_size < header.len() || header_size > limits.header_bytes {
                return Err(Error("invalid EDF header size".into()));
            }
            let previous = header.len();
            resize_bytes(&mut header, header_size)?;
            input.read_exact(&mut header[previous..])?;
            total = total
                .checked_add(
                    header_size
                        .saturating_mul(2)
                        .saturating_add(fields.len().saturating_mul(128)),
                )
                .ok_or_else(|| Error("EDF header storage overflow".into()))?;
            if total > limits.metadata_bytes {
                return Err(Error("EDF exceeds total metadata budget".into()));
            }
            let payload = offset
                .checked_add(header_size as u64)
                .ok_or_else(|| Error("EDF offset overflow".into()))?;
            offset = payload
                .checked_add(bytes as u64)
                .filter(|&n| n <= snapshot.bytes())
                .ok_or_else(|| Error("truncated EDF binary payload".into()))?;
            pages.push(Page {
                metadata: Metadata {
                    shape,
                    pixel_type,
                    little_endian,
                    compression,
                    header,
                    fields,
                },
                offset: payload,
                bytes,
            });
        }
        if pages.is_empty() {
            return Err(Error("empty EDF file".into()));
        }
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            pages,
        })
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.pages.len()
    }
    #[must_use]
    pub fn metadata(&self, index: usize) -> Option<&Metadata> {
        self.pages.get(index).map(|p| &p.metadata)
    }
    /// Read one original frame into worker-owned, reusable native buffers.
    /// # Errors
    /// Invalid index, changed source, corrupt/truncated/oversized decoded stream.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        let page = self
            .pages
            .get(index)
            .ok_or_else(|| Error("EDF frame index outside file".into()))?;
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(page.offset))?;
        resize_bytes(&mut buffer.encoded, page.bytes)?;
        file.read_exact(&mut buffer.encoded)?;
        let meta = &page.metadata;
        let count = meta.shape[0] * meta.shape[1];
        let bytes = count * meta.pixel_type.bytes();
        buffer
            .pixels
            .resize(meta.pixel_type, count)
            .map_err(Error)?;
        if matches!(meta.compression.as_str(), "NONE" | "NO" | "NO COMPRESSION") {
            copy_samples(&buffer.encoded, &mut buffer.pixels, meta.little_endian)?;
        } else {
            resize_bytes(&mut buffer.decoded, bytes)?;
            match meta.compression.as_str() {
                "GZIP" => decode_exact(
                    &mut flate2::read::MultiGzDecoder::new(&buffer.encoded[..]),
                    &mut buffer.decoded,
                )?,
                "ZLIB" | "Z" => {
                    let mut decoder = flate2::read::ZlibDecoder::new(&buffer.encoded[..]);
                    decode_exact(&mut decoder, &mut buffer.decoded)?;
                    if decoder.total_in() != buffer.encoded.len() as u64 {
                        return Err(Error("extra EDF zlib payload bytes".into()));
                    }
                }
                "BZ2" | "BZIP2" => decode_exact(
                    &mut bzip2::read::MultiBzDecoder::new(&buffer.encoded[..]),
                    &mut buffer.decoded,
                )?,
                _ => return Err(Error("unsupported EDF compression".into())),
            }
            copy_samples(&buffer.decoded, &mut buffer.pixels, meta.little_endian)?;
        }
        self.snapshot.check_handle(&file).map_err(Error)?;
        buffer.shape = Some(meta.shape);
        Ok(())
    }
}
fn edf_type(s: &str) -> Result<PixelType, Error> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "signedbyte" | "signed8" => PixelType::I8,
        "unsignedbyte" | "unsigned8" => PixelType::U8,
        "signedshort" | "signed16" => PixelType::I16,
        "unsignedshort" | "unsigned16" | "unsignedshortinteger" => PixelType::U16,
        "signedinteger" | "signed32" | "signedlong" => PixelType::I32,
        "unsignedinteger" | "unsigned32" | "unsignedlong" => PixelType::U32,
        "signed64" => PixelType::I64,
        "unsigned64" => PixelType::U64,
        "floatvalue" | "float" | "floatieee32" | "float32" => PixelType::F32,
        "double" | "doublevalue" | "floatieee64" | "doubleieee64" => PixelType::F64,
        _ => return Err(Error(format!("unsupported EDF scalar type {s}"))),
    })
}

fn decode_exact(input: &mut impl Read, output: &mut [u8]) -> Result<(), Error> {
    input.read_exact(output)?;
    let mut extra = [0];
    if input.read(&mut extra)? != 0 {
        return Err(Error(
            "EDF decoded stream exceeds declared dimensions".into(),
        ));
    }
    Ok(())
}
