//! Source-bound gzip/bzip2 file wrappers. Decompression is streamed to a private
//! temporary file under an explicit disk/output bound, never an unbounded RAM
//! buffer. The original source is checked around staging and every frame read.
pub use crate::binary::Error;
use crate::{PixelBuffer, cbf, edf, native, source::Snapshot, tiff};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub encoded_bytes: u64,
    /// Maximum staged uncompressed file size; a disk bound, not a RAM budget.
    pub source_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            encoded_bytes: 2 * 1024 * 1024 * 1024,
            source_bytes: 8 * 1024 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Bzip2,
}
#[derive(Debug, Clone, Copy)]
pub enum Format {
    Native(native::Format),
    Edf,
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
    _staged: Option<tempfile::NamedTempFile>,
    snapshot: Snapshot,
    source_handle: File,
    compression: Compression,
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
        if transport.encoded_bytes == 0
            || transport.source_bytes == 0
            || transport.source_bytes == u64::MAX
        {
            return Err(Error(
                "wrapper limits must be positive and source_bytes below u64::MAX".into(),
            ));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let mut source_handle = File::open(&snapshot.path)?;
        snapshot.check_handle(&source_handle).map_err(Error)?;
        let mut prefix = [0; 3];
        let n = snapshot.bytes().min(3) as usize;
        source_handle.read_exact(&mut prefix[..n])?;
        source_handle.seek(SeekFrom::Start(0))?;
        let compression = if n >= 2 && prefix[..2] == [31, 139] {
            Compression::Gzip
        } else if n == 3 && prefix == *b"BZh" {
            Compression::Bzip2
        } else {
            Compression::None
        };
        let mut staged = None;
        if compression != Compression::None {
            if snapshot.bytes() > transport.encoded_bytes {
                return Err(Error("wrapped source exceeds encoded file limit".into()));
            }
            let mut temp = tempfile::NamedTempFile::new()?;
            let decoder: Box<dyn Read + '_> = match compression {
                Compression::Gzip => {
                    Box::new(flate2::read::MultiGzDecoder::new(&mut source_handle))
                }
                Compression::Bzip2 => {
                    Box::new(bzip2::read::MultiBzDecoder::new(&mut source_handle))
                }
                Compression::None => unreachable!(),
            };
            let bytes = std::io::copy(
                &mut decoder.take(transport.source_bytes + 1),
                temp.as_file_mut(),
            )?;
            if bytes > transport.source_bytes {
                return Err(Error("wrapper output exceeds staged source limit".into()));
            }
            staged = Some(temp);
        } else if snapshot.bytes() > transport.source_bytes {
            return Err(Error("source exceeds file size limit".into()));
        }
        snapshot.check_handle(&source_handle).map_err(Error)?;
        let input = staged
            .as_ref()
            .map_or(snapshot.path.as_path(), |f| f.path());
        let backend = match format {
            Format::Native(format) => Backend::Native(native::Reader::open(input, format, image)?),
            Format::Edf => Backend::Edf(edf::Reader::open(input, image)?),
            Format::Cbf(mode) => Backend::Cbf(cbf::Reader::open_with_mode(input, image, mode)?),
            Format::Tiff => Backend::Tiff(Box::new(
                tiff::Reader::open(input, tiff).map_err(|e| Error(e.to_string()))?,
            )),
        };
        snapshot.check_handle(&source_handle).map_err(Error)?;
        Ok(Self {
            backend,
            _staged: staged,
            snapshot,
            source_handle,
            compression,
        })
    }
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.compression
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
        self.snapshot
            .check_handle(&self.source_handle)
            .map_err(Error)?;
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
        self.snapshot
            .check_handle(&self.source_handle)
            .map_err(Error)?;
        buffer.valid = true;
        Ok(())
    }
}
