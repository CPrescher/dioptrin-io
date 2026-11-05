//! Source-bound gzip/bzip2 file wrappers. Decompression is streamed to a private
//! temporary file under an explicit disk/output bound, never an unbounded RAM
//! buffer. The original source is checked around staging and every frame read.
pub use crate::binary::Error;
pub use crate::transport::{Compression, Limits};
use crate::{PixelBuffer, cbf, edf, native, tiff, transport::PreparedSource};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum Format {
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
    pub fn compression(&self) -> Compression {
        self.source.compression()
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        match &self.backend {
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
