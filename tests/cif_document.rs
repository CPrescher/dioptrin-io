#![cfg(feature = "cbf")]
use dioptrin_io::cbf::{
    Limits,
    document::{Coverage, Document, Provenance, Reader},
};
use std::io::{self, Read, Write};
const SOURCE: &[u8] = include_bytes!("data/companions/scalars-loops.cif");
const DIAGNOSTICS: &[u8] = include_bytes!("data/companions/diagnostics.cif");
const DUPLICATES: &[u8] = include_bytes!("data/companions/duplicates.cif");
#[test]
fn shared_projection_preserves_original_scalars_loops_quotes_and_numeric_text() {
    let doc = Document::from_bytes(SOURCE, Limits::default()).unwrap();
    assert_eq!(doc.original, SOURCE);
    assert_eq!(doc.provenance, Provenance::Buffer);
    assert_eq!(doc.coverage, Coverage::HeaderProjection);
    assert!(doc.coverage_warning.contains("full CIF grammar"));
    assert!(doc.cif.diagnostics.is_empty());
    assert_eq!(doc.cif.data_blocks, ["independent"]);
    for (key, value) in [
        ("_title", "two words"),
        ("_double", "double quoted"),
        ("_numeric_text", "00012.340(5)"),
        ("_unknown", "?"),
        ("_inapplicable", "."),
        ("_multiline", "first line\nsecond line"),
    ] {
        assert_eq!(doc.cif.fields[key], value);
    }
    assert_eq!(doc.cif.loops[0].columns, ["_atom_label", "_atom_x"]);
    assert_eq!(
        doc.cif.loops[0].rows,
        [["A1", "1.234(5)"], ["B two", "-0.000"]]
    );
}
#[test]
fn duplicates_partial_rows_unknown_syntax_and_boundary_are_explicit_projections() {
    let doc = Document::from_bytes(DIAGNOSTICS, Limits::default()).unwrap();
    assert_eq!(doc.original, DIAGNOSTICS);
    assert_eq!(doc.cif.fields["_duplicate"], "final");
    assert_eq!(
        doc.cif.entries[..2],
        [
            ("_duplicate".to_string(), "first".to_string()),
            ("_duplicate".to_string(), "final".to_string())
        ]
    );
    assert_eq!(doc.cif.data_blocks, ["first", "second"]);
    assert_eq!(doc.cif.loops[0].rows, [["1", "2"]]);
    assert!(!doc.cif.fields.contains_key("_orphan"));
    assert!(!doc.cif.fields.contains_key("_after_boundary"));
    for text in [
        "duplicate CIF item",
        "unsupported CIF token save_",
        "has no value",
        "incomplete CIF loop row",
        "multiple CIF data blocks",
    ] {
        assert!(
            doc.cif.diagnostics.iter().any(|item| item.contains(text)),
            "{text}"
        );
    }
    assert!(doc.coverage_warning.contains("_array_data.data"));
    let duplicate = Document::from_bytes(DUPLICATES, Limits::default()).unwrap();
    assert_eq!(duplicate.cif.entries.len(), 3);
    assert_eq!(duplicate.cif.diagnostics.len(), 1);
}
#[test]
fn invalid_text_truncated_values_and_quoted_boundary_keep_bytes_and_diagnostics() {
    for source in [b"_x 'unfinished".as_slice(), b"_x\n;unfinished", b"\xff"] {
        let doc = Document::from_bytes(source, Limits::default()).unwrap();
        assert_eq!(doc.original, source);
        assert!(!doc.cif.diagnostics.is_empty());
    }
    let doc =
        Document::from_bytes(b"_x '_array_data.data'\n_y retained", Limits::default()).unwrap();
    assert_eq!(doc.cif.fields["_x"], "_array_data.data");
    assert_eq!(doc.cif.fields["_y"], "retained");
    assert!(doc.cif.diagnostics.is_empty());
}
struct Nonseek<'a>(&'a [u8]);
impl Read for Nonseek<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = out.len().min(self.0.len()).min(5);
        out[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}
#[test]
fn byte_stream_and_retained_metadata_limits_are_bounded() {
    let doc = Document::from_reader(Nonseek(SOURCE), Limits::default()).unwrap();
    assert_eq!(doc.original, SOURCE);
    assert_eq!(doc.provenance, Provenance::StreamCapture);
    for limits in [
        Limits {
            encoded_bytes: SOURCE.len() - 1,
            ..Limits::default()
        },
        Limits {
            header_bytes: SOURCE.len() - 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: SOURCE.len(),
            ..Limits::default()
        },
        Limits {
            metadata_bytes: SOURCE.len() + std::mem::size_of::<Document>() + 1,
            ..Limits::default()
        },
    ] {
        assert!(Document::from_bytes(SOURCE, limits).is_err());
        assert!(Document::from_reader(Nonseek(SOURCE), limits).is_err());
    }
    let mut original = SOURCE.to_vec();
    let doc = Document::from_bytes(&original, Limits::default()).unwrap();
    original.fill(0);
    assert_eq!(doc.original, SOURCE);
}
#[test]
fn path_access_rejects_source_growth_and_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.cif");
    std::fs::write(&path, SOURCE).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(
        reader.metadata().unwrap().provenance,
        Provenance::PathSnapshot
    );
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"# changed")
        .unwrap();
    assert!(reader.metadata().is_err());
    std::fs::write(&path, SOURCE).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let other = dir.path().join("other.cif");
    std::fs::write(&other, SOURCE).unwrap();
    std::fs::rename(other, &path).unwrap();
    assert!(reader.metadata().is_err() && reader.source_path().is_err());
}
