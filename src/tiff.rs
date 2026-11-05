//! Count-preserving TIFF/BigTIFF detector pages using the MIT `tiff` crate.
//!
//! Single-channel BlackIsZero images only. The stored row order and Orientation
//! tag are retained; no display transform occurs. JPEG, palette/color, packed
//! samples and WhiteIsZero are explicitly rejected. Header tags are retained as
//! typed values, including private tags understood by the backend's value parser.
use crate::{PixelBuffer, PixelType, source::Snapshot};
use std::{collections::BTreeMap, fmt, fs::File, io::BufReader, path::Path};
pub use tiff_backend::decoder::ifd::Value as TagValue;
use tiff_backend::{
    ColorType,
    decoder::{Decoder, DecodingResult},
    tags::Tag,
};

#[derive(Debug)]
pub struct Error(String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<tiff_backend::TiffError> for Error {
    fn from(e: tiff_backend::TiffError) -> Self {
        Self(e.to_string())
    }
}

/// Admission limits for pages, decoded pixels, source tags and decoder scratch.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub frames: usize,
    pub frame_bytes: usize,
    /// Combined source tag payload and estimated typed-value storage for all pages.
    pub metadata_bytes: usize,
    pub intermediate_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            frames: 100_000,
            frame_bytes: 256 * 1024 * 1024,
            metadata_bytes: 16 * 1024 * 1024,
            intermediate_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Original page metadata. Height and width refer to stored rows/columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    pub little_endian: bool,
    /// TIFF Orientation, retained without applying it (default 1).
    pub orientation: u16,
    pub compression: u16,
    /// Typed IFD tags, keyed by original numeric tag ID.
    pub tags: BTreeMap<u16, TagValue>,
}

/// Worker-owned reusable native pixels, inaccessible after a failed read.
#[derive(Debug, Default)]
pub struct FrameBuffer {
    pixels: PixelBuffer,
    shape: Option<[usize; 2]>,
}
impl FrameBuffer {
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        self.shape.map(|_| &self.pixels)
    }
    #[must_use]
    pub fn shape(&self) -> Option<[usize; 2]> {
        self.shape
    }
}

/// Immutable TIFF page index and one decoding handle. Use one per worker.
/// Sources must remain unchanged; ordinary metadata changes fail closed.
pub struct Reader {
    decoder: Decoder<BufReader<File>>,
    identity_handle: File,
    snapshot: Snapshot,
    pages: Vec<Metadata>,
}
impl Reader {
    /// Validate/index all primary IFD pages without reading detector samples.
    /// # Errors
    /// Malformed/cyclic/truncated IFDs, resource bounds or unsupported encodings.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        if [
            limits.frames,
            limits.frame_bytes,
            limits.metadata_bytes,
            limits.intermediate_bytes,
        ]
        .contains(&0)
        {
            return Err(Error("TIFF limits must be positive".into()));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let file = File::open(&snapshot.path).map_err(|e| Error(e.to_string()))?;
        let identity_handle = file.try_clone().map_err(|e| Error(e.to_string()))?;
        snapshot.check_handle(&identity_handle).map_err(Error)?;
        let mut native = tiff_backend::decoder::Limits::default();
        native.decoding_buffer_size = limits.frame_bytes;
        native.ifd_value_size = limits.metadata_bytes;
        native.intermediate_buffer_size = limits.intermediate_bytes;
        // Decoder::new initially uses the upstream defaults while parsing the
        // first IFD (1 MiB per tag); our tighter limits apply immediately after.
        let mut decoder = Decoder::new(BufReader::new(file))?.with_limits(native);
        let mut pages = Vec::new();
        let mut metadata_bytes = 0_usize;
        loop {
            if pages.len() >= limits.frames {
                return Err(Error("TIFF exceeds frame limit".into()));
            }
            pages.push(metadata(&mut decoder, limits, &mut metadata_bytes)?);
            if !decoder.more_images() {
                break;
            }
            decoder.next_image()?;
        }
        snapshot.check_handle(&identity_handle).map_err(Error)?;
        Ok(Self {
            decoder,
            identity_handle,
            snapshot,
            pages,
        })
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.pages.len()
    }
    #[must_use]
    pub fn metadata(&self, index: usize) -> Option<&Metadata> {
        self.pages.get(index)
    }
    /// Decode one stored page, retaining the allocation across same-type reads.
    /// # Errors
    /// Changed source, invalid index or malformed/truncated compressed samples.
    pub fn read_frame_into(&mut self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot
            .check_handle(&self.identity_handle)
            .map_err(Error)?;
        let page = self
            .pages
            .get(index)
            .ok_or_else(|| Error("TIFF page index outside file".into()))?;
        self.decoder.seek_to_image(index)?;
        let mut native = into_native(std::mem::take(&mut buffer.pixels));
        // Move the allocation back even on failure; no partial frame is exposed.
        let decoded = self.decoder.read_image_to_buffer(&mut native);
        buffer.pixels = from_native(native)?;
        decoded?;
        if buffer.pixels.len() != page.shape[0] * page.shape[1]
            || buffer.pixels.pixel_type() != page.pixel_type
        {
            return Err(Error("TIFF decoded sample count/type mismatch".into()));
        }
        self.snapshot
            .check_handle(&self.identity_handle)
            .map_err(Error)?;
        buffer.shape = Some(page.shape);
        Ok(())
    }
}

fn metadata(
    decoder: &mut Decoder<BufReader<File>>,
    limits: Limits,
    total: &mut usize,
) -> Result<Metadata, Error> {
    let (w, h) = decoder.dimensions()?;
    let ColorType::Gray(bits) = decoder.colortype()? else {
        return Err(Error(
            "TIFF detector pages must be single-channel grayscale".into(),
        ));
    };
    let format = decoder
        .find_tag_unsigned::<u16>(Tag::SampleFormat)?
        .unwrap_or(1);
    let samples = decoder
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)?
        .unwrap_or(1);
    let photo = decoder
        .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)?
        .unwrap_or(1);
    if samples != 1 || photo != 1 {
        return Err(Error("TIFF detector pages require one BlackIsZero sample; display inversions are unsupported".into()));
    }
    let compression = decoder
        .find_tag_unsigned::<u16>(Tag::Compression)?
        .unwrap_or(1);
    if ![1, 5, 8, 32946, 32773].contains(&compression) {
        return Err(Error(format!(
            "unsupported TIFF detector compression {compression}; lossy codecs are not admitted"
        )));
    }
    let pixel_type = match (format, bits) {
        (1, 8) => PixelType::U8,
        (1, 16) => PixelType::U16,
        (1, 32) => PixelType::U32,
        (1, 64) => PixelType::U64,
        (2, 8) => PixelType::I8,
        (2, 16) => PixelType::I16,
        (2, 32) => PixelType::I32,
        (2, 64) => PixelType::I64,
        (3, 32) => PixelType::F32,
        (3, 64) => PixelType::F64,
        _ => {
            return Err(Error(format!(
                "unsupported TIFF detector sample format {format}, bits {bits}"
            )));
        }
    };
    if w == 0
        || h == 0
        || (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(pixel_type.bytes()))
            .is_none_or(|n| n > limits.frame_bytes)
    {
        return Err(Error("TIFF page exceeds decoded-byte limit".into()));
    }
    let ifd = decoder.image_ifd();
    for (_, entry) in ifd.directory().iter() {
        // Value enum/list storage is larger than source TIFF scalars. Bound both.
        let estimate = entry
            .count()
            .checked_mul(40)
            .and_then(|n| usize::try_from(n).ok());
        *total = estimate
            .and_then(|n| total.checked_add(n))
            .ok_or_else(|| Error("TIFF metadata size overflow".into()))?;
        if *total > limits.metadata_bytes {
            return Err(Error("TIFF exceeds total metadata budget".into()));
        }
    }
    let tags: BTreeMap<_, _> = ifd
        .tag_iter()
        .map(|r| r.map(|(tag, value)| (tag.to_u16(), value)))
        .collect::<Result<_, _>>()?;
    let orientation = decoder
        .find_tag_unsigned::<u16>(Tag::Orientation)?
        .unwrap_or(1);
    if !(1..=8).contains(&orientation) {
        return Err(Error("invalid TIFF Orientation tag".into()));
    }
    Ok(Metadata {
        shape: [h as usize, w as usize],
        pixel_type,
        little_endian: decoder.byte_order() == tiff_backend::tags::ByteOrder::LittleEndian,
        orientation,
        compression,
        tags,
    })
}
fn into_native(pixels: PixelBuffer) -> DecodingResult {
    match pixels {
        PixelBuffer::U8(v) => DecodingResult::U8(v),
        PixelBuffer::U16(v) => DecodingResult::U16(v),
        PixelBuffer::U32(v) => DecodingResult::U32(v),
        PixelBuffer::U64(v) => DecodingResult::U64(v),
        PixelBuffer::I8(v) => DecodingResult::I8(v),
        PixelBuffer::I16(v) => DecodingResult::I16(v),
        PixelBuffer::I32(v) => DecodingResult::I32(v),
        PixelBuffer::I64(v) => DecodingResult::I64(v),
        PixelBuffer::F32(v) => DecodingResult::F32(v),
        PixelBuffer::F64(v) => DecodingResult::F64(v),
        _ => unreachable!("TIFF backend cannot produce complex sample buffers"),
    }
}
fn from_native(pixels: DecodingResult) -> Result<PixelBuffer, Error> {
    Ok(match pixels {
        DecodingResult::U8(v) => PixelBuffer::U8(v),
        DecodingResult::U16(v) => PixelBuffer::U16(v),
        DecodingResult::U32(v) => PixelBuffer::U32(v),
        DecodingResult::U64(v) => PixelBuffer::U64(v),
        DecodingResult::I8(v) => PixelBuffer::I8(v),
        DecodingResult::I16(v) => PixelBuffer::I16(v),
        DecodingResult::I32(v) => PixelBuffer::I32(v),
        DecodingResult::I64(v) => PixelBuffer::I64(v),
        DecodingResult::F32(v) => PixelBuffer::F32(v),
        DecodingResult::F64(v) => PixelBuffer::F64(v),
        DecodingResult::F16(_) => {
            return Err(Error("float16 detector samples are not supported".into()));
        }
    })
}
