#![cfg(any(feature = "tiff", feature = "jpeg", feature = "jpeg2000"))]
use dioptrin_io::PixelBuffer;
use std::path::{Path, PathBuf};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/images")
        .join(name)
}
fn bytes(pixels: &PixelBuffer) -> Vec<u8> {
    macro_rules! scalar {
        ($v:expr) => {
            $v.iter().flat_map(|v| v.to_le_bytes()).collect()
        };
    }
    match pixels {
        PixelBuffer::U8(v) => v.clone(),
        PixelBuffer::I8(v) => v.iter().map(|&v| v as u8).collect(),
        PixelBuffer::U16(v) => scalar!(v),
        PixelBuffer::U32(v) => scalar!(v),
        PixelBuffer::U64(v) => scalar!(v),
        PixelBuffer::I16(v) => scalar!(v),
        PixelBuffer::I32(v) => scalar!(v),
        PixelBuffer::I64(v) => scalar!(v),
        PixelBuffer::F32(v) => scalar!(v),
        PixelBuffer::F64(v) => scalar!(v),
        _ => panic!("unexpected complex codec samples"),
    }
}
fn check(name: &str, pixels: &PixelBuffer) {
    assert_eq!(
        bytes(pixels),
        std::fs::read(fixture(&(name.to_owned() + ".samples"))).unwrap(),
        "{name}"
    );
}
#[cfg(feature = "tiff")]
#[test]
fn tiff_preserves_color_planes_packed_values_float16_and_group4() {
    use dioptrin_io::tiff::{FrameBuffer, Limits, Reader};
    for name in [
        "rgb-contig.tif",
        "rgb-separate.tif",
        "multiband.tif",
        "float16.tif",
        "packed1-white.tif",
        "packed1-black.tif",
        "packed2-white.tif",
        "packed2-black.tif",
        "packed4-white.tif",
        "packed4-black.tif",
        "group4.tif",
        "jpeg-gray.tif",
        "jpeg-rgb.tif",
    ] {
        let mut reader = Reader::open(fixture(name), Limits::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut buffer = FrameBuffer::default();
        reader
            .read_frame_into(0, &mut buffer)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        if name.starts_with("jpeg-") {
            // TIFF's zune backend and libjpeg-turbo use different IDCT rounding.
            // This is lossy decode qualification, not exact FabIO count parity.
            let actual = bytes(buffer.pixels().unwrap());
            let reference = std::fs::read(fixture(&(name.to_owned() + ".samples"))).unwrap();
            assert_eq!(actual.len(), reference.len());
            assert!(
                actual
                    .iter()
                    .zip(reference)
                    .all(|(&a, b)| a.abs_diff(b) <= 1)
            );
        } else {
            check(name, buffer.pixels().unwrap());
        }
        assert_eq!(
            buffer.sample_shape().unwrap(),
            reader.metadata(0).unwrap().sample_shape()
        );
        if name.starts_with("rgb-") {
            assert_eq!(reader.metadata(0).unwrap().orientation, 8);
            assert_eq!(buffer.sample_shape(), Some(vec![2, 5, 3]));
        }
        if name == "float16.tif" {
            assert_eq!(reader.metadata(0).unwrap().bits_per_sample, 16);
            assert_eq!(
                reader.metadata(0).unwrap().pixel_type,
                dioptrin_io::PixelType::F32
            );
        }
        if name.starts_with("jpeg-") {
            assert!(reader.metadata(0).unwrap().lossy);
        }
        reader.read_frame_into(0, &mut buffer).unwrap();
        if !name.starts_with("jpeg-") {
            check(name, buffer.pixels().unwrap());
        }
    }
    for name in ["palette.tif", "white-float.tif"] {
        assert!(Reader::open(fixture(name), Limits::default()).is_err());
    }
    // Only primary IFDs are frames. SubIFD pointers stay explicit source metadata.
    let reader = Reader::open(fixture("subifd.tif"), Limits::default()).unwrap();
    assert_eq!(reader.frames(), 1);
    assert!(reader.metadata(0).unwrap().tags.contains_key(&330));
}
#[cfg(feature = "tiff")]
#[test]
fn detector_headers_preserve_units_arrays_duplicates_and_header_endianness() {
    use dioptrin_io::detector_tiff::{Format, FrameBuffer, Header, Limits, Reader, Value};
    let mut reader =
        Reader::open(fixture("pilatus.tif"), Format::Pilatus, Limits::default()).unwrap();
    let Header::Pilatus(header) = reader.metadata(0).unwrap().header else {
        panic!("wrong header")
    };
    assert_eq!(header.fields["Pixel_size"], "172e-6 m x 172e-6 m");
    assert_eq!(header.fields["Exposure_time"], "0.2 s");
    assert_eq!(header.fields["Tau"], "0 s");
    assert_eq!(
        header
            .entries
            .iter()
            .filter(|(k, _)| k == "Exposure_time")
            .count(),
        2
    );
    assert!(header.unparsed_lines.iter().any(|v| v == "plain retained"));
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    check("pilatus.tif", buffer.pixels().unwrap());
    for name in ["mar-le.mccd", "mar-be.mccd"] {
        let mut reader = Reader::open(fixture(name), Format::MarCcd, Limits::default()).unwrap();
        let Header::MarCcd(header) = reader.metadata(0).unwrap().header else {
            panic!("wrong header")
        };
        assert_eq!(header.little_endian, name == "mar-le.mccd");
        assert_eq!(header.bytes.len(), 3072);
        assert_eq!(header.fields["header_size"], Value::Unsigned(3072));
        assert_eq!(header.fields["xtal_to_detector"], Value::Signed(-12345));
        let Value::ShortArray(percentile) = &header.fields["percentile[128]"] else {
            panic!("wrong percentile")
        };
        assert_eq!(percentile[0], 65535);
        reader.read_frame_into(0, &mut buffer).unwrap();
        check(name, buffer.pixels().unwrap());
    }
    assert!(Reader::open(fixture("rgb-contig.tif"), Format::MarCcd, Limits::default()).is_err());
}
#[cfg(feature = "jpeg")]
#[test]
fn jpeg_reference_counts_color_and_orientation_metadata() {
    use dioptrin_io::jpeg::{FrameBuffer, Limits, Reader};
    for name in [
        "gray.jpg",
        "gray-progressive.jpg",
        "rgb.jpg",
        "rgb-progressive.jpg",
    ] {
        let reader = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut buffer = FrameBuffer::default();
        reader.read_frame_into(&mut buffer).unwrap();
        check(name, buffer.pixels().unwrap());
        assert!(reader.metadata().lossy);
        assert_eq!(reader.metadata().shape, [7, 11]);
        assert_eq!(
            reader.metadata().channels,
            if name.starts_with("rgb") { 3 } else { 1 }
        );
        assert!(
            reader
                .metadata()
                .segments
                .iter()
                .any(|s| s.marker == 0xe1 && s.payload.starts_with(b"Exif\0\0"))
        );
        assert_eq!(
            buffer.sample_shape().unwrap(),
            reader.metadata().sample_shape()
        );
    }
}
#[cfg(feature = "jpeg")]
#[test]
fn lossless_jpeg_preserves_unscaled_12_and_16_bit_counts() {
    use dioptrin_io::jpeg::{FrameBuffer, Limits, Reader};
    for bits in [12, 16] {
        let name = format!("lossless{bits}.jpg");
        let reader = Reader::open(fixture(&name), Limits::default()).unwrap();
        let mut buffer = FrameBuffer::default();
        reader.read_frame_into(&mut buffer).unwrap();
        assert!(!reader.metadata().lossy);
        assert_eq!(reader.metadata().precision, bits);
        check(&name, buffer.pixels().unwrap());
    }
}

#[cfg(feature = "jpeg2000")]
#[test]
fn jpeg2000_keeps_signed_values_native_precision_and_channels() {
    use dioptrin_io::jpeg2000::{FrameBuffer, Limits, Reader};
    for name in [
        "uint8.jp2",
        "uint16.jp2",
        "int16.jp2",
        "uint12.jp2",
        "rgb.jp2",
        "rgb.j2k",
        "lossy.jp2",
    ] {
        let reader = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut buffer = FrameBuffer::default();
        reader
            .read_frame_into(&mut buffer)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        check(name, buffer.pixels().unwrap());
        assert_eq!(
            reader.metadata().lossy,
            if name == "lossy.jp2" {
                Some(true)
            } else {
                None
            }
        );
        assert_eq!(
            reader.metadata().default_reversible_wavelet,
            Some(name != "lossy.jp2")
        );
        assert_eq!(
            buffer.sample_shape().unwrap(),
            reader.metadata().sample_shape()
        );
        if name == "uint12.jp2" {
            assert_eq!(reader.metadata().precision, vec![12]);
        }
        if name == "int16.jp2" {
            assert_eq!(reader.metadata().signed, vec![true]);
        }
    }
}
#[cfg(any(feature = "jpeg", feature = "jpeg2000"))]
#[test]
fn raster_bounds_malformed_streams_and_source_replacement_fail_closed() {
    // Same checks independently exercise both optional backends.
    macro_rules! exercise {
        ($module:ident,$name:literal) => {{
            use dioptrin_io::$module::{FrameBuffer, Limits, Reader};
            let source = std::fs::read(fixture($name)).unwrap();
            let path = std::env::temp_dir().join(format!(
                "dioptrin-images-{}-{}",
                std::process::id(),
                $name
            ));
            for limit in [
                Limits {
                    encoded_bytes: 1,
                    ..Limits::default()
                },
                Limits {
                    frame_bytes: 1,
                    ..Limits::default()
                },
                Limits {
                    metadata_bytes: 1,
                    ..Limits::default()
                },
                Limits {
                    decoder_bytes: 1,
                    ..Limits::default()
                },
            ] {
                match Reader::open(fixture($name), limit) {
                    Err(_) => {}
                    Ok(r) => assert!(r.read_frame_into(&mut FrameBuffer::default()).is_err()),
                }
            }
            for n in [0, 2, 10, source.len() / 2, source.len() - 2] {
                std::fs::write(&path, &source[..n]).unwrap();
                match Reader::open(&path, Limits::default()) {
                    Err(_) => {}
                    Ok(r) => assert!(r.read_frame_into(&mut FrameBuffer::default()).is_err()),
                }
            }
            std::fs::write(&path, &source).unwrap();
            let reader = Reader::open(&path, Limits::default()).unwrap();
            let mut buffer = FrameBuffer::default();
            reader.read_frame_into(&mut buffer).unwrap();
            std::fs::write(&path, b"replaced").unwrap();
            assert!(reader.read_frame_into(&mut buffer).is_err());
            assert!(buffer.pixels().is_none());
            std::fs::remove_file(path).unwrap();
        }};
    }
    #[cfg(feature = "jpeg")]
    exercise!(jpeg, "gray.jpg");
    #[cfg(feature = "jpeg2000")]
    exercise!(jpeg2000, "uint16.jp2");
}

#[cfg(feature = "jpeg2000")]
#[test]
fn jpeg2000_admission_rejects_hostile_boxes_precision_subsampling_and_shape() {
    use dioptrin_io::jpeg2000::{Limits, Reader};
    let original = std::fs::read(fixture("uint16.jp2")).unwrap();
    let siz = original.windows(2).position(|v| v == [0xff, 0x51]).unwrap();
    let path = std::env::temp_dir().join(format!("dioptrin-jp2-bounds-{}", std::process::id()));
    let mut cases = Vec::new();
    let mut data = original.clone();
    data[..4].copy_from_slice(&1u32.to_be_bytes());
    data[8..16].copy_from_slice(&u64::MAX.to_be_bytes());
    cases.push(data);
    let mut data = original.clone();
    data[siz + 4 + 36] = 31;
    cases.push(data); // precision=32 cannot be represented by backend signed components
    let mut data = original.clone();
    data[siz + 4 + 37] = 2;
    cases.push(data); // component subsampling
    let mut data = original.clone();
    data[siz + 4 + 2..siz + 4 + 6].copy_from_slice(&u32::MAX.to_be_bytes());
    cases.push(data);
    let mut data = original.clone();
    data[siz + 4 + 6..siz + 4 + 10].copy_from_slice(&0u32.to_be_bytes());
    cases.push(data);
    for data in cases {
        std::fs::write(&path, data).unwrap();
        assert!(Reader::open(&path, Limits::default()).is_err());
    }
    std::fs::remove_file(path).unwrap();
}
#[cfg(feature = "tiff")]
#[test]
fn malformed_detector_headers_and_packed_fill_order_fail_explicitly() {
    use dioptrin_io::{detector_tiff, tiff};
    let path =
        std::env::temp_dir().join(format!("dioptrin-detector-bounds-{}", std::process::id()));
    let source = std::fs::read(fixture("mar-le.mccd")).unwrap();
    for (offset, word) in [(1024 + 28, 0u32), (1024 + 36, 4096u32)] {
        let mut data = source.clone();
        data[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        std::fs::write(&path, data).unwrap();
        assert!(
            detector_tiff::Reader::open(
                &path,
                detector_tiff::Format::MarCcd,
                tiff::Limits::default()
            )
            .is_err()
        );
    }
    // Replace the packed file's compression entry with FillOrder=2.
    let mut data = std::fs::read(fixture("packed2-black.tif")).unwrap();
    let offset = 10 + 3 * 12;
    data[offset..offset + 2].copy_from_slice(&266u16.to_le_bytes());
    data[offset + 8..offset + 12].copy_from_slice(&2u32.to_le_bytes());
    std::fs::write(&path, data).unwrap();
    assert!(tiff::Reader::open(&path, tiff::Limits::default()).is_err());
    std::fs::remove_file(path).unwrap();
}
