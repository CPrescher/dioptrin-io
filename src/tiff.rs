//! Count-preserving TIFF/BigTIFF detector pages using the MIT `tiff` crate.
//!
//! Stored row order, scalar sample bits and Orientation are retained. Supported
//! integer WhiteIsZero inversions are exactly undone, packed gray samples are
//! unpacked without intensity scaling, and color planes are interleaved.
//! Float16 source values widen exactly to f32, with the source bit width retained.
//! Palette and unsupported photometrics fail explicitly. Tags remain
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
pub struct Error(pub(crate) String);
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
    /// Original TIFF scalar bit width (may be 1/2/4 for unpacked u8 output).
    pub bits_per_sample: u8,
    /// Number of original scalar channels; buffers interleave channels last.
    pub channels: usize,
    pub photometric_interpretation: u16,
    pub planar_configuration: u16,
    /// True for TIFF JPEG/WebP compression; decoded values retain that provenance.
    pub lossy: bool,
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
    channels: usize,
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
    /// Full scalar-array shape: (height,width) or (height,width,channels).
    #[must_use]
    pub fn sample_shape(&self) -> Option<Vec<usize>> {
        self.shape.map(|s| {
            if self.channels == 1 {
                s.to_vec()
            } else {
                vec![s[0], s[1], self.channels]
            }
        })
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
        let layout = decoded?;
        let mut byte_count = buffer
            .pixels
            .len()
            .checked_mul(buffer.pixels.pixel_type().bytes())
            .ok_or_else(|| Error("TIFF decoded size overflow".into()))?;
        if page.bits_per_sample == 16 && page.pixel_type == PixelType::F32 {
            byte_count /= 2;
        }
        if byte_count != layout.complete_len {
            return Err(Error(
                "TIFF backend returned only a subset of color planes".into(),
            ));
        }
        if page.photometric_interpretation == 0 {
            undo_white(&mut buffer.pixels)?;
        }
        if page.bits_per_sample < 8 {
            let PixelBuffer::U8(packed) = &buffer.pixels else {
                return Err(Error("packed TIFF output must be u8".into()));
            };
            let stride = layout
                .row_stride
                .ok_or_else(|| Error("missing packed TIFF row stride".into()))?
                .get();
            let mut unpacked = Vec::new();
            unpacked
                .try_reserve_exact(page.shape[0] * page.shape[1])
                .map_err(|e| Error(e.to_string()))?;
            for y in 0..page.shape[0] {
                for x in 0..page.shape[1] {
                    let offset = x * usize::from(page.bits_per_sample);
                    let byte = *packed
                        .get(y * stride + offset / 8)
                        .ok_or_else(|| Error("truncated packed TIFF row".into()))?;
                    unpacked.push(
                        (byte >> (8 - usize::from(page.bits_per_sample) - offset % 8))
                            & ((1u8 << page.bits_per_sample) - 1),
                    );
                }
            }
            buffer.pixels = PixelBuffer::U8(unpacked);
        } else if layout.planes > 1 {
            if layout.planes != page.channels {
                return Err(Error("TIFF planar channel mismatch".into()));
            }
            interleave(
                &mut buffer.pixels,
                page.shape[0] * page.shape[1],
                page.channels,
            )?;
        }
        if buffer.pixels.len() != page.shape[0] * page.shape[1] * page.channels
            || buffer.pixels.pixel_type() != page.pixel_type
        {
            return Err(Error("TIFF decoded sample count/type mismatch".into()));
        }
        self.snapshot
            .check_handle(&self.identity_handle)
            .map_err(Error)?;
        buffer.shape = Some(page.shape);
        buffer.channels = page.channels;
        Ok(())
    }
}

fn metadata(
    decoder: &mut Decoder<BufReader<File>>,
    limits: Limits,
    total: &mut usize,
) -> Result<Metadata, Error> {
    let (w, h) = decoder.dimensions()?;
    let color = decoder.colortype()?;
    let (bits, channels) = match color {
        ColorType::Gray(bits) => (bits, 1),
        ColorType::RGB(bits) | ColorType::YCbCr(bits) => (bits, 3),
        ColorType::RGBA(bits) => (bits, 4),
        ColorType::CMYK(bits) => (bits, 4),
        ColorType::CMYKA(bits) => (bits, 5),
        ColorType::Multiband {
            bit_depth,
            num_samples,
        } => (bit_depth, usize::from(num_samples)),
        _ => return Err(Error(
            "unsupported TIFF photometric interpretation; no implicit color conversion is applied"
                .into(),
        )),
    };
    let formats = decoder
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)?
        .unwrap_or_else(|| vec![1]);
    let format = *formats
        .first()
        .ok_or_else(|| Error("empty TIFF SampleFormat".into()))?;
    if formats.iter().any(|&v| v != format) {
        return Err(Error("mixed TIFF sample types are unsupported".into()));
    }
    let samples = decoder
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)?
        .unwrap_or(1);
    let photo = decoder
        .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)?
        .unwrap_or(1);
    let planar = decoder
        .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)?
        .unwrap_or(1);
    let fill_order = decoder
        .find_tag_unsigned::<u16>(Tag::FillOrder)?
        .unwrap_or(1);
    if usize::from(samples) != channels
        || channels == 0
        || ![1, 2].contains(&planar)
        || ![0, 1, 2, 5, 6].contains(&photo)
    {
        return Err(Error(
            "unsupported TIFF scalar/channel/photometric layout".into(),
        ));
    }
    if photo == 0 && (channels != 1 || format != 1) {
        return Err(Error("WhiteIsZero requires unsigned integer source samples; floating inversion is not lossless".into()));
    }
    if fill_order != 1 {
        return Err(Error(
            "TIFF FillOrder other than MSB-first is unsupported".into(),
        ));
    }
    if bits < 8 && (channels != 1 || format != 1 || ![1, 2, 4].contains(&bits)) {
        return Err(Error(
            "unsupported packed TIFF layout; admit 1/2/4-bit single-channel MSB-first integers"
                .into(),
        ));
    }
    let compression = decoder
        .find_tag_unsigned::<u16>(Tag::Compression)?
        .unwrap_or(1);
    if ![1, 4, 5, 7, 8, 32946, 32773].contains(&compression) {
        return Err(Error(format!(
            "unsupported TIFF detector compression {compression}"
        )));
    }
    if photo == 6 && compression != 7 {
        return Err(Error(
            "TIFF YCbCr admitted only through native JPEG component decoding".into(),
        ));
    }
    if compression == 4 && (channels != 1 || bits != 1) {
        return Err(Error(
            "TIFF Group 4 requires single-channel bilevel data".into(),
        ));
    }
    if compression == 7 && (format != 1 || bits != 8 || ![1, 3, 4].contains(&channels)) {
        return Err(Error(
            "unsupported TIFF JPEG precision/channel layout".into(),
        ));
    }
    let pixel_type = match (format, bits) {
        (1, 1 | 2 | 4 | 8) => PixelType::U8,
        (1, 16) => PixelType::U16,
        (1, 32) => PixelType::U32,
        (1, 64) => PixelType::U64,
        (2, 8) => PixelType::I8,
        (2, 16) => PixelType::I16,
        (2, 32) => PixelType::I32,
        (2, 64) => PixelType::I64,
        (3, 16 | 32) => PixelType::F32,
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
            .and_then(|n| n.checked_mul(channels))
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
        bits_per_sample: bits,
        channels,
        photometric_interpretation: photo,
        planar_configuration: planar,
        lossy: matches!(compression, 6 | 7 | 50001),
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
        DecodingResult::F16(v) => PixelBuffer::F32(
            v.into_iter()
                .map(|x| {
                    // Bit expansion preserves even signaling NaN payloads without a
                    // floating arithmetic operation or hardware NaN quieting.
                    let bits = u32::from(x.to_bits());
                    let sign = (bits & 0x8000) << 16;
                    let exp = (bits >> 10) & 31;
                    let frac = bits & 1023;
                    let out = match (exp, frac) {
                        (0, 0) => sign,
                        (0, _) => {
                            let leading = frac.ilog2();
                            sign | ((leading + 103) << 23)
                                | ((frac - (1 << leading)) << (23 - leading))
                        }
                        (31, _) => sign | 0x7f800000 | (frac << 13),
                        _ => sign | ((exp + 112) << 23) | (frac << 13),
                    };
                    f32::from_bits(out)
                })
                .collect(),
        ),
    })
}

fn undo_white(pixels: &mut PixelBuffer) -> Result<(), Error> {
    match pixels {
        PixelBuffer::U8(v) => v.iter_mut().for_each(|x| *x = !*x),
        PixelBuffer::U16(v) => v.iter_mut().for_each(|x| *x = !*x),
        PixelBuffer::U32(v) => v.iter_mut().for_each(|x| *x = !*x),
        PixelBuffer::U64(v) => v.iter_mut().for_each(|x| *x = !*x),
        _ => {
            return Err(Error(
                "TIFF WhiteIsZero inversion is not losslessly reversible for this type".into(),
            ));
        }
    }
    Ok(())
}
fn interleave(pixels: &mut PixelBuffer, plane: usize, channels: usize) -> Result<(), Error> {
    macro_rules! rearrange {($($v:ident);*)=>{match pixels {$(PixelBuffer::$v(v)=>{
        if v.len()!=plane*channels {return Err(Error("TIFF planar sample count mismatch".into()));}
        let mut interleaved=Vec::new();interleaved.try_reserve_exact(v.len()).map_err(|e|Error(e.to_string()))?;
        for index in 0..plane {for channel in 0..channels {interleaved.push(v[channel*plane+index]);}}
        *v=interleaved;
    }),*,_=>return Err(Error("unsupported TIFF planar sample type".into()))}};}
    rearrange!(U8;U16;U32;U64;I8;I16;I32;I64;F32;F64);
    Ok(())
}
impl Metadata {
    #[must_use]
    pub fn sample_shape(&self) -> Vec<usize> {
        if self.channels == 1 {
            self.shape.to_vec()
        } else {
            vec![self.shape[0], self.shape[1], self.channels]
        }
    }
}
