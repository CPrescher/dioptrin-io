#![cfg(any(
    feature = "containers",
    feature = "cbf",
    feature = "native",
    feature = "mar345",
    feature = "oxford"
))]
#[cfg(any(
    feature = "containers",
    feature = "cbf",
    feature = "mar345",
    feature = "oxford"
))]
fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}
#[cfg(feature = "containers")]
#[test]
fn dm3_typed_values_keep_names_unicode_float_bits_and_bounds_in_both_byte_orders() {
    use dioptrin_io::{
        PixelBuffer,
        dm3::{Limits, Reader, TagValue},
    };
    for endian in ["le", "be"] {
        let reader = Reader::open(
            fixture(&format!(
                "containers/dm3-multiple-images-stack-{endian}.dm3"
            )),
            Limits::default(),
        )
        .unwrap();
        let m = reader.metadata();
        let tag = |name: &str| {
            m.tags
                .iter()
                .find(|t| t.raw_name == name.as_bytes())
                .unwrap()
        };
        let TagValue::Utf16(units) = tag("Unicode")
            .decode_small_value(m.little_endian, 1000)
            .unwrap()
            .unwrap()
        else {
            panic!("string")
        };
        assert_eq!(String::from_utf16(&units).unwrap(), "AμZ");
        let TagValue::Structure { name, fields } = tag("Named Struct")
            .decode_small_value(m.little_endian, 1000)
            .unwrap()
            .unwrap()
        else {
            panic!("struct")
        };
        assert_eq!(name, b"abc");
        assert_eq!(fields[0].0, b"xy");
        let TagValue::Numeric { values, scalar } = &fields[0].1 else {
            panic!("scalar")
        };
        assert!(*scalar);
        assert_eq!(values, &PixelBuffer::I32(vec![-123]));
        for name in ["Nested Array", "Struct Array"] {
            let TagValue::Opaque(raw) = tag(name)
                .decode_small_value(m.little_endian, 0)
                .unwrap()
                .unwrap()
            else {
                panic!("opaque")
            };
            assert_eq!(raw, tag(name).raw.as_deref().unwrap());
        }
        assert!(
            tag("Unicode")
                .decode_small_value(m.little_endian, 5)
                .is_err()
        );
        assert!(
            tag("Named Struct")
                .decode_small_value(m.little_endian, 0)
                .is_err()
        );
        let reader = Reader::open(
            fixture(&format!("containers/dm3-float32-{endian}.dm3")),
            Limits::default(),
        )
        .unwrap();
        let m = reader.metadata();
        let data = m.tags.iter().find(|t| t.raw_name == b"Data").unwrap();
        let TagValue::Numeric {
            values: PixelBuffer::F32(values),
            scalar,
        } = data
            .decode_small_value(m.little_endian, 24)
            .unwrap()
            .unwrap()
        else {
            panic!("array")
        };
        assert!(!scalar);
        assert_eq!(values[1].to_bits(), 0x80000000);
        assert!(values[2].is_nan());
        assert_eq!(values[3], f32::INFINITY);
        assert_eq!(values[4], f32::NEG_INFINITY);
        assert_eq!(values[5], -1.25);
        assert!(data.decode_small_value(m.little_endian, 23).is_err());
    }
}
#[cfg(feature = "cbf")]
#[test]
fn cif_metadata_additions_leave_original_binary_counts_unchanged() {
    use dioptrin_io::cbf::{FrameBuffer, Limits, Reader};
    let source = fixture("cbf/uint16.cbf");
    let original = std::fs::read(&source).unwrap();
    let mut expected = FrameBuffer::default();
    Reader::open(&source, Limits::default())
        .unwrap()
        .read_frame_into(&mut expected)
        .unwrap();
    let index = original
        .windows(b"_array_data.data".len())
        .position(|w| w == b"_array_data.data")
        .unwrap();
    let mut changed = original[..index].to_vec();
    changed.extend_from_slice(
        b"_sample.name 'crystal # 1'\nloop_\n_sample.axis\n_sample.unit\nx mm\ny mm\n",
    );
    changed.extend_from_slice(&original[index..]);
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), &changed).unwrap();
    let reader = Reader::open(file.path(), Limits::default()).unwrap();
    assert_eq!(reader.metadata().cif.fields["_sample.name"], "crystal # 1");
    assert_eq!(
        reader.metadata().cif.loops[0].rows,
        [["x", "mm"], ["y", "mm"]]
    );
    assert!(reader.metadata().cif.diagnostics.is_empty());
    let mut actual = FrameBuffer::default();
    reader.read_frame_into(&mut actual).unwrap();
    assert_eq!(expected.pixels(), actual.pixels());
    assert_eq!(expected.shape(), actual.shape());
    assert_eq!(
        reader.metadata().header,
        changed[..reader.metadata().header.len()]
    );
}
#[cfg(feature = "native")]
#[test]
fn pnm_keeps_original_dimension_and_maximum_tokens() {
    use dioptrin_io::{
        PixelBuffer,
        native::{Format, FrameBuffer, Limits, Reader},
    };
    let file = tempfile::NamedTempFile::new().unwrap();
    let raw = b"P2\n0003 0002\n000255\n1 2 3 4 5 6\n";
    std::fs::write(file.path(), raw).unwrap();
    let reader = Reader::open(file.path(), Format::Pnm, Limits::default()).unwrap();
    assert_eq!(reader.metadata().fields["WIDTH"], "0003");
    assert_eq!(reader.metadata().fields["HEIGHT"], "0002");
    assert_eq!(reader.metadata().fields["MAXVAL"], "000255");
    assert_eq!(reader.metadata().shape, [2, 3]);
    let mut frame = FrameBuffer::default();
    reader.read_frame_into(0, &mut frame).unwrap();
    assert_eq!(
        frame.pixels(),
        Some(&PixelBuffer::U8(vec![1, 2, 3, 4, 5, 6]))
    );
}
#[cfg(feature = "mar345")]
#[test]
fn mar345_repeated_groups_and_documented_binary_units_are_exposed_separately() {
    use dioptrin_io::mar345::{HeaderValue, Limits, Reader};
    let source = fixture("mar345/overflow-padded-be.mar345");
    let mut raw = std::fs::read(source).unwrap();
    let index = raw.windows(13).position(|w| w == b"END OF HEADER").unwrap();
    let text = b"COUNTS START 1 END 2\nCOUNTS MIN 3 MAX 4\nREMARK\nEND OF HEADER\n";
    raw[index..index + text.len()].copy_from_slice(text);
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), raw).unwrap();
    let reader = Reader::open(file.path(), Limits::default()).unwrap();
    let fields: std::collections::BTreeMap<_, _> = reader.metadata().binary_scalars().into();
    assert_eq!(fields["PixelLength"], HeaderValue::Float(0.15));
    assert_eq!(fields["PixelHeight"], HeaderValue::Float(0.151));
    let fields: std::collections::BTreeMap<_, _> = reader.metadata().text_scalars().collect();
    for (key, value) in [
        ("COUNTS_START", "1"),
        ("COUNTS_END", "2"),
        ("COUNTS_MIN", "3"),
        ("COUNTS_MAX", "4"),
    ] {
        assert_eq!(fields[key], value);
    }
    assert_eq!(
        reader
            .metadata()
            .entries
            .iter()
            .filter(|(key, _)| key == "COUNTS")
            .count(),
        2
    );
    assert_eq!(reader.metadata().fields["COUNTS"], "MIN 3 MAX 4");
    assert_eq!(reader.metadata().fields["REMARK"], "");
}
#[cfg(feature = "oxford")]
#[test]
fn oxford_signed_overflow_flags_are_source_metadata_and_do_not_alter_pixels() {
    use dioptrin_io::oxford::{FrameBuffer, HeaderValue, Limits, Reader};
    let source = fixture("legacy/oxford-raw.img");
    let reader = Reader::open(&source, Limits::default()).unwrap();
    let offset = reader.metadata().sections[1].offset;
    let mut expected = FrameBuffer::default();
    reader.read_frame_into(&mut expected).unwrap();
    let mut raw = std::fs::read(source).unwrap();
    raw[offset + 464..offset + 466].copy_from_slice(&(-1i16).to_le_bytes());
    raw[offset + 466..offset + 468].copy_from_slice(&32767i16.to_le_bytes());
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), raw).unwrap();
    let reader = Reader::open(file.path(), Limits::default()).unwrap();
    assert!(matches!(
        reader.metadata().fields["Overflow flag"],
        HeaderValue::Signed(-1)
    ));
    assert!(matches!(
        reader.metadata().fields["Overflow after remeasure flag"],
        HeaderValue::Signed(32767)
    ));
    let mut actual = FrameBuffer::default();
    reader.read_frame_into(&mut actual).unwrap();
    assert_eq!(actual.pixels(), expected.pixels());
}
