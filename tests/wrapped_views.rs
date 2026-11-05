#![cfg(feature = "wrappers")]
use dioptrin_io::{PixelBuffer, native, tiff, wrapped};
use std::io::Write;
#[test]
fn compressed_consumer_views_preserve_raw_reads_and_track_the_original_source() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tiff-view");
    let source = std::fs::read(root.join("packed2-black.tif")).unwrap();
    let raw = std::fs::read(root.join("packed2-black.tif.raw")).unwrap();
    let displayed = std::fs::read(root.join("packed2-black.tif.pillow")).unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    for bzip in [false, true] {
        let bytes = if bzip {
            let mut encoder =
                bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
            encoder.write_all(&source).unwrap();
            encoder.finish().unwrap()
        } else {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&source).unwrap();
            encoder.finish().unwrap()
        };
        std::fs::write(file.path(), bytes).unwrap();
        let mut reader = wrapped::Reader::open(
            file.path(),
            wrapped::Format::Tiff,
            wrapped::Limits::default(),
            native::Limits::default(),
            tiff::Limits::default(),
        )
        .unwrap();
        let mut stored = wrapped::FrameBuffer::default();
        reader.read_frame_into(0, &mut stored).unwrap();
        assert_eq!(stored.pixels(), Some(&PixelBuffer::U8(raw.clone())));
        let image = reader
            .read_tiff_view(0, tiff::view::Policy::Pillow13, 1024 * 1024)
            .unwrap();
        let tiff::view::Pixels::Scalar(PixelBuffer::U8(values)) = image.pixels else {
            panic!("expected byte display");
        };
        assert_eq!(values, displayed);
        assert_eq!(image.provenance.source_bits_per_sample, 2);
        assert!(
            image
                .provenance
                .operations
                .contains(&tiff::view::Operation::FullRangePackedGray)
        );
        reader.read_frame_into(0, &mut stored).unwrap();
        assert_eq!(stored.pixels(), Some(&PixelBuffer::U8(raw.clone())));
        assert!(
            reader
                .read_tiff_view(0, tiff::view::Policy::Pillow13, 1)
                .is_err()
        );
        std::fs::write(file.path(), b"changed compressed original").unwrap();
        assert!(
            reader
                .read_tiff_view(0, tiff::view::Policy::Pillow13, 1024 * 1024)
                .is_err()
        );
    }
}
