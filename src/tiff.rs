//! Count-preserving TIFF/BigTIFF detector pages using the MIT `tiff` crate.
//!
//! Stored row order, scalar sample bits and Orientation are retained. Supported
//! WhiteIsZero samples bypass the backend display inversion, packed gray samples are
//! unpacked without intensity scaling, and uniform color planes are interleaved.
//! YCbCr exposes explicit native component grids and sampling units.
//! Float16 source values widen exactly to f32, with the source bit width retained.
//! Palettes retain raw indices and ColorMap; CFA retains its scalar plane. Tags remain
//! typed values, including private tags understood by the backend's value parser.
use crate::{PixelBuffer, PixelType, source::Snapshot};
pub mod api;
pub mod compatibility;
pub use compatibility::{Options, TileTables, UncompressedWindows};
pub mod directories;
mod group3;
pub mod header;
mod ifd_view;
#[cfg(feature = "jpeg")]
mod jpeg_components;
#[cfg(feature = "jpeg")]
pub mod ojpeg;
pub mod thunderscan;
#[cfg(feature = "jpeg")]
mod ycbcr_jpeg;
use ifd_view::{Admission, ChunkWindows, FileView, OriginalPhoto};
pub mod view;
#[cfg(feature = "jpeg")]
mod view_jpeg;
mod view_ycbcr;
pub mod ycbcr;
use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};
pub use tiff_backend::decoder::ifd::Value as TagValue;
use tiff_backend::{
    ColorType,
    decoder::{ChunkType, Decoder, DecodingResult},
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

/// Original nonstructural metadata that cannot be decoded as declared. No
/// offset/text repair is attempted; source descriptor and bytes remain visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaqueTag {
    pub field_type: u16,
    pub count: u64,
    pub ifd_entry: Vec<u8>,
    pub value_bytes: Vec<u8>,
    pub reason: String,
}

/// Native source layout. Component grids are exposed independently, without
/// requiring a shared full-resolution HWC array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleLayout {
    Uniform,
    YCbCrComponents { horizontal: u16, vertical: u16 },
}

/// Original page metadata. Height and width refer to stored rows/columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    /// Original TIFF scalar bit width (may be 1/2/4 for unpacked u8 output).
    pub bits_per_sample: u8,
    /// Number of declared source components. Uniform buffers interleave channels;
    /// component layouts expose independently shaped grids through read_ycbcr_into.
    pub channels: usize,
    pub sample_layout: SampleLayout,
    pub photometric_interpretation: u16,
    pub planar_configuration: u16,
    /// Logical bit order in each stored byte; source tag retained unchanged.
    pub fill_order: u16,
    /// True for TIFF JPEG/WebP compression; decoded values retain that provenance.
    pub lossy: bool,
    pub little_endian: bool,
    /// TIFF Orientation, retained without applying it (default 1).
    pub orientation: u16,
    pub compression: u16,
    /// Typed IFD tags, keyed by original numeric tag ID.
    pub tags: BTreeMap<u16, TagValue>,
    pub opaque_tags: BTreeMap<u16, OpaqueTag>,
    pub compatibility: Option<compatibility::Interpretation>,
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
    decoder: Decoder<BufReader<FileView>>,
    identity_handle: File,
    snapshot: Snapshot,
    pages: Vec<Metadata>,
    ifd_offsets: Vec<u64>,
    #[cfg(feature = "jpeg")]
    ojpeg_pages: Vec<Option<ojpeg::SourceMetadata>>,
    chunk_windows: Vec<ChunkWindows>,
    intermediate_bytes: usize,
    limits: Limits,
}
impl Reader {
    /// Validate/index all primary IFD pages without reading detector samples.
    /// # Errors
    /// Malformed/cyclic/truncated IFDs, resource bounds or unsupported encodings.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        Self::open_with_options(path, limits, Options::default())
    }
    /// Explicit source-metadata interpretations; default options preserve strict admission.
    /// # Errors
    /// Incomplete/ambiguous source windows, malformed metadata or resource bounds.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        limits: Limits,
        options: Options,
    ) -> Result<Self, Error> {
        Self::open_indexed(path, limits, options, None)
    }
    /// Select exactly one reachable directory, including a SubIFD. Primary
    /// indexing remains unchanged; arbitrary offsets are never admitted.
    /// # Errors
    /// Invalid/cyclic metadata graph, unknown offset, unsupported selected
    /// layout, resource bounds or source changes.
    pub fn open_directory(
        path: impl AsRef<Path>,
        offset: u64,
        limits: Limits,
        options: Options,
    ) -> Result<Self, Error> {
        Self::open_indexed(path, limits, options, Some(offset))
    }
    fn open_indexed(
        path: impl AsRef<Path>,
        limits: Limits,
        options: Options,
        selected: Option<u64>,
    ) -> Result<Self, Error> {
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
        if !std::fs::metadata(&snapshot.path)
            .map_err(|e| Error(e.to_string()))?
            .is_file()
        {
            return Err(Error("TIFF path snapshots require regular files".into()));
        }
        let file = File::open(&snapshot.path).map_err(|e| Error(e.to_string()))?;
        let identity_handle = file.try_clone().map_err(|e| Error(e.to_string()))?;
        snapshot.check_handle(&identity_handle).map_err(Error)?;
        let mut native = tiff_backend::decoder::Limits::default();
        native.decoding_buffer_size = limits.frame_bytes;
        native.ifd_value_size = limits.metadata_bytes;
        native.intermediate_buffer_size = limits.intermediate_bytes;
        // Decoder::new initially uses the upstream defaults while parsing the
        // first IFD (1 MiB per tag); our tighter limits apply immediately after.
        let admission = if selected.is_some() {
            Admission::graph(file, limits, options)?
        } else {
            Admission::open(file, limits, options)?
        };
        if selected.is_some_and(|offset| !admission.links.contains_key(&offset)) {
            return Err(Error(
                "selected TIFF directory is not reachable from the source root".into(),
            ));
        }
        let Admission {
            file,
            photos,
            mut opaque,
            retained,
            mut interpretations,
            ..
        } = admission;
        let overrides = interpretations
            .iter()
            .map(|(&offset, value)| (offset, value.backend()))
            .collect();
        let mut decoder = Decoder::new_at_directory(
            BufReader::new(file),
            selected.map(tiff_backend::tags::IfdPointer),
            native,
            overrides,
        )?;
        let mut pages = Vec::new();
        let mut ifd_offsets = Vec::new();
        #[cfg(feature = "jpeg")]
        let mut ojpeg_pages = Vec::new();
        let mut chunk_windows = Vec::new();
        let mut metadata_bytes = retained;
        loop {
            if pages.len() >= limits.frames {
                return Err(Error("TIFF exceeds frame limit".into()));
            }
            let ifd = decoder
                .ifd_pointer()
                .ok_or_else(|| Error("missing TIFF primary IFD pointer".into()))?
                .0;
            let original = photos.get(&ifd);
            let mut page = metadata(
                &mut decoder,
                limits,
                &mut metadata_bytes,
                original,
                opaque.remove(&ifd).unwrap_or_default(),
            )?;
            page.compatibility = interpretations.remove(&ifd);
            let (offset_tag, count_tag) = if decoder.get_chunk_type() == ChunkType::Tile {
                (Tag::TileOffsets, Tag::TileByteCounts)
            } else {
                (Tag::StripOffsets, Tag::StripByteCounts)
            };
            let (offsets, lengths) = if page.compatibility.is_some() {
                let (a, b) = decoder.effective_chunk_tables();
                (a.to_vec(), b.to_vec())
            } else {
                (
                    decoder.get_tag(offset_tag)?.into_u64_vec()?,
                    decoder.get_tag(count_tag)?.into_u64_vec()?,
                )
            };
            if matches!(page.sample_layout, SampleLayout::YCbCrComponents { .. })
                && [1, 5].contains(&page.compression)
            {
                ycbcr::validate(&page, limits)?;
            } else {
                validate_chunk_lengths(&mut decoder, &page, &offsets, &lengths)?;
            }
            metadata_bytes = offsets
                .len()
                .checked_mul(16)
                .and_then(|n| n.checked_add(32))
                .and_then(|n| metadata_bytes.checked_add(n))
                .filter(|&n| n <= limits.metadata_bytes)
                .ok_or_else(|| Error("TIFF chunk windows exceed metadata budget".into()))?;
            chunk_windows.push(decoder.inner().get_ref().check_chunks(&offsets, &lengths)?);
            if page.compression == 7 {
                validate_jpeg_chunks(&mut decoder, &page, &snapshot, limits)?;
            }
            #[cfg(feature = "jpeg")]
            {
                let info = if page.compression == 6 {
                    let info = ojpeg::inspect(&snapshot, &identity_handle, &page, limits)?;
                    metadata_bytes = metadata_bytes
                        .checked_add(info.retained_bytes)
                        .filter(|&n| n <= limits.metadata_bytes)
                        .ok_or_else(|| {
                            Error("TIFF old JPEG headers exceed metadata budget".into())
                        })?;
                    let offsets: Vec<_> = info.source_windows.iter().map(|w| w.offset).collect();
                    let lengths: Vec<_> =
                        info.source_windows.iter().map(|w| w.bytes as u64).collect();
                    decoder.inner().get_ref().check_chunks(&offsets, &lengths)?;
                    Some(info)
                } else {
                    None
                };
                ojpeg_pages.push(info);
            }
            metadata_bytes = metadata_bytes
                .checked_add(8)
                .filter(|&n| n <= limits.metadata_bytes)
                .ok_or_else(|| Error("TIFF IFD offsets exceed metadata budget".into()))?;
            ifd_offsets
                .try_reserve_exact(1)
                .map_err(|e| Error(e.to_string()))?;
            ifd_offsets.push(ifd);
            pages.push(page);
            if selected.is_some() || !decoder.more_images() {
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
            ifd_offsets,
            #[cfg(feature = "jpeg")]
            ojpeg_pages,
            chunk_windows,
            intermediate_bytes: limits.intermediate_bytes,
            limits,
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
    /// Original OJPEG windows/header reconstruction provenance for a legacy page.
    #[cfg(feature = "jpeg")]
    #[must_use]
    pub fn ojpeg_metadata(&self, index: usize) -> Option<&ojpeg::SourceMetadata> {
        self.ojpeg_pages.get(index).and_then(Option::as_ref)
    }
    /// Read native Y/Cb/Cr component grids and exact TIFF sampling units.
    /// Color conversion and chroma expansion are separate explicit display views.
    /// # Errors
    /// Unsupported layout, resource bounds, malformed codec or changed source.
    pub fn read_ycbcr_into(
        &mut self,
        index: usize,
        buffer: &mut ycbcr::Buffer,
    ) -> Result<(), Error> {
        buffer.invalidate();
        self.snapshot
            .check_handle(&self.identity_handle)
            .map_err(Error)?;
        let page = self
            .pages
            .get(index)
            .ok_or_else(|| Error("TIFF component page index outside file".into()))?;
        let SampleLayout::YCbCrComponents {
            horizontal,
            vertical,
        } = page.sample_layout
        else {
            return Err(Error(
                "TIFF page does not expose a native YCbCr component layout".into(),
            ));
        };
        let (chunks, storage) = if [1, 5].contains(&page.compression) {
            (
                ycbcr::decode(
                    &self.snapshot,
                    &self.identity_handle,
                    page,
                    self.limits.intermediate_bytes,
                )?,
                ycbcr::Storage::TiffSamplingUnits,
            )
        } else {
            #[cfg(feature = "jpeg")]
            {
                let chunks = match page.compression {
                    6 => ojpeg::decode(&self.snapshot, &self.identity_handle, page, self.limits)?,
                    7 => ycbcr_jpeg::decode(
                        &self.snapshot,
                        &self.identity_handle,
                        page,
                        self.limits,
                    )?,
                    _ => return Err(Error("unsupported TIFF component compression".into())),
                };
                (chunks, ycbcr::Storage::JpegDecodedComponents)
            }
            #[cfg(not(feature = "jpeg"))]
            {
                return Err(Error(
                    "TIFF JPEG components require the jpeg feature".into(),
                ));
            }
        };
        self.snapshot
            .check_handle(&self.identity_handle)
            .map_err(Error)?;
        buffer.set(
            page.shape,
            [usize::from(horizontal), usize::from(vertical)],
            storage,
            chunks,
        );
        Ok(())
    }
    /// Read only validated complete ThunderScan rows; unavailable rows have no samples.
    /// # Errors
    /// Wrong codec, changed source, malformed source windows or resource bounds.
    pub fn read_thunderscan_available_into(
        &mut self,
        index: usize,
        frame: &mut thunderscan::AvailableFrame,
    ) -> Result<(), Error> {
        frame.invalidate();
        let page = self
            .pages
            .get(index)
            .ok_or_else(|| Error("TIFF page index outside file".into()))?;
        thunderscan::decode(
            &self.snapshot,
            &self.identity_handle,
            page,
            self.limits,
            frame,
        )
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
        if page.sample_layout != SampleLayout::Uniform {
            return Err(Error(
                "TIFF native component grids require read_ycbcr_into; no implicit expansion".into(),
            ));
        }
        if page.compression == 32809 {
            let mut available = thunderscan::AvailableFrame::default();
            thunderscan::decode(
                &self.snapshot,
                &self.identity_handle,
                page,
                self.limits,
                &mut available,
            )?;
            if !available.complete() {
                return Err(Error(
                    "incomplete ThunderScan source; request available rows explicitly".into(),
                ));
            }
            buffer.pixels = available.into_pixels();
            buffer.shape = Some(page.shape);
            buffer.channels = 1;
            return Ok(());
        }
        if [2, 3].contains(&page.compression) {
            group3::decode(
                &self.snapshot,
                &self.identity_handle,
                page,
                self.intermediate_bytes,
                &mut buffer.pixels,
            )?;
            buffer.shape = Some(page.shape);
            buffer.channels = page.channels;
            return Ok(());
        }
        self.decoder
            .seek_to_directory(tiff_backend::tags::IfdPointer(self.ifd_offsets[index]))?;
        // Discard any metadata prefetch before switching to bounded sample
        // windows. The backend seeks to every chunk before reading it.
        discard_prefetch(self.decoder.inner())?;
        self.decoder
            .inner()
            .get_mut()
            .begin_samples(Arc::clone(&self.chunk_windows[index]));
        let mut native = into_native(std::mem::take(&mut buffer.pixels));
        // Move the allocation back even on failure; no partial frame is exposed.
        let decoded = self.decoder.read_image_to_buffer(&mut native);
        self.decoder.inner().get_mut().end_samples();
        buffer.pixels = from_native(native)?;
        // Discard sample prefetch before permitting subsequent metadata reads.
        discard_prefetch(self.decoder.inner())?;
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
        if page.bits_per_sample < 8 {
            if page.fill_order == 2 {
                let PixelBuffer::U8(packed) = &mut buffer.pixels else {
                    return Err(Error("LSB-first packed TIFF requires byte buffer".into()));
                };
                for byte in packed {
                    *byte = byte.reverse_bits();
                }
            }
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
    decoder: &mut Decoder<BufReader<FileView>>,
    limits: Limits,
    total: &mut usize,
    original: Option<&OriginalPhoto>,
    opaque_tags: BTreeMap<u16, OpaqueTag>,
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
    let backend_photo = decoder
        .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)?
        .unwrap_or(1);
    if backend_photo == 0 {
        return Err(Error(
            "unsupported TIFF WhiteIsZero tag representation".into(),
        ));
    }
    let photo = original.map_or(backend_photo, |p| p.value);
    let planar = decoder
        .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)?
        .unwrap_or(1);
    let fill_order = decoder
        .find_tag_unsigned::<u16>(Tag::FillOrder)?
        .unwrap_or(1);
    if usize::from(samples) != channels
        || channels == 0
        || ![1, 2].contains(&planar)
        || ![0, 1, 2, 3, 5, 6, 32803].contains(&photo)
    {
        return Err(Error(
            "unsupported TIFF scalar/channel/photometric layout".into(),
        ));
    }
    if photo == 32803 && (channels != 1 || format != 1 || ![8, 16].contains(&bits)) {
        return Err(Error(
            "CFA TIFF requires a single native unsigned8/16 sample plane".into(),
        ));
    }
    if photo == 3 && (channels != 1 || format != 1 || ![1, 2, 4, 8, 16].contains(&bits)) {
        return Err(Error("unsupported TIFF palette index layout".into()));
    }
    if ![1, 2].contains(&fill_order) {
        return Err(Error("invalid TIFF FillOrder".into()));
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
    if ![1, 2, 3, 4, 5, 6, 7, 8, 32946, 32773, 32809].contains(&compression) {
        return Err(Error(format!(
            "unsupported TIFF detector compression {compression}"
        )));
    }
    if compression == 6 && !cfg!(feature = "jpeg") {
        return Err(Error(
            "old JPEG TIFF requires the optional jpeg feature".into(),
        ));
    }
    if fill_order == 2
        && !([2, 3].contains(&compression) || compression == 1 && bits < 8 && channels == 1)
    {
        return Err(Error(
            "LSB-first TIFF is admitted only for CCITT RLE/Group3 and uncompressed packed grayscale".into(),
        ));
    }
    if compression == 32809
        && (bits != 4
            || channels != 1
            || format != 1
            || planar != 1
            || ![0, 1].contains(&photo)
            || decoder.get_chunk_type() != ChunkType::Strip
            || decoder
                .find_tag_unsigned::<u16>(Tag::Predictor)?
                .unwrap_or(1)
                != 1)
    {
        return Err(Error(
            "ThunderScan requires unsigned four-bit single-plane strips without predictor".into(),
        ));
    }
    if [2, 3].contains(&compression) {
        let options = decoder
            .find_tag_unsigned::<u32>(Tag::Unknown(292))?
            .unwrap_or(0);
        if bits != 1
            || format != 1
            || channels != 1
            || planar != 1
            || ![0, 1].contains(&photo)
            || options & !5 != 0
            || compression == 2 && options != 0
            || decoder
                .find_tag_unsigned::<u16>(Tag::Predictor)?
                .unwrap_or(1)
                != 1
            || decoder.get_chunk_type() != ChunkType::Strip
        {
            return Err(Error(
                "CCITT RLE/Group3 TIFF requires bilevel strips without uncompressed mode or predictor"
                    .into(),
            ));
        }
        if w > u32::from(u16::MAX) {
            return Err(Error(
                "Group3 TIFF width exceeds native decoder limit".into(),
            ));
        }
    }
    if photo == 6 && ![1, 5, 6, 7].contains(&compression) {
        return Err(Error("unsupported TIFF YCbCr compression".into()));
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
    let mut tags: BTreeMap<_, _> = ifd
        .tag_iter()
        .map(|r| r.map(|(tag, value)| (tag.to_u16(), value)))
        .collect::<Result<_, _>>()?;
    if let Some(original) = original {
        tags.insert(262, original.typed.clone());
    }
    for tag in opaque_tags.keys() {
        tags.remove(tag);
    }
    if photo == 3 {
        let colors = decoder.get_tag(Tag::ColorMap)?.into_u16_vec()?;
        if colors.len()
            != 3usize
                .checked_shl(u32::from(bits))
                .ok_or_else(|| Error("TIFF palette size overflow".into()))?
        {
            return Err(Error(
                "TIFF ColorMap length disagrees with index precision".into(),
            ));
        }
    }
    let orientation = decoder
        .find_tag_unsigned::<u16>(Tag::Orientation)?
        .unwrap_or(1);
    if !(1..=8).contains(&orientation) {
        return Err(Error("invalid TIFF Orientation tag".into()));
    }
    let sample_layout = if photo == 6 {
        let factors = tags
            .get(&530)
            .map(|v| v.clone().into_u16_vec())
            .transpose()?
            .unwrap_or_else(|| vec![2, 2]);
        let [horizontal, vertical] = factors.as_slice() else {
            return Err(Error("YCbCr sampling tag must have two factors".into()));
        };
        if ![1, 2, 4].contains(horizontal) || ![1, 2, 4].contains(vertical) || vertical > horizontal
        {
            return Err(Error("unsupported TIFF YCbCr sampling factors".into()));
        }
        SampleLayout::YCbCrComponents {
            horizontal: *horizontal,
            vertical: *vertical,
        }
    } else {
        SampleLayout::Uniform
    };
    Ok(Metadata {
        sample_layout,
        shape: [h as usize, w as usize],
        pixel_type,
        bits_per_sample: bits,
        channels,
        photometric_interpretation: photo,
        planar_configuration: planar,
        fill_order,
        lossy: matches!(compression, 6 | 7 | 50001),
        little_endian: decoder.byte_order() == tiff_backend::tags::ByteOrder::LittleEndian,
        orientation,
        compression,
        tags,
        opaque_tags,
        compatibility: None,
    })
}
// BufReader::stream_position preserves its cache. This actual seek is needed
// to discard bytes prefetched under the previous metadata/sample access mode.
#[allow(clippy::seek_from_current)]
fn discard_prefetch(reader: &mut BufReader<FileView>) -> Result<(), Error> {
    reader
        .seek(SeekFrom::Current(0))
        .map_err(|e| Error(e.to_string()))?;
    Ok(())
}
// The native uncompressed codec ignores declared chunk byte counts. Admit
// source bytes (including packed row alignment and horizontal tile padding),
// rather than the decoder's potentially widened/interleaved output layout.
fn validate_chunk_lengths(
    decoder: &mut Decoder<BufReader<FileView>>,
    page: &Metadata,
    offsets: &[u64],
    lengths: &[u64],
) -> Result<(), Error> {
    let chunks = if [2, 3].contains(&page.compression) {
        // RowsPerStrip=UINT32_MAX is the standard one-strip/default dialect.
        // Upstream strip_count() uses an overflowing height+rows-1 expression;
        // this codec uses the clipped logical source row count instead.
        let rows = page
            .tags
            .get(&278)
            .map(|v| v.clone().into_u64())
            .transpose()?
            .unwrap_or(page.shape[0] as u64)
            .min(page.shape[0] as u64);
        if rows == 0 {
            return Err(Error("zero Group3 RowsPerStrip".into()));
        }
        u32::try_from((page.shape[0] as u64).div_ceil(rows))
            .map_err(|_| Error("Group3 strip count exceeds platform".into()))?
    } else if decoder.get_chunk_type() == ChunkType::Tile {
        decoder.tile_count()?
    } else {
        decoder.strip_count()?
    };
    if chunks == 0 || offsets.len() != chunks as usize || lengths.len() != offsets.len() {
        return Err(Error("invalid TIFF chunk table".into()));
    }
    if page.compression == 1 {
        let samples = if page.planar_configuration == 2 {
            1
        } else {
            page.channels as u64
        };
        let row = u64::from(decoder.chunk_dimensions().0)
            .checked_mul(u64::from(page.bits_per_sample))
            .and_then(|n| n.checked_mul(samples))
            .and_then(|n| n.checked_add(7))
            .map(|n| n / 8)
            .ok_or_else(|| Error("TIFF encoded row size overflow".into()))?;
        for (index, &length) in lengths.iter().enumerate() {
            let required = row
                .checked_mul(u64::from(decoder.chunk_data_dimensions(index as u32).1))
                .ok_or_else(|| Error("TIFF encoded chunk size overflow".into()))?;
            if length < required {
                return Err(Error(
                    "TIFF uncompressed chunk shorter than required source rows".into(),
                ));
            }
        }
    }
    Ok(())
}
// TIFF's JPEG backend reads only the bytes required by its IFD layout. Its
// native decoder can otherwise accept an RGB JPEG strip labeled as gray and
// silently discard two thirds of the component samples. Admit every encoded
// chunk against that layout before native pixel decoding.
fn validate_jpeg_chunks(
    decoder: &mut Decoder<BufReader<FileView>>,
    page: &Metadata,
    snapshot: &Snapshot,
    limits: Limits,
) -> Result<(), Error> {
    use tiff_jpeg_header::{
        JpegDecoder,
        zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions},
    };
    let tiled = decoder.get_chunk_type() == ChunkType::Tile;
    let (offset_tag, count_tag, chunks) = if tiled {
        (Tag::TileOffsets, Tag::TileByteCounts, decoder.tile_count()?)
    } else {
        (
            Tag::StripOffsets,
            Tag::StripByteCounts,
            decoder.strip_count()?,
        )
    };
    let offsets = decoder
        .find_tag_unsigned_vec::<u64>(offset_tag)?
        .ok_or_else(|| Error("TIFF JPEG lacks chunk offsets".into()))?;
    let lengths = decoder
        .find_tag_unsigned_vec::<u64>(count_tag)?
        .ok_or_else(|| Error("TIFF JPEG lacks chunk byte counts".into()))?;
    if chunks == 0 || offsets.len() != chunks as usize || lengths.len() != offsets.len() {
        return Err(Error("invalid TIFF JPEG chunk table".into()));
    }
    let tables = decoder
        .find_tag(Tag::JPEGTables)?
        .map(|value| value.into_u8_vec())
        .transpose()?;
    if tables.as_ref().is_some_and(|bytes| {
        bytes.len() < 4 || !bytes.starts_with(&[0xff, 0xd8]) || !bytes.ends_with(&[0xff, 0xd9])
    }) {
        return Err(Error("invalid TIFF JPEG shared-table envelope".into()));
    }
    let planar = page.planar_configuration == 2;
    let channels = if planar { 1 } else { page.channels };
    let color = if planar {
        ColorSpace::Luma
    } else {
        match (page.photometric_interpretation, channels) {
            (0 | 1, 1) => ColorSpace::Luma,
            (2, 3) => ColorSpace::RGB,
            (5, 4) => ColorSpace::CMYK,
            (6, 3) => ColorSpace::YCbCr,
            _ => {
                return Err(Error(
                    "unsupported ambiguous TIFF JPEG channel/photometric layout".into(),
                ));
            }
        }
    };
    let mut source = File::open(&snapshot.path).map_err(|e| Error(e.to_string()))?;
    snapshot.check_handle(&source).map_err(Error)?;
    let source_bytes = source.metadata().map_err(|e| Error(e.to_string()))?.len();
    let mut encoded = Vec::new();
    for (index, (&offset, &length)) in offsets.iter().zip(&lengths).enumerate() {
        let length = usize::try_from(length)
            .ok()
            .filter(|&n| {
                n >= 2
                    && offset
                        .checked_add(n as u64)
                        .is_some_and(|end| end <= source_bytes)
            })
            .ok_or_else(|| Error("TIFF JPEG chunk exceeds source bounds".into()))?;
        let prefix = tables.as_ref().map_or(0, |bytes| bytes.len() - 2);
        let combined = length
            .checked_add(prefix)
            .filter(|&n| n <= limits.intermediate_bytes)
            .ok_or_else(|| Error("TIFF JPEG header/payload exceeds intermediate limit".into()))?;
        encoded.clear();
        encoded
            .try_reserve_exact(combined)
            .map_err(|e| Error(e.to_string()))?;
        encoded.resize(combined, 0);
        source
            .seek(SeekFrom::Start(offset))
            .map_err(|e| Error(e.to_string()))?;
        source
            .read_exact(&mut encoded[prefix..])
            .map_err(|e| Error(e.to_string()))?;
        if encoded[prefix..prefix + 2] != [0xff, 0xd8] {
            return Err(Error("TIFF JPEG chunk lacks SOI".into()));
        }
        // Match the backend's shared-table reconstruction exactly: one SOI,
        // shared tables without EOI, then the chunk without its original SOI.
        if let Some(tables) = &tables {
            encoded[..prefix].copy_from_slice(&tables[..prefix]);
            encoded.copy_within(prefix + 2.., prefix);
            encoded.truncate(combined - 2);
        }
        let dimensions = if tiled {
            decoder.chunk_dimensions()
        } else {
            decoder.chunk_data_dimensions(index as u32)
        };
        let options = DecoderOptions::default()
            .set_max_width(dimensions.0 as usize)
            .set_max_height(dimensions.1 as usize)
            .set_strict_mode(true);
        let mut jpeg = JpegDecoder::new_with_options(ZCursor::new(encoded.as_slice()), options);
        // The mature parser rejects non-8-bit SOF precision itself.
        jpeg.decode_headers()
            .map_err(|e| Error(format!("invalid TIFF JPEG header: {e}")))?;
        let info = jpeg
            .info()
            .ok_or_else(|| Error("TIFF JPEG lacks frame header".into()))?;
        if (u32::from(info.width), u32::from(info.height)) != dimensions
            || usize::from(info.components) != channels
            || jpeg.input_colorspace() != Some(color)
        {
            return Err(Error(
                "TIFF JPEG chunk layout disagrees with IFD dimensions/components/photometric"
                    .into(),
            ));
        }
    }
    snapshot.check_handle(&source).map_err(Error)?;
    Ok(())
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
    /// Native array shape only when all channels share one uniform grid.
    #[must_use]
    pub fn uniform_sample_shape(&self) -> Option<Vec<usize>> {
        (self.sample_layout == SampleLayout::Uniform).then(|| self.sample_shape())
    }
    /// Logical raster/channel shape. For component layouts this is NOT a native
    /// array shape or an allocation size; use read_ycbcr_into and its plane grids.
    #[must_use]
    pub fn sample_shape(&self) -> Vec<usize> {
        if self.channels == 1 {
            self.shape.to_vec()
        } else {
            vec![self.shape[0], self.shape[1], self.channels]
        }
    }
}
