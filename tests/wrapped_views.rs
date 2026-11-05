#![cfg(feature = "wrappers")]
use dioptrin_io::{PixelBuffer, native, tiff, wrapped};
use std::io::Write;

#[test]
fn external_edf_window_compatibility_is_explicit_through_both_wrappers() {
    use dioptrin_io::edf::{ExternalPayloads, ExternalWindow};
    let directory = tempfile::tempdir().unwrap();
    let payload = directory.path().join("counts.bin");
    let original = [42, 43, 0, 0, 255, 255, 1, 0, 1, 128];
    let header = b"{\nEDF_BinarySize = 0; EDF_BinaryFileName = counts.bin; EDF_BinaryFilePosition = 2; EDF_BinaryFileSize = 80; Dim_1 = 2; Dim_2 = 2; DataType = UnsignedShort; ByteOrder = LowByteFirst;\n}\n";
    for bzip in [false, true] {
        std::fs::write(&payload, original).unwrap();
        let source = directory.path().join(if bzip {
            "header.ehf.bz2"
        } else {
            "header.ehf.gz"
        });
        let bytes = if bzip {
            let mut encoder =
                bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
            encoder.write_all(header).unwrap();
            encoder.finish().unwrap()
        } else {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(header).unwrap();
            encoder.finish().unwrap()
        };
        std::fs::write(&source, bytes).unwrap();
        let open = |format| {
            wrapped::Reader::open(
                &source,
                format,
                wrapped::Limits::default(),
                native::Limits::default(),
                tiff::Limits::default(),
            )
        };
        assert!(
            open(wrapped::Format::EdfExternal {
                policy: ExternalPayloads::FollowSnapshot,
                basenames: false
            })
            .is_err()
        );
        assert!(
            open(wrapped::Format::EdfExternalWindow {
                policy: ExternalPayloads::FollowSnapshot,
                basenames: false,
                window: ExternalWindow::Exact
            })
            .is_err()
        );
        let format = wrapped::Format::EdfExternalWindow {
            policy: ExternalPayloads::FollowSnapshot,
            basenames: false,
            window: ExternalWindow::UpToDeclared,
        };
        let mut reader = open(format).unwrap();
        let wrapped::Metadata::Edf(metadata) = reader.metadata(0).unwrap() else {
            panic!("expected EDF metadata")
        };
        assert_eq!(metadata.declared_stored_bytes, 80);
        assert_eq!(metadata.stored_bytes, 8);
        assert_eq!(
            metadata.external_window_policy,
            Some(ExternalWindow::UpToDeclared)
        );
        assert_eq!(reader.source_paths().len(), 2);
        let mut frame = wrapped::FrameBuffer::default();
        reader.read_frame_into(0, &mut frame).unwrap();
        assert_eq!(
            frame.pixels(),
            Some(&PixelBuffer::U16(vec![0, 65535, 1, 32769]))
        );
        std::fs::write(&payload, [0]).unwrap();
        assert!(reader.read_frame_into(0, &mut frame).is_err());
        assert!(frame.pixels().is_none());
        std::fs::write(&payload, original).unwrap();
        let mut reader = open(format).unwrap();
        reader.read_frame_into(0, &mut frame).unwrap();
        std::fs::write(&source, b"changed original wrapper").unwrap();
        assert!(reader.read_frame_into(0, &mut frame).is_err());
        assert!(frame.pixels().is_none());
    }
}

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
