//! Content evidence independent of decoder availability. Inspection is not a
//! successful read: malformed payloads still fail their selected decoder.
use std::fmt;

/// Number of unwrapped bytes sufficient for all fixed-offset probes here.
pub const PREFIX_BYTES: usize = 8192;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Codec {
    Npy,
    Dtrek,
    Fit2dMask,
    HiPic,
    Pnm,
    Fit2dSpreadsheet,
    Edf,
    Cbf,
    Tiff,
    Jpeg,
    Jpeg2000,
    Bruker,
    Oxford,
    Raxis,
    Ge,
    Pixi,
    Spe,
    Mrc,
    Dm3,
    Fit2d,
    Kcd,
    Mpa,
    Esperanto,
    Mar345,
    Xsd,
    Hdf5,
    Eiger,
    Lima,
    Lambda,
    Sparse,
    Xcalibur,
}
impl Codec {
    #[must_use]
    pub fn feature(self) -> &'static str {
        match self {
            Self::Npy
            | Self::Dtrek
            | Self::Fit2dMask
            | Self::HiPic
            | Self::Pnm
            | Self::Fit2dSpreadsheet => "native",
            Self::Edf => "edf",
            Self::Cbf => "cbf",
            Self::Tiff => "tiff",
            Self::Jpeg => "jpeg",
            Self::Jpeg2000 => "jpeg2000",
            Self::Bruker => "bruker",
            Self::Oxford => "oxford",
            Self::Raxis => "raxis",
            Self::Ge | Self::Pixi => "fixed",
            Self::Spe | Self::Mrc | Self::Dm3 => "containers",
            Self::Fit2d => "fit2d",
            Self::Kcd => "kcd",
            Self::Mpa => "mpa",
            Self::Esperanto => "esperanto",
            Self::Mar345 => "mar345",
            Self::Xsd => "xsd",
            Self::Hdf5 | Self::Eiger | Self::Lima | Self::Lambda => "hdf5",
            Self::Sparse => "sparse",
            Self::Xcalibur => "xcalibur",
        }
    }
    #[must_use]
    pub fn available(self) -> bool {
        [
            ("native", cfg!(feature = "native")),
            ("edf", cfg!(feature = "edf")),
            ("cbf", cfg!(feature = "cbf")),
            ("tiff", cfg!(feature = "tiff")),
            ("jpeg", cfg!(feature = "jpeg")),
            ("jpeg2000", cfg!(feature = "jpeg2000")),
            ("bruker", cfg!(feature = "bruker")),
            ("oxford", cfg!(feature = "oxford")),
            ("raxis", cfg!(feature = "raxis")),
            ("fixed", cfg!(feature = "fixed")),
            ("containers", cfg!(feature = "containers")),
            ("fit2d", cfg!(feature = "fit2d")),
            ("kcd", cfg!(feature = "kcd")),
            ("mpa", cfg!(feature = "mpa")),
            ("esperanto", cfg!(feature = "esperanto")),
            ("mar345", cfg!(feature = "mar345")),
            ("xsd", cfg!(feature = "xsd")),
            ("hdf5", cfg!(feature = "hdf5")),
            ("sparse", cfg!(feature = "sparse")),
            ("xcalibur", cfg!(feature = "xcalibur")),
        ]
        .contains(&(self.feature(), true))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    Signature,
    StructuredHeader,
    /// Not sufficient to choose a decoder without an explicit caller choice.
    AmbiguousHeader,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub codec: Codec,
    pub evidence: Evidence,
    pub reason: &'static str,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detection {
    pub candidates: Vec<Candidate>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}
impl Detection {
    pub(crate) fn add(&mut self, codec: Codec, evidence: Evidence, reason: &'static str) {
        self.candidates.push(Candidate {
            codec,
            evidence,
            reason,
        });
    }
    /// Resolve content evidence without trying decoders or filename fallbacks.
    /// # Errors
    /// Unknown/ambiguous evidence or the selected optional feature is disabled.
    pub fn require_unique(&self) -> Result<Codec, Error> {
        let [candidate] = self.candidates.as_slice() else {
            return Err(Error(format!(
                "unknown or ambiguous detector content: {:?}",
                self.candidates
            )));
        };
        if candidate.evidence == Evidence::AmbiguousHeader {
            return Err(Error(format!(
                "ambiguous content requires an explicit format: {:?}",
                candidate.codec
            )));
        }
        if !candidate.codec.available() {
            return Err(Error(format!(
                "format {:?} requires disabled feature {}",
                candidate.codec,
                candidate.codec.feature()
            )));
        }
        Ok(candidate.codec)
    }
}
/// Inspect at most PREFIX_BYTES original unwrapped bytes. No extension, display
/// correction, raw layout or byte-offset dialect is guessed. HDF5 detector
/// conventions require the separately bounded container inspection API.
#[must_use]
pub fn probe(prefix: &[u8]) -> Detection {
    use Codec as C;
    use Evidence as E;
    let b = &prefix[..prefix.len().min(PREFIX_BYTES)];
    let mut result = Detection::default();
    for (magic, codec) in [
        (&b"\x93NUMPY"[..], C::Npy),
        (b"II*\0", C::Tiff),
        (b"MM\0*", C::Tiff),
        (b"II+\0", C::Tiff),
        (b"MM\0+", C::Tiff),
        (b"\xff\xd8\xff", C::Jpeg),
        (b"\0\0\0\x0cjP  \r\n\x87\n", C::Jpeg2000),
        (b"\xff\x4f\xff\x51", C::Jpeg2000),
        (b"\x89HDF\r\n\x1a\n", C::Hdf5),
        (b"OD", C::Oxford),
        (b"IM", C::HiPic),
        (b"R-AXIS", C::Raxis),
        (b"ADEPT", C::Ge),
        (b"M\0\0\0A\0\0\0S\0\0\0K\0\0\0", C::Fit2dMask),
        (b"\\$FFF_START", C::Fit2d),
        (b"\x0a\xb8\x03\0", C::Pixi),
        (b"ESPERANTO FORMAT", C::Esperanto),
    ] {
        if b.starts_with(magic) {
            result.add(codec, E::Signature, "format signature");
        }
    }
    let cbf_marker = b"###CBF: VERSION";
    let first_line = b
        .split(|&v| v == b'\r' || v == b'\n')
        .next()
        .unwrap_or_default();
    if first_line
        .get(..cbf_marker.len())
        .is_some_and(|v| v.eq_ignore_ascii_case(cbf_marker))
        && first_line
            .get(cbf_marker.len())
            .is_none_or(u8::is_ascii_whitespace)
    {
        result.add(C::Cbf, E::Signature, "CBF version marker on first line");
    } else if cif_binary_array(b) {
        result.add(
            C::Cbf,
            E::StructuredHeader,
            "CIF array data and binary section framing",
        );
    }
    if !b.starts_with(b"\x89HDF\r\n\x1a\n")
        && [512, 1024, 2048, 4096]
            .iter()
            .any(|&offset| b.get(offset..offset + 8) == Some(b"\x89HDF\r\n\x1a\n"))
    {
        result.add(
            C::Hdf5,
            E::Signature,
            "HDF5 superblock after bounded user block",
        );
    }
    if [b"\x2d\x04", b"\xd2\x04", b"\x04\x2d", b"\x04\xd2"]
        .iter()
        .any(|p| b.starts_with(*p))
    {
        result.add(
            C::Mar345,
            E::Signature,
            "MAR image marker in stored byte order",
        );
    }
    if b.get(..b.len().min(80))
        .and_then(|line| line.strip_prefix(b"FORMAT"))
        .is_some_and(|tail| {
            let tail = tail.trim_ascii_start();
            tail.strip_prefix(b":").is_some_and(|value| {
                value
                    .trim_ascii_start()
                    .first()
                    .is_some_and(u8::is_ascii_digit)
            })
        })
    {
        result.add(C::Bruker, E::StructuredHeader, "Bruker FORMAT record");
    }
    let trimmed = b
        .iter()
        .position(|&v| !v.is_ascii_whitespace())
        .map_or(&b[b.len()..], |n| &b[n..]);
    if let Some(header) = trimmed.strip_prefix(b"{") {
        let header = &header[..header
            .iter()
            .position(|&v| v == b'}')
            .unwrap_or(header.len())];
        let dtrek = assignment(header, b"HEADER_BYTES");
        let edf = [
            &b"Dim_1"[..],
            b"Dim_2",
            b"DataType",
            b"ByteOrder",
            b"EDF_DataBlockID",
            b"EDF_DataFormatVersion",
            b"EDF_DataBlocks",
            b"EDF_BlockBoundary",
            b"EDF_BinarySize",
            b"EDF_HeaderSize",
            b"Edf_HeaderSize",
            b"HeaderID",
        ]
        .iter()
        .any(|key| assignment(header, key));
        if dtrek {
            result.add(C::Dtrek, E::StructuredHeader, "HEADER_BYTES assignment");
        }
        if edf {
            result.add(C::Edf, E::StructuredHeader, "EDF header field assignment");
        }
        if !dtrek && !edf {
            result.add(
                C::Edf,
                E::AmbiguousHeader,
                "an opening brace alone does not identify EDF",
            );
        }
    }
    if b.len() >= 3 && b[0] == b'P' && (b'1'..=b'6').contains(&b[1]) && b[2].is_ascii_whitespace() {
        result.add(C::Pnm, E::Signature, "Netpbm type and separator");
    }
    if b.starts_with(b"No")
        && b.windows(11).any(|v| v == b"X dimension")
        && b.windows(11).any(|v| v == b"Y dimension")
    {
        result.add(C::Kcd, E::StructuredHeader, "KCD dimensions and preamble");
    }
    if mpa_records(trimmed) {
        result.add(
            C::Mpa,
            E::StructuredHeader,
            "MPA format declaration with ADC and data records",
        );
    }
    if b.len() >= 12
        && b[..4] == [0, 0, 0, 3]
        && u32::from_be_bytes(b[4..8].try_into().unwrap()) >= 12
        && u32::from_be_bytes(b[8..12].try_into().unwrap()) <= 1
        && b.get(208..212) != Some(b"MAP ")
    {
        result.add(
            C::Dm3,
            E::StructuredHeader,
            "DM3 version/length/byte-order declarations",
        );
    }
    if b.get(208..212) == Some(b"MAP ") {
        result.add(C::Mrc, E::Signature, "MRC MAP marker at offset 208");
    }
    if b.len() >= 1996 {
        let version = f32::from_le_bytes(b[1992..1996].try_into().unwrap());
        let ty = u16::from_le_bytes(b[108..110].try_into().unwrap());
        if (2.0..4.0).contains(&version)
            && matches!(ty, 0..=3)
            && b[42..44] != [0, 0]
            && b[656..658] != [0, 0]
        {
            result.add(
                C::Spe,
                E::StructuredHeader,
                "SPE version/type/dimension declarations",
            );
        }
    }
    if xml_array_element(b) {
        result.add(
            C::Xsd,
            E::StructuredHeader,
            "XML array/XSData start element",
        );
    }
    if b.len() >= 1854
        && (0x10001..=0x1000cu32).contains(&u32::from_le_bytes(b[..4].try_into().unwrap()))
    {
        result.add(
            C::Xcalibur,
            E::StructuredHeader,
            "CCD characteristics version and complete fixed header",
        );
    }
    // A blank GE header is indistinguishable from arbitrary zero-filled input.
    // SPE and other positive evidence take precedence over that weak observation.
    if result.candidates.is_empty() && b.len() >= 10 && b[..10] == [0; 10] {
        result.add(
            C::Ge,
            E::AmbiguousHeader,
            "blank prefix is not unique to GE",
        );
    }
    // Spreadsheet has no magic: a dimensions line is merely a candidate, never
    // permission to turn arbitrary numerical text into detector data.
    if result.candidates.is_empty()
        && let Ok(s) = std::str::from_utf8(b)
        && s.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .any(|l| {
                let mut p = l.split_whitespace();
                p.next().is_some_and(|s| s.parse::<usize>().is_ok())
                    && p.next().is_some_and(|s| s.parse::<usize>().is_ok())
            })
    {
        result.add(
            C::Fit2dSpreadsheet,
            E::AmbiguousHeader,
            "numerical dimensions without format signature",
        );
    }
    result
}

// The versionless writer output still has CIF syntax: an array-data item opens
// a semicolon value immediately followed by the MIME binary section. Ignore
// comments and other semicolon values rather than matching their contents.
fn cif_binary_array(prefix: &[u8]) -> bool {
    let mut started = false;
    let mut multiline = false;
    let mut array_state = 0;
    for raw in prefix.split(|&v| v == b'\n') {
        let line = raw.trim_ascii();
        if line.is_empty() || (!multiline && line.starts_with(b"#")) {
            continue;
        }
        if !started {
            if !(line.starts_with(b"data_") && line.len() > 5 || line == b"_array_data.data") {
                return false;
            }
            started = true;
        }
        if array_state == 1 {
            if line != b";" {
                return false;
            }
            array_state = 2;
        } else if array_state == 2 {
            return line == b"--CIF-BINARY-FORMAT-SECTION--";
        } else if raw.starts_with(b";") {
            multiline = !multiline;
        } else if !multiline && line == b"_array_data.data" {
            array_state = 1;
        }
    }
    false
}

fn mpa_records(prefix: &[u8]) -> bool {
    // A structured text header starts with an assignment, not an XML/CIF
    // container which happens to quote the same records in a text value.
    let first = prefix
        .split(|&v| v == b'\r' || v == b'\n')
        .next()
        .unwrap_or_default();
    let Some(equal) = first.iter().position(|&v| v == b'=') else {
        return false;
    };
    if equal == 0
        || !first[..equal]
            .iter()
            .all(|v| v.is_ascii_alphanumeric() || *v == b'_')
    {
        return false;
    }
    let mut format = false;
    let mut adc = false;
    let mut data = false;
    for line in prefix
        .split(|&v| v == b'\r' || v == b'\n')
        .map(<[u8]>::trim_ascii)
    {
        format |= line.strip_prefix(b"mpafmt=").is_some_and(|v| !v.is_empty());
        adc |= line
            .strip_prefix(b"[ADC")
            .and_then(|v| v.strip_suffix(b"]"))
            .is_some_and(|v| !v.is_empty() && v.iter().all(u8::is_ascii_digit));
        data |= line
            .strip_prefix(b"[DATA")
            .and_then(|v| v.strip_suffix(b"]"))
            .is_some_and(|v| {
                v.first().is_some_and(u8::is_ascii_digit)
                    && v.iter()
                        .all(|v| v.is_ascii_digit() || v.is_ascii_whitespace() || *v == b',')
            });
    }
    format && adc && data
}

fn assignment(header: &[u8], key: &[u8]) -> bool {
    header
        .split(|&b| b == b';' || b == b'\n' || b == b'\r')
        .any(|field| {
            field
                .trim_ascii_start()
                .strip_prefix(key)
                .is_some_and(|tail| tail.trim_ascii_start().starts_with(b"="))
        })
}
/// Small lexical probe, not an XML parser. It skips quoted attributes,
/// declarations, comments and CDATA so their text cannot become format evidence.
fn xml_array_element(prefix: &[u8]) -> bool {
    let prefix = prefix
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(prefix)
        .trim_ascii_start();
    if !prefix.starts_with(b"<") {
        return false;
    }
    let mut remaining = prefix;
    while let Some(start) = remaining.iter().position(|&b| b == b'<') {
        remaining = &remaining[start..];
        let terminator = if remaining.starts_with(b"<!--") {
            Some(&b"-->"[..])
        } else if remaining.starts_with(b"<![CDATA[") {
            Some(&b"]]>"[..])
        } else if remaining.starts_with(b"<?") {
            Some(&b"?>"[..])
        } else {
            None
        };
        if let Some(end) = terminator {
            let Some(offset) = remaining.windows(end.len()).position(|v| v == end) else {
                return false;
            };
            remaining = &remaining[offset + end.len()..];
            continue;
        }
        let mut quote = None;
        let mut brackets = 0usize;
        let mut stop = None;
        for (i, &byte) in remaining.iter().enumerate().skip(1) {
            if let Some(q) = quote {
                if byte == q {
                    quote = None;
                }
                continue;
            }
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'[' => brackets += 1,
                b']' => brackets = brackets.saturating_sub(1),
                b'>' if brackets == 0 => {
                    stop = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(stop) = stop else {
            return false;
        };
        if !remaining.starts_with(b"<!") && !remaining.starts_with(b"</") {
            let name = &remaining[1..stop];
            let end = name
                .iter()
                .position(|b| b.is_ascii_whitespace() || *b == b'/')
                .unwrap_or(name.len());
            let local = name[..end]
                .rsplit(|&b| b == b':')
                .next()
                .unwrap_or_default();
            if local == b"array" || local.starts_with(b"XSData") {
                return true;
            }
        }
        remaining = &remaining[stop + 1..];
    }
    false
}
