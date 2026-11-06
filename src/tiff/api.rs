//! Low-level TIFF read controls over the existing immutable decoder.
//! Stored data and explicitly requested FabIO display views remain separate.
use super::{Error, FrameBuffer, Limits, Metadata, view};
use crate::{PixelBuffer, PixelType};
use std::{ops::Range, path::Path};

/// Inclusive logical source-row selection. Negative rows never wrap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Rows {
    #[default]
    All,
    Inclusive {
        first: i64,
        last: i64,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewPolicy {
    FabioTiffIo,
    /// Literal pinned TiffIO RGB/palette mono rule: .114*R+.587*G+.299*B,
    /// ignoring components beyond the first three. This is an explicit view.
    FabioTiffIoMono,
}
/// Source metadata is borrowed unchanged, including original typed/opaque tags.
/// Reference dictionary aliases/defaults are presentation, not original tags.
pub struct Info<'a> {
    pub index: usize,
    pub ifd_offset: u64,
    pub images: usize,
    pub source: &'a Metadata,
}
/// Header-only source metadata, without asserting any pixel layout or availability.
pub struct DirectoryInfo<'a> {
    pub index: usize,
    pub images: usize,
    pub source: &'a super::header::Page,
}
/// Metadata controls independent of the sample decoder's admitted layouts.
pub struct MetadataReader {
    inner: super::header::Reader,
}
impl MetadataReader {
    /// # Errors
    /// Nonregular/invalid TIFF metadata, source changes or resource bounds.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        Ok(Self {
            inner: super::header::Reader::open(path, limits)?,
        })
    }
    /// # Errors
    /// Original source or caller path binding changed.
    pub fn get_number_of_images(&self) -> Result<usize, Error> {
        Ok(self.inner.pages()?.len())
    }
    /// Borrowed iterator: no duplicate retained offset vector is allocated.
    /// # Errors
    /// Original source or caller path binding changed.
    pub fn get_image_file_directories(
        &self,
    ) -> Result<impl ExactSizeIterator<Item = u64> + '_, Error> {
        Ok(self.inner.pages()?.iter().map(|page| page.ifd_offset))
    }
    /// # Errors
    /// Invalid signed index or changed original source.
    pub fn get_info(&self, index: i64) -> Result<DirectoryInfo<'_>, Error> {
        let pages = self.inner.pages()?;
        let index = usize::try_from(index)
            .ok()
            .filter(|&i| i < pages.len())
            .ok_or_else(|| {
                Error("TIFF metadata API image index outside immutable source".into())
            })?;
        Ok(DirectoryInfo {
            index,
            images: pages.len(),
            source: &pages[index],
        })
    }
    pub fn close(self) {
        drop(self);
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    SelectSourceRows { first: usize, last: usize },
    FabioBgrLuminanceF32,
    IgnoreExtraComponents { components: usize },
}
pub struct View {
    pub image: view::Image,
    pub rows: Range<usize>,
    pub ifd_offset: u64,
    pub policy: ViewPolicy,
    /// Additional transformations after the existing image-view provenance.
    pub operations: Vec<Operation>,
}
/// No decoded cache or live refresh. Reuses the mature primary IFD/sample reader.
pub struct Reader {
    inner: super::Reader,
    limits: Limits,
}
impl Reader {
    /// # Errors
    /// Nonregular/malformed/unsupported TIFF, source changes or resource bounds.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        Self::open_with_options(path, limits, super::Options::default())
    }
    /// Reuse an explicitly chosen existing source-window interpretation. There
    /// is no retry after strict failure; original tags and interpretation remain
    /// available in Info.source, and strict open() is unchanged.
    /// # Errors
    /// Source/layout/resource failures under the selected compatibility policy.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        limits: Limits,
        options: super::Options,
    ) -> Result<Self, Error> {
        Ok(Self {
            inner: super::Reader::open_with_options(path, limits, options)?,
            limits,
        })
    }
    fn check(&self) -> Result<(), Error> {
        self.inner
            .snapshot
            .check_handle(&self.inner.identity_handle)
            .map_err(Error)
    }
    fn index(&self, index: i64) -> Result<usize, Error> {
        self.check()?;
        usize::try_from(index)
            .ok()
            .filter(|&i| i < self.inner.frames())
            .ok_or_else(|| Error("TIFF API image index outside immutable source".into()))
    }
    /// Frozen number of primary IFD images; rejects source growth/replacement.
    /// # Errors
    /// Source identity or caller path binding changed.
    pub fn get_number_of_images(&self) -> Result<usize, Error> {
        self.check()?;
        Ok(self.inner.frames())
    }
    /// Original primary IFD offsets, in chain order. No rescan or SubIFD guessing.
    /// # Errors
    /// Source identity or caller path binding changed.
    pub fn get_image_file_directories(&self) -> Result<&[u64], Error> {
        self.check()?;
        Ok(&self.inner.ifd_offsets)
    }
    /// # Errors
    /// Invalid signed index or changed source.
    pub fn get_info(&self, index: i64) -> Result<Info<'_>, Error> {
        let index = self.index(index)?;
        Ok(Info {
            index,
            ifd_offset: self.inner.ifd_offsets[index],
            images: self.inner.frames(),
            source: &self.inner.pages[index],
        })
    }
    /// Original stored words in selected source-row order; palettes remain
    /// indices and Orientation remains metadata. Component grids require the
    /// existing component API rather than fake full-resolution channel arrays.
    /// A full bounded frame is decoded before in-place cropping; selecting fewer
    /// rows does not reduce decoder admission or establish partial I/O speed.
    /// # Errors
    /// Invalid index/rows, changed source, resource or codec/layout failure.
    pub fn get_data_into(
        &mut self,
        index: i64,
        rows: Rows,
        buffer: &mut FrameBuffer,
    ) -> Result<(), Error> {
        buffer.shape = None;
        let result = (|| {
            let index = self.index(index)?;
            let range = row_range(rows, self.inner.pages[index].shape[0])?;
            self.inner.read_frame_into(index, buffer)?;
            crop(
                &mut buffer.pixels,
                &range,
                buffer.shape.unwrap()[1],
                buffer.channels,
            )?;
            buffer.shape = Some([range.len(), self.inner.pages[index].shape[1]]);
            self.check()
        })();
        if result.is_err() {
            buffer.shape = None;
        }
        result
    }
    /// Alias for the stored-data read, without implicit display conversion.
    /// # Errors
    /// Same source/selection/resource/codec failures as get_data_into.
    pub fn get_image_into(
        &mut self,
        index: i64,
        rows: Rows,
        buffer: &mut FrameBuffer,
    ) -> Result<(), Error> {
        self.get_data_into(index, rows, buffer)
    }
    /// Explicit reference display view. Palette lookup reuses `view::convert`;
    /// mono/crop are consumer transformations with separate provenance. The
    /// simultaneous source/output buffers are charged to frame_bytes; decoder
    /// scratch remains separately bounded by intermediate_bytes.
    /// # Errors
    /// Source/selection/codec failure, unsupported view or aggregate byte limit.
    pub fn read_view(&mut self, index: i64, rows: Rows, policy: ViewPolicy) -> Result<View, Error> {
        let index = self.index(index)?;
        let metadata = &self.inner.pages[index];
        let range = row_range(rows, metadata.shape[0])?;
        let source_bytes = metadata.shape[0]
            .checked_mul(metadata.shape[1])
            .and_then(|n| n.checked_mul(metadata.channels))
            .and_then(|n| n.checked_mul(metadata.pixel_type.bytes()))
            .ok_or_else(|| Error("TIFF API source size overflow".into()))?;
        let output_limit = self
            .limits
            .frame_bytes
            .checked_sub(source_bytes)
            .filter(|&n| n > 0)
            .ok_or_else(|| Error("TIFF API source plus view exceeds frame byte limit".into()))?;
        let mut raw = FrameBuffer::default();
        self.inner.read_frame_into(index, &mut raw)?;
        let metadata = &self.inner.pages[index];
        let mut image = view::convert(metadata, &raw, view::Policy::FabioTiffIo, output_limit)?;
        drop(raw);
        let mut operations = Vec::new();
        if policy == ViewPolicy::FabioTiffIoMono && image.provenance.shape.len() == 3 {
            let components = image.provenance.shape[2];
            if components < 3 {
                return Err(Error(
                    "TIFF API mono view requires at least three components".into(),
                ));
            }
            let view::Pixels::Scalar(pixels) = &image.pixels else {
                return Err(Error(
                    "TIFF API mono source must be scalar components".into(),
                ));
            };
            let converted = mono(pixels, components, self.limits.frame_bytes)?;
            image.pixels = view::Pixels::Scalar(converted);
            image.provenance.shape.pop();
            image.provenance.output_type = view::OutputType::Scalar(PixelType::F32);
            operations.push(Operation::FabioBgrLuminanceF32);
            if components > 3 {
                operations.push(Operation::IgnoreExtraComponents {
                    components: components - 3,
                });
            }
        }
        let width = image.provenance.shape[1];
        let components = image.provenance.shape.get(2).copied().unwrap_or(1);
        let view::Pixels::Scalar(pixels) = &mut image.pixels else {
            return Err(Error("TIFF API view did not produce scalar samples".into()));
        };
        crop(pixels, &range, width, components)?;
        if range.start != 0 || range.end != metadata.shape[0] {
            operations.push(Operation::SelectSourceRows {
                first: range.start,
                last: range.end - 1,
            });
        }
        image.provenance.shape[0] = range.len();
        self.check()?;
        Ok(View {
            image,
            rows: range,
            ifd_offset: self.inner.ifd_offsets[index],
            policy,
            operations,
        })
    }
    /// Return the existing reader for specialized component/metadata APIs.
    #[must_use]
    pub fn into_inner(self) -> super::Reader {
        self.inner
    }
    /// Release the owned decoder and source handles. Rust prevents later reads.
    pub fn close(self) {
        drop(self);
    }
}
fn row_range(rows: Rows, height: usize) -> Result<Range<usize>, Error> {
    match rows {
        Rows::All => Ok(0..height),
        Rows::Inclusive { first, last } => {
            let first = usize::try_from(first)
                .map_err(|_| Error("TIFF API negative/unrepresentable first row".into()))?;
            let last = usize::try_from(last)
                .map_err(|_| Error("TIFF API negative/unrepresentable last row".into()))?;
            if first > last || last >= height {
                return Err(Error(
                    "TIFF API inclusive row selection outside source".into(),
                ));
            }
            Ok(first..last + 1)
        }
    }
}
fn crop(
    pixels: &mut PixelBuffer,
    rows: &Range<usize>,
    width: usize,
    components: usize,
) -> Result<(), Error> {
    let stride = width
        .checked_mul(components)
        .ok_or_else(|| Error("TIFF API row stride overflow".into()))?;
    let first = rows
        .start
        .checked_mul(stride)
        .ok_or_else(|| Error("TIFF API row offset overflow".into()))?;
    let last = rows
        .end
        .checked_mul(stride)
        .filter(|&n| n <= pixels.len())
        .ok_or_else(|| Error("TIFF API row window exceeds decoded samples".into()))?;
    macro_rules! select {($($kind:ident);*)=>{match pixels {$(PixelBuffer::$kind(v)=>{v.copy_within(first..last,0);v.truncate(last-first);}),*}};}
    select!(U8;U16;U32;U64;I8;I16;I32;I64;F32;F64;C32;C64;CI16);
    Ok(())
}
fn mono(pixels: &PixelBuffer, components: usize, budget: usize) -> Result<PixelBuffer, Error> {
    if !pixels.len().is_multiple_of(components) {
        return Err(Error("TIFF API component count mismatch".into()));
    }
    let count = pixels.len() / components;
    pixels
        .len()
        .checked_mul(pixels.pixel_type().bytes())
        .and_then(|n| count.checked_mul(4).and_then(|m| n.checked_add(m)))
        .filter(|&n| n <= budget)
        .ok_or_else(|| Error("TIFF API mono input plus output exceeds frame byte limit".into()))?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|e| Error(e.to_string()))?;
    macro_rules! luminance {($($kind:ident);*)=>{match pixels {
        $(PixelBuffer::$kind(v)=>{for p in v.chunks_exact(components){output.push((p[0] as f64*0.114+p[1] as f64*0.587+p[2] as f64*0.299) as f32);}}),*,
        PixelBuffer::F32(v)=>{for p in v.chunks_exact(components){output.push(p[0]*0.114f32+p[1]*0.587f32+p[2]*0.299f32);}},
        _=>return Err(Error("TIFF API mono view does not admit complex components".into()))
    }};}
    luminance!(U8;U16;U32;U64;I8;I16;I32;I64;F64);
    Ok(PixelBuffer::F32(output))
}
