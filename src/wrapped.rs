//! Source-bound gzip/bzip2 file wrappers. Decompression is streamed to a private
//! temporary file under an explicit disk/output bound, never an unbounded RAM
//! buffer. The original source is checked around staging and every frame read.
pub use crate::binary::Error;
pub use crate::transport::{Compression, Limits};
use crate::{PixelBuffer, cbf, edf, native, tiff, transport::PreparedSource};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum Format {
    #[cfg(feature = "containers")]
    Spe,
    #[cfg(feature = "containers")]
    Mrc,
    #[cfg(feature = "containers")]
    Dm3,
    #[cfg(feature = "bruker")]
    Bruker,
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
    Cbf(cbf::ByteOffsetMode),
    Tiff,
}

enum Backend {
    #[cfg(feature = "containers")]
    Spe(crate::spe::Reader),
    #[cfg(feature = "containers")]
    Mrc(Box<crate::mrc::Reader>),
    #[cfg(feature = "containers")]
    Dm3(crate::dm3::Reader),
    #[cfg(feature = "bruker")]
    Bruker(crate::bruker::Reader),
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
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        if !self.valid {
            return None;
        }
        match &self.buffer {
            Buffer::Empty => None,
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
            Buffer::Native(b) => b.shape(),
            Buffer::Tiff(b) => b.shape(),
        }
    }
}

pub enum Metadata<'a> {
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
            #[cfg(feature = "containers")]
            Format::Spe => Backend::Spe(crate::spe::Reader::open(input, image)?),
            #[cfg(feature = "containers")]
            Format::Mrc => Backend::Mrc(Box::new(crate::mrc::Reader::open(input, image)?)),
            #[cfg(feature = "containers")]
            Format::Dm3 => Backend::Dm3(crate::dm3::Reader::open(input, image)?),
            #[cfg(feature = "bruker")]
            Format::Bruker => Backend::Bruker(crate::bruker::Reader::open(input, image)?),
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
            Format::Edf | Format::EdfExternal { .. } => {
                let (external_payloads, external_basenames) = match format {
                    Format::EdfExternal { policy, basenames } => (policy, basenames),
                    _ => (edf::ExternalPayloads::Reject, false),
                };
                Backend::Edf(edf::Reader::open_with_options(
                    input,
                    image,
                    edf::Options {
                        external_payloads,
                        external_basenames,
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
            #[cfg(feature = "containers")]
            Backend::Spe(r) => r.frames(),
            #[cfg(feature = "containers")]
            Backend::Mrc(r) => r.frames(),
            #[cfg(feature = "containers")]
            Backend::Dm3(r) => r.frames(),
            #[cfg(feature = "bruker")]
            Backend::Bruker(r) => r.frames(),
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
            #[cfg(feature = "containers")]
            Backend::Spe(r) => Some(Metadata::Spe(r.metadata())),
            #[cfg(feature = "containers")]
            Backend::Mrc(r) => Some(Metadata::Mrc(r.metadata())),
            #[cfg(feature = "containers")]
            Backend::Dm3(r) => Some(Metadata::Dm3(r.metadata())),
            #[cfg(feature = "bruker")]
            Backend::Bruker(r) => Some(Metadata::Bruker(r.metadata())),
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
                    #[cfg(feature = "containers")]
                    Backend::Spe(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "containers")]
                    Backend::Mrc(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "containers")]
                    Backend::Dm3(r) => r.read_frame_into(index, b)?,
                    #[cfg(feature = "bruker")]
                    Backend::Bruker(r) => r.read_frame_into(b)?,
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
