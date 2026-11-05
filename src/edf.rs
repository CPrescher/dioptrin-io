//! Bounded EDF frames, general headers and explicitly followed binary sources.
pub use crate::binary::{Error, FrameBuffer, Limits};
pub use crate::transport::{Compression, Limits as TransportLimits};
use crate::{
    PixelType,
    binary::{copy_samples, resize_bytes},
    transport::PreparedSource,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExternalPayloads {
    #[default]
    Reject,
    /// Follow exactly the header filename, snapshotting the source.
    FollowSnapshot,
    /// Additionally allow .gz/.bz2 counterparts only when the exact filename
    /// is absent. Ambiguous counterparts fail; a present corrupt file is never retried.
    FollowCompressedSnapshot,
}
/// Interpretation of a declared external binary-file read window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExternalWindow {
    /// The complete declared window must exist, including any storage padding.
    #[default]
    Exact,
    /// Explicit compatibility with an oversized external read request. Read up
    /// to EOF, retaining declared/effective sizes; every sample/codec byte needed
    /// for a complete image must still exist. Inline blocks are never shortened.
    UpToDeclared,
}
#[derive(Debug, Clone)]
pub struct Options {
    pub external_payloads: ExternalPayloads,
    pub external_window: ExternalWindow,
    /// Directory of the original header when a caller has staged its wrapper.
    pub source_directory: Option<PathBuf>,
    /// Explicit FabIO acquisition-file relocation convention. Default false:
    /// preserve relative/absolute header paths rather than discarding them.
    pub external_basenames: bool,
    pub transport: TransportLimits,
    pub source_files: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            external_payloads: ExternalPayloads::Reject,
            external_window: ExternalWindow::Exact,
            source_directory: None,
            external_basenames: false,
            transport: TransportLimits::default(),
            source_files: 128,
        }
    }
}
#[derive(Debug, Clone)]
pub struct GeneralHeader {
    pub header: Vec<u8>,
    pub fields: BTreeMap<String, String>,
}
#[derive(Debug, Clone)]
pub struct PayloadSource {
    pub path: PathBuf,
    pub bytes: u64,
    pub outer_compression: Compression,
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub pixel_type: PixelType,
    pub little_endian: bool,
    pub compression: String,
    /// Effective bytes read from the admitted source window.
    pub stored_bytes: usize,
    /// Original declared physical/window byte count (or shape-derived default).
    pub declared_stored_bytes: usize,
    /// None for inline physical blocks, which always retain exact-size admission.
    pub external_window_policy: Option<ExternalWindow>,
    /// Exact local header through its closing newline, including original padding.
    pub header: Vec<u8>,
    /// Case-normalized keys, last local assignment wins; raw bytes retain repeats.
    /// Non-reserved general fields are inherited without changing local fields.
    pub fields: BTreeMap<String, String>,
    pub general_header: Option<usize>,
    pub payload_source: usize,
    pub payload_offset: u64,
}
struct Page {
    metadata: Metadata,
    offset: u64,
    bytes: usize,
    source: usize,
}
pub struct Reader {
    sources: Vec<PreparedSource>,
    pages: Vec<Page>,
    general: Vec<GeneralHeader>,
    records: Vec<PayloadSource>,
    decoded_limit: usize,
    trailing_padding: usize,
}
impl Reader {
    /// Open immutable inline EDF, including bounded outer gzip/bzip2. External
    /// sources require explicit options; counts are never repaired or zero padded.
    /// # Errors
    /// Invalid/truncated/unsupported layouts, dimensions, headers, codecs or limits.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        Self::open_with_options(path, limits, Options::default())
    }
    /// Open with explicit external-file and wrapper budgets. Every resolved source
    /// is checked around every frame, including already staged compressed sources.
    /// # Errors
    /// Missing/ambiguous/changed sources, unsupported interpretation or read bounds.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        limits: Limits,
        options: Options,
    ) -> Result<Self, Error> {
        limits.validate()?;
        if options.source_files == 0 {
            return Err(Error("EDF source-file limit must be positive".into()));
        }
        let first = PreparedSource::open(path, options.transport)?;
        let directory = options
            .source_directory
            .clone()
            .unwrap_or_else(|| first.original_path().parent().unwrap().to_path_buf());
        let mut input = BufReader::new(File::open(first.path())?);
        first.check_input_handle(input.get_ref())?;
        let mut records = vec![PayloadSource {
            path: first.original_path().to_path_buf(),
            bytes: first.bytes(),
            outer_compression: first.compression(),
        }];
        let mut source_bytes = first.bytes();
        let mut sources = vec![first];
        let mut pages = Vec::new();
        let mut general: Vec<GeneralHeader> = Vec::new();
        let mut active_general = None;
        let mut offset = 0u64;
        let mut metadata_bytes = 0usize;
        let mut blocks = 0usize;
        let mut trailing_padding = 0;
        while offset < sources[0].bytes() {
            if blocks >= limits.frames {
                return Err(Error("EDF exceeds header/frame limit".into()));
            }
            blocks += 1;
            input.seek(SeekFrom::Start(offset))?;
            // Classic acquisition files may finish with up to one block of
            // zero/ASCII-space storage padding, including leading-newline variants.
            // Retain its size explicitly; never ignore non-padding tail records.
            let remaining = sources[0].bytes() - offset;
            if !pages.is_empty() && remaining < 512 {
                let mut tail = vec![0; remaining as usize];
                input.read_exact(&mut tail)?;
                if tail.iter().all(|&b| b == 0 || b.is_ascii_whitespace()) {
                    trailing_padding = tail.len();
                    break;
                }
                input.seek(SeekFrom::Start(offset))?;
            }
            let Some((header, local)) = read_header(&mut input, limits)? else {
                break;
            };
            let payload = offset
                .checked_add(header.len() as u64)
                .ok_or_else(|| Error("EDF header offset overflow".into()))?;
            let is_general = local.contains_key("EDF_DATAFORMATVERSION");
            metadata_bytes = metadata_bytes
                .checked_add(header_cost(&header, &local))
                .filter(|n| *n <= limits.metadata_bytes)
                .ok_or_else(|| Error("EDF metadata exceeds total budget".into()))?;
            if is_general {
                let bytes = optional_number(&local, "EDF_BINARYSIZE")?
                    .or(optional_number(&local, "SIZE")?)
                    .unwrap_or(0);
                if bytes != 0 {
                    return Err(Error(
                        "general EDF binary blocks require a separate metadata-payload contract"
                            .into(),
                    ));
                }
                active_general = Some(general.len());
                general.push(GeneralHeader {
                    header,
                    fields: local,
                });
                offset = payload;
                continue;
            }
            let mut fields = local;
            if let Some(index) = active_general {
                for (key, value) in &general[index].fields {
                    if !key.starts_with("EDF_")
                        && !["SIZE", "IMAGE", "HEADERID"].contains(&key.as_str())
                    {
                        fields.entry(key.clone()).or_insert_with(|| value.clone());
                    }
                }
            }
            if header_cost(&[], &fields)
                .checked_add(metadata_bytes)
                .is_none_or(|n| n > limits.metadata_bytes)
            {
                return Err(Error("inherited EDF metadata exceeds budget".into()));
            }
            metadata_bytes = metadata_bytes
                .checked_add(header_cost(&[], &fields))
                .ok_or_else(|| Error("EDF metadata overflow".into()))?;
            if fields
                .keys()
                .filter(|k| k.starts_with("DIM_"))
                .any(|k| k != "DIM_1" && k != "DIM_2" && fields[k] != "1")
            {
                return Err(Error("EDF volume dimensions are unsupported".into()));
            }
            let shape = [number(&fields, "DIM_2")?, number(&fields, "DIM_1")?];
            let pixel_type = edf_type(field(&fields, "DATATYPE")?)?;
            let count = limits.pixels(shape, pixel_type)?;
            let decoded = count * pixel_type.bytes();
            let little_endian = match field(&fields, "BYTEORDER")?.to_ascii_lowercase().as_str() {
                "lowbytefirst" => true,
                "highbytefirst" => false,
                _ => return Err(Error("unsupported EDF byte order".into())),
            };
            let compression = match fields
                .get("COMPRESSION")
                .map(|s| s.to_ascii_uppercase())
                .as_deref()
            {
                None | Some("NONE" | "NO" | "NO COMPRESSION" | "FALSE") => "NONE",
                Some("GZIP" | "GZIPCOMPRESSION") => "GZIP",
                Some("Z" | "ZLIB" | "ZCOMPRESSION") => "ZLIB",
                Some("BZ2" | "BZ" | "BZIP2" | "BZCOMPRESSION" | "BZ2COMPRESSION") => "BZ2",
                Some(value) => return Err(Error(format!("unsupported EDF compression {value}"))),
            }
            .to_owned();
            let plain = compression == "NONE";
            let physical = optional_number(&fields, "EDF_BINARYSIZE")?
                .or(optional_number(&fields, "SIZE")?)
                .unwrap_or(if plain { decoded } else { 0 });
            if physical > limits.encoded_bytes {
                return Err(Error("EDF encoded block exceeds limit".into()));
            }
            offset = payload
                .checked_add(physical as u64)
                .filter(|&n| n <= sources[0].bytes())
                .ok_or_else(|| Error("truncated EDF binary payload".into()))?;
            let (source, frame_offset, declared_bytes) =
                if let Some(name) = fields.get("EDF_BINARYFILENAME") {
                    if options.external_payloads == ExternalPayloads::Reject {
                        return Err(Error(
                            "external EDF payload requires explicit FollowSnapshot policy".into(),
                        ));
                    }
                    let name = Path::new(name);
                    let name = if options.external_basenames {
                        name.file_name()
                            .map(Path::new)
                            .ok_or_else(|| Error("invalid EDF external filename".into()))?
                    } else {
                        name
                    };
                    let mut path = if name.is_absolute() {
                        name.to_path_buf()
                    } else {
                        directory.join(name)
                    };
                    if !path.try_exists()?
                        && options.external_payloads == ExternalPayloads::FollowCompressedSnapshot
                    {
                        let candidates =
                            [suffixed_path(&path, ".gz"), suffixed_path(&path, ".bz2")];
                        let found = candidates
                            .into_iter()
                            .filter_map(|p| match p.try_exists() {
                                Ok(true) => Some(Ok(p)),
                                Ok(false) => None,
                                Err(e) => Some(Err(e)),
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        if found.len() != 1 {
                            return Err(Error(
                                "missing or ambiguous compressed EDF payload counterpart".into(),
                            ));
                        }
                        path = found[0].clone();
                    }
                    let canonical = std::fs::canonicalize(path)?;
                    let source = if let Some(index) =
                        sources.iter().position(|s| s.original_path() == canonical)
                    {
                        index
                    } else {
                        if sources.len() >= options.source_files {
                            return Err(Error("EDF exceeds source-file limit".into()));
                        }
                        let remaining = options
                            .transport
                            .source_bytes
                            .checked_sub(source_bytes)
                            .filter(|&n| n > 0)
                            .ok_or_else(|| {
                                Error("EDF sources exceed total staging/source budget".into())
                            })?;
                        let prepared = PreparedSource::open(
                            &canonical,
                            TransportLimits {
                                source_bytes: remaining,
                                ..options.transport
                            },
                        )?;
                        source_bytes = source_bytes
                            .checked_add(prepared.bytes())
                            .ok_or_else(|| Error("EDF total source size overflow".into()))?;
                        metadata_bytes = metadata_bytes
                            .checked_add(
                                canonical
                                    .as_os_str()
                                    .len()
                                    .saturating_mul(2)
                                    .saturating_add(256),
                            )
                            .filter(|n| *n <= limits.metadata_bytes)
                            .ok_or_else(|| {
                                Error("EDF source manifest exceeds metadata budget".into())
                            })?;
                        records.push(PayloadSource {
                            path: canonical,
                            bytes: prepared.bytes(),
                            outer_compression: prepared.compression(),
                        });
                        sources.push(prepared);
                        sources.len() - 1
                    };
                    if source == 0 {
                        return Err(Error(
                            "EDF external payload must be a distinct source file".into(),
                        ));
                    }
                    let start =
                        optional_number(&fields, "EDF_BINARYFILEPOSITION")?.unwrap_or(0) as u64;
                    let bytes = optional_number(&fields, "EDF_BINARYFILESIZE")?
                        .unwrap_or(if plain { decoded } else { 0 });
                    (source, start, bytes)
                } else {
                    (0, payload, physical)
                };
            let external_window_policy = (source != 0).then_some(options.external_window);
            let bytes = if external_window_policy == Some(ExternalWindow::UpToDeclared) {
                let available = sources[source]
                    .bytes()
                    .checked_sub(frame_offset)
                    .ok_or_else(|| Error("EDF external window starts beyond its source".into()))?;
                u64::try_from(declared_bytes)
                    .map_err(|_| Error("EDF external window exceeds u64".into()))?
                    .min(available) as usize
            } else {
                declared_bytes
            };
            if bytes == 0
                || bytes > limits.encoded_bytes
                || (plain && bytes < decoded)
                || frame_offset
                    .checked_add(bytes as u64)
                    .is_none_or(|n| n > sources[source].bytes())
            {
                return Err(Error("invalid/truncated EDF payload window".into()));
            }
            let metadata = Metadata {
                shape,
                pixel_type,
                little_endian,
                compression,
                stored_bytes: bytes,
                declared_stored_bytes: declared_bytes,
                external_window_policy,
                header,
                fields,
                general_header: active_general,
                payload_source: source,
                payload_offset: frame_offset,
            };
            pages.push(Page {
                metadata,
                offset: frame_offset,
                bytes,
                source,
            });
        }
        if pages.is_empty() {
            return Err(Error("EDF file has no image frames".into()));
        }
        for source in &sources {
            source.check()?;
        }
        Ok(Self {
            sources,
            pages,
            general,
            records,
            decoded_limit: limits.frame_bytes,
            trailing_padding,
        })
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.pages.len()
    }
    #[must_use]
    pub fn metadata(&self, index: usize) -> Option<&Metadata> {
        self.pages.get(index).map(|p| &p.metadata)
    }
    #[must_use]
    pub fn trailing_padding(&self) -> usize {
        self.trailing_padding
    }
    #[must_use]
    pub fn general_headers(&self) -> &[GeneralHeader] {
        &self.general
    }
    #[must_use]
    pub fn payload_sources(&self) -> &[PayloadSource] {
        &self.records
    }
    /// Read original samples into reusable storage. All source identities are
    /// checked before and after reading; failed output is inaccessible.
    /// # Errors
    /// Out-of-range frame, changed source, corrupt stream or exact-size mismatch.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        for source in &self.sources {
            source.check()?;
        }
        let page = self
            .pages
            .get(index)
            .ok_or_else(|| Error("EDF frame index outside file".into()))?;
        let mut file = File::open(self.sources[page.source].path())?;
        file.seek(SeekFrom::Start(page.offset))?;
        self.sources[page.source].check_input_handle(&file)?;
        resize_bytes(&mut buffer.encoded, page.bytes)?;
        file.read_exact(&mut buffer.encoded)?;
        let meta = &page.metadata;
        let count = meta.shape[0] * meta.shape[1];
        let bytes = count * meta.pixel_type.bytes();
        buffer
            .pixels
            .resize(meta.pixel_type, count)
            .map_err(Error)?;
        if meta.compression == "NONE" {
            copy_samples(
                &buffer.encoded[..bytes],
                &mut buffer.pixels,
                meta.little_endian,
            )?;
        } else {
            resize_bytes(&mut buffer.decoded, bytes)?;
            decode_payload(
                &meta.compression,
                &buffer.encoded,
                &mut buffer.decoded,
                self.decoded_limit,
            )?;
            copy_samples(&buffer.decoded, &mut buffer.pixels, meta.little_endian)?;
        }
        self.sources[page.source].check_input_handle(&file)?;
        for source in &self.sources {
            source.check()?;
        }
        buffer.shape = Some(meta.shape);
        Ok(())
    }
}
fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c == '\0' || c.is_ascii_whitespace())
}
fn field<'a>(fields: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, Error> {
    fields
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| Error(format!("missing EDF field {key}")))
}
fn number(fields: &BTreeMap<String, String>, key: &str) -> Result<usize, Error> {
    field(fields, key)?
        .parse()
        .map_err(|_| Error(format!("invalid EDF field {key}")))
}
fn optional_number(fields: &BTreeMap<String, String>, key: &str) -> Result<Option<usize>, Error> {
    if fields.contains_key(key) {
        number(fields, key).map(Some)
    } else {
        Ok(None)
    }
}
fn header_cost(header: &[u8], fields: &BTreeMap<String, String>) -> usize {
    fields.iter().fold(header.len(), |n, (k, v)| {
        n.saturating_add(k.len())
            .saturating_add(v.len())
            .saturating_add(128)
    })
}
type HeaderBlock = (Vec<u8>, BTreeMap<String, String>);

fn read_header(input: &mut impl Read, limits: Limits) -> Result<Option<HeaderBlock>, Error> {
    let mut header = Vec::new();
    let mut opened = false;
    loop {
        if header.len() >= limits.header_bytes {
            return Err(Error(
                "EDF header exceeds limit or lacks closing newline".into(),
            ));
        }
        let mut b = [0];
        if input.read(&mut b)? == 0 {
            if !opened && header.iter().all(u8::is_ascii_whitespace) {
                return Ok(None);
            }
            return Err(Error("truncated EDF header".into()));
        }
        if !opened {
            if b[0] == b'{' {
                opened = true;
            } else if !b[0].is_ascii_whitespace() {
                return Err(Error("invalid EDF header opening".into()));
            }
        }
        header.push(b[0]);
        if header.ends_with(b"}\n") || header.ends_with(b"}\r\n") {
            break;
        }
    }
    let start = header.iter().position(|&b| b == b'{').unwrap() + 1;
    let end = header.len() - if header.ends_with(b"}\r\n") { 3 } else { 2 };
    let text: String = header[start..end].iter().map(|&b| char::from(b)).collect();
    let mut fields = BTreeMap::new();
    for entry in text.split(';') {
        if let Some((key, value)) = entry.split_once('=') {
            let key = trim(key).to_ascii_uppercase();
            if key.is_empty() {
                return Err(Error("empty EDF header key".into()));
            }
            fields.insert(key, trim(value).to_owned());
        }
    }
    if let Some(declared) =
        optional_number(&fields, "EDF_HEADERSIZE")?.or(optional_number(&fields, "HEADERSIZE")?)
        && declared > limits.header_bytes
    {
        return Err(Error("EDF declared header size exceeds limit".into()));
    }
    Ok(Some((header, fields)))
}
fn decode_member(
    input: &mut impl Read,
    output: &mut [u8],
    filled: &mut usize,
) -> Result<(), Error> {
    while *filled < output.len() {
        let n = input.read(&mut output[*filled..])?;
        if n == 0 {
            return Ok(());
        }
        *filled += n;
    }
    let mut extra = [0];
    if input.read(&mut extra)? != 0 {
        return Err(Error(
            "EDF decoded stream exceeds declared dimensions".into(),
        ));
    }
    Ok(())
}
fn decode_payload(codec: &str, input: &[u8], output: &mut [u8], limit: usize) -> Result<(), Error> {
    if output.len() > limit {
        return Err(Error("EDF decode exceeds frame budget".into()));
    }
    let mut offset = 0;
    let mut filled = 0;
    loop {
        let cursor = Cursor::new(&input[offset..]);
        let consumed = match codec {
            "GZIP" => {
                let mut decoder = flate2::bufread::GzDecoder::new(cursor);
                decode_member(&mut decoder, output, &mut filled)?;
                decoder.into_inner().position() as usize
            }
            "ZLIB" => {
                let mut decoder = flate2::bufread::ZlibDecoder::new(cursor);
                decode_member(&mut decoder, output, &mut filled)?;
                decoder.into_inner().position() as usize
            }
            "BZ2" => {
                let mut decoder = bzip2::bufread::BzDecoder::new(cursor);
                decode_member(&mut decoder, output, &mut filled)?;
                decoder.into_inner().position() as usize
            }
            _ => return Err(Error("unsupported EDF compression".into())),
        };
        if consumed == 0 {
            return Err(Error("empty EDF compressed member".into()));
        }
        offset = offset
            .checked_add(consumed)
            .ok_or_else(|| Error("EDF codec offset overflow".into()))?;
        let tail = &input[offset..];
        if (codec == "GZIP" && tail.starts_with(&[31, 139]))
            || (codec == "BZ2" && tail.starts_with(b"BZh"))
        {
            continue;
        }
        // Recorded acquisition slots are padded to 512 bytes with zeros/spaces.
        // Unknown tails and additional zlib streams are not silently discarded.
        if !tail.is_empty()
            && (tail.len() >= 512
                || !input.len().is_multiple_of(512)
                || !tail.iter().all(|&b| b == 0 || b.is_ascii_whitespace()))
        {
            return Err(Error("unexplained EDF trailing encoded bytes".into()));
        }
        break;
    }
    if filled != output.len() {
        return Err(Error("truncated EDF compressed samples".into()));
    }
    Ok(())
}

fn edf_type(s: &str) -> Result<PixelType, Error> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "signedbyte" | "signed8" => PixelType::I8,
        "unsignedbyte" | "unsigned8" => PixelType::U8,
        "signedshort" | "signed16" => PixelType::I16,
        "unsignedshort" | "unsigned16" | "unsignedshortinteger" => PixelType::U16,
        "signedinteger" | "signed32" | "signedlong" => PixelType::I32,
        "unsignedinteger" | "unsigned32" | "unsignedlong" => PixelType::U32,
        "signed64" => PixelType::I64,
        "unsigned64" => PixelType::U64,
        "floatvalue" | "float" | "floatieee32" | "float32" => PixelType::F32,
        "double" | "doublevalue" | "floatieee64" | "doubleieee64" => PixelType::F64,
        _ => return Err(Error(format!("unsupported EDF scalar type {s}"))),
    })
}

fn suffixed_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    name.into()
}

#[cfg(all(test, unix))]
mod path_tests {
    use super::*;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    #[test]
    fn counterpart_paths_preserve_exact_source_bytes() {
        let path = PathBuf::from(std::ffi::OsString::from_vec(
            b"acquisition-\xff/data.bin".to_vec(),
        ));
        assert_eq!(
            suffixed_path(&path, ".gz").as_os_str().as_bytes(),
            b"acquisition-\xff/data.bin.gz"
        );
    }
}
