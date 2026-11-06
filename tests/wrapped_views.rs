#![cfg(feature = "wrappers")]
use dioptrin_io::{PixelBuffer, native, tiff, wrapped};
use std::io::Write;

#[test]
fn wrapped_ycbcr_keeps_native_grids_and_invalidates_them_on_original_source_changes() {
    // Independent two-by-two TIFF: one original unit, no color expansion.
    let entries: [(u16, u16, u32, u32); 12] = [
        (256, 4, 1, 2),
        (257, 4, 1, 2),
        (258, 3, 3, 158),
        (259, 3, 1, 1),
        (262, 3, 1, 6),
        (273, 4, 1, 164),
        (274, 3, 1, 8),
        (277, 3, 1, 3),
        (278, 4, 1, 2),
        (279, 4, 1, 6),
        (284, 3, 1, 1),
        (530, 3, 2, 0x0002_0002),
    ];
    let mut tiff_bytes = b"II*\0\x08\0\0\0".to_vec();
    tiff_bytes.extend(12u16.to_le_bytes());
    for (tag, kind, count, value) in entries {
        tiff_bytes.extend(tag.to_le_bytes());
        tiff_bytes.extend(kind.to_le_bytes());
        tiff_bytes.extend(count.to_le_bytes());
        tiff_bytes.extend(value.to_le_bytes());
    }
    tiff_bytes.extend(0u32.to_le_bytes());
    tiff_bytes.extend([8, 0, 8, 0, 8, 0]);
    tiff_bytes.extend([1, 2, 3, 4, 128, 128]);
    let file = tempfile::NamedTempFile::new().unwrap();
    for bzip in [false, true] {
        let bytes = if bzip {
            let mut encoder =
                bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
            encoder.write_all(&tiff_bytes).unwrap();
            encoder.finish().unwrap()
        } else {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&tiff_bytes).unwrap();
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
        let mut buffer = tiff::ycbcr::Buffer::default();
        reader.read_tiff_ycbcr_into(0, &mut buffer).unwrap();
        assert_eq!(buffer.shape(), Some([2, 2]));
        assert_eq!(buffer.chunks().unwrap()[0].planes[0].samples, [1, 2, 3, 4]);
        assert_eq!(
            buffer.chunks().unwrap()[0].sampling_units.as_deref(),
            Some(&[1, 2, 3, 4, 128, 128][..])
        );
        let mut scalar = wrapped::FrameBuffer::default();
        assert!(reader.read_frame_into(0, &mut scalar).is_err());
        assert!(scalar.pixels().is_none());
        let view = reader
            .read_tiff_view(0, tiff::view::Policy::Pillow13, 16)
            .unwrap();
        assert_eq!(
            view.provenance.source_layout,
            tiff::SampleLayout::YCbCrComponents {
                horizontal: 2,
                vertical: 2
            }
        );
        assert_eq!(view.provenance.source_orientation, 8);
        reader.read_tiff_ycbcr_into(0, &mut buffer).unwrap();
        assert_eq!(buffer.chunks().unwrap()[0].planes[0].samples, [1, 2, 3, 4]);
        assert!(reader.read_tiff_ycbcr_into(1, &mut buffer).is_err());
        assert!(buffer.shape().is_none() && buffer.chunks().is_none());
        reader.read_tiff_ycbcr_into(0, &mut buffer).unwrap();
        std::fs::write(file.path(), b"changed compressed original").unwrap();
        assert!(reader.read_tiff_ycbcr_into(0, &mut buffer).is_err());
        assert!(buffer.shape().is_none() && buffer.storage().is_none());
    }
}

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
