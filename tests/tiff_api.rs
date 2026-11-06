#![cfg(feature = "tiff")]
use dioptrin_io::{
    PixelBuffer,
    tiff::{
        self,
        api::{MetadataReader, Reader, Rows, ViewPolicy},
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/tiff-api")
        .join(name)
}
#[test]
fn directory_chain_order_and_signed_info_preserve_original_metadata_without_counts() {
    let reader = Reader::open(fixture("stack-nonmonotonic.tif"), tiff::Limits::default()).unwrap();
    assert_eq!(reader.get_number_of_images().unwrap(), 2);
    assert_eq!(reader.get_image_file_directories().unwrap(), [512, 8]);
    let first = reader.get_info(0).unwrap();
    assert_eq!(first.source.shape, [3, 2]);
    assert_eq!(first.source.orientation, 6);
    assert!(!first.source.little_endian);
    assert_eq!(reader.get_info(1).unwrap().source.shape, [2, 3]);
    assert!(reader.get_info(-1).is_err());
    assert!(reader.get_info(2).is_err());
    assert!(reader.get_info(i64::MAX).is_err());
    let metadata =
        MetadataReader::open(fixture("stack-nonmonotonic.tif"), tiff::Limits::default()).unwrap();
    assert_eq!(
        metadata
            .get_image_file_directories()
            .unwrap()
            .collect::<Vec<_>>(),
        [512, 8]
    );
    assert!(metadata.get_info(-1).is_err());
    assert_eq!(metadata.get_info(0).unwrap().source.tags, first.source.tags);
}
#[test]
fn data_alias_and_inclusive_crops_retain_dtype_words_endian_and_orientation() {
    let mut reader =
        Reader::open(fixture("stack-nonmonotonic.tif"), tiff::Limits::default()).unwrap();
    let mut buffer = tiff::FrameBuffer::default();
    reader.get_data_into(0, Rows::All, &mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::U16(vec![0, 65535, 32769, 1, 2, 3]))
    );
    reader
        .get_image_into(0, Rows::Inclusive { first: 1, last: 1 }, &mut buffer)
        .unwrap();
    assert_eq!(buffer.shape(), Some([1, 2]));
    assert_eq!(buffer.pixels(), Some(&PixelBuffer::U16(vec![32769, 1])));
    reader.get_data_into(1, Rows::All, &mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::I32(vec![i32::MIN, i32::MAX, 0, -1, 17, -23]))
    );
    for rows in [
        Rows::Inclusive { first: -1, last: 1 },
        Rows::Inclusive { first: 0, last: -1 },
        Rows::Inclusive { first: 1, last: 0 },
        Rows::Inclusive { first: 0, last: 2 },
        Rows::Inclusive {
            first: 0,
            last: i64::MAX,
        },
    ] {
        assert!(reader.get_data_into(1, rows, &mut buffer).is_err());
        assert!(buffer.pixels().is_none());
        assert!(buffer.sample_shape().is_none());
    }
    assert!(reader.get_image_into(-1, Rows::All, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
#[test]
fn palette_and_literal_bgr_mono_are_explicit_views_and_leave_source_indices_intact() {
    let mut reader = Reader::open(fixture("palette8.tif"), tiff::Limits::default()).unwrap();
    let mut buffer = tiff::FrameBuffer::default();
    reader.get_data_into(0, Rows::All, &mut buffer).unwrap();
    assert_eq!(buffer.pixels(), Some(&PixelBuffer::U8(vec![0, 1, 255, 2])));
    let image = reader
        .read_view(0, Rows::All, ViewPolicy::FabioTiffIo)
        .unwrap();
    assert_eq!(image.image.provenance.shape, vec![2, 2, 3]);
    let tiff::view::Pixels::Scalar(values) = image.image.pixels else {
        panic!()
    };
    assert_eq!(
        values,
        PixelBuffer::U8(vec![0, 255, 0, 1, 254, 0, 255, 0, 0, 2, 253, 0])
    );
    reader.get_data_into(0, Rows::All, &mut buffer).unwrap();
    assert_eq!(buffer.pixels(), Some(&PixelBuffer::U8(vec![0, 1, 255, 2])));
    let mut rgb = Reader::open(fixture("rgb8.tif"), tiff::Limits::default()).unwrap();
    let image = rgb
        .read_view(
            0,
            Rows::Inclusive { first: 1, last: 1 },
            ViewPolicy::FabioTiffIoMono,
        )
        .unwrap();
    assert_eq!(image.rows, 1..2);
    assert_eq!(image.image.provenance.shape, vec![1, 2]);
    assert_eq!(image.image.provenance.source_shape, vec![3, 2, 3]);
    assert_eq!(image.image.provenance.source_orientation, 6);
    let tiff::view::Pixels::Scalar(PixelBuffer::F32(values)) = image.image.pixels else {
        panic!()
    };
    assert_eq!(
        values,
        vec![
            (17f64 * 0.114 + 33f64 * 0.587 + 65f64 * 0.299) as f32,
            (254f64 * 0.114 + 0.587 + 3f64 * 0.299) as f32
        ]
    );
}
#[test]
fn stored_white_samples_and_packbits_do_not_reproduce_reference_substitution() {
    let mut reader = Reader::open(fixture("white.tif"), tiff::Limits::default()).unwrap();
    let mut buffer = tiff::FrameBuffer::default();
    reader.get_data_into(0, Rows::All, &mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::U8(vec![0, 1, 255, 128, 17, 254]))
    );
    let mut reader = Reader::open(fixture("packbits-one.tif"), tiff::Limits::default()).unwrap();
    reader.get_data_into(0, Rows::All, &mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::U8(vec![0, 1, 2, 3, 4, 5]))
    );
}
#[test]
fn empty_metadata_and_pixel_layout_admission_are_separate() {
    let metadata = MetadataReader::open(fixture("empty-ifd.tif"), tiff::Limits::default()).unwrap();
    assert_eq!(metadata.get_number_of_images().unwrap(), 0);
    assert!(metadata.get_info(0).is_err());
    assert!(Reader::open(fixture("empty-ifd.tif"), tiff::Limits::default()).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unsupported.tif");
    let mut bytes = fs::read(fixture("white.tif")).unwrap();
    // 259 is compression; retain valid source IFD metadata with unknown codec.
    let entries = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize;
    for entry in (10..10 + entries * 12).step_by(12) {
        if u16::from_le_bytes(bytes[entry..entry + 2].try_into().unwrap()) == 259 {
            bytes[entry + 8..entry + 10].copy_from_slice(&65000u16.to_le_bytes());
        }
    }
    fs::write(&path, bytes).unwrap();
    let metadata = MetadataReader::open(&path, tiff::Limits::default()).unwrap();
    assert_eq!(metadata.get_number_of_images().unwrap(), 1);
    assert!(Reader::open(&path, tiff::Limits::default()).is_err());
}
#[test]
fn known_legacy_windows_require_explicit_policy_and_keep_missing_original_tags() {
    let path = fixture("missing-bytecounts.tif");
    assert!(Reader::open(&path, tiff::Limits::default()).is_err());
    let options = tiff::Options {
        uncompressed_windows: tiff::compatibility::UncompressedWindows::FromShape,
        ..Default::default()
    };
    let mut reader = Reader::open_with_options(&path, tiff::Limits::default(), options).unwrap();
    let info = reader.get_info(0).unwrap();
    assert!(!info.source.tags.contains_key(&279));
    assert!(info.source.compatibility.is_some());
    let mut output = tiff::FrameBuffer::default();
    reader.get_data_into(0, Rows::All, &mut output).unwrap();
    assert_eq!(
        output.pixels(),
        Some(&PixelBuffer::U8(vec![0, 1, 255, 128, 17, 254]))
    );
}
#[test]
fn frame_and_view_working_buffers_are_admitted_before_output_allocation() {
    let path = fixture("rgb-budget.tif");
    assert!(
        Reader::open(
            &path,
            tiff::Limits {
                frame_bytes: 767,
                ..Default::default()
            }
        )
        .is_err()
    );
    let mut reader = Reader::open(
        &path,
        tiff::Limits {
            frame_bytes: 1535,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        reader
            .read_view(0, Rows::All, ViewPolicy::FabioTiffIo)
            .is_err()
    );
    let mut reader = Reader::open(
        &path,
        tiff::Limits {
            frame_bytes: 1791,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        reader
            .read_view(0, Rows::All, ViewPolicy::FabioTiffIoMono)
            .is_err()
    ); // 768 input +1024 mono output.
    let mut reader = Reader::open(
        &path,
        tiff::Limits {
            frame_bytes: 1792,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        reader
            .read_view(0, Rows::All, ViewPolicy::FabioTiffIoMono)
            .is_ok()
    );
    assert!(
        Reader::open(
            fixture("stack-nonmonotonic.tif"),
            tiff::Limits {
                frames: 1,
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn mutation_invalidates_sample_and_all_metadata_access_without_cache_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("snapshot.tif");
    fs::copy(fixture("white.tif"), &path).unwrap();
    let mut reader = Reader::open(&path, tiff::Limits::default()).unwrap();
    let metadata = MetadataReader::open(&path, tiff::Limits::default()).unwrap();
    let mut buffer = tiff::FrameBuffer::default();
    reader.get_data_into(0, Rows::All, &mut buffer).unwrap();
    fs::rename(&path, dir.path().join("old.tif")).unwrap();
    fs::copy(fixture("white.tif"), &path).unwrap();
    assert!(reader.get_data_into(0, Rows::All, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    assert!(reader.get_info(0).is_err());
    assert!(reader.get_number_of_images().is_err());
    assert!(reader.get_image_file_directories().is_err());
    assert!(
        reader
            .read_view(0, Rows::All, ViewPolicy::FabioTiffIo)
            .is_err()
    );
    assert!(metadata.get_number_of_images().is_err());
    assert!(metadata.get_info(0).is_err());
    assert!(metadata.get_image_file_directories().is_err());
    assert!(Reader::open(dir.path(), tiff::Limits::default()).is_err());
    assert!(MetadataReader::open(dir.path(), tiff::Limits::default()).is_err());
}
#[test]
fn crop_keeps_float64_reserved_words_without_conversion() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tiff/float64.tif");
    let mut reader = Reader::open(path, tiff::Limits::default()).unwrap();
    let mut buffer = tiff::FrameBuffer::default();
    reader.get_data_into(0, Rows::All, &mut buffer).unwrap();
    let Some(PixelBuffer::F64(values)) = buffer.pixels() else {
        panic!()
    };
    assert_eq!(values[1].to_bits(), (-0.0f64).to_bits());
    assert_eq!(values[2].to_bits(), 0x7ff8000000000000);
    reader
        .get_data_into(0, Rows::Inclusive { first: 1, last: 1 }, &mut buffer)
        .unwrap();
    let Some(PixelBuffer::F64(values)) = buffer.pixels() else {
        panic!()
    };
    assert_eq!(
        values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        [f64::INFINITY, f64::NEG_INFINITY, -1.25]
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>()
    );
}
#[cfg(unix)]
#[test]
fn fifo_sources_are_rejected_before_blocking_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let native = Reader::open(&path, tiff::Limits::default()).is_err();
        let metadata = MetadataReader::open(&path, tiff::Limits::default()).is_err();
        send.send(native && metadata).unwrap();
    });
    assert!(
        receive
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("FIFO admission must not block")
    );
    worker.join().unwrap();
}
