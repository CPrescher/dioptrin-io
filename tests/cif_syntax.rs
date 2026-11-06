#![cfg(feature = "cbf")]
use dioptrin_io::cbf::{
    document::Provenance,
    syntax::{Document, Item, Limits, Null, Reader, Style},
};
use std::io::{self, Read, Write};

const SCOPES:&[u8]=b"#\\#CIF_1.1\ndata_First\n_Title 'Scope one'\n_unknown ?\n_literal '?'\nloop_\n_atom_id\n_atom_value\nA 9007199254740993\nB .\nsave_Definition\n_TITLE \"separate scope\"\nloop_ _key _text\n1 'two words'\nsave_\nDaTa_SECOND\n_title 'Scope two'\n";
#[test]
fn blocks_frames_loops_null_markers_and_casefold_lookups_preserve_tokens() {
    let doc = Document::from_bytes(SCOPES, Limits::default()).unwrap();
    assert_eq!(doc.original, SCOPES);
    assert_eq!(doc.provenance, Provenance::Buffer);
    assert_eq!(doc.blocks.len(), 2);
    let first = doc.block("FIRST").unwrap();
    assert_eq!(doc.raw(first.name), Some(b"data_First".as_slice()));
    let title = doc.scalar(&first.items, "_tItLe").unwrap();
    assert_eq!(doc.text(title.value.token), Some("Scope one"));
    assert_eq!(title.value.token.style, Style::SingleQuoted);
    assert_eq!(
        doc.scalar(&first.items, "_unknown").unwrap().value.null,
        Some(Null::Unknown)
    );
    assert_eq!(
        doc.scalar(&first.items, "_literal").unwrap().value.null,
        None
    );
    let Item::Loop(group) = &first.items[3] else {
        panic!("loop");
    };
    let rows: Vec<_> = group.rows().collect();
    assert_eq!(doc.loop_column(&first.items, "_ATOM_VALUE").unwrap().1, 1);
    assert_eq!(rows.len(), 2);
    assert_eq!(doc.text(rows[0][1].token), Some("9007199254740993"));
    assert_eq!(rows[1][1].null, Some(Null::Inapplicable));
    let frame = doc.save_frame(first, "DEFINITION").unwrap();
    assert_eq!(
        doc.text(doc.scalar(&frame.items, "_title").unwrap().value.token),
        Some("separate scope")
    );
    assert_eq!(
        doc.text(
            doc.scalar(&doc.blocks[1].items, "_TITLE")
                .unwrap()
                .value
                .token
        ),
        Some("Scope two")
    );
    assert!(doc.block("missing").is_none());
}
#[test]
fn whitespace_comments_quotes_and_semicolons_follow_cif11_boundaries() {
    for eol in ["\n", "\r\n", "\r"] {
        let text = format!(
            "data_x{eol}_a 'a dog's life'{eol}_b \"\"{eol}_c abc#def{eol}_d ;literal{eol}_e{eol};first{eol} second{eol}; # comment{eol}_f '\\'{eol}"
        );
        let doc = Document::from_bytes(text.as_bytes(), Limits::default()).unwrap();
        let fields = &doc.blocks[0].items;
        for (key, value) in [
            ("_a", "a dog's life"),
            ("_b", ""),
            ("_c", "abc#def"),
            ("_d", ";literal"),
            ("_f", "\\"),
        ] {
            assert_eq!(
                doc.text(doc.scalar(fields, key).unwrap().value.token),
                Some(value)
            );
        }
        let token = doc.scalar(fields, "_e").unwrap().value.token;
        assert_eq!(doc.text(token), Some(format!("first{eol} second").as_str()));
        assert!(doc.raw(token).unwrap().starts_with(b";first"));
        assert_eq!(token.style, Style::TextField);
    }
    for source in [
        b"".as_slice(),
        b"# comment at EOF",
        b"data_empty",
        b"data_x _a 'value'",
    ] {
        Document::from_bytes(source, Limits::default()).unwrap();
    }
    let doc = Document::from_bytes(
        b"data_x _a 'data_not_control' _b 'loop_' _c '_tag'",
        Limits::default(),
    )
    .unwrap();
    assert_eq!(doc.blocks[0].items.len(), 3);
}
#[test]
fn malformed_grammar_never_returns_a_flat_or_partial_document() {
    for source in [
        "_x outside",
        "data_",
        "data_x _ value",
        "data_x _x",
        "data_x _x _y 1",
        "data_x loop_",
        "data_x loop_ 1",
        "data_x loop_ _a",
        "data_x loop_ _a _b 1",
        "data_x _A 1 _a 2",
        "data_X data_x",
        "data_x loop_ _a _A 1 2",
        "data_x _a 1 loop_ _A 2",
        "data_x loop_ _a 1 _A 2",
        "data_x save_a _x 1 save_ save_A _y 2 save_",
        "data_x save_",
        "data_x save_a save_",
        "data_x save_a _x 1",
        "data_x save_a _x 1 data_y",
        "data_x save_a save_b _x 1 save_ save_",
        "global_ _x 1",
        "data_x _x global_",
        "data_x loop_ _a 1 stop_",
        "data_x _x [1]",
        "data_x _x ]",
        "data_x _x $frame",
        "data_x _x 'line\nbreak'",
        "data_x _x 'unfinished",
        "data_x\n_x\n;unfinished",
        "data_x\n_x\n;value\n;trailing",
        "data_x _x 'value'#comment",
    ] {
        assert!(
            Document::from_bytes(source.as_bytes(), Limits::default()).is_err(),
            "{source:?}"
        );
    }
}
#[test]
fn cif11_fixed_restrictions_and_unsupported_versions_are_distinct_from_resource_limits() {
    for forbidden in [0, 11, 12, 31, 127, 128, 255] {
        let mut source = b"data_x _a 'prefix".to_vec();
        source.push(forbidden);
        source.extend_from_slice(b"suffix'");
        assert!(
            Document::from_bytes(&source, Limits::default())
                .unwrap_err()
                .to_string()
                .contains("forbidden character")
        );
    }
    assert!(
        Document::from_bytes(b"#\\#CIF_9.0\ndata_x", Limits::default())
            .unwrap_err()
            .to_string()
            .contains("unsupported CIF syntax version")
    );
    let good = format!("data_{}\n_{} 1\n", "b".repeat(75), "x".repeat(74));
    Document::from_bytes(good.as_bytes(), Limits::default()).unwrap();
    for source in [
        format!("data_{}", "b".repeat(76)),
        format!("data_x\n_{} 1", "x".repeat(75)),
        format!("data_x save_{}\n_x 1 save_", "f".repeat(76)),
        format!("data_x\n#{}", "c".repeat(2048)),
    ] {
        assert!(Document::from_bytes(source.as_bytes(), Limits::default()).is_err());
    }
    Document::from_bytes(
        format!("data_x\n#{}", "c".repeat(2047)).as_bytes(),
        Limits::default(),
    )
    .unwrap();
}
struct Nonseek<'a>(&'a [u8]);
impl Read for Nonseek<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = out.len().min(3).min(self.0.len());
        out[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}
#[test]
fn source_stream_and_aggregate_limits_apply_before_partial_results_escape() {
    let streamed = Document::from_reader(Nonseek(SCOPES), Limits::default()).unwrap();
    assert_eq!(streamed.provenance, Provenance::StreamCapture);
    assert_eq!(streamed.original, SCOPES);
    for limits in [
        Limits {
            source_bytes: SCOPES.len() - 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: SCOPES.len(),
            ..Limits::default()
        },
        Limits {
            tokens: 1,
            ..Limits::default()
        },
        Limits {
            blocks: 1,
            ..Limits::default()
        },
        Limits {
            items: 1,
            ..Limits::default()
        },
        Limits {
            loop_columns: 1,
            ..Limits::default()
        },
        Limits {
            values: 1,
            ..Limits::default()
        },
        Limits {
            tokens: usize::MAX,
            ..Limits::default()
        },
        Limits {
            source_bytes: 0,
            ..Limits::default()
        },
    ] {
        assert!(Document::from_bytes(SCOPES, limits).is_err(), "{limits:?}");
        assert!(
            Document::from_reader(Nonseek(SCOPES), limits).is_err(),
            "{limits:?}"
        );
    }
    let source = b"data_x\nloop_ _x\n1 2 3 4 5";
    assert!(
        Document::from_bytes(
            source,
            Limits {
                values: 4,
                ..Limits::default()
            }
        )
        .is_err()
    );
}
#[test]
fn path_source_mutation_and_symlink_retarget_reject_all_metadata_access() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Reader::open(dir.path(), Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("require regular files")
    );
    let path = dir.path().join("document.cif");
    std::fs::write(&path, SCOPES).unwrap();
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
    assert!(reader.source_path().is_err());
    std::fs::write(&path, SCOPES).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let replacement = dir.path().join("replacement.cif");
    std::fs::write(&replacement, SCOPES).unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    assert!(reader.metadata().is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let alias = dir.path().join("alias.cif");
        symlink(&path, &alias).unwrap();
        let reader = Reader::open(&alias, Limits::default()).unwrap();
        std::fs::write(&replacement, SCOPES).unwrap();
        std::fs::remove_file(&alias).unwrap();
        symlink(&replacement, &alias).unwrap();
        assert!(reader.metadata().is_err());
        assert!(reader.source_path().is_err());
    }
}
#[test]
fn every_truncation_and_ascii_byte_mutation_is_bounded_and_panic_free() {
    for end in 0..SCOPES.len() {
        let _ = Document::from_bytes(&SCOPES[..end], Limits::default());
    }
    for index in 0..SCOPES.len() {
        for byte in [0, b'\'', b';', b'\r', b'_', b'#', 255] {
            let mut source = SCOPES.to_vec();
            source[index] = byte;
            let _ = Document::from_bytes(&source, Limits::default());
        }
    }
}
