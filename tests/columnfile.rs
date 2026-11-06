#![cfg(feature = "columnfile")]
use dioptrin_io::columnfile::{Format, Limits, Provenance, Reader, Table};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/columnfile")
        .join(name)
}
#[test]
fn independent_labels_tokens_comments_and_float64_view_remain_distinct() {
    let bytes = fs::read(fixture("matrix.dat")).unwrap();
    let table = Table::from_bytes(&bytes, Format::Ascii, Limits::default()).unwrap();
    assert_eq!(table.original(), bytes);
    assert_eq!(table.shape(), [3, 2]);
    assert_eq!(
        table.labels().collect::<Vec<_>>(),
        vec![&b"angle"[..], &b"intensity"[..]]
    );
    assert_eq!(
        table
            .values()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        [
            0.0f64,
            -0.0,
            9007199254740992.0,
            0.0125,
            f64::INFINITY,
            f64::NEG_INFINITY
        ]
        .iter()
        .map(|v| v.to_bits())
        .collect::<Vec<_>>()
    );
    assert_eq!(
        table.integer_exactness().collect::<Vec<_>>(),
        vec![Some(true), Some(true), Some(false), None, None, None]
    );
    assert_eq!(table.lexemes().nth(2), Some(&b"9007199254740993"[..]));
    let bytes = fs::read(fixture("opaque-comments.dat")).unwrap();
    let table = Table::from_bytes(&bytes, Format::Ascii, Limits::default()).unwrap();
    assert_eq!(
        table.labels().collect::<Vec<_>>(),
        vec![&b"\xff"[..], &b"counts"[..], &b"counts"[..]]
    );
    assert_eq!(
        table.comments().collect::<Vec<_>>(),
        vec![
            &b"# \xff counts counts\r\n"[..],
            &b"  # exposure \xfe\r\n"[..]
        ]
    );
    assert_eq!(table.shape(), [1, 3]);
    assert_eq!(table.original(), bytes);
}
#[test]
fn integer_exactness_does_not_use_saturating_integer_roundtrip_casts() {
    let bytes=b"# value\n9223372036854775807\n18446744073709551615\n-9223372036854775808\n18446744073709551616\n+0001\n-0000\n1000000000000000000000000\n";
    let table = Table::from_bytes(bytes, Format::Ascii, Limits::default()).unwrap();
    assert_eq!(
        table.integer_exactness().collect::<Vec<_>>(),
        vec![
            Some(false),
            Some(false),
            Some(true),
            Some(true),
            Some(true),
            Some(true),
            Some(false)
        ]
    );
}
#[test]
fn empty_rows_wide_labels_and_final_token_boundaries_are_explicit() {
    let empty = Table::from_bytes(b"# x y\n", Format::Ascii, Limits::default()).unwrap();
    assert_eq!(empty.shape(), [0, 2]);
    assert!(empty.values().is_empty());
    let final_row = Table::from_bytes(b"# x y\n1 -0", Format::Ascii, Limits::default()).unwrap();
    assert_eq!(final_row.shape(), [1, 2]);
    assert_eq!(final_row.values()[1].to_bits(), (-0.0f64).to_bits());
    let wide = Table::from_bytes(
        &fs::read(fixture("wide.dat")).unwrap(),
        Format::Ascii,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(wide.shape(), [1, 33]);
    assert_eq!(wide.values(), (0..33).map(|i| i as f64).collect::<Vec<_>>());
}
#[test]
fn malformed_rows_labels_numeric_tokens_and_binary_selection_never_supply_values() {
    for bytes in [
        &b""[..],
        b"x y\n1 2\n",
        b"#\n",
        b"# x y\n1\n",
        b"# x y\n1 2 3\n",
        b"# x y\n1 mistake\n",
        b"# x y\n\n",
        b"# x\n1\0\n",
        b"# x\n0x1p2\n",
    ] {
        assert!(
            Table::from_bytes(bytes, Format::Ascii, Limits::default()).is_err(),
            "{bytes:?}"
        );
    }
    assert!(Table::from_bytes(b"# x\n1\n", Format::Binary, Limits::default()).is_err());
}
#[test]
fn resource_limits_are_checked_for_source_lines_rows_columns_values_and_metadata() {
    let bytes = b"# x y\n1 2\n3 4\n";
    for limits in [
        Limits {
            source_bytes: bytes.len() - 1,
            ..Limits::default()
        },
        Limits {
            line_bytes: 5,
            ..Limits::default()
        },
        Limits {
            rows: 1,
            ..Limits::default()
        },
        Limits {
            columns: 1,
            ..Limits::default()
        },
        Limits {
            value_bytes: 31,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: bytes.len(),
            ..Limits::default()
        },
        Limits {
            // Source + object + six spans fit individually; the aggregate needs
            // seven spans (two labels, one comment, four numeric lexemes).
            metadata_bytes: bytes.len()
                + std::mem::size_of::<Table>()
                + 6 * std::mem::size_of::<std::ops::Range<usize>>(),
            ..Limits::default()
        },
    ] {
        assert!(Table::from_bytes(bytes, Format::Ascii, limits).is_err());
    }
}
struct Short<'a>(&'a [u8]);
impl Read for Short<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = out.len().min(1).min(self.0.len());
        out[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}
#[test]
fn nonseekable_owned_streams_and_current_position_are_bounded_without_consuming_invalid_options() {
    let bytes = b"# x y\n1 -0\n";
    let mut backing = bytes.to_vec();
    let copy = Table::from_bytes(&backing, Format::Ascii, Limits::default()).unwrap();
    backing.fill(0);
    assert_eq!(copy.original(), bytes);
    assert_eq!(copy.provenance(), Provenance::Buffer);
    let table = Table::from_reader(Short(bytes), Format::Ascii, Limits::default()).unwrap();
    assert_eq!(table.provenance(), Provenance::Stream);
    assert_eq!(table.original(), bytes);
    let mut cursor = io::Cursor::new([&b"skip"[..], bytes].concat());
    cursor.set_position(4);
    assert_eq!(
        Table::from_reader(&mut cursor, Format::Ascii, Limits::default())
            .unwrap()
            .original(),
        bytes
    );
    let mut cursor = io::Cursor::new(bytes);
    assert!(Table::from_reader(&mut cursor, Format::Binary, Limits::default()).is_err());
    assert_eq!(cursor.position(), 0);
    assert!(
        Table::from_reader(
            &mut cursor,
            Format::Ascii,
            Limits {
                rows: 0,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert_eq!(cursor.position(), 0);
    assert!(
        Table::from_reader(
            &mut cursor,
            Format::Ascii,
            Limits {
                source_bytes: 4,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert_eq!(cursor.position(), 5);
    struct Failure;
    impl Read for Failure {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("injected"))
        }
    }
    assert!(Table::from_reader(Failure, Format::Ascii, Limits::default()).is_err());
}
#[test]
fn immutable_path_reader_rejects_growth_replacement_and_nonregular_sources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.dat");
    let original = b"# x y\n1 2\n";
    assert!(Reader::open(dir.path(), Format::Ascii, Limits::default()).is_err());
    fs::write(&path, original).unwrap();
    let reader = Reader::open(&path, Format::Ascii, Limits::default()).unwrap();
    assert_eq!(
        reader.table().unwrap().provenance(),
        Provenance::PathSnapshot
    );
    fs::rename(&path, dir.path().join("old.dat")).unwrap();
    fs::write(&path, original).unwrap();
    assert!(reader.table().is_err());
    assert!(reader.source_path().is_err());
    let reader = Reader::open(&path, Format::Ascii, Limits::default()).unwrap();
    fs::write(&path, b"# x y\n1 2\n3 4\n").unwrap();
    assert!(reader.table().is_err());
}
#[cfg(unix)]
#[test]
fn symlink_retargeting_does_not_substitute_another_identical_table() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    let link = dir.path().join("link");
    fs::write(&a, b"# x\n1\n").unwrap();
    fs::write(&b, b"# x\n1\n").unwrap();
    std::os::unix::fs::symlink(&a, &link).unwrap();
    let reader = Reader::open(&link, Format::Ascii, Limits::default()).unwrap();
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&b, &link).unwrap();
    assert!(reader.table().is_err());
}
