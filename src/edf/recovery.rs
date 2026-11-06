//! Explicit immutable EDF recovery. Output is a typed contiguous sample span,
//! never a zero-filled or implicitly complete dense frame.
use super::{Error, GeneralHeader, Limits, Metadata};
use crate::{PixelBuffer, binary::copy_samples, source::Snapshot, transport::Compression};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write},
    ops::Range,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Options {
    pub edf: super::Options,
    pub members: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            edf: super::Options::default(),
            members: 100_000,
        }
    }
}
/// Availability and integrity are independent. In particular, all declared
/// samples can exist while the containing member lacks a verified footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrity {
    ContainerComplete,
    VerifiedMember,
    UnverifiedTruncated,
    ChecksumFailure,
    DecodeFailure,
}
#[derive(Debug, Clone)]
pub struct MemberReport {
    pub encoded_range: Range<u64>,
    pub decoded_range: Range<u64>,
    pub integrity: Integrity,
    pub diagnostic: Option<String>,
}
#[derive(Debug, Clone)]
pub struct SourceReport {
    pub path: PathBuf,
    pub encoded_bytes: u64,
    pub decoded_bytes: u64,
    pub compression: Compression,
    pub integrity: Integrity,
    pub verified_decoded_bytes: u64,
    pub members: Vec<MemberReport>,
    pub diagnostic: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    TruncatedHeader {
        offset: u64,
        bytes: Vec<u8>,
    },
    TruncatedPayload {
        image_record: usize,
        offset: u64,
        expected_bytes: usize,
        available_bytes: usize,
    },
    UnparsedRecord {
        offset: u64,
        header: Vec<u8>,
        reason: String,
    },
    NoImageHeader,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    AllSamples,
    Prefix,
    HeadersOnly,
}
#[derive(Debug, Clone)]
pub struct SpanReport {
    pub declared_shape: [usize; 2],
    pub declared_samples: usize,
    pub present_samples: Range<usize>,
    pub missing_samples: Option<Range<usize>>,
    pub verified_samples: Range<usize>,
    pub expected_payload_bytes: usize,
    pub available_payload_bytes: usize,
    pub availability: Availability,
    pub outer_integrity: Integrity,
    pub payload_integrity: Integrity,
    pub complete_verified_frame: bool,
    pub payload_members: Vec<MemberReport>,
    pub diagnostic: Option<String>,
}
#[derive(Debug, Default)]
pub struct SampleBuffer {
    pixels: PixelBuffer,
    report: Option<SpanReport>,
    partial: Vec<u8>,
}
impl SampleBuffer {
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        self.report.as_ref().map(|_| &self.pixels)
    }
    #[must_use]
    pub fn report(&self) -> Option<&SpanReport> {
        self.report.as_ref()
    }
    #[must_use]
    pub fn partial_element_bytes(&self) -> Option<&[u8]> {
        self.report.as_ref().map(|_| self.partial.as_slice())
    }
    fn invalidate(&mut self) {
        self.report = None;
        self.partial.clear();
    }
}
struct Page {
    metadata: Metadata,
    expected: usize,
    available: usize,
}
struct Source {
    snapshot: Snapshot,
    identity: File,
    staged: Option<tempfile::NamedTempFile>,
    staged_snapshot: Option<Snapshot>,
    report: SourceReport,
}
impl Source {
    fn path(&self) -> &Path {
        self.staged
            .as_ref()
            .map_or(&self.snapshot.path, |t| t.path())
    }
    fn check(&self) -> Result<(), Error> {
        self.snapshot.check_handle(&self.identity).map_err(Error)?;
        if let (Some(stamp), Some(file)) = (&self.staged_snapshot, &self.staged) {
            stamp.check_handle(file.as_file()).map_err(Error)?;
        }
        Ok(())
    }
    fn check_input(&self, file: &File) -> Result<(), Error> {
        self.staged_snapshot
            .as_ref()
            .unwrap_or(&self.snapshot)
            .check_handle(file)
            .map_err(Error)?;
        self.check()
    }
    fn open(path: impl AsRef<Path>, limits: Limits, options: &Options) -> Result<Self, Error> {
        let transport = options.edf.transport;
        if transport.encoded_bytes == 0
            || transport.source_bytes == 0
            || transport.source_bytes == u64::MAX
            || options.members == 0
        {
            return Err(Error(
                "EDF recovery limits must be positive and finite".into(),
            ));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if !std::fs::metadata(&snapshot.path)?.is_file() {
            return Err(Error(
                "EDF recovery requires a regular immutable source".into(),
            ));
        }
        let mut identity = File::open(&snapshot.path)?;
        snapshot.check_handle(&identity).map_err(Error)?;
        if !identity.metadata()?.is_file() {
            return Err(Error(
                "EDF recovery requires a regular immutable source".into(),
            ));
        }
        let mut magic = [0; 3];
        let n = snapshot.bytes().min(3) as usize;
        identity.read_exact(&mut magic[..n])?;
        identity.seek(SeekFrom::Start(0))?;
        let compression = if n >= 2 && magic[..2] == [31, 139] {
            Compression::Gzip
        } else if n == 3 && magic == *b"BZh" {
            Compression::Bzip2
        } else {
            Compression::None
        };
        let mut staged = None;
        let (decoded_bytes, verified_decoded_bytes, integrity, members, diagnostic);
        if compression == Compression::None {
            if snapshot.bytes() > transport.source_bytes {
                return Err(Error("EDF recovery source exceeds byte budget".into()));
            }
            decoded_bytes = snapshot.bytes();
            verified_decoded_bytes = decoded_bytes;
            integrity = Integrity::ContainerComplete;
            members = Vec::new();
            diagnostic = None;
        } else {
            if snapshot.bytes() > transport.encoded_bytes {
                return Err(Error("EDF recovery wrapper exceeds encoded budget".into()));
            }
            let mut temp = tempfile::NamedTempFile::new()?;
            let mut input = BufReader::new(&mut identity);
            let decoded = {
                let mut output = BufWriter::new(temp.as_file_mut());
                let result = decode_members(
                    &mut input,
                    &mut output,
                    compression,
                    DecodeLimits {
                        bytes: transport.source_bytes,
                        header: limits.header_bytes,
                        members: options.members,
                        metadata: limits.metadata_bytes,
                        padding: false,
                    },
                )?;
                output.flush()?;
                result
            };
            decoded_bytes = decoded.bytes;
            verified_decoded_bytes = decoded.verified;
            integrity = decoded.integrity;
            members = decoded.members;
            diagnostic = decoded.diagnostic;
            staged = Some(temp);
        }
        let staged_snapshot = staged
            .as_ref()
            .map(|t| Snapshot::open(t.path()).map_err(Error))
            .transpose()?;
        snapshot.check_handle(&identity).map_err(Error)?;
        let report = SourceReport {
            path: snapshot.path.clone(),
            encoded_bytes: snapshot.bytes(),
            decoded_bytes,
            compression,
            integrity,
            verified_decoded_bytes,
            members,
            diagnostic,
        };
        Ok(Self {
            snapshot,
            identity,
            staged,
            staged_snapshot,
            report,
        })
    }
}
pub struct Reader {
    source: Source,
    pages: Vec<Page>,
    general: Vec<GeneralHeader>,
    stop: Option<Stop>,
    limits: Limits,
    options: Options,
    metadata_bytes: usize,
}
impl Reader {
    /// Open original bytes under an explicit prefix-recovery contract. Valid
    /// compressed members are independently verified; failed member output stays
    /// visibly unverified even if every declared image sample was decoded.
    /// # Errors
    /// Resource admission, source changes, I/O or unsupported external payloads.
    pub fn open(path: impl AsRef<Path>, limits: Limits, options: Options) -> Result<Self, Error> {
        limits.validate()?;
        let source = Source::open(path, limits, &options)?;
        let mut metadata_bytes = source
            .report
            .members
            .iter()
            .try_fold(
                source
                    .report
                    .path
                    .as_os_str()
                    .len()
                    .saturating_mul(2)
                    .saturating_add(512),
                |n, m| n.checked_add(256 + m.diagnostic.as_ref().map_or(0, String::len)),
            )
            .filter(|&n| n <= limits.metadata_bytes)
            .ok_or_else(|| Error("EDF recovery source metadata exceeds budget".into()))?;
        let mut input = BufReader::new(File::open(source.path())?);
        source.check_input(input.get_ref())?;
        let mut offset = 0u64;
        let mut pages = Vec::new();
        let mut general: Vec<GeneralHeader> = Vec::new();
        let mut active_general = None;
        let mut stop = None;
        let mut blocks = 0usize;
        while offset < source.report.decoded_bytes {
            if blocks >= limits.frames {
                return Err(Error("EDF recovery exceeds header/frame limit".into()));
            }
            blocks += 1;
            input.seek(SeekFrom::Start(offset))?;
            let header_read = super::read_header_record(&mut input, limits, false)?;
            let (header, mut fields) = match header_read {
                super::HeaderRecord::Empty => break,
                super::HeaderRecord::Complete(value) => value,
                super::HeaderRecord::Incomplete(bytes) => {
                    charge(&mut metadata_bytes, bytes.len(), limits)?;
                    stop = Some(Stop::TruncatedHeader { offset, bytes });
                    break;
                }
                super::HeaderRecord::Invalid(header, reason) => {
                    charge(&mut metadata_bytes, header.len() + reason.len(), limits)?;
                    stop = Some(Stop::UnparsedRecord {
                        offset,
                        header,
                        reason,
                    });
                    break;
                }
            };
            charge(
                &mut metadata_bytes,
                super::header_cost(&header, &fields),
                limits,
            )?;
            let payload = offset
                .checked_add(header.len() as u64)
                .ok_or_else(|| Error("EDF recovery header offset overflow".into()))?;
            if fields.contains_key("EDF_DATAFORMATVERSION") {
                let declared =
                    super::optional_number(&fields, "EDF_BINARYSIZE").and_then(|binary| {
                        Ok(binary
                            .or(super::optional_number(&fields, "SIZE")?)
                            .unwrap_or(0))
                    });
                let reason = match declared {
                    Ok(0) => None,
                    Ok(_) => Some("general binary metadata payload is unsupported".to_owned()),
                    Err(error) => Some(error.to_string()),
                };
                if let Some(reason) = reason {
                    stop = Some(Stop::UnparsedRecord {
                        offset,
                        header,
                        reason,
                    });
                    break;
                }
                active_general = Some(general.len());
                general.push(GeneralHeader { header, fields });
                offset = payload;
                continue;
            }
            if let Some(index) = active_general {
                for (key, value) in &general[index].fields {
                    if !key.starts_with("EDF_")
                        && !["SIZE", "IMAGE", "HEADERID"].contains(&key.as_str())
                    {
                        fields.entry(key.clone()).or_insert_with(|| value.clone());
                    }
                }
            }
            charge(
                &mut metadata_bytes,
                super::header_cost(&[], &fields),
                limits,
            )?;
            if fields.contains_key("EDF_BINARYFILENAME") {
                return Err(Error("EDF recovery external sources require a separate explicit source-span contract".into()));
            }
            // A malformed later header may stop recovery; a configured resource
            // bound is never softened into a successful partial index.
            if let (Ok(h), Ok(w), Ok(ty)) = (
                super::number(&fields, "DIM_2"),
                super::number(&fields, "DIM_1"),
                super::field(&fields, "DATATYPE").and_then(super::edf_type),
            ) {
                let bytes = h
                    .checked_mul(w)
                    .and_then(|n| n.checked_mul(ty.bytes()))
                    .ok_or_else(|| Error("EDF recovery shape overflows platform".into()))?;
                if bytes > limits.frame_bytes {
                    return Err(Error("EDF recovery frame exceeds byte budget".into()));
                }
            }
            let original_header = header.clone();
            let parsed = parse_metadata(
                header,
                fields,
                payload,
                active_general,
                limits,
                &options.edf,
            );
            let (mut metadata, expected) = match parsed {
                Ok(value) => value,
                Err(error) => {
                    stop = Some(Stop::UnparsedRecord {
                        offset,
                        header: original_header,
                        reason: error.to_string(),
                    });
                    break;
                }
            };
            if expected > limits.encoded_bytes {
                return Err(Error("EDF recovery encoded window exceeds budget".into()));
            }
            let remaining = source
                .report
                .decoded_bytes
                .checked_sub(payload)
                .ok_or_else(|| Error("EDF recovery payload starts outside source".into()))?;
            let available = remaining.min(expected as u64) as usize;
            metadata.stored_bytes = available;
            let index = pages.len();
            pages.push(Page {
                metadata,
                expected,
                available,
            });
            if available < expected {
                stop = Some(Stop::TruncatedPayload {
                    image_record: index,
                    offset: payload,
                    expected_bytes: expected,
                    available_bytes: available,
                });
                break;
            }
            offset = payload
                .checked_add(expected as u64)
                .ok_or_else(|| Error("EDF recovery payload navigation overflow".into()))?;
        }
        if pages.is_empty() && stop.is_none() {
            stop = Some(Stop::NoImageHeader);
        }
        source.check_input(input.get_ref())?;
        Ok(Self {
            source,
            pages,
            general,
            stop,
            limits,
            options,
            metadata_bytes,
        })
    }
    /// Number of complete image headers, including records with no samples.
    #[must_use]
    pub fn image_records(&self) -> usize {
        self.pages.len()
    }
    #[must_use]
    pub fn metadata(&self, index: usize) -> Option<&Metadata> {
        self.pages.get(index).map(|p| &p.metadata)
    }
    #[must_use]
    pub fn source_report(&self) -> &SourceReport {
        &self.source.report
    }
    #[must_use]
    pub fn stop(&self) -> Option<&Stop> {
        self.stop.as_ref()
    }
    #[must_use]
    pub fn general_headers(&self) -> &[GeneralHeader] {
        &self.general
    }
    /// Read only present complete scalar words, retaining partial-word bytes
    /// separately. No shape-sized allocation or missing-value synthesis occurs.
    /// # Errors
    /// Changed source, invalid record, bounds or overlong/unsupported decoding.
    pub fn read_samples_into(&self, index: usize, buffer: &mut SampleBuffer) -> Result<(), Error> {
        buffer.invalidate();
        self.source.check()?;
        let page = self
            .pages
            .get(index)
            .ok_or_else(|| Error("EDF recovery image record outside source".into()))?;
        let meta = &page.metadata;
        let declared_samples = self.limits.pixels(meta.shape, meta.pixel_type)?;
        let needed = declared_samples * meta.pixel_type.bytes();
        let mut file = File::open(self.source.path())?;
        self.source.check_input(&file)?;
        file.seek(SeekFrom::Start(meta.payload_offset))?;
        let take = if meta.compression == "NONE" {
            page.available.min(needed)
        } else {
            page.available
        };
        let mut encoded = Vec::new();
        super::resize_bytes(&mut encoded, take)?;
        file.read_exact(&mut encoded)?;
        let (bytes, decoded) = if meta.compression == "NONE" {
            (
                encoded,
                DecodeSummary {
                    bytes: take as u64,
                    verified: take as u64,
                    integrity: Integrity::ContainerComplete,
                    members: Vec::new(),
                    diagnostic: None,
                },
            )
        } else {
            let codec = match meta.compression.as_str() {
                "GZIP" => Compression::Gzip,
                "BZ2" => Compression::Bzip2,
                "ZLIB" => Compression::None,
                _ => return Err(Error("unsupported EDF recovery payload codec".into())),
            };
            let mut bytes = Vec::new();
            let mut input = Cursor::new(encoded.as_slice());
            let decoded = if meta.compression == "ZLIB" {
                decode_zlib(&mut input, &mut bytes, needed as u64)?
            } else {
                decode_members(
                    &mut input,
                    &mut bytes,
                    codec,
                    DecodeLimits {
                        bytes: needed as u64,
                        header: self.limits.header_bytes,
                        members: self.options.members,
                        metadata: self
                            .limits
                            .metadata_bytes
                            .saturating_sub(self.metadata_bytes),
                        padding: true,
                    },
                )?
            };
            (bytes, decoded)
        };
        if bytes.len() > needed {
            return Err(Error(
                "EDF recovery decoded output exceeds declared shape".into(),
            ));
        }
        let scalar = meta.pixel_type.bytes();
        let count = bytes.len() / scalar;
        let typed = count * scalar;
        buffer
            .pixels
            .resize(meta.pixel_type, count)
            .map_err(Error)?;
        copy_samples(&bytes[..typed], &mut buffer.pixels, meta.little_endian)?;
        buffer.partial.extend_from_slice(&bytes[typed..]);
        let outer = self.source.report.integrity;
        let available_end = meta
            .payload_offset
            .checked_add(page.available as u64)
            .ok_or_else(|| Error("EDF recovery source span overflow".into()))?;
        let outer_integrity = if outer == Integrity::ContainerComplete {
            outer
        } else if available_end <= self.source.report.verified_decoded_bytes {
            Integrity::VerifiedMember
        } else {
            outer
        };
        let verified = if meta.compression == "NONE" {
            self.source
                .report
                .verified_decoded_bytes
                .saturating_sub(meta.payload_offset)
                .min(typed as u64) as usize
                / scalar
        } else if matches!(
            outer_integrity,
            Integrity::ContainerComplete | Integrity::VerifiedMember
        ) {
            (decoded.verified.min(typed as u64) as usize) / scalar
        } else {
            0
        };
        let complete_verified_frame = count == declared_samples
            && buffer.partial.is_empty()
            && page.available == page.expected
            && matches!(
                outer_integrity,
                Integrity::ContainerComplete | Integrity::VerifiedMember
            )
            && decoded.integrity == Integrity::ContainerComplete;
        let availability = if count == declared_samples {
            Availability::AllSamples
        } else if bytes.is_empty() {
            Availability::HeadersOnly
        } else {
            Availability::Prefix
        };
        self.source.check_input(&file)?;
        buffer.report = Some(SpanReport {
            declared_shape: meta.shape,
            declared_samples,
            present_samples: 0..count,
            missing_samples: (count < declared_samples).then_some(count..declared_samples),
            verified_samples: 0..verified,
            expected_payload_bytes: page.expected,
            available_payload_bytes: page.available,
            availability,
            outer_integrity,
            payload_integrity: decoded.integrity,
            complete_verified_frame,
            payload_members: decoded.members,
            diagnostic: decoded.diagnostic,
        });
        Ok(())
    }
}
fn charge(used: &mut usize, bytes: usize, limits: Limits) -> Result<(), Error> {
    *used = used
        .checked_add(bytes)
        .filter(|&n| n <= limits.metadata_bytes)
        .ok_or_else(|| Error("EDF recovery metadata exceeds budget".into()))?;
    Ok(())
}
fn parse_metadata(
    header: Vec<u8>,
    fields: BTreeMap<String, String>,
    payload: u64,
    general_header: Option<usize>,
    limits: Limits,
    options: &super::Options,
) -> Result<(Metadata, usize), Error> {
    if fields
        .keys()
        .filter(|k| k.starts_with("DIM_"))
        .any(|k| k != "DIM_1" && k != "DIM_2" && fields[k] != "1")
    {
        return Err(Error("EDF recovery volume layout is unsupported".into()));
    }
    let shape = [
        super::number(&fields, "DIM_2")?,
        super::number(&fields, "DIM_1")?,
    ];
    let pixel_type = super::edf_type(super::field(&fields, "DATATYPE")?)?;
    let decoded = limits.pixels(shape, pixel_type)? * pixel_type.bytes();
    let little_endian = match super::field(&fields, "BYTEORDER")?
        .to_ascii_uppercase()
        .as_str()
    {
        "LOWBYTEFIRST" => true,
        "HIGHBYTEFIRST" => false,
        _ => return Err(Error("unsupported EDF recovery byte order".into())),
    };
    let compression = super::effective_compression(&fields, &options.payload_interpretation)?;
    let plain = compression == "NONE";
    let declared = super::optional_number(&fields, "EDF_BINARYSIZE")?
        .or(super::optional_number(&fields, "SIZE")?)
        .unwrap_or(if plain { decoded } else { 0 });
    let expected = super::effective_inline_bytes(
        &fields,
        declared,
        decoded,
        plain,
        false,
        options.inline_layout,
    )?;
    if expected == 0 {
        return Err(Error(
            "EDF recovery requires a declared nonempty payload window".into(),
        ));
    }
    let metadata = Metadata {
        shape,
        pixel_type,
        little_endian,
        declared_compression: fields.get("COMPRESSION").cloned(),
        payload_interpretation: options.payload_interpretation.clone(),
        inline_layout_policy: Some(options.inline_layout),
        declared_header_bytes: fields
            .get("EDF_HEADERSIZE")
            .or_else(|| fields.get("HEADERSIZE"))
            .cloned(),
        parsed_header_bytes: header.len(),
        legacy_size_bytes: super::optional_number(&fields, "SIZE")?,
        compression,
        stored_bytes: 0,
        declared_stored_bytes: declared,
        external_window_policy: None,
        header,
        fields,
        general_header,
        payload_source: 0,
        payload_offset: payload,
    };
    Ok((metadata, expected))
}

struct DecodeSummary {
    bytes: u64,
    verified: u64,
    integrity: Integrity,
    members: Vec<MemberReport>,
    diagnostic: Option<String>,
}
struct DecodeLimits {
    bytes: u64,
    header: usize,
    members: usize,
    metadata: usize,
    padding: bool,
}
fn decode_members(
    input: &mut (impl BufRead + Seek),
    output: &mut impl Write,
    codec: Compression,
    limits: DecodeLimits,
) -> Result<DecodeSummary, Error> {
    let mut result = DecodeSummary {
        bytes: 0,
        verified: 0,
        integrity: Integrity::ContainerComplete,
        members: Vec::new(),
        diagnostic: None,
    };
    let mut metadata = 0usize;
    let start = input.stream_position()?;
    let end = if limits.padding {
        let end = input.seek(SeekFrom::End(0))?;
        input.seek(SeekFrom::Start(start))?;
        Some(end)
    } else {
        None
    };
    loop {
        if let Some(end) = end {
            let offset = input.stream_position()?;
            let remain = end.saturating_sub(offset);
            if end.is_multiple_of(512) && remain > 0 && remain < 512 {
                let mut padding = [0; 511];
                input.read_exact(&mut padding[..remain as usize])?;
                if padding[..remain as usize]
                    .iter()
                    .all(|&v| v == 0 || v.is_ascii_whitespace())
                {
                    break;
                }
                input.seek(SeekFrom::Start(offset))?;
            }
        }
        if input.fill_buf()?.is_empty() {
            break;
        }
        if result.members.len() >= limits.members {
            return Err(Error("EDF recovery compression-member limit".into()));
        }
        metadata = metadata
            .checked_add(256)
            .filter(|&n| n <= limits.metadata)
            .ok_or_else(|| Error("EDF recovery compression metadata exceeds budget".into()))?;
        let encoded_start = input.stream_position()?;
        let decoded_start = result.bytes;
        let admission = admit_header(input, codec, limits.header)?;
        let fault = match admission {
            HeaderAdmission::Complete => {
                input.seek(SeekFrom::Start(encoded_start))?;
                match codec {
                    Compression::Gzip => {
                        let mut reader = flate2::bufread::GzDecoder::new(&mut *input);
                        drain_prefix(&mut reader, output, &mut result.bytes, limits.bytes)?
                    }
                    Compression::Bzip2 => {
                        let mut reader = bzip2::bufread::BzDecoder::new(&mut *input);
                        drain_prefix(&mut reader, output, &mut result.bytes, limits.bytes)?
                    }
                    Compression::None => {
                        return Err(Error("EDF recovery member codec must be gzip/bzip2".into()));
                    }
                }
            }
            HeaderAdmission::Fault(integrity, message) => Some((integrity, message)),
        };
        let end = input.stream_position()?;
        if end <= encoded_start && fault.is_none() {
            return Err(Error("EDF recovery codec made no source progress".into()));
        }
        let (integrity, diagnostic) =
            fault.map_or((Integrity::VerifiedMember, None), |(i, m)| (i, Some(m)));
        if let Some(message) = &diagnostic {
            metadata = metadata
                .checked_add(message.len())
                .filter(|&n| n <= limits.metadata)
                .ok_or_else(|| Error("EDF recovery diagnostic exceeds metadata budget".into()))?;
        }
        result.members.push(MemberReport {
            encoded_range: encoded_start..end,
            decoded_range: decoded_start..result.bytes,
            integrity,
            diagnostic: diagnostic.clone(),
        });
        if diagnostic.is_some() {
            result.integrity = integrity;
            result.diagnostic = diagnostic;
            break;
        }
        result.verified = result.bytes;
    }
    if result.members.is_empty() {
        result.integrity = Integrity::UnverifiedTruncated;
        result.diagnostic = Some("no compressed member bytes are available".into());
    }
    Ok(result)
}
fn drain_prefix(
    input: &mut impl Read,
    output: &mut impl Write,
    bytes: &mut u64,
    limit: u64,
) -> Result<Option<(Integrity, String)>, Error> {
    let mut byte = [0];
    loop {
        match input.read(&mut byte) {
            Ok(0) => return Ok(None),
            Ok(1) => {
                *bytes = bytes
                    .checked_add(1)
                    .filter(|&n| n <= limit)
                    .ok_or_else(|| {
                        Error(
                            "EDF recovery decoded source exceeds byte budget/declared shape".into(),
                        )
                    })?;
                output.write_all(&byte)?;
            }
            Ok(_) => unreachable!(),
            Err(error) => {
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                if !matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::InvalidInput
                        | std::io::ErrorKind::InvalidData
                ) {
                    return Err(Error(error.to_string()));
                }
                let message = error.to_string();
                let lower = message.to_ascii_lowercase();
                let integrity = if lower.contains("checksum") || lower.contains("crc") {
                    Integrity::ChecksumFailure
                } else if error.kind() == std::io::ErrorKind::UnexpectedEof {
                    Integrity::UnverifiedTruncated
                } else {
                    Integrity::DecodeFailure
                };
                return Ok(Some((integrity, message)));
            }
        }
    }
}
enum HeaderAdmission {
    Complete,
    Fault(Integrity, String),
}
fn admit_header(
    input: &mut (impl BufRead + Seek),
    codec: Compression,
    limit: usize,
) -> Result<HeaderAdmission, Error> {
    let start = input.stream_position()?;
    let mut count = 0usize;
    fn take(input: &mut impl Read, count: &mut usize, limit: usize) -> Result<Option<u8>, Error> {
        if *count >= limit {
            return Err(Error(
                "EDF recovery compression header exceeds header budget".into(),
            ));
        }
        let mut byte = [0];
        if input.read(&mut byte)? == 0 {
            return Ok(None);
        }
        *count += 1;
        Ok(Some(byte[0]))
    }
    let incomplete = || {
        HeaderAdmission::Fault(
            Integrity::UnverifiedTruncated,
            "incomplete compressed member header".into(),
        )
    };
    if codec == Compression::Bzip2 {
        let mut prefix = [0; 4];
        for value in &mut prefix {
            let Some(v) = take(input, &mut count, limit)? else {
                return Ok(incomplete());
            };
            *value = v;
        }
        if &prefix[..3] != b"BZh" || !b"123456789".contains(&prefix[3]) {
            return Ok(HeaderAdmission::Fault(
                Integrity::DecodeFailure,
                "invalid bzip2 member header".into(),
            ));
        }
    } else {
        let mut fixed = [0; 10];
        for value in &mut fixed {
            let Some(v) = take(input, &mut count, limit)? else {
                return Ok(incomplete());
            };
            *value = v;
        }
        if fixed[..3] != [31, 139, 8] || fixed[3] & 0xe0 != 0 {
            return Ok(HeaderAdmission::Fault(
                Integrity::DecodeFailure,
                "invalid gzip member header".into(),
            ));
        }
        let flags = fixed[3];
        if flags & 4 != 0 {
            let Some(a) = take(input, &mut count, limit)? else {
                return Ok(incomplete());
            };
            let Some(b) = take(input, &mut count, limit)? else {
                return Ok(incomplete());
            };
            let length = u16::from_le_bytes([a, b]);
            for _ in 0..length {
                if take(input, &mut count, limit)?.is_none() {
                    return Ok(incomplete());
                }
            }
        }
        for flag in [8, 16] {
            if flags & flag != 0 {
                loop {
                    match take(input, &mut count, limit)? {
                        Some(0) => break,
                        Some(_) => {}
                        None => return Ok(incomplete()),
                    }
                }
            }
        }
        if flags & 2 != 0 {
            for _ in 0..2 {
                if take(input, &mut count, limit)?.is_none() {
                    return Ok(incomplete());
                }
            }
        }
    }
    input.seek(SeekFrom::Start(start))?;
    Ok(HeaderAdmission::Complete)
}
fn decode_zlib(
    input: &mut Cursor<&[u8]>,
    output: &mut Vec<u8>,
    limit: u64,
) -> Result<DecodeSummary, Error> {
    let mut bytes = 0;
    let (fault, position) = {
        let mut reader = flate2::bufread::ZlibDecoder::new(&mut *input);
        let fault = drain_prefix(&mut reader, output, &mut bytes, limit)?;
        drop(reader);
        (fault, input.position())
    };
    let mut integrity = Integrity::ContainerComplete;
    let mut diagnostic = None;
    if let Some((state, message)) = fault {
        integrity = state;
        diagnostic = Some(message);
    } else if position != input.get_ref().len() as u64
        && !{
            let all = input.get_ref();
            let tail = &all[position as usize..];
            all.len().is_multiple_of(512)
                && tail.len() < 512
                && tail.iter().all(|&v| v == 0 || v.is_ascii_whitespace())
        }
    {
        integrity = Integrity::DecodeFailure;
        diagnostic = Some("unexplained bytes after zlib payload".into());
    }
    Ok(DecodeSummary {
        bytes,
        verified: if integrity == Integrity::ContainerComplete {
            bytes
        } else {
            0
        },
        integrity,
        members: Vec::new(),
        diagnostic,
    })
}
