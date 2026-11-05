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
        let mut reader = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut buffer = FrameBuffer::default();
        reader.read_frame_into(0, &mut buffer).unwrap();
        check(name, buffer.pixels().unwrap());
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
#[cfg(feature = "jpeg2000")]
#[test]
fn jpeg2000_channel_definitions_cannot_reorder_stored_components() {
    use dioptrin_io::jpeg2000::{FrameBuffer, Limits, Reader};
    let source = fixture("rgb.jp2");
    let reader = Reader::open(&source, Limits::default()).unwrap();
    let mut output = FrameBuffer::default();
    reader.read_frame_into(&mut output).unwrap();
    assert_eq!(reader.metadata().channels, 3);
    assert_eq!(output.sample_shape(), Some(vec![3, 4, 3]));
    let original = std::fs::read(&source).unwrap();
    let mut header = 0;
    while &original[header + 4..header + 8] != b"jp2h" {
        header += u32::from_be_bytes(original[header..header + 4].try_into().unwrap()) as usize;
    }
    let header_length = u32::from_be_bytes(original[header..header + 4].try_into().unwrap());
    let path = std::env::temp_dir().join(format!("dioptrin-jp2-cdef-{}", std::process::id()));
    for associations in [[3u16, 2, 1], [1, 2, 3]] {
        // A legal same-precision channel definition can swap R/B in OpenJPEG
        // without changing SIZ dimensions, precision or signedness. Reject the
        // mapping before decoding; identity cdef is also explicitly unsupported.
        let mut definition = Vec::new();
        definition.extend(28u32.to_be_bytes());
        definition.extend(b"cdef");
        definition.extend(3u16.to_be_bytes());
        for (channel, association) in associations.iter().enumerate() {
            definition.extend((channel as u16).to_be_bytes());
            definition.extend(0u16.to_be_bytes());
            definition.extend(association.to_be_bytes());
        }
        let mut bytes = original.clone();
        let end = header + header_length as usize;
        bytes.splice(end..end, definition);
        bytes[header..header + 4].copy_from_slice(&(header_length + 28).to_be_bytes());
        std::fs::write(&path, bytes).unwrap();
        let error = match Reader::open(&path, Limits::default()) {
            Ok(_) => panic!("cdef must not silently reorder source channels"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cdef"));
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

#[cfg(feature = "jpeg")]
#[test]
fn jpeg_point_transforms_in_every_lossless_scan_retain_lossy_provenance() {
    use dioptrin_io::jpeg::{FrameBuffer, Limits, Reader};
    for (name, points) in [
        ("point-transform.jpg", vec![2]),
        ("point-transform12.jpg", vec![3]),
        ("later-point-transform.jpg", vec![0, 2, 0]),
        ("lossless8-scans.jpg", vec![0, 0, 0]),
    ] {
        let reader = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut buffer = FrameBuffer::default();
        reader.read_frame_into(&mut buffer).unwrap();
        assert_eq!(reader.metadata().lossy, points.iter().any(|&p| p != 0));
        assert_eq!(reader.metadata().lossless_point_transforms, points);
        check(name, buffer.pixels().unwrap());
    }
    let error = Reader::open(fixture("precision7.jpg"), Limits::default())
        .err()
        .expect("unsupported precision must fail at admission");
    assert!(error.to_string().contains("below 8 bits"));
    // A later scan must not silently replace an already decoded component.
    let path = std::env::temp_dir().join(format!("dioptrin-jpeg-scans-{}", std::process::id()));
    let source = std::fs::read(fixture("later-point-transform.jpg")).unwrap();
    let scans: Vec<_> = source
        .windows(2)
        .enumerate()
        .filter_map(|(i, v)| (v == [255, 218]).then_some(i))
        .collect();
    // Reject unsupported component counts before jpeg-decoder::info(), which
    // assumes 1/3/4 channels internally.
    let frame = source.windows(2).position(|v| v == [255, 195]).unwrap();
    let mut two_channels = source.clone();
    two_channels[frame + 9] = 2;
    two_channels.drain(frame + 16..frame + 19);
    two_channels[frame + 2..frame + 4].copy_from_slice(&14u16.to_be_bytes());
    std::fs::write(&path, two_channels).unwrap();
    assert!(
        Reader::open(&path, Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("component table")
    );
    let mut repeated = source.clone();
    repeated[scans[1] + 5] = 1;
    std::fs::write(&path, repeated).unwrap();
    assert!(Reader::open(&path, Limits::default()).is_err());
    let mut missing = source[..scans[2]].to_vec();
    missing.extend([255, 217]);
    std::fs::write(&path, missing).unwrap();
    assert!(Reader::open(&path, Limits::default()).is_err());
    let mut truncated = source.clone();
    truncated.drain(scans[1] + 5..scans[1] + 7);
    std::fs::write(&path, truncated).unwrap();
    assert!(Reader::open(&path, Limits::default()).is_err());
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "tiff")]
#[test]
fn tiff_jpeg_rejects_contradictory_chunk_components_dimensions_and_precision() {
    use dioptrin_io::tiff::{Limits, Reader};
    let source = std::fs::read(fixture("jpeg-rgb.tif")).unwrap();
    let path =
        std::env::temp_dir().join(format!("dioptrin-tiff-jpeg-layout-{}", std::process::id()));
    let ifd = u32::from_le_bytes(source[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(source[ifd..ifd + 2].try_into().unwrap()) as usize;
    for mutation in 0..4 {
        let mut bytes = source.clone();
        for i in 0..count {
            let offset = ifd + 2 + i * 12;
            let tag = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
            if mutation == 0 && [258, 262, 277].contains(&tag) {
                // JPEG remains RGB while all TIFF layout tags now claim gray.
                bytes[offset + 4..offset + 8].copy_from_slice(&1u32.to_le_bytes());
                bytes[offset + 8..offset + 12]
                    .copy_from_slice(&(if tag == 258 { 8u32 } else { 1 }).to_le_bytes());
            }
            if (mutation == 1 && tag == 256) || (mutation == 2 && tag == 257) {
                bytes[offset + 8..offset + 12].copy_from_slice(&1u32.to_le_bytes());
            }
        }
        if mutation == 3 {
            let sof = bytes.windows(2).position(|b| b == [0xff, 0xc0]).unwrap();
            bytes[sof + 4] = 12;
        }
        std::fs::write(&path, bytes).unwrap();
        assert!(
            Reader::open(&path, Limits::default()).is_err(),
            "mutation {mutation}"
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "tiff")]
#[test]
fn tiff_jpeg_validates_all_strips_tiles_and_shared_tables_without_losing_samples() {
    use dioptrin_io::{
        PixelBuffer,
        tiff::{FrameBuffer, Limits, Reader},
    };
    // Independent CC0 16x16 constant-128 JPEG, Pillow quality100. It decodes
    // exactly to128. Tiny runtime TIFF containers avoid adding detector files.
    const JPEG: &str = "ffd8ffe000104a46494600010100000100010000ffdb00430001010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101ffc0000b080010001001011100ffc4001f0000010501010101010100000000000000000102030405060708090a0bffc400b5100002010303020403050504040000017d01020300041105122131410613516107227114328191a1082342b1c11552d1f02433627282090a161718191a25262728292a3435363738393a434445464748494a535455565758595a636465666768696a737475767778797a838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7f8f9faffda0008010100003f0028a28affd9";
    let jpeg: Vec<u8> = JPEG
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect();
    let path =
        std::env::temp_dir().join(format!("dioptrin-tiff-jpeg-valid-{}", std::process::id()));
    for tiled in [false, true] {
        for shared in [false, true] {
            let mut tables = vec![0xff, 0xd8];
            let mut strip = vec![0xff, 0xd8];
            let mut cursor = 2;
            while jpeg[cursor + 1] != 0xda {
                let marker = jpeg[cursor + 1];
                let length =
                    u16::from_be_bytes(jpeg[cursor + 2..cursor + 4].try_into().unwrap()) as usize;
                let end = cursor + 2 + length;
                if shared && [0xdb, 0xc4].contains(&marker) {
                    tables.extend(&jpeg[cursor..end]);
                } else {
                    strip.extend(&jpeg[cursor..end]);
                }
                cursor = end;
            }
            strip.extend(&jpeg[cursor..]);
            tables.extend([0xff, 0xd9]);
            let chunks = if tiled { 4u32 } else { 2 };
            let width = if tiled { 29u32 } else { 16 };
            let height = if tiled { 31u32 } else { 32 };
            let mut fields = vec![
                (256u16, 4u16, 1u32, width),
                (257, 4, 1, height),
                (258, 3, 1, 8),
                (259, 3, 1, 7),
                (262, 3, 1, 1),
                (277, 3, 1, 1),
            ];
            if tiled {
                fields.extend([(322, 4, 1, 16), (323, 4, 1, 16)]);
            } else {
                fields.push((278, 4, 1, 16));
            }
            fields.push((if tiled { 324 } else { 273 }, 4, chunks, 0));
            fields.push((if tiled { 325 } else { 279 }, 4, chunks, 0));
            if shared {
                fields.push((347, 7, tables.len() as u32, 0));
            }
            fields.sort_by_key(|f| f.0);
            let arrays = 8 + 2 + fields.len() * 12 + 4;
            let table_start = arrays + chunks as usize * 8;
            let payload = table_start + if shared { tables.len() } else { 0 };
            let mut bytes = b"II\x2a\0\x08\0\0\0".to_vec();
            bytes.extend((fields.len() as u16).to_le_bytes());
            for (tag, kind, count, mut value) in fields {
                if tag == 273 || tag == 324 {
                    value = arrays as u32;
                }
                if tag == 279 || tag == 325 {
                    value = arrays as u32 + chunks * 4;
                }
                if tag == 347 {
                    value = table_start as u32;
                }
                bytes.extend(tag.to_le_bytes());
                bytes.extend(kind.to_le_bytes());
                bytes.extend(count.to_le_bytes());
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend(0u32.to_le_bytes());
            for i in 0..chunks {
                bytes.extend((payload as u32 + i * strip.len() as u32).to_le_bytes());
            }
            for _ in 0..chunks {
                bytes.extend((strip.len() as u32).to_le_bytes());
            }
            if shared {
                bytes.extend(&tables);
            }
            for _ in 0..chunks {
                bytes.extend(&strip);
            }
            std::fs::write(&path, &bytes).unwrap();
            let mut reader = Reader::open(&path, Limits::default())
                .unwrap_or_else(|e| panic!("tiled{tiled} shared{shared}: {e}"));
            let mut output = FrameBuffer::default();
            reader.read_frame_into(0, &mut output).unwrap();
            assert_eq!(output.shape(), Some([height as usize, width as usize]));
            assert_eq!(
                output.pixels(),
                Some(&PixelBuffer::U8(vec![
                    128;
                    width as usize * height as usize
                ]))
            );
            // Corrupt only the last record: admission must validate all chunks.
            let last = payload + (chunks as usize - 1) * strip.len();
            let sof = bytes[last..]
                .windows(2)
                .position(|b| b == [0xff, 0xc0])
                .unwrap()
                + last;
            bytes[sof + 7..sof + 9].copy_from_slice(&1u16.to_be_bytes());
            std::fs::write(&path, &bytes).unwrap();
            assert!(Reader::open(&path, Limits::default()).is_err());
        }
    }
    std::fs::remove_file(path).unwrap();
}
