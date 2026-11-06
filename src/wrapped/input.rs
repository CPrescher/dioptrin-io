use super::{Error, Format, Limits, Reader, native, tiff};
use crate::{
    discovery::{self, Codec, Detection},
    transport::PreparedSource,
};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
/// Capture provenance. A stream's directory is supplied by its caller, never
/// inferred from a Read object's name or a private staging filename. An origin
/// authorizes relative resolution; external links still require their explicit
/// backend policy and their complete source graph is frozen when opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub stream: bool,
    pub origin_directory: Option<PathBuf>,
    pub encoded_bytes: u64,
    pub decoded_bytes: u64,
    pub compression: super::Compression,
}
/// Owns bounded immutable input before a decoder is selected. Dropping this
/// object cleans up all private staging files. Opening consumes it so the reader
/// keeps its complete capture alive until after the decoder is dropped.
#[derive(Clone)]
pub struct Input {
    pub(super) source: std::sync::Arc<PreparedSource>,
    transport: Limits,
}
impl Input {
    /// Prepare a path using its actual parent as the relative-link origin.
    /// # Errors
    /// Source mutation, invalid transport or I/O/output bounds.
    pub fn open(path: impl AsRef<Path>, transport: Limits) -> Result<Self, Error> {
        Ok(Self {
            source: std::sync::Arc::new(PreparedSource::open(path, transport)?),
            transport,
        })
    }
    /// Capture current position through EOF without seeking or retaining input.
    /// Successful capture owns a copy: subsequent changes to the caller's bytes
    /// or backing file do not change it. Errors do not rewind consumed input;
    /// detecting an oversized stream reads at most encoded_bytes plus one byte.
    /// # Errors
    /// Read failure, invalid explicit origin directory or transport bounds.
    pub fn from_reader(
        input: impl Read,
        origin: Option<&Path>,
        transport: Limits,
    ) -> Result<Self, Error> {
        Ok(Self {
            source: std::sync::Arc::new(PreparedSource::from_reader(input, origin, transport)?),
            transport,
        })
    }
    /// Capture an immutable copy under the same byte limits as a stream.
    /// # Errors
    /// Transport bounds, invalid origin or temporary-file I/O.
    pub fn from_bytes(
        bytes: &[u8],
        origin: Option<&Path>,
        transport: Limits,
    ) -> Result<Self, Error> {
        Self::from_reader(bytes, origin, transport)
    }
    /// # Errors
    /// Original/captured source changed while preparing provenance.
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
    /// Inspect content without running image decoders or consulting extensions.
    /// HDF5 convention inspection uses bounded metadata and rejects external links.
    /// # Errors
    /// Source mutation, I/O or malformed HDF5 convention metadata.
    pub fn discover(&self) -> Result<Detection, Error> {
        let result = self.discover_prefix()?;
        #[cfg(feature = "hdf5")]
        let mut result = result;
        #[cfg(feature = "hdf5")]
        if result.candidates.len() == 1 && result.candidates[0].codec == Codec::Hdf5 {
            let codec = crate::hdf5::discovery::discover_snapshot(
                self.source.path(),
                Default::default(),
                self.source.source_directory(),
            )
            .map_err(|e| Error(e.to_string()))?;
            result.candidates[0].codec = codec;
            if codec != Codec::Hdf5 {
                result.candidates[0].evidence = discovery::Evidence::StructuredHeader;
                result.candidates[0].reason = "HDF5 detector convention metadata";
            }
        }
        self.source.check()?;
        Ok(result)
    }
    fn discover_prefix(&self) -> Result<Detection, Error> {
        self.source.check()?;
        let mut file = std::fs::File::open(self.source.path())?;
        self.source.check_input_handle(&file)?;
        let mut prefix = Vec::new();
        file.by_ref()
            .take(discovery::PREFIX_BYTES as u64)
            .read_to_end(&mut prefix)?;
        // HDF5 user-block offsets above the bounded prefix require an explicit
        // format; never scan arbitrary source contents looking for coincidental magic.
        file.seek(SeekFrom::Start(0))?;
        self.source.check_input_handle(&file)?;
        Ok(discovery::probe(&prefix))
    }
    /// Unwrapped physical immutable snapshot for specialized existing readers.
    /// For gzip/bzip2 this is decoded staging, not the encoded stream capture.
    /// Keep an Input clone alive until those readers are dropped; it owns private captures.
    /// The returned path is transport provenance, not a stream's original name.
    /// # Errors
    /// Original or private snapshot changed.
    pub fn source_path(&self) -> Result<&Path, Error> {
        self.source.check()?;
        Ok(self.source.path())
    }
    /// Checked physical transport roots, encoded first and decoded second when
    /// distinct. Stream roots identify private owned captures, not the caller's
    /// filename or relative-link origin. External linked files are represented
    /// by the opened reader's source graph rather than this transport list.
    /// Keep an Input clone or its opened reader alive while using these paths.
    /// # Errors
    /// Original, encoded capture or decoded staging changed.
    pub fn transport_paths(&self) -> Result<Vec<&Path>, Error> {
        self.source.check()?;
        let mut paths = vec![self.source.original_path()];
        if self.source.path() != self.source.original_path() {
            paths.push(self.source.path());
        }
        Ok(paths)
    }
    /// Reuse the selected existing decoder; no fallback on a failed read.
    /// # Errors
    /// Source/resource/codec failures or unadmitted link resolution.
    pub fn read(
        self,
        format: Format,
        image: native::Limits,
        tiff: tiff::Limits,
    ) -> Result<Reader, Error> {
        Reader::from_prepared(self.source, format, self.transport, image, tiff)
    }
    /// Open unambiguous discovered content with native stored-data defaults.
    /// Byte-offset dialects, corrections, external links and sparse background
    /// reconstruction retain their existing explicit policies.
    /// # Errors
    /// Unknown/ambiguous content, disabled feature, missing dataset selection or decoder failures.
    pub fn read_discovered(
        self,
        image: native::Limits,
        tiff: tiff::Limits,
    ) -> Result<Reader, Error> {
        let codec = self
            .discover()?
            .require_unique()
            .map_err(|e| Error(e.to_string()))?;
        #[cfg(feature = "hdf5")]
        if matches!(codec, Codec::Eiger | Codec::Lima | Codec::Lambda) {
            use crate::hdf5::typed::Selection;
            let selection = match codec {
                Codec::Eiger => Selection::Eiger,
                Codec::Lima => Selection::Lima,
                _ => Selection::Lambda,
            };
            return self.read_hdf5(selection, Default::default(), None);
        }
        #[cfg(feature = "sparse")]
        if codec == Codec::Sparse {
            return self.read_sparse(Default::default(), Default::default());
        }
        let format = match codec {
            Codec::Npy => Format::Native(native::Format::Npy),
            Codec::Dtrek => Format::Native(native::Format::Dtrek),
            Codec::Fit2dMask => Format::Native(native::Format::Fit2dMask),
            Codec::HiPic => Format::Native(native::Format::HiPic),
            Codec::Pnm => Format::Native(native::Format::Pnm),
            Codec::Edf => Format::Edf,
            Codec::Cbf => Format::Cbf(crate::cbf::ByteOffsetMode::Extended64),
            Codec::Tiff => Format::Tiff,
            #[cfg(feature = "mar345")]
            Codec::Mar345 => Format::Mar345,
            #[cfg(feature = "esperanto")]
            Codec::Esperanto => Format::Esperanto,
            #[cfg(feature = "bruker")]
            Codec::Bruker => Format::Bruker,
            #[cfg(feature = "oxford")]
            Codec::Oxford => Format::Oxford,
            #[cfg(feature = "raxis")]
            Codec::Raxis => Format::Raxis(crate::raxis::Interpretation::Stored),
            #[cfg(feature = "fixed")]
            Codec::Ge => Format::Ge,
            #[cfg(feature = "fixed")]
            Codec::Pixi => Format::Pixi,
            #[cfg(feature = "containers")]
            Codec::Spe => Format::Spe,
            #[cfg(feature = "containers")]
            Codec::Mrc => Format::Mrc,
            #[cfg(feature = "containers")]
            Codec::Dm3 => Format::Dm3,
            #[cfg(feature = "jpeg")]
            Codec::Jpeg => Format::Jpeg(Default::default()),
            #[cfg(feature = "jpeg2000")]
            Codec::Jpeg2000 => Format::Jpeg2000(Default::default()),
            #[cfg(feature = "fit2d")]
            Codec::Fit2d => Format::Fit2d,
            #[cfg(feature = "kcd")]
            Codec::Kcd => Format::Kcd(crate::kcd::Interpretation::StoredReadouts),
            #[cfg(feature = "mpa")]
            Codec::Mpa => Format::Mpa,
            #[cfg(feature = "xsd")]
            Codec::Xsd => Format::Xsd,
            _ => {
                return Err(Error(format!(
                    "content {:?} requires its explicit dataset/mask/format API",
                    codec
                )));
            }
        };
        self.read(format, image, tiff)
    }
    #[cfg(feature = "hdf5")]
    /// Select an HDF5 dataset/convention and optional explicit VDS bindings.
    /// # Errors
    /// Unsupported storage, source mutation, bounds or unadmitted linked sources.
    pub fn read_hdf5(
        self,
        selection: crate::hdf5::typed::Selection<'_>,
        options: crate::hdf5::typed::Options,
        vds: Option<crate::hdf5::typed::VdsOptions<'_>>,
    ) -> Result<Reader, Error> {
        if self.source.is_stream()
            && self.source.source_directory().is_none()
            && options.external_links != crate::hdf5::typed::ExternalLinks::Reject
        {
            return Err(Error(
                "external HDF5 stream policy requires an explicit origin directory".into(),
            ));
        }
        let reader = crate::hdf5::typed::Reader::open_snapshot_with_origin(
            self.source.path(),
            selection,
            options,
            vds,
            self.source.source_directory(),
        )
        .map_err(|e| Error(e.to_string()))?;
        self.source.check()?;
        Ok(Reader {
            backend: super::Backend::Hdf5(reader),
            source: self.source,
        })
    }
    #[cfg(feature = "sparse")]
    /// Open source sparse components; dense reconstruction still requires opt-in.
    /// # Errors
    /// Unsupported sparse records, bounds, source mutation or unadmitted links.
    pub fn read_sparse(
        self,
        selection: crate::sparse::Selection<'_>,
        options: crate::sparse::Options,
    ) -> Result<Reader, Error> {
        if self.source.is_stream()
            && self.source.source_directory().is_none()
            && options.external_links != crate::sparse::ExternalLinks::Reject
        {
            return Err(Error(
                "external sparse stream policy requires an explicit origin directory".into(),
            ));
        }
        let reader = crate::sparse::Reader::open_snapshot_with_origin(
            self.source.path(),
            selection,
            options,
            self.source.source_directory(),
        )
        .map_err(|e| Error(e.to_string()))?;
        self.source.check()?;
        Ok(Reader {
            backend: super::Backend::Sparse(Box::new(reader)),
            source: self.source,
        })
    }
}
