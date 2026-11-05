//! Source-bound gzip/bzip2 file wrappers. Decompression is streamed to a private
//! temporary file under an explicit disk/output bound, never an unbounded RAM
//! buffer. The original source is checked around staging and every frame read.
pub use crate::binary::Error;
pub use crate::transport::{Compression, Limits};
use crate::{PixelBuffer, cbf, edf, native, tiff, transport::PreparedSource};
use std::path::Path;

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
}

enum Backend {
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
    Cbf(cbf::Reader),
    Tiff(Box<tiff::Reader>),
}
#[derive(Default)]
enum Buffer {
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
            #[cfg(any(feature = "jpeg", feature = "jpeg2000"))]
            Buffer::Raster(b) => b.shape(),
            Buffer::Native(b) => b.shape(),
            Buffer::Tiff(b) => b.shape(),
        }
    }
}

pub enum Metadata<'a> {
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
    source: PreparedSource,
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
        let source = PreparedSource::open(path, transport)?;
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
                Backend::Edf(edf::Reader::open_with_options(
                    input,
                    image,
                    edf::Options {
                        external_payloads,
                        external_basenames,
                        external_window,
                        source_directory: source.original_path().parent().map(Path::to_path_buf),
                        transport,
                        ..edf::Options::default()
                    },
                )?)
            }
            Format::Cbf(mode) => Backend::Cbf(cbf::Reader::open_with_mode(input, image, mode)?),
            Format::Tiff => Backend::Tiff(Box::new(
                tiff::Reader::open(input, tiff).map_err(|e| Error(e.to_string()))?,
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
        paths
    }
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.source.compression()
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        match &self.backend {
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
        .map_err(|e| Error(e.to_string()))?;
        self.source.check()?;
        Ok(image)
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
