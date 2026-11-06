#![cfg(feature = "mar345")]
use dioptrin_io::{
    PixelBuffer, PixelType,
    mar345::{FrameBuffer, Limits, OverflowLayout, Reader},
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/mar345")
        .join(name)
}
fn hash(pixels: &PixelBuffer) -> String {
    let PixelBuffer::U32(values) = pixels else {
        panic!("expected original uint32 counts")
    };
    let mut hash = Sha256::new();
    for value in values {
        hash.update(value.to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}
fn compare(path: &Path, case: &serde_json::Value) {
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(path).unwrap())),
        case["sha256"].as_str().unwrap(),
        "{}",
        path.display()
    );
    let reader =
        Reader::open(path, Limits::default()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(reader.frames(), 1);
    assert_eq!(reader.metadata().pixel_type, PixelType::U32);
    assert_eq!(reader.metadata().base_pixel_type, PixelType::U16);
    assert_eq!(reader.metadata().pck_version, 1);
    assert_eq!(
        reader.metadata().little_endian,
        case["little_endian"].as_bool().unwrap()
    );
    let mut output = FrameBuffer::default();
    reader
        .read_frame_into(0, &mut output)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        output.shape(),
        Some([
            case["shape"][0].as_u64().unwrap() as usize,
            case["shape"][1].as_u64().unwrap() as usize
        ])
    );
    assert_eq!(
        hash(output.pixels().unwrap()),
        case["sample_sha256"].as_str().unwrap(),
        "{}",
        path.display()
    );
    assert!(reader.read_frame_into(1, &mut output).is_err());
    assert!(output.pixels().is_none());
}
#[test]
fn independent_original_counts_all_field_widths_runs_wrapping_endian_and_overflows() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    for (name, case) in manifest["files"].as_object().unwrap() {
        compare(&fixture(name), case);
    }
}
#[test]
fn full_real_upstream_and_pinned_writer_variants() {
    if let Ok(path) = std::env::var("MAR345_PARITY_MANIFEST") {
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for (name, case) in manifest["files"].as_object().unwrap() {
            compare(Path::new(name), case);
        }
    }
}
#[test]
fn source_headers_unsigned_first_row_and_original_overflow_words_are_retained() {
    let reader = Reader::open(fixture("overflow-padded-be.mar345"), Limits::default()).unwrap();
    let meta = reader.metadata();
    assert_eq!(meta.overflow_layout, OverflowLayout::PaddedEight);
    assert_eq!(meta.binary_words[0], 1234);
    assert_eq!(meta.binary_words[6..8], [150, 151]);
    assert_eq!(meta.header.len(), 4096);
    assert_eq!(meta.overflow_records.len(), 4);
    assert_eq!(meta.overflow_records[0].one_based_index, 1);
    assert_eq!(meta.overflow_records[3].one_based_index, 20);
    assert_eq!(meta.overflow_records[3].value, u32::MAX);
    assert!(meta.overflow_bytes[32..].iter().all(|&b| b == 0));
    assert_eq!(meta.fields["UNKNOWN"], "reserved metadata");
    assert_eq!(meta.fields["REMARK"], "Original stored rows; no flip");
    assert!(meta.marker.starts_with(b"\nCCP4 packed image, X: "));
    assert!(meta.marker.ends_with(b"\n"));
    let compact = Reader::open(fixture("overflow-compact-le.mar345"), Limits::default()).unwrap();
    assert_eq!(compact.metadata().overflow_layout, OverflowLayout::Compact);
    assert_eq!(compact.metadata().overflow_bytes.len(), 32);
    assert_eq!(compact.metadata().text_start, 128);
    assert_eq!(&compact.metadata().header[64..68], &[0, 255, 128, 1]);
    let reader = Reader::open(fixture("first-row-unsigned.mar345"), Limits::default()).unwrap();
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    let PixelBuffer::U32(values) = output.pixels().unwrap() else {
        panic!("type")
    };
    assert_eq!(&values[..6], &[32768, 32769, 32770, 32771, 32772, 32773]);
    let reader = Reader::open(fixture("field7.mar345"), Limits::default()).unwrap();
    let raw = std::fs::read(fixture("field7.mar345")).unwrap();
    let offset = 4096 + reader.metadata().marker.len();
    assert_eq!(raw[offset], b' ');
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(output.pixels(), Some(&PixelBuffer::U32(vec![0; 32])));
}
#[test]
fn malformed_truncated_unsupported_resource_limited_and_changed_sources_fail_closed() {
    let temp = std::env::temp_dir().join(format!("dioptrin-mar345-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    let path = temp.join("invalid.mar345");
    let original = std::fs::read(fixture("overflow-padded-le.mar345")).unwrap();
    for n in [0, 63, 4095, original.len() - 1] {
        std::fs::write(&path, &original[..n]).unwrap();
        if let Ok(reader) = Reader::open(&path, Limits::default()) {
            let mut output = FrameBuffer::default();
            assert!(reader.read_frame_into(0, &mut output).is_err());
            assert!(output.pixels().is_none());
        }
    }
    for (word, value) in [
        (0, 0i32),
        (1, 0),
        (1, 1),
        (2, -1),
        (2, i32::MAX),
        (3, 2),
        (4, 2),
        (5, 21),
    ] {
        let mut bytes = original.clone();
        bytes[word * 4..word * 4 + 4].copy_from_slice(&value.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(
            Reader::open(&path, Limits::default()).is_err(),
            "header word {word}"
        );
    }
    for (position, index) in [
        (4096, 0u32),
        (4096, 21),
        (4104, 1),
        (4128, 1),
        (4132, 0xdeadbeef),
    ] {
        let mut bytes = original.clone();
        bytes[position..position + 4].copy_from_slice(&index.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(Reader::open(&path, Limits::default()).is_err());
    }
    for (old, new) in [
        (b"mar research".as_slice(), b"not research".as_slice()),
        (b"END OF HEADER".as_slice(), b"BAD OF HEADER".as_slice()),
        (b"X: 0005".as_slice(), b"X: 0006".as_slice()),
    ] {
        let mut bytes = original.clone();
        let at = bytes
            .windows(old.len())
            .position(|part| part == old)
            .unwrap();
        bytes[at..at + old.len()].copy_from_slice(new);
        std::fs::write(&path, bytes).unwrap();
        assert!(Reader::open(&path, Limits::default()).is_err());
    }
    let mut bytes = original.clone();
    let key = bytes
        .windows(b"CCP4 packed image, X: ".len())
        .position(|b| b == b"CCP4 packed image, X: ")
        .unwrap();
    bytes.splice(
        key..key + b"CCP4 packed image, X: ".len(),
        b"CCP4 packed image V2, X: ".iter().copied(),
    );
    std::fs::write(&path, bytes).unwrap();
    // Relabeling a V1 stream never triggers a V1 decoder retry under a V2 marker.
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(reader.metadata().pck_version, 2);
    let mut mislabeled = FrameBuffer::default();
    assert!(reader.read_frame_into(0, &mut mislabeled).is_err());
    assert!(mislabeled.pixels().is_none());
    let reader = Reader::open(fixture("overflow-padded-le.mar345"), Limits::default()).unwrap();
    let payload = 4096 + reader.metadata().overflow_bytes.len() + reader.metadata().marker.len();
    let mut bytes = original.clone();
    bytes[payload] = (bytes[payload] & !7) | 7;
    std::fs::write(&path, bytes).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let mut output = FrameBuffer::default();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    for suffix in [b"\0".as_slice(), b"garbage".as_slice()] {
        let mut bytes = original.clone();
        bytes.extend(suffix);
        std::fs::write(&path, bytes).unwrap();
        let reader = Reader::open(&path, Limits::default()).unwrap();
        assert!(reader.read_frame_into(0, &mut output).is_err());
        assert!(output.pixels().is_none());
    }
    let mut bytes = std::fs::read(fixture("zero-run128.mar345")).unwrap();
    *bytes.last_mut().unwrap() |= 128;
    std::fs::write(&path, bytes).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    let mut bytes = original.clone();
    bytes.splice(4160..4160, [0u8; 8]);
    std::fs::write(&path, bytes).unwrap();
    assert!(Reader::open(&path, Limits::default()).is_err());
    for limits in [
        Limits {
            frame_bytes: 1,
            ..Limits::default()
        },
        Limits {
            header_bytes: 4096,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 16384,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: 1,
            ..Limits::default()
        },
    ] {
        std::fs::write(&path, &original).unwrap();
        assert!(Reader::open(&path, limits).is_err());
    }
    let mut repeated = original.clone();
    let text = format!(
        "mar research\nHIGH 4\nFORMAT 5 PCK 20\n{}END OF HEADER\n",
        "REMARK a\n".repeat(300)
    );
    repeated[64..4096].fill(b' ');
    repeated[64..64 + text.len()].copy_from_slice(text.as_bytes());
    std::fs::write(&path, repeated).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(
        reader
            .metadata()
            .entries
            .iter()
            .filter(|(key, _)| key == "REMARK")
            .count(),
        300
    );
    assert!(
        Reader::open(
            &path,
            Limits {
                metadata_bytes: 20000,
                ..Limits::default()
            }
        )
        .is_err()
    );
    std::fs::write(&path, &original).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    reader.read_frame_into(0, &mut output).unwrap();
    std::fs::write(&path, b"changed").unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    std::fs::remove_dir_all(temp).unwrap();
}
