//! Bounded CBF single-image byte-offset input, preserving signed/unsigned counts.
//! CIF text is retained verbatim with bounded scalar/loop metadata; MIME fields
//! define the binary payload. Optional text syntax diagnostics never alter counts.
pub use crate::binary::{Error, FrameBuffer, Limits};
mod header;
use crate::{
    PixelType,
    binary::{byte_offset, resize_bytes},
    source::Snapshot,
};
pub use header::{CifHeader, CifLoop};
use md5::{Digest, Md5};
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
    /// All source text bytes before the binary marker, including CIF headers.
    pub header: Vec<u8>,
    pub binary_fields: BTreeMap<String, String>,
    /// Parsed scalar/loop CIF metadata. Original text remains in header and
    /// unsupported/malformed syntax is reported in diagnostics.
    pub cif: CifHeader,
    pub pilatus: Option<crate::pilatus::Header>,
}
/// Explicit handling of incompatible byte-offset escape dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOffsetMode {
    /// 8/16/32/64-bit escapes with checked declared-type counts.
    Extended64,
    /// FabIO/Pilatus signed32: 32-bit escapes are terminal and sums wrap at 32 bits.
    /// Admitted only for a declared signed32 image; never selected heuristically.
    Legacy32,
}
pub struct Reader {
    mode: ByteOffsetMode,
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
    offset: u64,
    bytes: usize,
    md5: Option<[u8; 16]>,
}
impl Reader {
    /// Open one immutable x-CBF_BYTE_OFFSET image. Byte-offset escapes are little
    /// endian; other conversions and multiple binary sections fail explicitly.
    /// # Errors
    /// Missing/duplicate/unsupported binary fields, bounds, changed/truncated data.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        Self::open_with_mode(path, limits, ByteOffsetMode::Extended64)
    }
    /// Open with an explicit byte-offset dialect. The two formats can be
    /// ambiguous at INT32_MIN; failed reads never try another interpretation.
    /// # Errors
    /// Same validation as `open`; Legacy32 requires signed32 source metadata.
    pub fn open_with_mode(
        path: impl AsRef<Path>,
        limits: Limits,
        mode: ByteOffsetMode,
    ) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut input = BufReader::new(file.try_clone()?);
        let mut header = Vec::new();
        loop {
            if header.len() >= limits.header_bytes {
                return Err(Error(
                    "CBF binary marker absent or header exceeds limit".into(),
                ));
            }
            let mut b = [0];
            input.read_exact(&mut b)?;
            header.push(b[0]);
            if header.ends_with(&[12, 26, 4, 213]) {
                header.truncate(header.len() - 4);
                break;
            }
        }
        let offset = header.len() as u64 + 4;
        let boundary = b"--CIF-BINARY-FORMAT-SECTION--";
        let start = header
            .windows(boundary.len())
            .position(|w| w == boundary)
            .ok_or_else(|| Error("missing CBF MIME section".into()))?;
        let text = std::str::from_utf8(&header[start + boundary.len()..])
            .map_err(|_| Error("CBF MIME fields must be UTF-8/ASCII".into()))?;
        let mut fields: BTreeMap<String, String> = BTreeMap::new();
        let mut previous: Option<String> = None;
        for line in text.lines() {
            if line.starts_with([' ', '\t']) {
                let key = previous
                    .as_ref()
                    .ok_or_else(|| Error("CBF MIME continuation without a field".into()))?;
                let value = fields.get_mut(key).unwrap();
                value.push(' ');
                value.push_str(line.trim());
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_ascii_lowercase();
                let value = value.trim().to_owned();
                previous = Some(key.clone());
                if fields.insert(key, value).is_some() {
                    return Err(Error("duplicate CBF MIME field".into()));
                }
            } else if !line.trim().is_empty() {
                return Err(Error("invalid CBF MIME field".into()));
            }
        }
        let content = fields
            .get("content-type")
            .ok_or_else(|| Error("missing CBF Content-Type".into()))?;
        let lower = content.to_ascii_lowercase();
        let conversion = lower
            .split(';')
            .filter_map(|p| p.trim().split_once('='))
            .find(|(k, _)| k.trim() == "conversions")
            .map(|(_, v)| v.trim().trim_matches('"'));
        if conversion != Some("x-cbf_byte_offset") {
            return Err(Error(
                "unsupported CBF conversion; only x-CBF_BYTE_OFFSET is implemented".into(),
            ));
        }
        let field = |key: &str| {
            fields
                .get(key)
                .map(|value| value.trim_matches('"'))
                .ok_or_else(|| Error(format!("missing CBF field {key}")))
        };
        let number = |key: &str| {
            field(key)?
                .parse::<usize>()
                .map_err(|_| Error(format!("invalid CBF field {key}")))
        };
        if let Some(v) = fields.get("content-transfer-encoding")
            && !v.eq_ignore_ascii_case("binary")
        {
            return Err(Error("unsupported CBF transfer encoding".into()));
        }
        if !field("x-binary-element-byte-order")?.eq_ignore_ascii_case("LITTLE_ENDIAN") {
            return Err(Error("unsupported CBF byte-offset byte order".into()));
        }
        let shape = [
            number("x-binary-size-second-dimension")?,
            number("x-binary-size-fastest-dimension")?,
        ];
        if fields
            .get("x-binary-size-third-dimension")
            .is_some_and(|v| v != "1")
        {
            return Err(Error(
                "CBF volume/stack binary sections are unsupported".into(),
            ));
        }
        let pixel_type = match field("x-binary-element-type")?
            .to_ascii_lowercase()
            .as_str()
        {
            "signed 8-bit integer" => PixelType::I8,
            "signed 16-bit integer" => PixelType::I16,
            "signed 32-bit integer" => PixelType::I32,
            "signed 64-bit integer" => PixelType::I64,
            "unsigned 8-bit integer" => PixelType::U8,
            "unsigned 16-bit integer" => PixelType::U16,
            "unsigned 32-bit integer" => PixelType::U32,
            "unsigned 64-bit integer" => PixelType::U64,
            _ => return Err(Error("unsupported CBF element type".into())),
        };
        if mode == ByteOffsetMode::Legacy32 && pixel_type != PixelType::I32 {
            return Err(Error(
                "Legacy32 byte-offset mode requires signed32 source metadata".into(),
            ));
        }
        let n = limits.pixels(shape, pixel_type)?;
        if number("x-binary-number-of-elements")? != n {
            return Err(Error("CBF dimensions/element count disagree".into()));
        }
        let bytes = number("x-binary-size")?;
        let end = offset
            .checked_add(bytes as u64)
            .filter(|&end| end <= snapshot.bytes())
            .ok_or_else(|| Error("truncated CBF binary payload".into()))?;
        if bytes == 0
            || bytes > limits.encoded_bytes
            || snapshot.bytes() - end > limits.header_bytes as u64
        {
            return Err(Error("CBF encoded payload/trailer exceeds limits".into()));
        }
        if header
            .len()
            .saturating_mul(2)
            .saturating_add(fields.len().saturating_mul(128))
            > limits.metadata_bytes
        {
            return Err(Error("CBF metadata exceeds limit".into()));
        }
        let used = header
            .len()
            .saturating_mul(2)
            .saturating_add(fields.len().saturating_mul(128));
        let cif = header::parse(&header[..start], limits.metadata_bytes.saturating_sub(used))?;
        let pilatus = if cif
            .fields
            .get("_array_data.header_convention")
            .is_some_and(|v| v == "PILATUS_1.2")
        {
            Some(
                crate::pilatus::Header::parse(
                    cif.fields
                        .get("_array_data.header_contents")
                        .map_or("", String::as_str),
                    limits
                        .metadata_bytes
                        .saturating_sub(used)
                        .saturating_sub(cif.retained_bytes()),
                )
                .map_err(Error)?,
            )
        } else {
            None
        };
        let mut trailer = Vec::new();
        input.seek(SeekFrom::Start(end))?;
        input
            .take(limits.header_bytes as u64 + 1)
            .read_to_end(&mut trailer)?;
        if trailer.len() > limits.header_bytes {
            return Err(Error("CBF trailer exceeds limit".into()));
        }
        if trailer.windows(4).any(|w| w == [12, 26, 4, 213]) {
            return Err(Error("multiple CBF binary sections are unsupported".into()));
        }
        let md5 = fields
            .get("content-md5")
            .map(|s| decode_md5(s))
            .transpose()?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            mode,
            snapshot,
            file,
            metadata: Metadata {
                shape,
                pixel_type,
                header,
                binary_fields: fields,
                cif,
                pilatus,
            },
            offset,
            bytes,
            md5,
        })
    }
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    /// Read exact original integer samples into reusable storage. Content-MD5 is
    /// verified when present. No CIF geometry/header interpretation is applied.
    /// # Errors
    /// Changed source, corrupt checksum, truncated escapes or integer overflow.
    pub fn read_frame_into(&self, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        resize_bytes(&mut buffer.encoded, self.bytes)?;
        // A fresh independent file handle avoids shared seek positions with other reads.
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(self.offset))?;
        file.read_exact(&mut buffer.encoded)?;
        if self
            .md5
            .is_some_and(|expected| Md5::digest(&buffer.encoded).as_slice() != expected)
        {
            return Err(Error("CBF Content-MD5 mismatch".into()));
        }
        let meta = &self.metadata;
        buffer
            .pixels
            .resize(meta.pixel_type, meta.shape[0] * meta.shape[1])
            .map_err(Error)?;
        byte_offset(
            &buffer.encoded,
            &mut buffer.pixels,
            self.mode == ByteOffsetMode::Extended64,
        )?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        buffer.shape = Some(meta.shape);
        Ok(())
    }
}
fn decode_md5(s: &str) -> Result<[u8; 16], Error> {
    // Standard base64 for the fixed 16-byte MIME checksum; no permissive padding.
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    if s.len() != 24 || !s.ends_with("==") {
        return Err(Error("invalid CBF Content-MD5 base64".into()));
    }
    let mut bytes = Vec::new();
    let mut bits = 0_u32;
    let mut n = 0;
    for b in s.bytes().take(22) {
        let value = ALPHABET
            .iter()
            .position(|&c| c == b)
            .ok_or_else(|| Error("invalid CBF MD5 base64".into()))?;
        bits = (bits << 6) | value as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            bytes.push((bits >> n) as u8);
        }
    }
    if bits & 15 != 0 {
        return Err(Error("invalid CBF MD5 base64 padding bits".into()));
    }
    bytes
        .try_into()
        .map_err(|_| Error("invalid CBF Content-MD5 length".into()))
}

pub mod document;

pub mod syntax;
