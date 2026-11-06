#![cfg(feature = "wrappers")]
use dioptrin_io::{
    cbf, native, tiff,
    wrapped::{Compression, Format, FrameBuffer, Limits, Reader},
};
use std::io::Write;

fn encoded(bytes: &[u8], compression: Compression) -> Vec<u8> {
    match compression {
        Compression::Gzip => {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(bytes).unwrap();
            e.finish().unwrap()
        }
        Compression::Bzip2 => {
            let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
            e.write_all(bytes).unwrap();
            e.finish().unwrap()
        }
        Compression::None => bytes.to_vec(),
    }
}
fn open(path: &std::path::Path, format: Format) -> Result<Reader, dioptrin_io::wrapped::Error> {
    Reader::open(
        path,
        format,
        Limits::default(),
        native::Limits::default(),
        tiff::Limits::default(),
    )
}
#[test]
fn same_files_through_both_wrappers_keep_counts_headers_and_stacks() {
    let cases = [
        (
            "native/npy-uint64-be-F.npy",
            Format::Native(native::Format::Npy),
        ),
        (
            "native/dtrek-int32-big.img",
            Format::Native(native::Format::Dtrek),
        ),
        (
            "native/mask-33.msk",
            Format::Native(native::Format::Fit2dMask),
        ),
        ("native/hipic.img", Format::Native(native::Format::HiPic)),
        ("native/p5-uint16.pgm", Format::Native(native::Format::Pnm)),
        ("edf/compressed-stack.edf", Format::Edf),
        ("cbf/int32.cbf", Format::Cbf(cbf::ByteOffsetMode::Legacy32)),
        ("tiff/le-bigtiff.tif", Format::Tiff),
        (
            "tiff-recovery/short-u16.tif",
            Format::TiffWithOptions(tiff::Options {
                uncompressed_windows: tiff::UncompressedWindows::FromShape,
                tile_tables: tiff::TileTables::Declared,
            }),
        ),
        #[cfg(feature = "mar345")]
        ("mar345/field32-nonzero.mar345", Format::Mar345),
        #[cfg(feature = "bruker")]
        ("legacy/bruker86-linear.sfrm", Format::BrukerLinear),
        #[cfg(feature = "esperanto")]
        ("esperanto/agi-extrema-cols34.esperanto", Format::Esperanto),
        #[cfg(feature = "fit2d")]
        ("records/fit2d-512-big.f2d", Format::Fit2d),
        #[cfg(feature = "kcd")]
        (
            "records/readouts.kcd",
            Format::Kcd(dioptrin_io::kcd::Interpretation::StoredReadouts),
        ),
        #[cfg(feature = "mpa")]
        ("records/sections.mpa", Format::Mpa),
        ("images/rgb-separate.tif", Format::Tiff),
        (
            "images/pilatus.tif",
            Format::DetectorTiff(dioptrin_io::detector_tiff::Format::Pilatus),
        ),
        #[cfg(feature = "jpeg")]
        ("images/rgb.jpg", Format::Jpeg(Default::default())),
        #[cfg(feature = "jpeg2000")]
        ("images/uint16.jp2", Format::Jpeg2000(Default::default())),
        #[cfg(feature = "fixed")]
        ("fixed/ge-uint16.ge", Format::Ge),
        #[cfg(feature = "containers")]
        ("containers/spe-v3-int32.spe", Format::Spe),
        #[cfg(feature = "containers")]
        ("containers/mrc-mode3-be.mrc", Format::Mrc),
        #[cfg(feature = "containers")]
        ("containers/dm3-uint16-be.dm3", Format::Dm3),
        #[cfg(feature = "bruker")]
        ("legacy/bruker100-baseline.sfrm", Format::Bruker),
        #[cfg(feature = "oxford")]
        ("legacy/oxford-ty1-i32.img", Format::Oxford),
        #[cfg(feature = "raxis")]
        (
            "legacy/raxis-big.img",
            Format::Raxis(dioptrin_io::raxis::Interpretation::Stored),
        ),
        #[cfg(feature = "xsd")]
        ("xsd/uint64-base32.xml", Format::Xsd),
    ];
    let path = tempfile::NamedTempFile::new().unwrap();
    for (name, format) in cases {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name);
        let bytes = std::fs::read(&source).unwrap();
        let mut reference = open(&source, format).unwrap();
        for compression in [Compression::Gzip, Compression::Bzip2] {
            // Concatenated members decode to one complete source file.
            let mut payload = encoded(&bytes[..bytes.len() / 2], compression);
            payload.extend(encoded(&bytes[bytes.len() / 2..], compression));
            std::fs::write(path.path(), payload).unwrap();
            let mut reader = open(path.path(), format).unwrap();
            assert_eq!(reader.compression(), compression);
            assert_eq!(reader.frames(), reference.frames());
            let mut expected = FrameBuffer::default();
            let mut actual = FrameBuffer::default();
            for i in (0..reader.frames()).rev() {
                reference.read_frame_into(i, &mut expected).unwrap();
                reader.read_frame_into(i, &mut actual).unwrap();
                assert_eq!(expected.shape(), actual.shape());
                assert_eq!(expected.sample_shape(), actual.sample_shape());
                assert_eq!(expected.pixels(), actual.pixels(), "{name}");
            }
            assert!(
                reader
                    .read_frame_into(reader.frames(), &mut actual)
                    .is_err()
            );
            assert!(actual.pixels().is_none());
        }
    }
}
#[test]
fn corrupt_truncated_bomb_limits_and_changed_original_fail_closed() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/native/mask-33.msk"),
    )
    .unwrap();
    let path = tempfile::NamedTempFile::new().unwrap();
    let format = Format::Native(native::Format::Fit2dMask);
    for compression in [Compression::Gzip, Compression::Bzip2] {
        let payload = encoded(&bytes, compression);
        for len in [1, 3, payload.len() / 2, payload.len() - 1] {
            std::fs::write(path.path(), &payload[..len]).unwrap();
            assert!(open(path.path(), format).is_err());
        }
        let mut corrupt = payload.clone();
        let i = corrupt.len() - 5;
        corrupt[i] ^= 255;
        std::fs::write(path.path(), corrupt).unwrap();
        assert!(open(path.path(), format).is_err());
        std::fs::write(path.path(), &payload).unwrap();
        for limits in [
            Limits {
                encoded_bytes: 1,
                ..Limits::default()
            },
            Limits {
                source_bytes: 1,
                ..Limits::default()
            },
        ] {
            assert!(
                Reader::open(
                    path.path(),
                    format,
                    limits,
                    native::Limits::default(),
                    tiff::Limits::default()
                )
                .is_err()
            );
        }
        let mut reader = open(path.path(), format).unwrap();
        let mut buffer = FrameBuffer::default();
        reader.read_frame_into(0, &mut buffer).unwrap();
        std::fs::write(path.path(), b"changed original").unwrap();
        assert!(reader.read_frame_into(0, &mut buffer).is_err());
        assert!(buffer.pixels().is_none());
    }
}

#[test]
fn wrapped_thunderscan_preserves_holes_and_invalidates_on_original_mutation() {
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tiff-recovery");
    let source = std::fs::read(fixtures.join("thunder-partial-stack.tif")).unwrap();
    let expected = std::fs::read(fixtures.join("thunder-partial-stack.tif.raw")).unwrap();
    let path = tempfile::NamedTempFile::new().unwrap();
    for compression in [Compression::Gzip, Compression::Bzip2] {
        std::fs::write(path.path(), encoded(&source, compression)).unwrap();
        let mut reader = open(path.path(), Format::Tiff).unwrap();
        let mut available = tiff::thunderscan::AvailableFrame::default();
        reader
            .read_tiff_thunderscan_available_into(0, &mut available)
            .unwrap();
        assert_eq!(available.shape(), Some([5, 7]));
        assert_eq!(
            available.pixels(),
            Some(&dioptrin_io::PixelBuffer::U8(expected.clone()))
        );
        assert_eq!(available.missing_rows().unwrap()[0].start_row, 3);
        assert_eq!(available.available_rows().unwrap()[1].start_row, 4);
        assert!(!available.complete());
        let mut dense = FrameBuffer::default();
        assert!(reader.read_frame_into(0, &mut dense).is_err());
        assert!(dense.pixels().is_none());
        reader.read_frame_into(1, &mut dense).unwrap();
        std::fs::write(path.path(), b"changed original").unwrap();
        assert!(
            reader
                .read_tiff_thunderscan_available_into(0, &mut available)
                .is_err()
        );
        assert!(available.shape().is_none());
        assert!(available.pixels().is_none());
        assert!(available.available_rows().is_none());
        assert!(available.missing_rows().is_none());
        assert!(available.strips().is_none());
        assert!(!available.complete());
    }
}

#[cfg(feature = "fit2d")]
#[test]
fn compressed_fit2d_auxiliary_arrays_keep_counts_and_invalidate_on_source_change() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/records/fit2d-512-big.f2d");
    let bytes = std::fs::read(&source).unwrap();
    let direct = dioptrin_io::fit2d::Reader::open(&source, native::Limits::default()).unwrap();
    for compression in [Compression::Gzip, Compression::Bzip2] {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), encoded(&bytes, compression)).unwrap();
        let wrapped = open(file.path(), Format::Fit2d).unwrap();
        let mut expected = dioptrin_io::fit2d::FrameBuffer::default();
        let mut actual = dioptrin_io::fit2d::FrameBuffer::default();
        for index in 0..direct.metadata().arrays.len() {
            direct.read_array_into(index, &mut expected).unwrap();
            wrapped.read_fit2d_array_into(index, &mut actual).unwrap();
            assert_eq!(actual.shape(), expected.shape());
            assert_eq!(actual.pixels(), expected.pixels());
        }
        assert!(
            wrapped
                .read_fit2d_array_into(usize::MAX, &mut actual)
                .is_err()
        );
        assert!(actual.pixels().is_none());
        wrapped.read_fit2d_array_into(0, &mut actual).unwrap();
        std::fs::write(file.path(), b"changed original outer source").unwrap();
        assert!(wrapped.read_fit2d_array_into(0, &mut actual).is_err());
        assert!(actual.pixels().is_none());
    }
}

#[test]
fn compressed_edf_general_headers_keep_source_scope_and_validate_original_identity() {
    let general = b"{\nEDF_DataFormatVersion=1.0; EDF_BinarySize=0; Motor=general;\n}\n";
    let local=b"{\nDim_1=2; Dim_2=1; DataType=UnsignedShort; ByteOrder=LowByteFirst; Size=4; Motor=local;\n}\n";
    let mut bytes = general.to_vec();
    bytes.extend_from_slice(local);
    bytes.extend_from_slice(&[1, 0, 255, 255]);
    for compression in [Compression::Gzip, Compression::Bzip2] {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), encoded(&bytes, compression)).unwrap();
        let mut reader = open(file.path(), Format::Edf).unwrap();
        let headers = reader.edf_general_headers().unwrap();
        assert_eq!(headers[0].header, general);
        assert_eq!(headers[0].fields["MOTOR"], "general");
        let dioptrin_io::wrapped::Metadata::Edf(local) = reader.metadata(0).unwrap() else {
            panic!("EDF")
        };
        assert_eq!(local.fields["MOTOR"], "local");
        let mut frame = FrameBuffer::default();
        reader.read_frame_into(0, &mut frame).unwrap();
        assert_eq!(
            frame.pixels(),
            Some(&dioptrin_io::PixelBuffer::U16(vec![1, 65535]))
        );
        std::fs::write(file.path(), b"changed outer original").unwrap();
        assert!(reader.edf_general_headers().is_err());
    }
}

#[test]
fn compressed_header_only_indexing_keeps_metadata_without_requiring_pixels() {
    use dioptrin_io::wrapped::{HeaderFormat, HeaderMetadata, HeaderReader};
    let mut dtrek=b"{\nHEADER_BYTES=512; Data_type=long int; DIM=2; SIZE1=0; SIZE2=0; BYTE_ORDER=little_endian;\n}\n".to_vec();
    dtrek.resize(512, b' ');
    let tiff = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tiff/uint16.tif"),
    )
    .unwrap();
    for (format, bytes) in [(HeaderFormat::Dtrek, dtrek), (HeaderFormat::Tiff, tiff)] {
        for compression in [Compression::Gzip, Compression::Bzip2] {
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), encoded(&bytes, compression)).unwrap();
            let reader = HeaderReader::open(
                file.path(),
                format,
                Limits::default(),
                native::Limits::default(),
                tiff::Limits::default(),
            )
            .unwrap();
            assert_eq!(reader.compression(), compression);
            assert_eq!(reader.source_prefix().unwrap(), bytes[..64]);
            match reader.metadata().unwrap() {
                dioptrin_io::wrapped::HeaderMetadata::Edf(_) => {
                    unreachable!("this fixture uses TIFF or dTREK")
                }
                HeaderMetadata::Dtrek(m) => assert_eq!(m.shape, [0, 0]),
                HeaderMetadata::Tiff(pages) => assert_eq!(pages.len(), 1),
            }
            std::fs::write(file.path(), b"replaced compressed source").unwrap();
            assert!(reader.metadata().is_err());
            assert!(reader.source_prefix().is_err());
        }
    }
}
