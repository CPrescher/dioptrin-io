//! XSDataImage XML with explicit base64/base32/base16 coding and little-endian
//! numerical samples. XML and base encodings use established parsers; DTDs,
//! ambiguous descriptors, invented dtype defaults and checksum repairs fail.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelType,
    binary::{copy_samples, resize_bytes},
    source::Snapshot,
};
use md5::{Digest, Md5};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coding {
    Base64,
    Base32,
    Base16,
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    pub dtype: String,
    pub coding: Coding,
    pub md5: Option<String>,
    /// Original XML before/after the data element, with every metadata byte
    /// retained. The complete payload is read again from its immutable source.
    pub header_parts: [Vec<u8>; 2],
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
    source_bytes: usize,
    limits: Limits,
}
impl Reader {
    /// Open one 2D XSData array. Non-2D/multiple arrays need a separate layout
    /// contract, and are rejected. Decoded samples are never cached.
    /// # Errors
    /// Invalid XML, duplicate/missing descriptors, unsupported dtype/coding or bounds.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let source_bytes = usize::try_from(snapshot.bytes())
            .map_err(|_| Error("XSD file size overflow".into()))?;
        if source_bytes > limits.encoded_bytes {
            return Err(Error("XSD source exceeds encoded limit".into()));
        }
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut source = Vec::new();
        resize_bytes(&mut source, source_bytes)?;
        file.read_exact(&mut source)?;
        let (metadata, _) = parse(&source, limits)?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            metadata,
            source_bytes,
            limits,
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
    /// Read exact samples and verify an optional MD5 of the decoded source bytes.
    /// # Errors
    /// Changed source, malformed encoded data, count mismatch or checksum failure.
    pub fn read_frame_into(&self, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(0))?;
        resize_bytes(&mut buffer.encoded, self.source_bytes)?;
        file.read_exact(&mut buffer.encoded)?;
        let (_, data) = parse(&buffer.encoded, self.limits)?;
        let encoding = match self.metadata.coding {
            Coding::Base64 => data_encoding::BASE64,
            Coding::Base32 => data_encoding::BASE32,
            Coding::Base16 => data_encoding::HEXUPPER,
        };
        let count = self.metadata.shape[0] * self.metadata.shape[1];
        let bytes = count * self.metadata.pixel_type.bytes();
        let mut encoded = Vec::new();
        encoded
            .try_reserve_exact(data.len())
            .map_err(|_| Error("XSD encoded scratch allocation failed".into()))?;
        encoded.extend(data.bytes().filter(|b| !b.is_ascii_whitespace()));
        // Decode to an exactly admitted sample buffer, not an input-sized result.
        let size = encoding
            .decode_len(encoded.len())
            .map_err(|e| Error(format!("invalid XSD coding length: {e}")))?;
        if size > bytes.saturating_add(4) || size < bytes {
            return Err(Error("XSD encoded size contradicts dimensions".into()));
        }
        resize_bytes(&mut buffer.encoded, size)?;
        let decoded = encoding
            .decode_mut(&encoded, &mut buffer.encoded)
            .map_err(|e| Error(format!("invalid XSD encoded samples: {e:?}")))?;
        if decoded != bytes {
            return Err(Error("XSD decoded size contradicts dimensions".into()));
        }
        buffer.encoded.truncate(decoded);
        if let Some(checksum) = &self.metadata.md5
            && format!("{:x}", Md5::digest(&buffer.encoded)) != *checksum
        {
            return Err(Error("XSD MD5 checksum mismatch".into()));
        }
        buffer
            .pixels
            .resize(self.metadata.pixel_type, count)
            .map_err(Error)?;
        copy_samples(&buffer.encoded, &mut buffer.pixels, true)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        buffer.shape = Some(self.metadata.shape);
        Ok(())
    }
}
fn parse(source: &[u8], limits: Limits) -> Result<(Metadata, String), Error> {
    let text = std::str::from_utf8(source).map_err(|_| Error("XSD XML is not UTF8".into()))?;
    let doc = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: (limits.metadata_bytes / 32).min(u32::MAX as usize) as u32,
            ..Default::default()
        },
    )
    .map_err(|e| Error(format!("invalid XSD XML: {e}")))?;
    let one = |name: &str| -> Result<roxmltree::Node<'_, '_>, Error> {
        let mut nodes = doc.descendants().filter(|n| n.has_tag_name(name));
        let first = nodes
            .next()
            .ok_or_else(|| Error(format!("missing XSD {name}")))?;
        if nodes.next().is_some() {
            return Err(Error(format!("duplicate XSD {name}")));
        }
        Ok(first)
    };
    let value = |node: roxmltree::Node<'_, '_>| -> Result<String, Error> {
        if node.children().any(|n| n.is_element())
            || node.children().filter(|n| n.is_text()).count() != 1
        {
            return Err(Error("nested XSD scalar value".into()));
        }
        Ok(node
            .text()
            .ok_or_else(|| Error("empty XSD descriptor".into()))?
            .trim()
            .into())
    };
    let shapes = doc
        .descendants()
        .filter(|n| n.has_tag_name("shape"))
        .map(|n| {
            value(n)?
                .parse::<usize>()
                .map_err(|_| Error("invalid XSD dimension".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let [width, height] = shapes.as_slice() else {
        return Err(Error("XSD requires exactly two dimensions".into()));
    };
    let shape = [*height, *width];
    let dtype = value(one("dtype")?)?;
    let pixel_type = match dtype.as_str() {
        "uint8" | "u1" | "|u1" => PixelType::U8,
        "int8" | "i1" | "|i1" => PixelType::I8,
        "uint16" | "u2" | "<u2" => PixelType::U16,
        "int16" | "i2" | "<i2" => PixelType::I16,
        "uint32" | "u4" | "<u4" => PixelType::U32,
        "int32" | "i4" | "<i4" => PixelType::I32,
        "uint64" | "u8" | "<u8" => PixelType::U64,
        "int64" | "i8" | "<i8" => PixelType::I64,
        "float32" | "f4" | "<f4" => PixelType::F32,
        "float64" | "f8" | "<f8" => PixelType::F64,
        "complex64" | "c8" | "<c8" => PixelType::C32,
        "complex128" | "c16" | "<c16" => PixelType::C64,
        _ => return Err(Error(format!("unsupported XSD dtype {dtype}"))),
    };
    let count = limits.pixels(shape, pixel_type)?;
    let size = value(one("size")?)?
        .parse::<usize>()
        .map_err(|_| Error("invalid XSD size".into()))?;
    if size != count {
        return Err(Error("XSD size contradicts dimensions".into()));
    }
    let coding = child_value(one("coding")?)?;
    let coding = match value(coding)?.as_str() {
        "base64" => Coding::Base64,
        "base32" => Coding::Base32,
        "base16" => Coding::Base16,
        _ => return Err(Error("unsupported XSD coding".into())),
    };
    let checksums: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("md5sum"))
        .collect();
    if checksums.len() > 1 {
        return Err(Error("duplicate XSD MD5".into()));
    }
    let md5 = checksums
        .first()
        .map(|n| {
            let n = child_value(*n)?;
            let s = value(n)?;
            if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error("invalid XSD MD5".into()));
            }
            Ok(s.to_ascii_lowercase())
        })
        .transpose()?;
    let data = one("data")?;
    if data.children().any(|n| n.is_element())
        || data.children().filter(|n| n.is_text()).count() != 1
    {
        return Err(Error("nested XSD data elements unsupported".into()));
    }
    let range = data.range();
    let metadata_bytes = source.len() - (range.end - range.start);
    if metadata_bytes > limits.header_bytes
        || metadata_bytes.saturating_mul(3) > limits.metadata_bytes
    {
        return Err(Error("XSD metadata exceeds limit".into()));
    }
    let payload = data
        .text()
        .ok_or_else(|| Error("missing XSD encoded payload".into()))?
        .to_owned();
    Ok((
        Metadata {
            shape,
            pixel_type,
            dtype,
            coding,
            md5,
            header_parts: [source[..range.start].to_vec(), source[range.end..].to_vec()],
        },
        payload,
    ))
}

fn child_value<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
) -> Result<roxmltree::Node<'a, 'input>, Error> {
    let mut nodes = node.children().filter(|n| n.has_tag_name("value"));
    let first = nodes
        .next()
        .ok_or_else(|| Error("missing XSD descriptor/value".into()))?;
    if nodes.next().is_some() {
        return Err(Error("duplicate XSD descriptor/value".into()));
    }
    Ok(first)
}
