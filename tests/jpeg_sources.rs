#![cfg(feature = "jpeg")]
use dioptrin_io::jpeg::{Limits, Reader};
#[test]
fn terminal_eoi_cannot_hide_a_second_image_or_unexplained_tail() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/images");
    let bytes = std::fs::read(root.join("gray.jpg")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tail.jpg");
    for tail in [b"garbage\xff\xd9".to_vec(), bytes.clone()] {
        let mut source = bytes.clone();
        source.extend(tail);
        std::fs::write(&path, source).unwrap();
        assert!(Reader::open(&path, Limits::default()).is_err());
    }
    for name in [
        "gray.jpg",
        "gray-progressive.jpg",
        "rgb-progressive.jpg",
        "lossless8-scans.jpg",
    ] {
        assert!(
            Reader::open(root.join(name), Limits::default()).is_ok(),
            "{name}"
        );
    }
}
