#![cfg(any(feature = "native", feature = "tiff", feature = "cbf"))]
#[cfg(feature = "native")]
#[test]
fn empty_and_truncated_dtrek_headers_do_not_claim_available_pixels() {
    use dioptrin_io::native::{DtrekHeader, Format, Limits, Reader};
    let file = tempfile::NamedTempFile::new().unwrap();
    for (width, height) in [(0, 0), (4, 2)] {
        let text = format!(
            "{{\nHEADER_BYTES=512;\nData_type=long int;\nDIM=2;\nSIZE1={width};\nSIZE2={height};\nBYTE_ORDER=little_endian;\n}}\n"
        );
        let mut bytes = text.into_bytes();
        bytes.resize(512, b' ');
        std::fs::write(file.path(), bytes).unwrap();
        let header = DtrekHeader::open(file.path(), Limits::default()).unwrap();
        assert_eq!(header.metadata().unwrap().shape, [height, width]);
        assert_eq!(
            header.metadata().unwrap().fields["SIZE1"],
            width.to_string()
        );
        assert!(Reader::open(file.path(), Format::Dtrek, Limits::default()).is_err());
        assert!(
            DtrekHeader::open(
                file.path(),
                Limits {
                    metadata_bytes: 1,
                    ..Limits::default()
                }
            )
            .is_err()
        );
        std::fs::write(file.path(), b"changed source").unwrap();
        assert!(header.metadata().is_err());
    }
}
#[cfg(feature = "tiff")]
fn header_tiff(little: bool, big: bool) -> Vec<u8> {
    let word = |n: u64, bytes: usize| {
        if little {
            n.to_le_bytes()[..bytes].to_vec()
        } else {
            n.to_be_bytes()[8 - bytes..].to_vec()
        }
    };
    let mut out = if little {
        b"II".to_vec()
    } else {
        b"MM".to_vec()
    };
    out.extend(word(if big { 43 } else { 42 }, 2));
    if big {
        out.extend(word(8, 2));
        out.extend([0, 0]);
    }
    out.extend(word(if big { 16 } else { 8 }, if big { 8 } else { 4 }));
    // Complete metadata declarations; no encoded samples are appended.
    let tags = [
        (256, 4, 3),
        (257, 4, 2),
        (258, 3, 12),
        (259, 3, 34712),
        (262, 3, 32845),
        (273, 4, 1000),
        (279, 4, 9),
    ];
    out.extend(word(tags.len() as u64, if big { 8 } else { 2 }));
    for (tag, ty, value) in tags {
        out.extend(word(tag, 2));
        out.extend(word(ty, 2));
        out.extend(word(1, if big { 8 } else { 4 }));
        let mut inline = word(value, if ty == 3 { 2 } else { 4 });
        inline.resize(if big { 8 } else { 4 }, 0);
        out.extend(inline);
    }
    out.extend(word(0, if big { 8 } else { 4 }));
    out
}
#[cfg(feature = "tiff")]
#[test]
fn classic_and_big_tiff_headers_are_independent_of_codec_and_sample_availability() {
    use dioptrin_io::tiff::{self, Limits, TagValue};
    for little in [true, false] {
        for big in [false, true] {
            let file = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            let bytes = header_tiff(little, big);
            std::fs::write(&file, &bytes).unwrap();
            let reader = tiff::header::Reader::open(&file, Limits::default()).unwrap();
            let pages = reader.pages().unwrap();
            assert_eq!(pages.len(), 1);
            assert_eq!(pages[0].tags[&256], TagValue::Unsigned(3));
            assert_eq!(pages[0].tags[&258], TagValue::Short(12));
            assert_eq!(pages[0].tags[&259], TagValue::Short(34712));
            assert_eq!(pages[0].tags[&262], TagValue::Short(32845));
            assert!(tiff::Reader::open(&file, Limits::default()).is_err());
            assert!(
                tiff::header::Reader::open(
                    &file,
                    Limits {
                        metadata_bytes: 1,
                        ..Limits::default()
                    }
                )
                .is_err()
            );
            let mut unknown = bytes.clone();
            let field_type = if big { 26 } else { 12 };
            unknown[field_type..field_type + 2].copy_from_slice(&[255, 255]);
            std::fs::write(&file, unknown).unwrap();
            assert!(tiff::header::Reader::open(&file, Limits::default()).is_err());
            let mut cyclic = bytes.clone();
            let n = cyclic.len();
            let pointer = if big { 16u64 } else { 8 };
            let encoded = if little {
                pointer.to_le_bytes()
            } else {
                pointer.to_be_bytes()
            };
            let size = if big { 8 } else { 4 };
            cyclic[n - size..].copy_from_slice(if little {
                &encoded[..size]
            } else {
                &encoded[8 - size..]
            });
            std::fs::write(&file, cyclic).unwrap();
            assert!(reader.pages().is_err());
            assert!(tiff::header::Reader::open(&file, Limits::default()).is_err());
        }
    }
}
#[cfg(any(feature = "tiff", feature = "cbf"))]
#[test]
fn shared_pilatus_parser_retains_units_unknown_text_and_invalid_optional_values() {
    use dioptrin_io::pilatus::{Header, Value};
    let text = "# Detector: PILATUS\\\n 6M\n# Pixel_size 0.000172 m x 0.000173 m\n# Silicon sensor, thickness 0.00045 m\n# Beam_xy (12.5, 4.25) pixels\n# Count_cutoff 1048575 counts\n# Exposure_time invalid s\n# Unrecognized source metadata\n";
    let header = Header::parse(text, 100_000).unwrap();
    assert_eq!(header.description, text);
    assert_eq!(
        header.typed_fields["Detector"],
        Value::Text("PILATUS 6M".into())
    );
    assert_eq!(
        header.typed_fields["Pixel_size"],
        Value::Sequence(vec![Value::Float(0.000172), Value::Float(0.000173)])
    );
    assert_eq!(
        header.typed_fields["sensor"],
        Value::Sequence(vec![Value::Text("Silicon".into()), Value::Float(0.00045)])
    );
    assert_eq!(
        header.typed_fields["Beam_xy"],
        Value::Sequence(vec![Value::Float(12.5), Value::Float(4.25)])
    );
    assert_eq!(header.typed_fields["Count_cutoff"], Value::Integer(1048575));
    assert!(matches!(
        header.typed_fields["Exposure_time"],
        Value::Invalid(_)
    ));
    assert_eq!(header.fields["Unrecognized"], "source metadata");
    assert!(Header::parse(text, 1).is_err());
}

#[cfg(feature = "cbf")]
#[test]
fn cbf_exposes_typed_pilatus_acquisition_without_altering_counts_or_original_text() {
    use dioptrin_io::{
        cbf::{FrameBuffer, Limits, Reader},
        pilatus::Value,
    };
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/cbf/uint16.cbf");
    let mut expected = FrameBuffer::default();
    Reader::open(&source, Limits::default())
        .unwrap()
        .read_frame_into(&mut expected)
        .unwrap();
    let bytes = std::fs::read(source).unwrap();
    let tag = b"_array_data.header_contents";
    let position = bytes.windows(tag.len()).position(|w| w == tag).unwrap();
    let mut changed = bytes[..position].to_vec();
    changed.extend_from_slice(b"_array_data.header_convention PILATUS_1.2\n");
    changed.extend_from_slice(&bytes[position..]);
    let tag = b"# Original header";
    let position = changed.windows(tag.len()).position(|w| w == tag).unwrap();
    let text = b"# Exposure_time 0.5 s\n# Pixel_size 0.000172 m x 0.000172 m\n";
    changed.splice(position..position, text.iter().copied());
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), changed).unwrap();
    let reader = Reader::open(file.path(), Limits::default()).unwrap();
    let header = reader.metadata().pilatus.as_ref().unwrap();
    assert_eq!(header.typed_fields["Exposure_time"], Value::Float(0.5));
    assert_eq!(header.fields["Exposure_time"], "0.5 s");
    assert!(header.description.contains("# No flips"));
    let mut actual = FrameBuffer::default();
    reader.read_frame_into(&mut actual).unwrap();
    assert_eq!(actual.pixels(), expected.pixels());
}
