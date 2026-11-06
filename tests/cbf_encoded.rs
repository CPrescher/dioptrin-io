#![cfg(feature = "cbf")]
use dioptrin_io::cbf::{EncodedBuffer, FrameBuffer, Limits, Reader};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/cbf-encoded")
        .join(name)
}

#[test]
fn encoded_capture_keeps_source_bytes_and_integrity_separate_from_counts() {
    for (name, expected, integrity) in [
        ("valid.cbf", &[1, 2][..], Some(true)),
        ("no-checksum.cbf", &[1, 2][..], None),
        ("wrong-checksum.cbf", &[1, 2][..], Some(false)),
        ("incomplete-escape.cbf", &[128][..], Some(true)),
        ("unsupported-conversion.cbf", &[1, 2][..], Some(true)),
        ("big-endian-label.cbf", &[1, 2][..], Some(true)),
    ] {
        let reader = Reader::open_encoded(fixture(name), Limits::default()).unwrap();
        let mut raw = EncodedBuffer::default();
        reader.read_encoded_into(&mut raw).unwrap();
        assert_eq!(raw.bytes(), Some(expected));
        assert_eq!(raw.integrity().unwrap().matches(), integrity);
        let mut frame = FrameBuffer::default();
        let decoded = reader.read_frame_into(&mut frame);
        if matches!(
            name,
            "wrong-checksum.cbf"
                | "incomplete-escape.cbf"
                | "unsupported-conversion.cbf"
                | "big-endian-label.cbf"
        ) {
            assert!(decoded.is_err());
            assert!(frame.pixels().is_none());
        } else {
            decoded.unwrap();
            assert_eq!(frame.shape(), Some([1, 2]));
        }
    }
}

#[test]
fn failed_encoded_reads_clear_reused_output_and_enforce_source_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.cbf");
    let bytes = std::fs::read(fixture("valid.cbf")).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let mut raw = EncodedBuffer::default();
    reader.read_encoded_into(&mut raw).unwrap();
    assert!(raw.bytes().is_some());
    std::fs::write(&path, [&bytes[..], b"growth"].concat()).unwrap();
    assert!(reader.read_encoded_into(&mut raw).is_err());
    assert!(raw.bytes().is_none());
    assert!(raw.integrity().is_none());
    assert!(Reader::open(directory.path(), Limits::default()).is_err());
    assert!(
        Reader::open(
            fixture("valid.cbf"),
            Limits {
                encoded_bytes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(Reader::open(fixture("unsupported-conversion.cbf"), Limits::default()).is_err());
    assert!(Reader::open(fixture("truncated.cbf"), Limits::default()).is_err());
    let reader = Reader::open_encoded(
        fixture("valid.cbf"),
        Limits {
            frame_bytes: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    reader.read_encoded_into(&mut raw).unwrap();
    assert_eq!(raw.bytes(), Some(&[1, 2][..]));
    let mut frame = FrameBuffer::default();
    assert!(reader.read_frame_into(&mut frame).is_err());
    assert!(frame.pixels().is_none());
}

#[cfg(unix)]
#[test]
fn encoded_capture_rejects_alias_retargeting_and_source_replacement() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("a.cbf");
    let b = directory.path().join("b.cbf");
    let alias = directory.path().join("alias.cbf");
    std::fs::copy(fixture("valid.cbf"), &a).unwrap();
    std::fs::copy(&a, &b).unwrap();
    symlink(&a, &alias).unwrap();
    let reader = Reader::open(&alias, Limits::default()).unwrap();
    let mut raw = EncodedBuffer::default();
    reader.read_encoded_into(&mut raw).unwrap();
    std::fs::remove_file(&alias).unwrap();
    symlink(&b, &alias).unwrap();
    assert!(reader.read_encoded_into(&mut raw).is_err());
    assert!(raw.bytes().is_none());
    let reader = Reader::open(&a, Limits::default()).unwrap();
    reader.read_encoded_into(&mut raw).unwrap();
    std::fs::rename(&b, &a).unwrap();
    assert!(reader.read_encoded_into(&mut raw).is_err());
    assert!(raw.bytes().is_none());
}
