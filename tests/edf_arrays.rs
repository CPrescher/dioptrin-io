#![cfg(feature = "edf")]
use dioptrin_io::{
    PixelBuffer,
    edf::{ArrayBuffer, FrameBuffer, FrameLayout, Limits, Reader, recovery},
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/edf-arrays")
        .join(name)
}
#[test]
fn full_rank_is_preserved_and_planes_are_explicit_source_coordinates_not_records() {
    let reader = Reader::open(fixture("volume-u16-be.edf"), Limits::default()).unwrap();
    assert_eq!(reader.frames(), 1);
    assert_eq!(reader.image_records(), 1);
    let meta = reader.metadata(0).unwrap();
    assert_eq!(meta.source_shape, [2, 3, 4]);
    assert_eq!(meta.shape, [3, 4]);
    assert_eq!(meta.source_samples, 24);
    assert_eq!(meta.plane_count, 2);
    assert_eq!(meta.frame_layout, FrameLayout::ArrayOrPlaneRequired);
    let mut array = ArrayBuffer::default();
    reader.read_array_into(0, &mut array).unwrap();
    assert_eq!(array.shape(), Some(&[2, 3, 4][..]));
    let expected = [vec![0u16, 65535, 32769], (0..21).collect()].concat();
    assert_eq!(array.pixels(), Some(&PixelBuffer::U16(expected.clone())));
    let mut plane = FrameBuffer::default();
    assert!(reader.read_frame_into(0, &mut plane).is_err());
    assert!(plane.pixels().is_none());
    for index in [1, 0, 1] {
        reader.read_plane_into(0, &[index], &mut plane).unwrap();
        assert_eq!(plane.shape(), Some([3, 4]));
        assert_eq!(
            plane.pixels(),
            Some(&PixelBuffer::U16(
                expected[index * 12..(index + 1) * 12].to_vec()
            ))
        );
    }
    for coordinates in [&[][..], &[2], &[usize::MAX], &[0, 0]] {
        assert!(reader.read_plane_into(0, coordinates, &mut plane).is_err());
        assert!(plane.pixels().is_none());
    }
    assert!(reader.read_array_into(1, &mut array).is_err());
    assert!(array.shape().is_none());
    assert!(array.pixels().is_none());
}
#[test]
fn compressed_rank4_and_rank5_preserve_i64_and_float64_words() {
    let reader = Reader::open(fixture("rank4-gzip.edf"), Limits::default()).unwrap();
    let mut array = ArrayBuffer::default();
    reader.read_array_into(0, &mut array).unwrap();
    assert_eq!(array.shape(), Some(&[2, 2, 2, 3][..]));
    let expected = [
        vec![i64::MIN, i64::MAX, -1, 0, (1 << 53) + 1, -((1 << 53) + 1)],
        (0..18).collect(),
    ]
    .concat();
    assert_eq!(array.pixels(), Some(&PixelBuffer::I64(expected.clone())));
    let mut plane = FrameBuffer::default();
    reader.read_plane_into(0, &[1, 1], &mut plane).unwrap();
    assert_eq!(
        plane.pixels(),
        Some(&PixelBuffer::I64(expected[18..].to_vec()))
    );
    let reader = Reader::open(fixture("rank5-zlib.edf"), Limits::default()).unwrap();
    reader.read_array_into(0, &mut array).unwrap();
    assert_eq!(array.shape(), Some(&[2, 1, 2, 1, 3][..]));
    let Some(PixelBuffer::F64(values)) = array.pixels() else {
        panic!()
    };
    assert_eq!(values[1].to_bits(), (-0.0f64).to_bits());
    assert_eq!(values[2].to_bits(), 0x7ff8000000000042);
    reader.read_plane_into(0, &[1, 0, 1], &mut plane).unwrap();
    let Some(PixelBuffer::F64(values)) = plane.pixels() else {
        panic!()
    };
    assert_eq!(
        values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        vec![
            f64::INFINITY.to_bits(),
            f64::NEG_INFINITY.to_bits(),
            1.25f64.to_bits()
        ]
    );
    let reader = Reader::open(fixture("rank3-bzip.edf"), Limits::default()).unwrap();
    reader.read_array_into(0, &mut array).unwrap();
    assert_eq!(array.shape(), Some(&[2, 2, 3][..]));
    assert_eq!(array.pixels().unwrap().len(), 12);
}
#[test]
fn scalar_rank1_singleton_and_implicit_dimensions_have_distinct_contracts() {
    let mut array = ArrayBuffer::default();
    let mut plane = FrameBuffer::default();
    for (name, shape, view) in [
        ("scalar.edf", vec![], [1, 1]),
        ("rank1.edf", vec![6], [1, 6]),
    ] {
        let reader = Reader::open(fixture(name), Limits::default()).unwrap();
        reader.read_array_into(0, &mut array).unwrap();
        assert_eq!(array.shape(), Some(shape.as_slice()));
        assert!(reader.read_frame_into(0, &mut plane).is_err());
        reader.read_plane_into(0, &[], &mut plane).unwrap();
        assert_eq!(plane.shape(), Some(view));
    }
    let reader = Reader::open(fixture("singleton-leading.edf"), Limits::default()).unwrap();
    assert_eq!(reader.metadata(0).unwrap().source_shape, [1, 1, 2, 3]);
    assert_eq!(
        reader.metadata(0).unwrap().frame_layout,
        FrameLayout::SingletonLeadingAxes
    );
    reader.read_frame_into(0, &mut plane).unwrap();
    assert_eq!(plane.shape(), Some([2, 3]));
    reader.read_array_into(0, &mut array).unwrap();
    assert_eq!(array.shape(), Some(&[1, 1, 2, 3][..]));
    let reader = Reader::open(fixture("sparse-dimensions.edf"), Limits::default()).unwrap();
    let meta = reader.metadata(0).unwrap();
    assert_eq!(meta.source_shape, [2, 1, 1, 3]);
    assert_eq!(meta.implicit_dimensions, [2, 3]);
    assert!(!meta.fields.contains_key("DIM_2"));
    reader.read_plane_into(0, &[1, 0], &mut plane).unwrap();
    assert_eq!(plane.pixels(), Some(&PixelBuffer::I16(vec![-1, 17, -23])));
}
#[test]
fn mixed_records_and_general_headers_do_not_change_record_or_plane_numbering() {
    let reader = Reader::open(fixture("mixed-records.edf"), Limits::default()).unwrap();
    assert_eq!(reader.frames(), 3);
    let mut array = ArrayBuffer::default();
    for index in [2, 0, 1] {
        reader.read_array_into(index, &mut array).unwrap();
        assert_eq!(reader.metadata(index).unwrap().record_index, index);
        assert_eq!(
            array.pixels().unwrap().len(),
            reader.metadata(index).unwrap().source_samples
        );
    }
    let reader = Reader::open(fixture("general-volume.edf"), Limits::default()).unwrap();
    assert_eq!(reader.frames(), 1);
    assert_eq!(reader.general_headers().len(), 1);
    assert_eq!(reader.metadata(0).unwrap().source_shape, [2, 2, 3]);
    assert_eq!(reader.metadata(0).unwrap().general_header, Some(0));
    reader.read_array_into(0, &mut array).unwrap();
    assert_eq!(array.pixels(), Some(&PixelBuffer::U16((0..12).collect())));
}
#[test]
fn recovery_reports_record_wide_prefixes_partial_words_and_integrity_separately() {
    let mut output = recovery::SampleBuffer::default();
    for (name, count, partial) in [
        ("volume-truncated.edf", 5, &b"\xfe\xff\x00"[..]),
        ("volume-header-only.edf", 0, &b""[..]),
    ] {
        let reader =
            recovery::Reader::open(fixture(name), Limits::default(), Default::default()).unwrap();
        assert_eq!(reader.image_records(), 1);
        reader.read_samples_into(0, &mut output).unwrap();
        let report = output.report().unwrap();
        assert_eq!(report.declared_source_shape, [2, 3, 4]);
        assert_eq!(report.declared_shape, [3, 4]);
        assert_eq!(report.declared_samples, 24);
        assert_eq!(report.present_samples, 0..count);
        assert_eq!(report.missing_samples, Some(count..24));
        assert_eq!(output.partial_element_bytes(), Some(partial));
        assert!(!report.complete_verified_record);
    }
    for (name, integrity) in [
        (
            "volume-corrupt-member.edf",
            recovery::Integrity::ChecksumFailure,
        ),
        (
            "volume-footer-short.edf",
            recovery::Integrity::UnverifiedTruncated,
        ),
    ] {
        let reader =
            recovery::Reader::open(fixture(name), Limits::default(), Default::default()).unwrap();
        reader.read_samples_into(0, &mut output).unwrap();
        assert_eq!(output.pixels().unwrap().len(), 12);
        let report = output.report().unwrap();
        assert_eq!(report.declared_source_shape, [2, 2, 3]);
        assert_eq!(report.missing_samples, None);
        assert_eq!(report.payload_integrity, integrity);
        assert!(!report.complete_verified_record);
        assert!(!report.complete_verified_frame);
        let strict = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut array = ArrayBuffer::default();
        assert!(strict.read_array_into(0, &mut array).is_err());
        assert!(array.shape().is_none());
        assert!(array.pixels().is_none());
    }
}
#[test]
fn rank_full_record_resources_and_source_mutations_never_offer_partial_dense_arrays() {
    for name in ["volume-u16-be.edf", "volume-header-only.edf"] {
        let limits = Limits {
            frame_bytes: 23,
            ..Default::default()
        };
        assert!(Reader::open(fixture(name), limits).is_err());
        assert!(recovery::Reader::open(fixture(name), limits, Default::default()).is_err());
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.edf");
    let mut bytes = fs::read(fixture("volume-u16-be.edf")).unwrap();
    let needle = b"Dim_3 = 2";
    let at = bytes
        .windows(needle.len())
        .position(|v| v == needle)
        .unwrap();
    bytes[at..at + 5].copy_from_slice(b"Dim_9"); // bounded rank9 with missing default axes still valid geometry.
    fs::write(&path, &bytes).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let mut array = ArrayBuffer::default();
    reader.read_array_into(0, &mut array).unwrap();
    assert_eq!(array.shape().unwrap().len(), 9);
    fs::rename(&path, dir.path().join("old.edf")).unwrap();
    fs::write(&path, bytes).unwrap();
    assert!(reader.read_array_into(0, &mut array).is_err());
    assert!(array.pixels().is_none());
    assert!(array.shape().is_none());
    let bad=b"{\nDim_1 = 1 ; Dim_33 = 1 ; DataType = UnsignedByte ; ByteOrder = LowByteFirst ; Size = 1 ;\n}\n\x01";
    fs::write(&path, bad).unwrap();
    assert!(Reader::open(&path, Limits::default()).is_err());
    assert!(recovery::Reader::open(&path, Limits::default(), Default::default()).is_err());
}

#[test]
fn volume_external_snapshot_bounds_and_selected_planes_cover_the_complete_source() {
    use dioptrin_io::edf::{ExternalPayloads, Options};
    let dir = tempfile::tempdir().unwrap();
    let header = dir.path().join("volume.ehf");
    let payload = dir.path().join("counts.bin");
    let fields = b"{\nEDF_BinarySize = 0; EDF_BinaryFileName = counts.bin; EDF_BinaryFilePosition = 2; EDF_BinaryFileSize = 16; Dim_1 = 2; Dim_2 = 2; Dim_3 = 2; DataType = UnsignedShort; ByteOrder = HighByteFirst;\n}\n";
    fs::write(&header, fields).unwrap();
    let mut bytes = vec![0x7a, 0x7b];
    for word in [0u16, u16::MAX, 32769, 3, 4, 5, 6, 7] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    fs::write(&payload, &bytes).unwrap();
    let options = Options {
        external_payloads: ExternalPayloads::FollowSnapshot,
        ..Default::default()
    };
    let reader = Reader::open_with_options(&header, Limits::default(), options.clone()).unwrap();
    assert_eq!(reader.metadata(0).unwrap().source_shape, [2, 2, 2]);
    assert_eq!(reader.payload_sources().len(), 2);
    let mut plane = FrameBuffer::default();
    reader.read_plane_into(0, &[1], &mut plane).unwrap();
    assert_eq!(plane.pixels(), Some(&PixelBuffer::U16(vec![4, 5, 6, 7])));
    let replacement = dir.path().join("replacement.bin");
    fs::write(&replacement, &bytes).unwrap();
    fs::rename(replacement, &payload).unwrap();
    assert!(reader.read_plane_into(0, &[0], &mut plane).is_err());
    assert!(plane.pixels().is_none());
    // The full 16-byte volume is mandatory even when selecting its first plane.
    fs::write(&payload, &bytes[..10]).unwrap();
    assert!(Reader::open_with_options(&header, Limits::default(), options).is_err());
}
