#![cfg(feature = "cbf")]
use dioptrin_io::cbf::syntax::{Composite, Document, Limits, Null, Span, Style, Token, Version};
fn parse(body: &str) -> Result<Document, dioptrin_io::cbf::Error> {
    Document::from_bytes(format!("#\\#CIF_2.0\n{body}").as_bytes(), Limits::default())
}
fn scalar(doc: &Document, name: &str) -> dioptrin_io::cbf::syntax::Value {
    doc.scalar(&doc.blocks[0].items, name).unwrap().value
}

#[test]
fn nested_composites_preserve_original_lexemes_null_types_and_large_numbers() {
    let doc =
        parse("data_Unicode\n_list [9007199254740993 ? '.' [\"αβ\" {\"key\":[1 2]}] [] {}]\n")
            .unwrap();
    assert_eq!(doc.version, Version::Cif20);
    let value = scalar(&doc, "_LIST");
    assert_eq!(value.token.style, Style::List);
    assert!(
        doc.raw(value.token)
            .unwrap()
            .starts_with(b"[9007199254740993")
    );
    let Some(Composite::List(values)) = doc.composite(value) else {
        panic!("list");
    };
    assert_eq!(doc.text(values[0].token), Some("9007199254740993"));
    assert_eq!(values[1].null, Some(Null::Unknown));
    assert_eq!(values[2].null, None);
    let Some(Composite::List(inner)) = doc.composite(values[3]) else {
        panic!("nested list");
    };
    assert_eq!(doc.logical_text(inner[0].token).unwrap().text, "αβ");
    let found = doc.table_get(inner[1], "key").unwrap().unwrap();
    let Some(Composite::List(pair)) = doc.composite(found) else {
        panic!("table/list");
    };
    assert_eq!(pair.len(), 2);
    assert_eq!(doc.composites.len(), 6);
}
#[test]
fn versioned_quote_rules_and_unicode_line_semantics_are_explicit_views() {
    let source = "#\\#CIF_2.0\r\ndata_x\r\n_single 'α\u{2028}β'\r\n_triple '''first\r\n;not a delimiter\rsecond\nlast'''\r\n";
    let doc = Document::from_bytes(source.as_bytes(), Limits::default()).unwrap();
    assert_eq!(doc.original, source.as_bytes());
    let value = scalar(&doc, "_triple");
    assert_eq!(value.token.style, Style::TripleSingleQuoted);
    assert_eq!(
        doc.text(value.token),
        Some("first\r\n;not a delimiter\rsecond\nlast")
    );
    let logical = doc.logical_text(value.token).unwrap();
    assert_eq!(logical.text, "first\n;not a delimiter\nsecond\nlast");
    assert!(logical.normalized_eol);
    assert!(!logical.folded);
    assert_eq!(logical.prefix, None);
    assert_eq!(
        doc.logical_text(scalar(&doc, "_single").token)
            .unwrap()
            .text,
        "α\u{2028}β"
    );
    assert!(parse("data_x _a 'a dog's life'").is_err());
    let doc = Document::from_bytes(b"data_x _a 'a dog's life'", Limits::default()).unwrap();
    assert_eq!(doc.version, Version::Cif11);
}
#[test]
fn text_prefix_folding_and_false_prefixes_follow_the_normative_procedures() {
    let cases = [
        (
            ";P>\\\nP>data_example\nP>;embedded\n;",
            "data_example\n;embedded",
            Some("P>"),
            false,
        ),
        (
            ";\\\nfirst\\  \n second\ntrailing\\\n;",
            "first second\ntrailing",
            None,
            true,
        ),
        (
            ";P>\\\\ \nP>first\\\nP> second\n;",
            "first second",
            Some("P>"),
            true,
        ),
        (
            ";P>\\\nP>first\\\nP> second\n;",
            "first\\\n second",
            Some("P>"),
            false,
        ),
        (
            ";P>\\\nP>first\nunprefixed\n;",
            "P>\\\nP>first\nunprefixed",
            None,
            false,
        ),
        (";\\\n;", "", None, true),
        (";P>\\\n;", "", Some("P>"), false),
        (";P>\\\\\n;", "", Some("P>"), true),
    ];
    for (raw, expected, prefix, folded) in cases {
        let doc = parse(&format!("data_x\n_text\n{raw}\n")).unwrap();
        let token = scalar(&doc, "_text").token;
        let view = doc.logical_text(token).unwrap();
        assert_eq!(view.text, expected, "{raw:?}");
        assert_eq!(view.prefix.as_deref(), prefix);
        assert_eq!(view.folded, folded);
        assert_eq!(doc.raw(token), Some(raw.as_bytes()));
    }
}
#[test]
fn canonical_caseless_names_are_not_lowercasing_or_compatibility_folding() {
    let doc = parse("data_Å\n_Straße 1\nsave_É\n_K 2\nsave_\n").unwrap();
    assert!(doc.block("A\u{30a}").is_some());
    assert!(doc.scalar(&doc.blocks[0].items, "_STRASSE").is_some());
    let frame = doc.save_frame(&doc.blocks[0], "e\u{301}").unwrap();
    assert!(doc.scalar(&frame.items, "_k").is_some());
    for body in [
        "data_Å data_A\u{30a}",
        "data_x _Å 1 _a\u{30a} 2",
        "data_x _Straße 1 _STRASSE 2",
        "data_x save_É save_ save_e\u{301} save_",
    ] {
        assert!(parse(body).is_err(), "{body}");
    }
    // Canonical matching leaves compatibility-only equivalences distinct.
    assert!(parse("data_x _① 1 _1 2").is_ok());
    assert!(parse("data_x save_empty save_").is_ok());
    assert!(Document::from_bytes(b"data_x save_empty save_", Limits::default()).is_err());
}
#[test]
fn compound_boundaries_require_whitespace_and_colon_adjacency() {
    for body in [
        "data_x _a [1[2]]",
        "data_x _a [\"a\"\"b\"]",
        "data_x _a [[1][2]]",
        "data_x _a {'key' :1}",
        "data_x _a {bare:1}",
        "data_x _a {;key\n;:1}",
        "data_x _a {'k':}",
        "data_x _a [1}",
        "data_x _a [1]x",
        "data_x _a 'x'_b 1",
        "data_x _a {'a':'v'#comment\n'b':2}",
        "data_x _a {'k':#comment\nword}",
    ] {
        assert!(parse(body).is_err(), "{body}");
    }
    for body in [
        "data_x _a [1]",
        "data_x _a ['a']",
        "data_x _a {'key':1}",
        "data_x _a {'key':[1 2]}",
        "data_x _a [#comment\n1 2]",
        "data_x _a [\n;value\n;]",
        "data_x _a {'colon'::value}",
        "data_x _a {'a':1'b':2}",
        "data_x _a {'a':1#not_comment\n'b':2}",
        "data_x _a {'k': #comment\nword}",
    ] {
        assert!(parse(body).is_ok(), "{body}");
    }
}
#[test]
fn duplicate_table_entries_are_preserved_without_silent_overwrite() {
    let doc = parse("data_x _table {'same':1 'same':2 'Other':3}").unwrap();
    let value = scalar(&doc, "_table");
    let Some(Composite::Table(entries)) = doc.composite(value) else {
        panic!("table");
    };
    assert_eq!(entries.len(), 3);
    assert!(doc.table_get(value, "same").is_err());
    assert!(doc.table_get(value, "other").unwrap().is_none());
    assert_eq!(
        doc.text(doc.table_get(value, "Other").unwrap().unwrap().token),
        Some("3")
    );
}
#[test]
fn unicode_and_version_constraints_are_applied_before_ast_allocation() {
    for c in [
        '\0',
        '\u{7f}',
        '\u{80}',
        '\u{9f}',
        '\u{fdd0}',
        '\u{fffe}',
        '\u{10ffff}',
        '\u{feff}',
    ] {
        assert!(parse(&format!("data_x _a '{c}'")).is_err(), "{c:?}");
    }
    for c in ['\u{a0}', '\u{fdf0}', '\u{1fffd}', '\u{10fffd}'] {
        assert!(parse(&format!("data_x _a '{c}'")).is_ok(), "{c:?}");
    }
    let mut bytes = b"#\\#CIF_2.0\ndata_x _a ".to_vec();
    bytes.push(255);
    assert!(Document::from_bytes(&bytes, Limits::default()).is_err());
    assert!(Document::from_bytes(b"\xef\xbb\xbf#\\#CIF_2.0\ndata_x", Limits::default()).is_ok());
    assert!(Document::from_bytes(b"#\\#CIF_2.0 extra\ndata_x", Limits::default()).is_err());
    assert!(parse(&format!("data_{} _x 1", "α".repeat(100))).is_ok());
    assert!(parse(&format!("data_x\n#{}", "α".repeat(2047))).is_ok());
    assert!(parse(&format!("data_x\n#{}", "α".repeat(2048))).is_err());
}
#[test]
fn nested_values_view_working_bytes_and_public_mutations_are_bounded() {
    let source = b"#\\#CIF_2.0\ndata_x _a [[1]]";
    assert!(
        Document::from_bytes(
            source,
            Limits {
                nesting: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        Document::from_bytes(
            source,
            Limits {
                values: 2,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        Document::from_bytes(
            source,
            Limits {
                nesting: 129,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let doc = Document::from_bytes(
        b"#\\#CIF_2.0\ndata_x _a 'value'",
        Limits {
            logical_text_bytes: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(doc.logical_text(scalar(&doc, "_a").token).is_err());
    let token = Token {
        span: Span {
            start: usize::MAX,
            end: 0,
        },
        content: Span {
            start: 0,
            end: usize::MAX,
        },
        style: Style::TextField,
    };
    assert!(doc.logical_text(token).is_err());
    assert!(doc.raw(token).is_none());
    let mut value = scalar(&doc, "_a");
    value.composite = Some(usize::MAX);
    assert!(doc.composite(value).is_none());
    assert!(doc.table_get(value, "key").is_err());
    let deep = format!(
        "#\\#CIF_2.0\ndata_x _a {}0{}",
        "[\n".repeat(20000),
        "\n]".repeat(20000)
    );
    assert!(Document::from_bytes(deep.as_bytes(), Limits::default()).is_err());
}
#[test]
fn historical_formerly_unsupported_source_is_read_without_changing_its_bytes() {
    let bytes = include_bytes!("data/cif-syntax/cif2.cif");
    let doc = Document::from_bytes(bytes, Limits::default()).unwrap();
    assert_eq!(doc.version, Version::Cif20);
    assert_eq!(doc.original, bytes);
    assert_eq!(
        doc.logical_text(scalar(&doc, "_unicode").token)
            .unwrap()
            .text,
        "αβ"
    );
}

#[test]
fn cif2_utf8_stream_capture_path_invalidation_and_mutations_are_panic_free() {
    use dioptrin_io::cbf::{document::Provenance, syntax::Reader};
    use std::io::{Read, Write};
    let source = "#\\#CIF_2.0\r\ndata_Å\r\n_a [\"αβ\" {'k':'''line\r\nbreak'''}]\r\n".as_bytes();
    struct Chunks<'a>(&'a [u8]);
    impl Read for Chunks<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let n = self.0.len().min(out.len()).min(3);
            out[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }
    let captured = Document::from_reader(Chunks(source), Limits::default()).unwrap();
    assert_eq!(captured.original, source);
    assert_eq!(captured.provenance, Provenance::StreamCapture);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntax.cif");
    std::fs::write(&path, source).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(reader.metadata().unwrap().version, Version::Cif20);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"# changed")
        .unwrap();
    assert!(reader.metadata().is_err());
    assert!(reader.source_path().is_err());
    for end in 0..source.len() {
        let _ = Document::from_bytes(&source[..end], Limits::default());
    }
    for index in 0..source.len() {
        for byte in [0, 255, b'\'', b'[', b'}', b':', b'\r'] {
            let mut changed = source.to_vec();
            changed[index] = byte;
            let _ = Document::from_bytes(&changed, Limits::default());
        }
    }
    assert!(captured.block(&"\u{300}".repeat(100_000)).is_none());
    let source = format!("#\\#CIF_2.0\ndata_x _{} 1", "α".repeat(500));
    assert!(
        Document::from_bytes(
            source.as_bytes(),
            Limits {
                metadata_bytes: source.len() + 2000,
                ..Limits::default()
            }
        )
        .is_err()
    );
}
