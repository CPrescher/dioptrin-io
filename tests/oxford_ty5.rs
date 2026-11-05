#![cfg(feature = "oxford")]
use dioptrin_io::{
    PixelBuffer, PixelType,
    oxford::{Compression, FrameBuffer, Limits, Reader},
};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/ty5")
        .join(name)
}
fn temporary(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file
}
#[test]
fn independent_row_order_inline_extrema_and_original_offsets_survive() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let reader = Reader::open(fixture(name), Limits::default()).unwrap();
        let metadata = reader.metadata();
        assert_eq!(metadata.compression, Compression::Ty5);
        assert_eq!(metadata.pixel_type, PixelType::I32);
        assert_eq!(metadata.source_type, None);
        assert_eq!(serde_json::json!(metadata.row_offsets), case["row_offsets"]);
        assert_eq!(serde_json::json!(metadata.shape), case["shape"]);
        assert_eq!(
            serde_json::json!([metadata.exception16, metadata.exception32]),
            case["exceptions"]
        );
        let values: Vec<_> = case["values"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r.as_array().unwrap())
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        let mut frame = FrameBuffer::default();
        for _ in 0..2 {
            reader.read_frame_into(&mut frame).unwrap();
            assert_eq!(
                frame.pixels(),
                Some(&PixelBuffer::I32(values.clone())),
                "{name}"
            );
        }
    }
}
#[test]
fn row_offsets_and_inline_windows_fail_closed_without_padding_or_crossing_rows() {
    let original = std::fs::read(fixture("non-square.img")).unwrap();
    let table = original.len() - 8;
    let mut cases = Vec::new();
    for (at, value) in [
        (table, 1u32),
        (table + 4, 0),
        (table + 4, u32::MAX),
        (table + 4, 2),
        (table + 4, 4),
    ] {
        let mut bytes = original.clone();
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        cases.push(bytes);
    }
    let mut escape = original.clone();
    // An inline int32 cannot consume the next row or its offset table.
    escape[5122] = 255;
    cases.push(escape);
    for length in [5120, 5121, table, original.len() - 1] {
        cases.push(original[..length].to_vec());
    }
    for bytes in cases {
        let file = temporary(&bytes);
        assert!(Reader::open(file.path(), Limits::default()).is_err());
    }
}
#[test]
fn checked_signed32_accumulation_and_exception_counts_are_required() {
    let original = std::fs::read(fixture("inline-extrema.img")).unwrap();
    let mut overflow = original.clone();
    overflow[5121..5125].copy_from_slice(&i32::MAX.to_le_bytes());
    let mut exceptions = original.clone();
    let start = exceptions.windows(3).position(|v| v == b"OI=").unwrap() + 3;
    exceptions[start..start + 7].copy_from_slice(b"      0");
    for bytes in [overflow, exceptions] {
        let file = temporary(&bytes);
        assert!(Reader::open(file.path(), Limits::default()).is_err());
    }
}
#[test]
fn row_tables_are_budgeted_and_mutated_sources_invalidate_reused_output() {
    let original = std::fs::read(fixture("non-square.img")).unwrap();
    for limits in [
        Limits {
            frame_bytes: 23,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: original.len() - 5120 - 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
    ] {
        let file = temporary(&original);
        assert!(Reader::open(file.path(), limits).is_err());
    }
    let mut file = temporary(&original);
    let reader = Reader::open(file.path(), Limits::default()).unwrap();
    let mut frame = FrameBuffer::default();
    reader.read_frame_into(&mut frame).unwrap();
    file.write_all(&[0]).unwrap();
    assert!(reader.read_frame_into(&mut frame).is_err());
    assert!(frame.pixels().is_none());
}
