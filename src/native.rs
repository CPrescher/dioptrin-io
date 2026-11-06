//! Bounded native NPY, d*TREK/ADSC, Fit2D masks, HiPiC, PNM and explicit raw input.
//!
//! Frames use stored logical axes in row-major buffers. Fortran NPY storage is
//! reordered without flipping axes. Unsupported scalar/layouts fail explicitly.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelBuffer, PixelType,
    binary::{copy_samples, resize_bytes},
    source::Snapshot,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Npy,
    Dtrek,
    Fit2dMask,
    HiPic,
    Pnm,
    Fit2dSpreadsheet,
}

#[derive(Clone, Copy, Debug)]
pub enum Offset {
    Absolute(u64),
    /// Last complete image before this many trailing bytes. FabIO offset -1
    /// corresponds to zero trailing bytes, -2 to one trailing byte, etc.
    FromEnd {
        trailing_bytes: u64,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct RawLayout {
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    pub little_endian: bool,
    pub offset: Offset,
}

#[derive(Clone, Debug)]
pub struct Metadata {
    pub format: Option<Format>,
    pub shape: [usize; 2],
    pub frames: usize,
    pub pixel_type: PixelType,
    pub little_endian: bool,
    /// Original NPY axes (including leading stack axes), otherwise image shape.
    pub source_shape: Vec<usize>,
    pub fortran_order: bool,
    /// Original header including padding/comments. Empty for explicit raw input.
    pub header: Vec<u8>,
    /// Parsed fields. HiPiC comment keys may repeat; the last value is exposed
    /// here and every original assignment is preserved in `header`.
    pub fields: BTreeMap<String, String>,
}

/// Frozen d*TREK metadata independent of pixel availability. Empty declared
/// dimensions are admitted here; the complete-image Reader remains strict.
pub struct DtrekHeader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
}
impl DtrekHeader {
    /// Read only a bounded header using the same parser as the image reader.
    /// # Errors
    /// Invalid header/declared layout, resource bounds or source mutation.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if !std::fs::metadata(&snapshot.path)?.is_file() {
            return Err(Error("native path snapshots require regular files".into()));
        }
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let (mut metadata, _) = dtrek(&mut file, limits, true)?;
        metadata.format = Some(Format::Dtrek);
        let cost = metadata
            .header
            .len()
            .saturating_mul(2)
            .saturating_add(metadata.fields.len().saturating_mul(128))
            .saturating_add(metadata.source_shape.len().saturating_mul(16));
        if cost > limits.metadata_bytes || metadata.frames > limits.frames {
            return Err(Error("d*TREK header metadata/frame limit".into()));
        }
        // Nonempty geometry is checked without allocating or reading samples.
        if !metadata.shape.contains(&0) {
            limits.pixels(metadata.shape, metadata.pixel_type)?;
        }
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            metadata,
        })
    }
    /// # Errors
    /// Original source changed since header indexing.
    pub fn metadata(&self) -> Result<&Metadata, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.metadata)
    }
}

enum Storage {
    Scalar,
    Mask,
    Ascii { max: u32 },
    Spreadsheet,
}

pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
    offset: u64,
    encoded: usize,
    storage: Storage,
}

impl Reader {
    /// Open an explicitly selected format as an immutable snapshot.
    /// # Errors
    /// Malformed, truncated, unsupported or oversized sources.
    pub fn open(path: impl AsRef<Path>, format: Format, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if !std::fs::metadata(&snapshot.path)?.is_file() {
            return Err(Error("native path snapshots require regular files".into()));
        }
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let (mut metadata, storage) = match format {
            Format::Npy => npy(&mut file, limits)?,
            Format::Dtrek => dtrek(&mut file, limits, false)?,
            Format::Fit2dMask => mask(&mut file, limits)?,
            Format::HiPic => hipic(&mut file, limits)?,
            Format::Pnm => pnm(&mut file, limits)?,
            Format::Fit2dSpreadsheet => spreadsheet(&mut file, limits)?,
        };
        metadata.format = Some(format);
        let offset = file.stream_position()?;
        let count = if format == Format::Npy && metadata.shape.contains(&0) {
            0
        } else {
            limits.pixels(metadata.shape, metadata.pixel_type)?
        };
        if (metadata.frames == 0 && format != Format::Npy) || metadata.frames > limits.frames {
            return Err(Error("native image exceeds frame limit".into()));
        }
        let frame_bytes = count * metadata.pixel_type.bytes();
        let encoded = match storage {
            Storage::Scalar => frame_bytes,
            Storage::Mask => metadata.shape[1]
                .div_ceil(32)
                .checked_mul(4)
                .and_then(|n| n.checked_mul(metadata.shape[0]))
                .ok_or_else(|| Error("mask size overflow".into()))?,
            Storage::Ascii { .. } | Storage::Spreadsheet => usize::try_from(
                snapshot
                    .bytes()
                    .checked_sub(offset)
                    .ok_or_else(|| Error("truncated ASCII image".into()))?,
            )
            .map_err(|_| Error("ASCII image size overflow".into()))?,
        };
        if encoded > limits.encoded_bytes {
            return Err(Error("native image exceeds encoded frame limit".into()));
        }
        let bytes = encoded
            .checked_mul(metadata.frames)
            .and_then(|n| offset.checked_add(n as u64))
            .ok_or_else(|| Error("native image size overflow".into()))?;
        if bytes != snapshot.bytes() {
            return Err(Error(
                "native image payload length does not match shape (truncated or trailing data)"
                    .into(),
            ));
        }
        // Raw and parsed metadata have separate copies and map allocation overhead.
        let metadata_bytes = metadata
            .header
            .len()
            .saturating_mul(2)
            .saturating_add(metadata.fields.len().saturating_mul(128))
            .saturating_add(metadata.source_shape.len().saturating_mul(16));
        if metadata_bytes > limits.metadata_bytes {
            return Err(Error("native image metadata exceeds limit".into()));
        }
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            metadata,
            offset,
            encoded,
            storage,
        })
    }

    /// Read an explicitly described binary image; never guesses a dtype or shape.
    /// Headers/trailing bytes are caller-owned and are not interpreted.
    /// # Errors
    /// Invalid bounds, offsets or incomplete image data.
    pub fn open_raw(
        path: impl AsRef<Path>,
        layout: RawLayout,
        limits: Limits,
    ) -> Result<Self, Error> {
        limits.validate()?;
        let count = limits.pixels(layout.shape, layout.pixel_type)?;
        let encoded = count * layout.pixel_type.bytes();
        if encoded > limits.encoded_bytes {
            return Err(Error("raw image exceeds encoded limit".into()));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let file = File::open(&snapshot.path)?;
        let offset = match layout.offset {
            Offset::Absolute(n) => n,
            Offset::FromEnd { trailing_bytes } => snapshot
                .bytes()
                .checked_sub(trailing_bytes)
                .and_then(|n| n.checked_sub(encoded as u64))
                .ok_or_else(|| Error("raw offset precedes beginning of source".into()))?,
        };
        if offset
            .checked_add(encoded as u64)
            .is_none_or(|n| n > snapshot.bytes())
        {
            return Err(Error("truncated explicit raw image".into()));
        }
        snapshot.check_handle(&file).map_err(Error)?;
        let metadata = Metadata {
            format: None,
            shape: layout.shape,
            frames: 1,
            pixel_type: layout.pixel_type,
            little_endian: layout.little_endian,
            source_shape: layout.shape.to_vec(),
            fortran_order: false,
            header: Vec::new(),
            fields: BTreeMap::new(),
        };
        Ok(Self {
            snapshot,
            file,
            metadata,
            offset,
            encoded,
            storage: Storage::Scalar,
        })
    }

    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.metadata.frames
    }

    /// Decode one exact frame. Failed reads invalidate public pixels/shape.
    /// # Errors
    /// Out-of-range frame, changed source, malformed/truncated payload.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        if index >= self.frames() {
            return Err(Error("native frame index outside source".into()));
        }
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        let meta = &self.metadata;
        let count = meta.shape[0] * meta.shape[1];
        buffer
            .pixels
            .resize(meta.pixel_type, count)
            .map_err(Error)?;
        resize_bytes(&mut buffer.encoded, self.encoded)?;
        if count == 0 {
            // Empty arrays retain their declared axes without iterating enormous
            // zero-width rows or inventing any samples/frames.
            self.snapshot.check_handle(&file).map_err(Error)?;
            buffer.shape = Some(meta.shape);
            return Ok(());
        }
        if meta.fortran_order {
            // At most one frame's payload is retained. Read samples from the
            // source; leading frame axes are interleaved in Fortran storage.
            let size = meta.pixel_type.bytes();
            let mut q = index;
            let mut frame_offset = 0;
            // Frame numbering follows C-order reshape of the leading axes.
            let mut stride = meta.frames;
            for &dim in meta.source_shape[..meta.source_shape.len().saturating_sub(2)]
                .iter()
                .rev()
            {
                stride /= dim;
                frame_offset += (q % dim) * stride;
                q /= dim;
            }
            for y in 0..meta.shape[0] {
                for x in 0..meta.shape[1] {
                    let sample = frame_offset + (y + x * meta.shape[0]) * meta.frames;
                    file.seek(SeekFrom::Start(self.offset + (sample * size) as u64))?;
                    let pos = (y * meta.shape[1] + x) * size;
                    file.read_exact(&mut buffer.encoded[pos..pos + size])?;
                }
            }
        } else {
            file.seek(SeekFrom::Start(self.offset + (index * self.encoded) as u64))?;
            file.read_exact(&mut buffer.encoded)?;
        }
        match self.storage {
            Storage::Scalar => {
                copy_samples(&buffer.encoded, &mut buffer.pixels, meta.little_endian)?
            }
            Storage::Mask => {
                let PixelBuffer::U8(out) = &mut buffer.pixels else {
                    unreachable!()
                };
                let row = meta.shape[1].div_ceil(32) * 4;
                for (y, values) in out.chunks_exact_mut(meta.shape[1]).enumerate() {
                    for (x, sample) in values.iter_mut().enumerate() {
                        *sample = (buffer.encoded[y * row + x / 8] >> (x % 8)) & 1;
                    }
                }
            }
            Storage::Ascii { max } => {
                let mut tokens = Tokens::new(&buffer.encoded);
                macro_rules! fill {
                    ($v:ident, $ty:ty) => {
                        if let PixelBuffer::$v(out) = &mut buffer.pixels {
                            for sample in out {
                                let n = number(tokens.next()?)?;
                                if n > max as usize {
                                    return Err(Error("PNM value exceeds MAXVAL".into()));
                                }
                                *sample = <$ty>::try_from(n)
                                    .map_err(|_| Error("PNM sample overflow".into()))?;
                            }
                        }
                    };
                }
                fill!(U8, u8);
                fill!(U16, u16);
                fill!(U32, u32);
                if tokens.optional()?.is_some() {
                    return Err(Error("extra PNM samples".into()));
                }
            }
            Storage::Spreadsheet => {
                let PixelBuffer::F32(out) = &mut buffer.pixels else {
                    unreachable!()
                };
                let text = std::str::from_utf8(&buffer.encoded)
                    .map_err(|_| Error("non-UTF8 Fit2D spreadsheet".into()))?;
                let mut y = 0;
                for line in text
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && !s.starts_with('#'))
                {
                    if y >= meta.shape[0] {
                        return Err(Error("extra Fit2D spreadsheet rows".into()));
                    }
                    let mut x = 0;
                    for token in line.split_ascii_whitespace() {
                        if x >= meta.shape[1] {
                            return Err(Error("extra Fit2D spreadsheet columns".into()));
                        }
                        // The reference interprets decimal text as f64 before
                        // storage in the format's f32 output representation.
                        out[y * meta.shape[1] + x] = token
                            .parse::<f64>()
                            .map_err(|_| Error("invalid Fit2D spreadsheet number".into()))?
                            as f32;
                        x += 1;
                    }
                    if x != meta.shape[1] {
                        return Err(Error("missing Fit2D spreadsheet columns".into()));
                    }
                    y += 1;
                }
                if y != meta.shape[0] {
                    return Err(Error("missing Fit2D spreadsheet rows".into()));
                }
            }
        }
        self.snapshot.check_handle(&file).map_err(Error)?;
        buffer.shape = Some(meta.shape);
        Ok(())
    }
}

fn meta(
    shape: [usize; 2],
    ty: PixelType,
    little: bool,
    header: Vec<u8>,
    fields: BTreeMap<String, String>,
) -> Metadata {
    Metadata {
        format: None,
        shape,
        frames: 1,
        pixel_type: ty,
        little_endian: little,
        source_shape: shape.to_vec(),
        fortran_order: false,
        header,
        fields,
    }
}
fn read_header(file: &mut File, size: usize, limits: Limits) -> Result<Vec<u8>, Error> {
    if size > limits.header_bytes || size.saturating_mul(2) > limits.metadata_bytes {
        return Err(Error("native header exceeds limit".into()));
    }
    let mut v = Vec::new();
    resize_bytes(&mut v, size)?;
    file.read_exact(&mut v)?;
    Ok(v)
}
fn number(bytes: &[u8]) -> Result<usize, Error> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Error("invalid unsigned image dimension/value".into()))
}
fn spreadsheet(file: &mut File, limits: Limits) -> Result<(Metadata, Storage), Error> {
    let mut input = BufReader::new(file);
    let mut header = Vec::new();
    loop {
        let start = header.len();
        loop {
            if header.len() >= limits.header_bytes {
                return Err(Error("Fit2D spreadsheet header exceeds limit".into()));
            }
            let mut byte = [0];
            input.read_exact(&mut byte)?;
            header.push(byte[0]);
            if byte[0] == b'\n' {
                break;
            }
        }
        let line = std::str::from_utf8(&header[start..])
            .map_err(|_| Error("invalid Fit2D spreadsheet header".into()))?
            .trim();
        if line.starts_with('#') {
            continue;
        }
        let mut tokens = line.split_ascii_whitespace();
        let width = number(
            tokens
                .next()
                .ok_or_else(|| Error("missing Fit2D spreadsheet width".into()))?
                .as_bytes(),
        )?;
        let height = number(
            tokens
                .next()
                .ok_or_else(|| Error("missing Fit2D spreadsheet height".into()))?
                .as_bytes(),
        )?;
        let fields = [("title".to_owned(), line.to_owned())].into();
        let position = input.stream_position()?;
        input.into_inner().seek(SeekFrom::Start(position))?;
        return Ok((
            meta([height, width], PixelType::F32, true, header, fields),
            Storage::Spreadsheet,
        ));
    }
}
fn field<'a>(fields: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, Error> {
    fields
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| Error(format!("missing native header field {key}")))
}
fn dtrek(file: &mut File, limits: Limits, header_only: bool) -> Result<(Metadata, Storage), Error> {
    // HEADER_BYTES must be the first assignment. Read only a bounded prefix
    // before allocating the declared padded header.
    let mut prefix = Vec::new();
    loop {
        if prefix.len() >= limits.header_bytes.min(256) {
            return Err(Error("missing d*TREK HEADER_BYTES".into()));
        }
        let mut b = [0];
        file.read_exact(&mut b)?;
        prefix.push(b[0]);
        if b[0] == b';' {
            break;
        }
    }
    let text = std::str::from_utf8(&prefix).map_err(|_| Error("non-ASCII d*TREK header".into()))?;
    let first = text
        .trim_start()
        .strip_prefix('{')
        .ok_or_else(|| Error("invalid d*TREK magic".into()))?;
    let (key, val) = first
        .trim()
        .trim_end_matches(';')
        .split_once('=')
        .ok_or_else(|| Error("invalid d*TREK assignment".into()))?;
    if key.trim() != "HEADER_BYTES" {
        return Err(Error("d*TREK HEADER_BYTES must be first".into()));
    }
    let size = number(val.trim().as_bytes())?;
    if size < prefix.len() {
        return Err(Error("d*TREK header length smaller than prefix".into()));
    }
    file.seek(SeekFrom::Start(0))?;
    let header = read_header(file, size, limits)?;
    let text = std::str::from_utf8(&header).map_err(|_| Error("non-ASCII d*TREK header".into()))?;
    let body = text
        .strip_prefix('{')
        .and_then(|s| s.rsplit_once('}').map(|p| p.0))
        .ok_or_else(|| Error("unclosed d*TREK header".into()))?;
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    for entry in body.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (k, v) = entry
            .split_once('=')
            .ok_or_else(|| Error("invalid d*TREK field".into()))?;
        if fields.insert(k.trim().into(), v.trim().into()).is_some() {
            return Err(Error("duplicate d*TREK field".into()));
        }
    }
    let rank = fields.get("DIM").map_or(Ok(2), |v| number(v.as_bytes()))?;
    if !(1..=32).contains(&rank) {
        return Err(Error("d*TREK rank must lie in 1..=32".into()));
    }
    let source_shape = (1..=rank)
        .rev()
        .map(|axis| {
            let size = number(field(&fields, &format!("SIZE{axis}"))?.as_bytes())?;
            if size == 0 && !header_only {
                return Err(Error("empty d*TREK dimension unsupported".into()));
            }
            Ok(size)
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let shape = if rank == 1 {
        [1, source_shape[0]]
    } else {
        [source_shape[rank - 2], source_shape[rank - 1]]
    };
    let frames = source_shape[..rank.saturating_sub(2)]
        .iter()
        .try_fold(1usize, |n, &d| n.checked_mul(d))
        .ok_or_else(|| Error("d*TREK leading dimensions overflow".into()))?;
    let ty = match fields.get("Data_type").map(String::as_str) {
        Some("signed char") => PixelType::I8,
        Some("unsigned char") => PixelType::U8,
        Some("short int") => PixelType::I16,
        Some("unsigned short int") => PixelType::U16,
        Some("long int") => PixelType::I32,
        Some("unsigned long int") => PixelType::U32,
        Some("float IEEE") => PixelType::F32,
        None if fields.get("TYPE").is_some_and(|v| v == "unsigned_short") => PixelType::U16,
        _ => return Err(Error("unsupported/missing d*TREK scalar type".into())),
    };
    let little = match fields.get("BYTE_ORDER").map(String::as_str) {
        None | Some("little_endian") => true,
        Some("big_endian") => false,
        _ => return Err(Error("invalid d*TREK BYTE_ORDER".into())),
    };
    let mut metadata = meta(shape, ty, little, header, fields);
    metadata.frames = frames;
    metadata.source_shape = source_shape;
    Ok((metadata, Storage::Scalar))
}
fn mask(file: &mut File, limits: Limits) -> Result<(Metadata, Storage), Error> {
    let header = read_header(file, 1024, limits)?;
    if [header[0], header[4], header[8], header[12]] != *b"MASK" {
        return Err(Error("invalid Fit2D mask magic".into()));
    }
    let width = i32::from_le_bytes(header[16..20].try_into().unwrap());
    let height = i32::from_le_bytes(header[20..24].try_into().unwrap());
    let shape = [
        usize::try_from(height).map_err(|_| Error("negative mask height".into()))?,
        usize::try_from(width).map_err(|_| Error("negative mask width".into()))?,
    ];
    Ok((
        meta(shape, PixelType::U8, true, header, BTreeMap::new()),
        Storage::Mask,
    ))
}
fn hipic(file: &mut File, limits: Limits) -> Result<(Metadata, Storage), Error> {
    let mut header = read_header(file, 64, limits)?;
    if &header[..2] != b"IM" {
        return Err(Error("invalid HiPiC magic".into()));
    }
    let u16_at = |n| u16::from_le_bytes([header[n], header[n + 1]]) as usize;
    let comment = u16_at(2);
    let shape = [u16_at(6), u16_at(4)];
    let mut fields = BTreeMap::new();
    for (key, pos) in [
        ("Dim_1_offset", 8),
        ("Dim_2_offset", 10),
        ("HeaderType", 12),
    ] {
        fields.insert(key.into(), u16_at(pos).to_string());
    }
    if comment
        .checked_add(64)
        .is_none_or(|n| n > limits.header_bytes || n.saturating_mul(2) > limits.metadata_bytes)
    {
        return Err(Error("HiPiC comments exceed header limit".into()));
    }
    header.extend(read_header(file, comment, limits)?);
    // Raw comment bytes survive even for non-UTF8 encodings. Parsed fields are
    // offered only when UTF8; no lossy text substitution is applied.
    if let Ok(text) = std::str::from_utf8(&header[64..]) {
        for entry in text
            .split('\0')
            .next()
            .unwrap_or("")
            .split([',', '\r', '\n'])
        {
            if let Some((k, v)) = entry.split_once('=') {
                // Real HiPiC comments repeat keys. The map follows the last
                // assignment; every original entry survives in header bytes.
                fields.insert(k.trim().into(), v.trim().trim_matches('"').into());
            }
        }
    }
    Ok((
        meta(shape, PixelType::U16, true, header, fields),
        Storage::Scalar,
    ))
}

struct Tokens<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Tokens<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn optional(&mut self) -> Result<Option<&'a [u8]>, Error> {
        loop {
            while self
                .bytes
                .get(self.pos)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.pos += 1;
            }
            if self.bytes.get(self.pos) == Some(&b'#') {
                while self.bytes.get(self.pos).is_some_and(|b| *b != b'\n') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
        let start = self.pos;
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'#')
        {
            self.pos += 1;
        }
        Ok((start != self.pos).then_some(&self.bytes[start..self.pos]))
    }
    fn next(&mut self) -> Result<&'a [u8], Error> {
        self.optional()?
            .ok_or_else(|| Error("missing PNM token/sample".into()))
    }
}
fn pnm(file: &mut File, limits: Limits) -> Result<(Metadata, Storage), Error> {
    // Tokenize the header one byte at a time, retaining original comments. The
    // single whitespace delimiter after MAXVAL belongs to the header. Binary
    // pixels may themselves be whitespace or '#'; never skip them.
    let mut input = BufReader::new(file);
    let mut header = Vec::new();
    let mut values: Vec<Vec<u8>> = Vec::new();
    let mut token = Vec::new();
    let mut comment = false;
    loop {
        if header.len() >= limits.header_bytes {
            return Err(Error("PNM header exceeds limit".into()));
        }
        let mut b = [0];
        input.read_exact(&mut b)?;
        let b = b[0];
        header.push(b);
        if comment {
            if b == b'\n' {
                comment = false;
            }
            continue;
        }
        if b == b'#' && token.is_empty() {
            comment = true;
            continue;
        }
        if b.is_ascii_whitespace() {
            if !token.is_empty() {
                values.push(std::mem::take(&mut token));
            }
            let needed = if values.first().is_some_and(|v| v == b"P1") {
                3
            } else {
                4
            };
            if values.len() == needed {
                break;
            }
        } else {
            token.push(b);
        }
    }
    let format = &values[0];
    if !matches!(format.as_slice(), b"P1" | b"P2" | b"P5") {
        return Err(Error("unsupported PNM subformat (P1/P2/P5 only)".into()));
    }
    // Undo BufReader's speculative reads so the caller observes exact offset.
    let position = input.stream_position()?;
    let file = input.into_inner();
    file.seek(SeekFrom::Start(position))?;
    let shape = [number(&values[2])?, number(&values[1])?];
    let max = if format == b"P1" {
        1
    } else {
        number(&values[3])?
    };
    let ty = match max {
        1..=255 => PixelType::U8,
        256..=65535 => PixelType::U16,
        65536..=2147483647 => PixelType::U32,
        _ => return Err(Error("invalid PNM MAXVAL".into())),
    };
    if format == b"P5" {
        let bytes = limits.pixels(shape, ty)? * ty.bytes();
        let remaining = file
            .metadata()?
            .len()
            .checked_sub(position)
            .ok_or_else(|| Error("truncated PNM raster".into()))?;
        // FabIO's writer terminates MAXVAL with space + newline. Admit extra
        // line whitespace only when the exact raster size proves its length.
        if remaining > bytes as u64 {
            let extra = usize::try_from(remaining - bytes as u64)
                .map_err(|_| Error("PNM header padding overflow".into()))?;
            if header
                .len()
                .checked_add(extra)
                .is_none_or(|n| n > limits.header_bytes)
            {
                return Err(Error("PNM header padding exceeds limit".into()));
            }
            let padding = read_header(file, extra, limits)?;
            if !padding.iter().all(u8::is_ascii_whitespace) || padding.last() != Some(&b'\n') {
                return Err(Error(
                    "extra PNM raster bytes are not line header padding".into(),
                ));
            }
            header.extend(padding);
        }
    }
    let fields = [
        (
            "SUBFORMAT".into(),
            String::from_utf8(format.clone()).unwrap(),
        ),
        (
            "WIDTH".into(),
            String::from_utf8(values[1].clone()).unwrap(),
        ),
        (
            "HEIGHT".into(),
            String::from_utf8(values[2].clone()).unwrap(),
        ),
        (
            "MAXVAL".into(),
            if format == b"P1" {
                "1".into()
            } else {
                String::from_utf8(values[3].clone()).unwrap()
            },
        ),
    ]
    .into();
    Ok((
        meta(shape, ty, false, header, fields),
        if format == b"P5" {
            Storage::Scalar
        } else {
            Storage::Ascii { max: max as u32 }
        },
    ))
}

// Narrow non-executing NPY literal parser: scalar dtype, bool and integer tuple.
// No pickle, structured/nested dtype or arbitrary Python expression is admitted.
struct Literal<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Literal<'a> {
    fn ws(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn eat(&mut self, b: u8) -> bool {
        self.ws();
        if self.bytes.get(self.pos) == Some(&b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, b: u8) -> Result<(), Error> {
        if self.eat(b) {
            Ok(())
        } else {
            Err(Error("invalid NPY header literal".into()))
        }
    }
    fn string(&mut self) -> Result<String, Error> {
        self.ws();
        let quote = *self
            .bytes
            .get(self.pos)
            .ok_or_else(|| Error("truncated NPY string".into()))?;
        if quote != b'\'' && quote != b'"' {
            return Err(Error("NPY structured/non-string dtype unsupported".into()));
        }
        self.pos += 1;
        let start = self.pos;
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|&b| b != quote && b != b'\\' && b.is_ascii_graphic())
        {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| Error("invalid NPY string".into()))?
            .to_owned();
        self.expect(quote)?;
        Ok(text)
    }
    fn dimension(&mut self) -> Result<usize, Error> {
        self.ws();
        let start = self.pos;
        while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
        number(&self.bytes[start..self.pos])
    }
}
fn npy(file: &mut File, limits: Limits) -> Result<(Metadata, Storage), Error> {
    let prefix = read_header(file, 8, limits)?;
    if &prefix[..6] != b"\x93NUMPY" || prefix[7] != 0 {
        return Err(Error("invalid NPY magic/version".into()));
    }
    let len_bytes = match prefix[6] {
        1 => 2,
        2 | 3 => 4,
        _ => return Err(Error("unsupported NPY version".into())),
    };
    let length = read_header(file, len_bytes, limits)?;
    let len = if len_bytes == 2 {
        u16::from_le_bytes(length[..].try_into().unwrap()) as usize
    } else {
        usize::try_from(u32::from_le_bytes(length[..].try_into().unwrap()))
            .map_err(|_| Error("NPY header length overflow".into()))?
    };
    let total = len
        .checked_add(8 + len_bytes)
        .ok_or_else(|| Error("NPY header length overflow".into()))?;
    if total > limits.header_bytes {
        return Err(Error("NPY header exceeds limit".into()));
    }
    let mut header = prefix;
    header.extend(length);
    header.extend(read_header(file, len, limits)?);
    let mut literal = Literal {
        bytes: &header[8 + len_bytes..],
        pos: 0,
    };
    literal.expect(b'{')?;
    let mut descr = None;
    let mut shape = None;
    let mut fortran = None;
    loop {
        if literal.eat(b'}') {
            break;
        }
        let key = literal.string()?;
        literal.expect(b':')?;
        match key.as_str() {
            "descr" if descr.is_none() => descr = Some(literal.string()?),
            "shape" if shape.is_none() => {
                literal.expect(b'(')?;
                let mut dims = Vec::new();
                loop {
                    if literal.eat(b')') {
                        break;
                    }
                    if dims.len() >= 32 {
                        return Err(Error("NPY exceeds dimension limit (32)".into()));
                    }
                    dims.push(literal.dimension()?);
                    if literal.eat(b')') {
                        break;
                    }
                    literal.expect(b',')?;
                }
                shape = Some(dims);
            }
            "fortran_order" if fortran.is_none() => {
                literal.ws();
                let remaining = &literal.bytes[literal.pos..];
                let (value, n) = if remaining.starts_with(b"True") {
                    (true, 4)
                } else if remaining.starts_with(b"False") {
                    (false, 5)
                } else {
                    return Err(Error("invalid NPY fortran_order".into()));
                };
                literal.pos += n;
                fortran = Some(value);
            }
            _ => return Err(Error("unknown/duplicate NPY header field".into())),
        }
        if literal.eat(b'}') {
            break;
        }
        literal.expect(b',')?;
    }
    literal.ws();
    if literal.pos != literal.bytes.len() {
        return Err(Error("extra NPY header expressions".into()));
    }
    let descr = descr.ok_or_else(|| Error("missing NPY dtype".into()))?;
    let source_shape = shape.ok_or_else(|| Error("missing NPY shape".into()))?;
    let fortran = fortran.ok_or_else(|| Error("missing NPY storage order".into()))?;
    let (order, scalar) = descr
        .split_at_checked(1)
        .ok_or_else(|| Error("empty NPY dtype".into()))?;
    let ty = match scalar {
        "b1" => PixelType::Bool,
        "f2" => PixelType::F16,
        "u1" => PixelType::U8,
        "u2" => PixelType::U16,
        "u4" => PixelType::U32,
        "u8" => PixelType::U64,
        "i1" => PixelType::I8,
        "i2" => PixelType::I16,
        "i4" => PixelType::I32,
        "i8" => PixelType::I64,
        "f4" => PixelType::F32,
        "f8" => PixelType::F64,
        "c8" => PixelType::C32,
        "c16" => PixelType::C64,
        _ => return Err(Error(format!("unsupported NPY dtype {descr}"))),
    };
    let little = match order {
        "<" => true,
        ">" => false,
        "|" if ty.bytes() == 1 => true,
        "=" => cfg!(target_endian = "little"),
        _ => return Err(Error("invalid NPY dtype byte order".into())),
    };
    let count = source_shape
        .iter()
        .try_fold(1usize, |n, &d| n.checked_mul(d))
        .ok_or_else(|| Error("invalid/overflowing NPY shape".into()))?;
    // Bound the complete source arithmetic before computing offsets/strides.
    count
        .checked_mul(ty.bytes())
        .ok_or_else(|| Error("NPY array bytes overflow".into()))?;
    let shape = match source_shape.as_slice() {
        [] => [1, 1],
        [n] => [1, *n],
        dims => [dims[dims.len() - 2], dims[dims.len() - 1]],
    };
    if !shape.contains(&0) {
        limits.pixels(shape, ty)?;
    }
    let mut metadata = meta(shape, ty, little, header, [("descr".into(), descr)].into());
    metadata.frames = source_shape[..source_shape.len().saturating_sub(2)]
        .iter()
        .try_fold(1usize, |n, &d| n.checked_mul(d))
        .ok_or_else(|| Error("NPY frame count overflow".into()))?;
    metadata.fortran_order = fortran;
    metadata.source_shape = source_shape;
    Ok((metadata, Storage::Scalar))
}
