#![cfg(any(feature = "cbf", feature = "tiff"))]
use dioptrin_io::pilatus::{CONVENTION, Document, Limits, Provenance, Reader, Value};
use std::io::{self, Read, Write};
const SOURCE: &[u8] = include_bytes!("data/lowlevel/pilatus.txt");
#[test]
fn original_units_repetitions_and_readonly_mapping_are_preserved() {
    let d = Document::from_bytes(SOURCE, Limits::default()).unwrap();
    assert_eq!(d.original, SOURCE);
    assert_eq!(d.convention, CONVENTION);
    assert_eq!(d.provenance, Provenance::Buffer);
    assert!(d.header.contains("Exposure_time"));
    assert_eq!(d.header.get("Exposure_time"), Some(&Value::Float(0.2)));
    assert_eq!(d.header.fields["Exposure_time"], "0.200 s");
    assert_eq!(
        d.header
            .entries
            .iter()
            .filter(|(k, _)| k == "Exposure_time")
            .count(),
        2
    );
    assert_eq!(d.header.get("N_oscillations"), Some(&Value::Integer(2)));
    let Value::Float(value) = d.header.get("Start_angle").unwrap() else {
        panic!()
    };
    assert_eq!(value.to_bits(), (-0.0f64).to_bits());
    assert!(!d.header.contains("Unknown"));
    assert!(d.header.get("missing").is_none());
}
#[test]
fn invalid_fields_conventions_utf8_and_limits_are_explicit() {
    let d = Document::from_bytes(
        include_bytes!("data/lowlevel/pilatus-invalid.txt"),
        Limits::default(),
    )
    .unwrap();
    assert!(matches!(
        d.header.get("Exposure_time"),
        Some(Value::Invalid(_))
    ));
    assert!(!d.diagnostics.is_empty());
    assert!(Document::from_bytes_with_convention(SOURCE, "unknown", Limits::default()).is_err());
    assert!(Document::from_bytes(b"\xff", Limits::default()).is_err());
    for limits in [
        Limits {
            header_bytes: SOURCE.len() - 1,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: SOURCE.len() - 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: SOURCE.len(),
            ..Limits::default()
        },
    ] {
        assert!(Document::from_bytes(SOURCE, limits).is_err());
    }
}
struct Stream<'a>(&'a [u8]);
impl Read for Stream<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = out.len().min(self.0.len()).min(5);
        out[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}
#[test]
fn sourcechecked_path_and_owned_stream_have_clear_contracts() {
    let d = Document::from_reader(Stream(SOURCE), Limits::default()).unwrap();
    assert_eq!(d.original, SOURCE);
    assert_eq!(d.provenance, Provenance::StreamCapture);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("header.txt");
    std::fs::write(&path, SOURCE).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"# edit")
        .unwrap();
    assert!(reader.metadata().is_err());
    assert!(Reader::open(dir.path(), Limits::default()).is_err());
}

#[test]
fn explicit_cleaned_view_preserves_raw_and_diagnoses_reference_index_errors() {
    let d = Document::from_bytes(b"# Detector: PILATUS\\\n# 100K\n", Limits::default()).unwrap();
    let cleaned = d.cleaned_lines(10000).unwrap();
    assert_eq!(cleaned.lines.len(), 1);
    assert_eq!(d.original, b"# Detector: PILATUS\\\n# 100K\n");
    let d = Document::from_bytes(b"# Lone\n", Limits::default()).unwrap();
    assert!(!d.diagnostics.is_empty());
    assert!(d.header.typed_fields.is_empty());
    let d = Document::from_bytes(b"# Detector: terminal\\", Limits::default()).unwrap();
    assert!(!d.cleaned_lines(10000).unwrap().diagnostics.is_empty());
    assert!(d.cleaned_lines(1).is_err());
}

#[test]
fn continuation_diagnostics_match_exact_reference_line_end_condition() {
    let d = Document::from_bytes(b"# Detector abc\\  ", Limits::default()).unwrap();
    assert!(!d.diagnostics.iter().any(|d| d.contains("terminal")));
    assert!(d.cleaned_lines(10000).unwrap().diagnostics.is_empty());
    let d = Document::from_bytes(
        b"# Exposure_time 1 s\r# Exposure_time 2 s",
        Limits::default(),
    )
    .unwrap();
    assert!(d.diagnostics.is_empty());
    assert_eq!(d.header.get("Exposure_time"), Some(&Value::Float(2.0)));
    assert_eq!(d.header.entries.len(), 2);
    assert_eq!(
        d.cleaned_lines(10000).unwrap().lines,
        ["  Exposure_time 1 s", "  Exposure_time 2 s"]
    );
    assert_eq!(d.original, b"# Exposure_time 1 s\r# Exposure_time 2 s");
}

#[test]
fn universal_line_boundaries_preserve_fields_and_charge_lines_before_allocation() {
    let separators = [
        "\n", "\r\n", "\r", "\u{b}", "\u{c}", "\u{1c}", "\u{1d}", "\u{1e}", "\u{85}", "\u{2028}",
        "\u{2029}",
    ];
    let mut source = String::new();
    for (index, separator) in separators.iter().enumerate() {
        source.push_str(&format!("# Exposure_time {} s{}", index + 1, separator));
    }
    let d = Document::from_bytes(source.as_bytes(), Limits::default()).unwrap();
    assert_eq!(d.original, source.as_bytes());
    assert_eq!(d.header.entries.len(), 11);
    assert_eq!(d.header.get("Exposure_time"), Some(&Value::Float(11.0)));
    assert_eq!(d.cleaned_lines(100000).unwrap().lines.len(), 11);
    assert!(d.diagnostics.is_empty());
    let many = "\u{2028}".repeat(100);
    assert!(dioptrin_io::pilatus::Header::parse(&many, 4000).is_err());
    assert!(dioptrin_io::pilatus::clean_string(&many, 4000).is_err());
}
#[test]
fn cr_continuations_and_lf_crlf_existing_results_match_without_source_normalization() {
    let raw = b"# Detector: PILATUS\\\r# 100K\r# Exposure_time 1 s\r# Exposure_time 2 s\r";
    let d = Document::from_bytes(raw, Limits::default()).unwrap();
    assert_eq!(d.original, raw);
    assert_eq!(
        d.header.get("Detector"),
        Some(&Value::Text("PILATUS 100K".into()))
    );
    assert_eq!(d.header.get("Exposure_time"), Some(&Value::Float(2.0)));
    assert_eq!(
        d.cleaned_lines(10000).unwrap().lines,
        [
            "  Detector  PILATUS  100K",
            "  Exposure_time 1 s",
            "  Exposure_time 2 s"
        ]
    );
    let lf = Document::from_bytes(SOURCE, Limits::default()).unwrap();
    let crlf = std::str::from_utf8(SOURCE).unwrap().replace('\n', "\r\n");
    let crlf = Document::from_bytes(crlf.as_bytes(), Limits::default()).unwrap();
    assert_eq!(lf.header.typed_fields, crlf.header.typed_fields);
    assert_eq!(lf.header.entries, crlf.header.entries);
    assert_eq!(
        lf.cleaned_lines(100000).unwrap().lines,
        crlf.cleaned_lines(100000).unwrap().lines
    );
}
