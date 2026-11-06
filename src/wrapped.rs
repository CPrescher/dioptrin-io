//! Source-bound gzip/bzip2 file wrappers. Decompression is streamed to a private
//! temporary file under an explicit disk/output bound, never an unbounded RAM
//! buffer. The original source is checked around staging and every frame read.
pub use crate::binary::Error;
pub use crate::transport::{Compression, Limits};
use crate::{PixelBuffer, cbf, edf, native, tiff, transport::PreparedSource};
use std::path::Path;
mod input;
pub use input::{Input, Provenance};

#[derive(Debug, Clone, Copy)]
pub enum Format {
    #[cfg(feature = "mar345")]
    Mar345,
    #[cfg(feature = "esperanto")]
    Esperanto,
    #[cfg(feature = "fit2d")]
    Fit2d,
    #[cfg(feature = "kcd")]
    Kcd(crate::kcd::Interpretation),
    #[cfg(feature = "mpa")]
    Mpa,
    DetectorTiff(crate::detector_tiff::Format),
    #[cfg(feature = "fixed")]
    Ge,
    #[cfg(feature = "fixed")]
    Pixi,
    #[cfg(feature = "jpeg")]
    Jpeg(crate::jpeg::Limits),
    #[cfg(feature = "jpeg2000")]
    Jpeg2000(crate::jpeg2000::Limits),
    #[cfg(feature = "containers")]
    Spe,
    #[cfg(feature = "containers")]
    Mrc,
    #[cfg(feature = "containers")]
    Dm3,
    #[cfg(feature = "bruker")]
    Bruker,
    #[cfg(feature = "bruker")]
    BrukerLinear,
    #[cfg(feature = "oxford")]
    Oxford,
    #[cfg(feature = "raxis")]
    Raxis(crate::raxis::Interpretation),
    #[cfg(feature = "xsd")]
    Xsd,
    Native(native::Format),
    Raw(native::RawLayout),
    Edf,
    /// Explicit external EDF source convention, including optional relocation.
    EdfExternal {
        policy: edf::ExternalPayloads,
        basenames: bool,
    },
    /// Explicit external-source and declared-window compatibility policies.
    EdfExternalWindow {
        policy: edf::ExternalPayloads,
        basenames: bool,
        window: edf::ExternalWindow,
    },
    Cbf(cbf::ByteOffsetMode),
    Tiff,
    /// Explicit legacy TIFF metadata interpretations; original tags are retained.
    TiffWithOptions(tiff::Options),
}

enum Backend {
    #[cfg(feature = "hdf5")]
    Hdf5(crate::hdf5::typed::Reader),
    #[cfg(feature = "sparse")]
    Sparse(Box<crate::sparse::Reader>),
    #[cfg(feature = "mar345")]
    Mar345(crate::mar345::Reader),
    #[cfg(feature = "esperanto")]
    Esperanto(crate::esperanto::Reader),
    #[cfg(feature = "fit2d")]
    Fit2d(crate::fit2d::Reader),
    #[cfg(feature = "kcd")]
    Kcd(crate::kcd::Reader),
    #[cfg(feature = "mpa")]
    Mpa(crate::mpa::Reader),
    DetectorTiff(Box<crate::detector_tiff::Reader>),
    #[cfg(feature = "fixed")]
    Ge(crate::ge::Reader),
    #[cfg(feature = "fixed")]
    Pixi(crate::pixi::Reader),
    #[cfg(feature = "jpeg")]
    Jpeg(crate::jpeg::Reader),
    #[cfg(feature = "jpeg2000")]
    Jpeg2000(crate::jpeg2000::Reader),
    #[cfg(feature = "containers")]
    Spe(crate::spe::Reader),
    #[cfg(feature = "containers")]
    Mrc(Box<crate::mrc::Reader>),
    #[cfg(feature = "containers")]
    Dm3(crate::dm3::Reader),
    #[cfg(feature = "bruker")]
    Bruker(crate::bruker::Reader),
    #[cfg(feature = "bruker")]
    BrukerLinear(crate::bruker::Reader),
    #[cfg(feature = "oxford")]
    Oxford(crate::oxford::Reader),
    #[cfg(feature = "raxis")]
    Raxis(crate::raxis::Reader),
    #[cfg(feature = "xsd")]
    Xsd(crate::xsd::Reader),
    Native(native::Reader),
    Edf(edf::Reader),
    Cbf(Box<cbf::Reader>),
    Tiff(Box<tiff::Reader>),
}
#[derive(Default)]
enum Buffer {
    #[cfg(feature = "hdf5")]
    Hdf5(crate::hdf5::typed::FrameBuffer),
    #[default]
    Empty,
    #[cfg(any(feature = "jpeg", feature = "jpeg2000"))]
    Raster(crate::raster::FrameBuffer),
    Native(native::FrameBuffer),
    Tiff(tiff::FrameBuffer),
}
#[derive(Default)]
pub struct FrameBuffer {
    buffer: Buffer,
    valid: bool,
}
impl FrameBuffer {
    #[must_use]
    pub fn sample_shape(&self) -> Option<Vec<usize>> {
        if !self.valid {
            return None;
        }
        match &self.buffer {
            Buffer::Empty => None,
            #[cfg(feature = "hdf5")]
            Buffer::Hdf5(b) => b.shape().map(|s| s.to_vec()),
            Buffer::Native(b) => b.shape().map(|s| s.to_vec()),
            Buffer::Tiff(b) => b.sample_shape(),
            #[cfg(any(feature = "jpeg", feature = "jpeg2000"))]
            Buffer::Raster(b) => b.sample_shape(),
        }
    }
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        if !self.valid {
            return None;
        }
        match &self.buffer {
            Buffer::Empty => None,
            #[cfg(feature = "hdf5")]
            Buffer::Hdf5(b) => b.pixels(),
            #[cfg(any(feature = "jpeg", feature = "jpeg2000"))]
            Buffer::Raster(b) => b.pixels(),
            Buffer::Native(b) => b.pixels(),
            Buffer::Tiff(b) => b.pixels(),
        }
    }
    #[must_use]
    pub fn shape(&self) -> Option<[usize; 2]> {
        if !self.valid {
            return None;
        }
        match &self.buffer {
            Buffer::Empty => None,
            #[cfg(feature = "hdf5")]
            Buffer::Hdf5(b) => b.shape(),
            #[cfg(any(feature = "jpeg", feature = "jpeg2000"))]
            Buffer::Raster(b) => b.shape(),
            Buffer::Native(b) => b.shape(),
            Buffer::Tiff(b) => b.shape(),
        }
    }
}

pub enum Metadata<'a> {
    #[cfg(feature = "hdf5")]
    Hdf5(&'a crate::hdf5::typed::Metadata),
    #[cfg(feature = "sparse")]
    Sparse(&'a crate::sparse::Metadata),
    #[cfg(feature = "mar345")]
    Mar345(&'a crate::mar345::Metadata),
    #[cfg(feature = "esperanto")]
    Esperanto(&'a crate::esperanto::Metadata),
    #[cfg(feature = "fit2d")]
    Fit2d(&'a crate::fit2d::Metadata),
    #[cfg(feature = "kcd")]
    Kcd(&'a crate::kcd::Metadata),
    #[cfg(feature = "mpa")]
    Mpa(&'a crate::mpa::Metadata),
    DetectorTiff(crate::detector_tiff::Metadata<'a>),
    #[cfg(feature = "fixed")]
    Ge(&'a crate::ge::Metadata),
    #[cfg(feature = "fixed")]
    Pixi(&'a crate::pixi::Metadata),
    #[cfg(feature = "jpeg")]
    Jpeg(&'a crate::jpeg::Metadata),
    #[cfg(feature = "jpeg2000")]
    Jpeg2000(&'a crate::jpeg2000::Metadata),
    #[cfg(feature = "containers")]
    Spe(&'a crate::spe::Metadata),
    #[cfg(feature = "containers")]
    Mrc(&'a crate::mrc::Metadata),
    #[cfg(feature = "containers")]
    Dm3(&'a crate::dm3::Metadata),
    #[cfg(feature = "bruker")]
    Bruker(&'a crate::bruker::Metadata),
    #[cfg(feature = "oxford")]
    Oxford(&'a crate::oxford::Metadata),
    #[cfg(feature = "raxis")]
    Raxis(&'a crate::raxis::Metadata),
    #[cfg(feature = "xsd")]
    Xsd(&'a crate::xsd::Metadata),
    Native(&'a native::Metadata),
    Edf(&'a edf::Metadata),
    Cbf(&'a cbf::Metadata),
    Tiff(&'a tiff::Metadata),
}
pub struct Reader {
    // Drop the backend before deleting its open staging file (Windows).
    backend: Backend,
    source: std::sync::Arc<PreparedSource>,
}
impl Reader {
    /// Open one explicit backend, detecting only the outer file wrapper by its
    /// magic bytes (not filename). No extension/dialect/format fallback occurs.
    /// # Errors
    /// Source changes, corrupt/truncated wrapper, source/output limits or backend errors.
    pub fn open(
        path: impl AsRef<Path>,
        format: Format,
        transport: Limits,
        image: native::Limits,
        tiff: tiff::Limits,
    ) -> Result<Self, Error> {
        Self::from_prepared(
            std::sync::Arc::new(PreparedSource::open(path, transport)?),
            format,
            transport,
            image,
            tiff,
        )
    }
    /// Capture from the current stream position to EOF. No seek or filename is
    /// required. The origin directory only authorizes relative-link resolution;
    /// it is not a claim that the stream has a backing file.
    /// # Errors
    /// Input I/O, encoded/decoded bounds, source/link contracts or decoder errors.
    pub fn from_reader(
        input: impl std::io::Read,
        origin: Option<&Path>,
        format: Format,
        transport: Limits,
        image: native::Limits,
        tiff: tiff::Limits,
    ) -> Result<Self, Error> {
        Self::from_prepared(
            std::sync::Arc::new(PreparedSource::from_reader(input, origin, transport)?),
            format,
            transport,
            image,
            tiff,
        )
    }
    pub(crate) fn from_prepared(
        source: std::sync::Arc<PreparedSource>,
        format: Format,
        transport: Limits,
        image: native::Limits,
        tiff: tiff::Limits,
    ) -> Result<Self, Error> {
        let input = source.path();
        let backend = match format {
            #[cfg(feature = "mar345")]
            Format::Mar345 => Backend::Mar345(crate::mar345::Reader::open(input, image)?),
            #[cfg(feature = "esperanto")]
            Format::Esperanto => Backend::Esperanto(crate::esperanto::Reader::open(input, image)?),
            #[cfg(feature = "fit2d")]
            Format::Fit2d => Backend::Fit2d(crate::fit2d::Reader::open(input, image)?),
            #[cfg(feature = "kcd")]
            Format::Kcd(interpretation) => Backend::Kcd(
                crate::kcd::Reader::open_with_interpretation(input, image, interpretation)?,
            ),
            #[cfg(feature = "mpa")]
            Format::Mpa => Backend::Mpa(crate::mpa::Reader::open(input, image)?),
            Format::DetectorTiff(format) => Backend::DetectorTiff(Box::new(
                crate::detector_tiff::Reader::open(input, format, tiff)
                    .map_err(|e| Error(e.to_string()))?,
            )),
            #[cfg(feature = "fixed")]
            Format::Ge => Backend::Ge(crate::ge::Reader::open(input, image)?),
            #[cfg(feature = "fixed")]
            Format::Pixi => Backend::Pixi(crate::pixi::Reader::open(input, image)?),
            #[cfg(feature = "jpeg")]
            Format::Jpeg(limits) => Backend::Jpeg(
                crate::jpeg::Reader::open(input, limits).map_err(|e| Error(e.to_string()))?,
            ),
            #[cfg(feature = "jpeg2000")]
            Format::Jpeg2000(limits) => Backend::Jpeg2000(
                crate::jpeg2000::Reader::open(input, limits).map_err(|e| Error(e.to_string()))?,
            ),
            #[cfg(feature = "containers")]
            Format::Spe => Backend::Spe(crate::spe::Reader::open(input, image)?),
            #[cfg(feature = "containers")]
            Format::Mrc => Backend::Mrc(Box::new(crate::mrc::Reader::open(input, image)?)),
            #[cfg(feature = "containers")]
            Format::Dm3 => Backend::Dm3(crate::dm3::Reader::open(input, image)?),
            #[cfg(feature = "bruker")]
            Format::Bruker => Backend::Bruker(crate::bruker::Reader::open(input, image)?),
            #[cfg(feature = "bruker")]
            Format::BrukerLinear => {
                Backend::BrukerLinear(crate::bruker::Reader::open(input, image)?)
            }
            #[cfg(feature = "oxford")]
            Format::Oxford => Backend::Oxford(crate::oxford::Reader::open(input, image)?),
            #[cfg(feature = "raxis")]
            Format::Raxis(interpretation) => {
                Backend::Raxis(crate::raxis::Reader::open_with_options(
                    input,
                    image,
                    crate::raxis::Options {
                        interpretation,
                        ..Default::default()
                    },
                )?)
            }
            #[cfg(feature = "xsd")]
            Format::Xsd => Backend::Xsd(crate::xsd::Reader::open(input, image)?),
            Format::Native(format) => Backend::Native(native::Reader::open(input, format, image)?),
            Format::Raw(layout) => Backend::Native(native::Reader::open_raw(input, layout, image)?),
            Format::Edf | Format::EdfExternal { .. } | Format::EdfExternalWindow { .. } => {
                let (external_payloads, external_basenames, external_window) = match format {
                    Format::EdfExternal { policy, basenames } => {
                        (policy, basenames, edf::ExternalWindow::Exact)
                    }
                    Format::EdfExternalWindow {
                        policy,
                        basenames,
                        window,
                    } => (policy, basenames, window),
                    _ => (
                        edf::ExternalPayloads::Reject,
                        false,
                        edf::ExternalWindow::Exact,
                    ),
                };
                if source.is_stream()
                    && source.source_directory().is_none()
                    && external_payloads != edf::ExternalPayloads::Reject
                {
                    return Err(Error(
                        "external EDF stream policy requires an explicit origin directory".into(),
                    ));
                }
                Backend::Edf(edf::Reader::open_with_options(
                    input,
                    image,
                    edf::Options {
                        external_payloads,
                        external_basenames,
                        external_window,
                        source_directory: source.source_directory().map(Path::to_path_buf),
                        transport,
                        ..edf::Options::default()
                    },
                )?)
            }
            Format::Cbf(mode) => {
                Backend::Cbf(Box::new(cbf::Reader::open_with_mode(input, image, mode)?))
            }
            Format::Tiff | Format::TiffWithOptions(_) => Backend::Tiff(Box::new(
                tiff::Reader::open_with_options(
                    input,
                    tiff,
                    match format {
                        Format::TiffWithOptions(options) => options,
                        _ => Default::default(),
                    },
                )
                .map_err(|e| Error(e.to_string()))?,
            )),
        };
        source.check()?;
        Ok(Self { backend, source })
    }
    #[must_use]
    pub fn source_paths(&self) -> Vec<&Path> {
        let mut paths = vec![self.source.original_path()];
        if let Backend::Edf(reader) = &self.backend {
            paths.extend(
                reader
                    .payload_sources()
                    .iter()
                    .skip(1)
                    .map(|s| s.path.as_path()),
            );
        }
        #[cfg(feature = "hdf5")]
        if let Backend::Hdf5(reader) = &self.backend {
            paths.extend(reader.sources().iter().skip(1).map(|s| s.path.as_path()));
        }
        #[cfg(feature = "sparse")]
        if let Backend::Sparse(reader) = &self.backend {
            paths.extend(reader.sources().iter().skip(1).map(|s| s.path.as_path()));
        }
        paths
    }
    /// Captured bytes stay distinct from a caller-claimed origin directory.
    /// # Errors
    /// Original or staged source changed.
    pub fn provenance(&self) -> Result<Provenance, Error> {
        self.source.check()?;
        Ok(Provenance {
            stream: self.source.is_stream(),
            origin_directory: self.source.source_directory().map(Path::to_path_buf),
            encoded_bytes: self.source.encoded_bytes(),
            decoded_bytes: self.source.bytes(),
            compression: self.source.compression(),
        })
    }
    #[cfg(feature = "hdf5")]
    #[must_use]
    pub fn hdf5_reader(&self) -> Option<&crate::hdf5::typed::Reader> {
        if let Backend::Hdf5(r) = &self.backend {
            Some(r)
        } else {
            None
        }
    }
    #[cfg(feature = "sparse")]
    #[must_use]
    pub fn sparse_reader(&self) -> Option<&crate::sparse::Reader> {
        if let Backend::Sparse(r) = &self.backend {
            Some(r)
        } else {
            None
        }
    }
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.source.compression()
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        match &self.backend {
            #[cfg(feature = "hdf5")]
            Backend::Hdf5(r) => r.frames(),
            #[cfg(feature = "sparse")]
            Backend::Sparse(r) => r.frames(),
            #[cfg(feature = "mar345")]
            Backend::Mar345(r) => r.frames(),
            #[cfg(feature = "esperanto")]
            Backend::Esperanto(r) => r.frames(),
            #[cfg(feature = "fit2d")]
            Backend::Fit2d(r) => r.frames(),
            #[cfg(feature = "kcd")]
            Backend::Kcd(r) => r.frames(),
            #[cfg(feature = "mpa")]
            Backend::Mpa(r) => r.frames(),
            Backend::DetectorTiff(r) => r.frames(),
            #[cfg(feature = "fixed")]
            Backend::Ge(r) => r.frames(),
            #[cfg(feature = "fixed")]
            Backend::Pixi(r) => r.frames(),
            #[cfg(feature = "jpeg")]
            Backend::Jpeg(r) => r.frames(),
            #[cfg(feature = "jpeg2000")]
            Backend::Jpeg2000(r) => r.frames(),
            #[cfg(feature = "containers")]
            Backend::Spe(r) => r.frames(),
            #[cfg(feature = "containers")]
            Backend::Mrc(r) => r.frames(),
            #[cfg(feature = "containers")]
            Backend::Dm3(r) => r.frames(),
            #[cfg(feature = "bruker")]
            Backend::Bruker(r) | Backend::BrukerLinear(r) => r.frames(),
            #[cfg(feature = "oxford")]
            Backend::Oxford(r) => r.frames(),
            #[cfg(feature = "raxis")]
            Backend::Raxis(r) => r.frames(),
            #[cfg(feature = "xsd")]
            Backend::Xsd(r) => r.frames(),
            Backend::Native(r) => r.frames(),
            Backend::Edf(r) => r.frames(),
            Backend::Cbf(_) => 1,
            Backend::Tiff(r) => r.frames(),
        }
    }
    #[must_use]
    pub fn metadata(&self, index: usize) -> Option<Metadata<'_>> {
        if index >= self.frames() {
            return None;
        }
        match &self.backend {
            #[cfg(feature = "hdf5")]
            Backend::Hdf5(r) => r.metadata(index).map(Metadata::Hdf5),
            #[cfg(feature = "sparse")]
            Backend::Sparse(r) => Some(Metadata::Sparse(r.metadata())),
            #[cfg(feature = "mar345")]
            Backend::Mar345(r) => Some(Metadata::Mar345(r.metadata())),
            #[cfg(feature = "esperanto")]
            Backend::Esperanto(r) => Some(Metadata::Esperanto(r.metadata())),
            #[cfg(feature = "fit2d")]
            Backend::Fit2d(r) => Some(Metadata::Fit2d(r.metadata())),
            #[cfg(feature = "kcd")]
            Backend::Kcd(r) => Some(Metadata::Kcd(r.metadata())),
            #[cfg(feature = "mpa")]
            Backend::Mpa(r) => Some(Metadata::Mpa(r.metadata())),
            Backend::DetectorTiff(r) => r.metadata(index).map(Metadata::DetectorTiff),
            #[cfg(feature = "fixed")]
            Backend::Ge(r) => Some(Metadata::Ge(r.metadata())),
            #[cfg(feature = "fixed")]
            Backend::Pixi(r) => Some(Metadata::Pixi(r.metadata())),
            #[cfg(feature = "jpeg")]
            Backend::Jpeg(r) => Some(Metadata::Jpeg(r.metadata())),
            #[cfg(feature = "jpeg2000")]
            Backend::Jpeg2000(r) => Some(Metadata::Jpeg2000(r.metadata())),
            #[cfg(feature = "containers")]
            Backend::Spe(r) => Some(Metadata::Spe(r.metadata())),
            #[cfg(feature = "containers")]
            Backend::Mrc(r) => Some(Metadata::Mrc(r.metadata())),
            #[cfg(feature = "containers")]
            Backend::Dm3(r) => Some(Metadata::Dm3(r.metadata())),
            #[cfg(feature = "bruker")]
            Backend::Bruker(r) | Backend::BrukerLinear(r) => Some(Metadata::Bruker(r.metadata())),
            #[cfg(feature = "oxford")]
            Backend::Oxford(r) => Some(Metadata::Oxford(r.metadata())),
            #[cfg(feature = "raxis")]
            Backend::Raxis(r) => Some(Metadata::Raxis(r.metadata())),
            #[cfg(feature = "xsd")]
            Backend::Xsd(r) => Some(Metadata::Xsd(r.metadata())),
            Backend::Native(r) => Some(Metadata::Native(r.metadata())),
            Backend::Edf(r) => r.metadata(index).map(Metadata::Edf),
            Backend::Cbf(r) => Some(Metadata::Cbf(r.metadata())),
            Backend::Tiff(r) => r.metadata(index).map(Metadata::Tiff),
        }
    }
    /// Read an explicitly selected TIFF consumer view. Stored-reader defaults
    /// and metadata remain unchanged; transformations are carried by the image.
    /// # Errors
    /// Non-TIFF backend, unsupported view/feature, bounds or changed sources.
    pub fn read_tiff_view(
        &mut self,
        index: usize,
        policy: tiff::view::Policy,
        max_output_bytes: usize,
    ) -> Result<tiff::view::Image, Error> {
        self.source.check()?;
        let Backend::Tiff(reader) = &mut self.backend else {
            return Err(Error(
                "TIFF consumer view requires the generic TIFF backend".into(),
            ));
        };
        let image = match policy {
            #[cfg(feature = "jpeg")]
            tiff::view::Policy::Pillow13 => {
                tiff::view::read_pillow13(reader, index, max_output_bytes)
            }
            _ => {
                if policy == tiff::view::Policy::Pillow13
                    && reader
                        .metadata(index)
                        .is_some_and(|metadata| metadata.uniform_sample_shape().is_none())
                {
                    let mut components = tiff::ycbcr::Buffer::default();
                    reader
                        .read_ycbcr_into(index, &mut components)
                        .map_err(|error| Error(error.to_string()))?;
                    tiff::view::convert_ycbcr(
                        reader.metadata(index).unwrap(),
                        &components,
                        policy,
                        max_output_bytes,
                    )
                } else {
                    let mut frame = tiff::FrameBuffer::default();
                    reader
                        .read_frame_into(index, &mut frame)
                        .map_err(|e| Error(e.to_string()))?;
                    tiff::view::convert(
                        reader
                            .metadata(index)
                            .ok_or_else(|| Error("TIFF view frame outside source".into()))?,
                        &frame,
                        policy,
                        max_output_bytes,
                    )
                }
            }
        }
        .map_err(|e| Error(e.to_string()))?;
        self.source.check()?;
        Ok(image)
    }

    /// Read original TIFF Y/Cb/Cr grids without chroma expansion. The original
    /// wrapper and decoded source are checked around each component read.
    /// # Errors
    /// Non-TIFF backend, unsupported component storage, bounds or changed source.
    pub fn read_tiff_ycbcr_into(
        &mut self,
        index: usize,
        buffer: &mut tiff::ycbcr::Buffer,
    ) -> Result<(), Error> {
        let result = (|| {
            self.source.check()?;
            let Backend::Tiff(reader) = &mut self.backend else {
                return Err(Error(
                    "TIFF component read requires the generic TIFF backend".into(),
                ));
            };
            reader
                .read_ycbcr_into(index, buffer)
                .map_err(|error| Error(error.to_string()))?;
            self.source.check()?;
            Ok(())
        })();
        if result.is_err() {
            *buffer = tiff::ycbcr::Buffer::default();
        }
        result
    }

    /// Read only complete ThunderScan rows, retaining explicit missing ranges.
    /// Original and staged sources are checked around every read.
    /// # Errors
    /// Non-TIFF backend, wrong codec, resource bounds or changed source.
    pub fn read_tiff_thunderscan_available_into(
        &mut self,
        index: usize,
        buffer: &mut tiff::thunderscan::AvailableFrame,
    ) -> Result<(), Error> {
        let result = (|| {
            self.source.check()?;
            let Backend::Tiff(reader) = &mut self.backend else {
                return Err(Error(
                    "available TIFF rows require the generic TIFF backend".into(),
                ));
            };
            reader
                .read_thunderscan_available_into(index, buffer)
                .map_err(|error| Error(error.to_string()))?;
            self.source.check()?;
            Ok(())
        })();
        if result.is_err() {
            *buffer = tiff::thunderscan::AvailableFrame::default();
        }
        result
    }

    /// Original legacy JPEG source windows and header interpretation, when the
    /// explicitly selected generic TIFF backend admits such a page.
    #[cfg(feature = "jpeg")]
    #[must_use]
    pub fn tiff_ojpeg_metadata(&self, index: usize) -> Option<&tiff::ojpeg::SourceMetadata> {
        match &self.backend {
            Backend::Tiff(reader) => reader.ojpeg_metadata(index),
            _ => None,
        }
    }

    /// Read through the selected backend; original wrapper mutation invalidates
    /// even an already staged source. No decoded counts are cached here.
    /// # Errors
    /// Invalid frame, changed original/staged source or backend decoding failure.
    pub fn read_frame_into(&mut self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.valid = false;
        self.source.check()?;
        if index >= self.frames() {
            return Err(Error("wrapped frame index outside source".into()));
        }
        match &mut self.backend {
            #[cfg(feature = "hdf5")]
            Backend::Hdf5(r) => {
                if !matches!(buffer.buffer, Buffer::Hdf5(_)) {
                    buffer.buffer = Buffer::Hdf5(crate::hdf5::typed::FrameBuffer::default());
                }
                let Buffer::Hdf5(b) = &mut buffer.buffer else {
                    unreachable!()
                };
                r.read_frame_into(index, b)
                    .map_err(|e| Error(e.to_string()))?;
            }
            #[cfg(feature = "jpeg")]
            Backend::Jpeg(r) => {
                if !matches!(buffer.buffer, Buffer::Raster(_)) {
                    buffer.buffer = Buffer::Raster(crate::raster::FrameBuffer::default());
                }
                let Buffer::Raster(b) = &mut buffer.buffer else {
                    unreachable!()
                };
                r.read_frame_into(b).map_err(|e| Error(e.to_string()))?;
            }
            #[cfg(feature = "jpeg2000")]
            Backend::Jpeg2000(r) => {
                if !matches!(buffer.buffer, Buffer::Raster(_)) {
                    buffer.buffer = Buffer::Raster(crate::raster::FrameBuffer::default());
                }
                let Buffer::Raster(b) = &mut buffer.buffer else {
                    unreachable!()
                };
                r.read_frame_into(b).map_err(|e| Error(e.to_string()))?;
            }
            Backend::DetectorTiff(r) => {
                if !matches!(buffer.buffer, Buffer::Tiff(_)) {
                    buffer.buffer = Buffer::Tiff(tiff::FrameBuffer::default());
                }
                let Buffer::Tiff(b) = &mut buffer.buffer else {
                    unreachable!()
                };
                r.read_frame_into(index, b)
                    .map_err(|e| Error(e.to_string()))?;
            }
            Backend::Tiff(r) => {
                if !matches!(buffer.buffer, Buffer::Tiff(_)) {
                    buffer.buffer = Buffer::Tiff(tiff::FrameBuffer::default());
                }
                let Buffer::Tiff(b) = &mut buffer.buffer else {
                    unreachable!()
                };
                r.read_frame_into(index, b)
                    .map_err(|e| Error(e.to_string()))?;
            }
            backend => {
                if !matches!(buffer.buffer, Buffer::Native(_)) {
                    buffer.buffer = Buffer::Native(native::FrameBuffer::default());
                }
                let Buffer::Native(b) = &mut buffer.buffer else {
                    unreachable!()
                };
                match backend {
                    #[cfg(feature = "hdf5")]
                    Backend::Hdf5(_) => unreachable!(),
                    #[cfg(feature = "sparse")]
                    Backend::Sparse(r) => r
                        .read_frame_into(index, b)
                        .map_err(|e| Error(e.to_string()))?,
                    #[cfg(feature = "mar345")]
                    Backend::Mar345(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "esperanto")]
                    Backend::Esperanto(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "fit2d")]
                    Backend::Fit2d(r) => r.read_frame_into(b)?,
                    #[cfg(feature = "kcd")]
                    Backend::Kcd(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "mpa")]
                    Backend::Mpa(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "fixed")]
                    Backend::Ge(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "fixed")]
                    Backend::Pixi(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "jpeg")]
                    Backend::Jpeg(_) => unreachable!(),
                    #[cfg(feature = "jpeg2000")]
                    Backend::Jpeg2000(_) => unreachable!(),
                    Backend::DetectorTiff(_) => unreachable!(),
                    #[cfg(feature = "containers")]
                    Backend::Spe(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "containers")]
                    Backend::Mrc(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "containers")]
                    Backend::Dm3(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "bruker")]
                    Backend::Bruker(r) => r.read_frame_into(b)?,
                    #[cfg(feature = "bruker")]
                    Backend::BrukerLinear(r) => crate::bruker::view::read_linear_into(r, b)?,
                    #[cfg(feature = "oxford")]
                    Backend::Oxford(r) => r.read_frame_into(b)?,
                    #[cfg(feature = "raxis")]
                    Backend::Raxis(r) => r.read_frame_into(b)?,
                    #[cfg(feature = "xsd")]
                    Backend::Xsd(r) => r.read_frame_into(b)?,
                    Backend::Native(r) => r.read_frame_into(index, b)?,
                    Backend::Edf(r) => r.read_frame_into(index, b)?,
                    Backend::Cbf(r) => r.read_frame_into(b)?,
                    Backend::Tiff(_) => unreachable!(),
                }
            }
        }
        self.source.check()?;
        buffer.valid = true;
        Ok(())
    }
}

#[cfg(feature = "fit2d")]
impl Reader {
    /// Read one original FIT2D data/mask/axis array from the indexed backend,
    /// including through an outer compressed source. No arrays are combined.
    /// # Errors
    /// Non-FIT2D backend, invalid index, resource limits or changed sources.
    pub fn read_fit2d_array_into(
        &self,
        index: usize,
        buffer: &mut crate::fit2d::FrameBuffer,
    ) -> Result<(), Error> {
        buffer.shape = None;
        self.source.check()?;
        let Backend::Fit2d(reader) = &self.backend else {
            return Err(Error(
                "FIT2D array access requires the FIT2D backend".into(),
            ));
        };
        reader.read_array_into(index, buffer)?;
        if let Err(error) = self.source.check() {
            buffer.shape = None;
            return Err(error);
        }
        Ok(())
    }
}

impl Reader {
    /// Frozen EDF general headers from the indexed source, separate from inherited
    /// per-frame fields. Outer source identity is validated before access.
    /// # Errors
    /// Non-EDF backend or changed outer source.
    pub fn edf_general_headers(&self) -> Result<&[crate::edf::GeneralHeader], Error> {
        self.source.check()?;
        let Backend::Edf(reader) = &self.backend else {
            return Err(Error(
                "EDF general-header access requires EDF backend".into(),
            ));
        };
        Ok(reader.general_headers())
    }
}

/// Metadata indexing independent of image availability, including outer wrappers.
#[derive(Clone, Copy, Debug)]
pub enum HeaderFormat {
    Edf,
    Dtrek,
    Tiff,
}
pub enum HeaderMetadata<'a> {
    Edf(EdfHeaders<'a>),
    Dtrek(&'a native::Metadata),
    Tiff(&'a [tiff::header::Page]),
}
enum HeaderBackend {
    Edf(Box<edf::recovery::Reader>),
    Dtrek(native::DtrekHeader),
    Tiff(tiff::header::Reader),
}
/// Borrowed source headers only. The underlying recovery index and its sample
/// methods remain private; dimensions describe declarations, not available counts.
pub struct EdfHeaders<'a> {
    reader: &'a edf::recovery::Reader,
}
impl EdfHeaders<'_> {
    pub fn images(&self) -> impl ExactSizeIterator<Item = &edf::Metadata> {
        (0..self.reader.image_records())
            .map(|i| self.reader.metadata(i).expect("indexed EDF metadata"))
    }
    #[must_use]
    pub fn general_headers(&self) -> &[edf::GeneralHeader] {
        self.reader.general_headers()
    }
}
/// No frame/pixel API is provided: these are original declarations, not evidence
/// that a codec can decode the payload or that all declared counts exist.
pub struct HeaderReader {
    backend: HeaderBackend,
    source: std::sync::Arc<PreparedSource>,
}
impl HeaderReader {
    /// Index headers with the existing bounded transport and format parsers.
    /// # Errors
    /// Malformed/truncated metadata, source mutation, transport or resource bounds.
    pub fn open(
        path: impl AsRef<Path>,
        format: HeaderFormat,
        transport: Limits,
        image: native::Limits,
        tiff: tiff::Limits,
    ) -> Result<Self, Error> {
        Self::from_prepared(
            std::sync::Arc::new(PreparedSource::open(path, transport)?),
            format,
            image,
            tiff,
        )
    }
    /// Capture a nonseekable stream for header-only access without pixel decoding.
    /// # Errors
    /// Transport I/O/bounds, malformed header or changed capture.
    pub fn from_reader(
        input: impl std::io::Read,
        format: HeaderFormat,
        transport: Limits,
        image: native::Limits,
        tiff: tiff::Limits,
    ) -> Result<Self, Error> {
        Self::from_prepared(
            std::sync::Arc::new(PreparedSource::from_reader(input, None, transport)?),
            format,
            image,
            tiff,
        )
    }
    fn from_prepared(
        source: std::sync::Arc<PreparedSource>,
        format: HeaderFormat,
        image: native::Limits,
        tiff: tiff::Limits,
    ) -> Result<Self, Error> {
        let backend = match format {
            HeaderFormat::Edf => {
                let reader = edf::recovery::Reader::open(source.path(), image, Default::default())?;
                if reader.image_records() == 0 && reader.general_headers().is_empty() {
                    return Err(Error("source has no admitted EDF header".into()));
                }
                HeaderBackend::Edf(Box::new(reader))
            }
            HeaderFormat::Dtrek => {
                HeaderBackend::Dtrek(native::DtrekHeader::open(source.path(), image)?)
            }
            HeaderFormat::Tiff => HeaderBackend::Tiff(
                tiff::header::Reader::open(source.path(), tiff)
                    .map_err(|e| Error(e.to_string()))?,
            ),
        };
        source.check()?;
        Ok(Self { backend, source })
    }
    /// # Errors
    /// Original or staged source changed since indexing.
    pub fn metadata(&self) -> Result<HeaderMetadata<'_>, Error> {
        self.source.check()?;
        match &self.backend {
            HeaderBackend::Edf(r) => Ok(HeaderMetadata::Edf(EdfHeaders { reader: r })),
            HeaderBackend::Dtrek(r) => Ok(HeaderMetadata::Dtrek(r.metadata()?)),
            HeaderBackend::Tiff(r) => Ok(HeaderMetadata::Tiff(
                r.pages().map_err(|e| Error(e.to_string()))?,
            )),
        }
    }
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.source.compression()
    }
    /// Original first 64 unwrapped source bytes, without interpreting sample bits.
    /// # Errors
    /// Source changed or failed while reading the bounded prefix.
    pub fn source_prefix(&self) -> Result<Vec<u8>, Error> {
        use std::io::Read;
        self.metadata()?;
        let mut prefix = Vec::new();
        std::fs::File::open(self.source.path())?
            .take(64)
            .read_to_end(&mut prefix)?;
        self.metadata()?;
        Ok(prefix)
    }
}
