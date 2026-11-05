#![cfg(feature = "native")]
use dioptrin_io::{
    PixelBuffer, PixelType,
    native::{Format, FrameBuffer, Limits, Offset, RawLayout, Reader},
};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/native")
        .join(name)
}
fn hash(pixels: &PixelBuffer) -> String {
    let mut hash = Sha256::new();
    macro_rules! values { ($($v:ident);*) => {match pixels { $(PixelBuffer::$v(v) => {for x in v {hash.update(x.to_le_bytes());}}),* }}; }
    values!(U8;U16;U32;U64;I8;I16;I32;I64;F32;F64);
    format!("{:x}", hash.finalize())
}
#[test]
fn independent_files_match_every_original_sample_bit_and_stack_order() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    let mut buffer = FrameBuffer::default();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let format = match case["format"].as_str().unwrap() {
            "Npy" => Format::Npy,
            "Dtrek" => Format::Dtrek,
            "Fit2dMask" => Format::Fit2dMask,
            "HiPic" => Format::HiPic,
            "Pnm" => Format::Pnm,
            "Fit2dSpreadsheet" => Format::Fit2dSpreadsheet,
            _ => panic!("format"),
        };
        let path = fixture(name);
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
            case["file_sha256"].as_str().unwrap()
        );
        let reader = Reader::open(&path, format, Limits::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let frames = case["frame_sha256"].as_array().unwrap();
        assert_eq!(reader.frames(), frames.len(), "{name}");
        for (index, expected) in frames.iter().enumerate().rev() {
            reader.read_frame_into(index, &mut buffer).unwrap();
            assert_eq!(
                hash(buffer.pixels().unwrap()),
                expected.as_str().unwrap(),
                "{name}: {index}"
            );
            assert_eq!(
                buffer.shape(),
                Some([
                    case["shape"][0].as_u64().unwrap() as usize,
                    case["shape"][1].as_u64().unwrap() as usize
                ])
            );
        }
        assert!(
            reader
                .read_frame_into(reader.frames(), &mut buffer)
                .is_err()
        );
        assert!(buffer.pixels().is_none());
    }
}

#[test]
fn explicit_raw_offsets_all_types_endianness_and_failed_reads() {
    let path = std::env::temp_dir().join(format!("dioptrin-raw-{}.bin", std::process::id()));
    let mut buffer = FrameBuffer::default();
    macro_rules! check {
        ($v:ident,$ty:ty,$zero:expr,$min:expr,$max:expr) => {{
            let values: [$ty; 3] = [$zero, $min, $max];
            for little in [true, false] {
                let mut bytes = b"Header!".to_vec();
                for x in values {
                    bytes.extend(if little {
                        x.to_le_bytes()
                    } else {
                        x.to_be_bytes()
                    });
                }
                bytes.extend(b"tail");
                std::fs::write(&path, bytes).unwrap();
                for offset in [Offset::Absolute(7), Offset::FromEnd { trailing_bytes: 4 }] {
                    let reader = Reader::open_raw(
                        &path,
                        RawLayout {
                            shape: [1, 3],
                            pixel_type: PixelType::$v,
                            little_endian: little,
                            offset,
                        },
                        Limits::default(),
                    )
                    .unwrap();
                    reader.read_frame_into(0, &mut buffer).unwrap();
                    let PixelBuffer::$v(actual) = buffer.pixels().unwrap() else {
                        panic!("type")
                    };
                    for (a, b) in actual.iter().zip(values) {
                        assert_eq!(a.to_le_bytes(), b.to_le_bytes());
                    }
                }
            }
        }};
    }
    check!(U8, u8, 0, u8::MIN, u8::MAX);
    check!(U16, u16, 0, u16::MIN, u16::MAX);
    check!(U32, u32, 0, u32::MIN, u32::MAX);
    check!(U64, u64, 0, u64::MIN, u64::MAX);
    check!(I8, i8, 0, i8::MIN, i8::MAX);
    check!(I16, i16, 0, i16::MIN, i16::MAX);
    check!(I32, i32, 0, i32::MIN, i32::MAX);
    check!(I64, i64, 0, i64::MIN, i64::MAX);
    check!(F32, f32, -0., f32::NEG_INFINITY, f32::NAN);
    check!(F64, f64, -0., f64::NEG_INFINITY, f64::NAN);
    let layout = RawLayout {
        shape: [1, 3],
        pixel_type: PixelType::F64,
        little_endian: false,
        offset: Offset::Absolute(7),
    };
    let reader = Reader::open_raw(&path, layout, Limits::default()).unwrap();
    std::fs::write(&path, b"changed").unwrap();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    drop(reader);
    assert!(Reader::open_raw(&path, layout, Limits::default()).is_err());
    assert!(
        Reader::open_raw(
            &path,
            RawLayout {
                offset: Offset::FromEnd {
                    trailing_bytes: u64::MAX
                },
                ..layout
            },
            Limits::default()
        )
        .is_err()
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn malformed_truncated_oversized_and_unsupported_sources_fail_closed() {
    let path = std::env::temp_dir().join(format!("dioptrin-native-bad-{}.bin", std::process::id()));
    for (name, format) in [
        ("npy-uint64-be-F.npy", Format::Npy),
        ("dtrek-int32-big.img", Format::Dtrek),
        ("mask-33.msk", Format::Fit2dMask),
        ("hipic.img", Format::HiPic),
        ("p5-uint16.pgm", Format::Pnm),
    ] {
        let bytes = std::fs::read(fixture(name)).unwrap();
        for len in [0, 1, bytes.len() / 2, bytes.len() - 1] {
            std::fs::write(&path, &bytes[..len]).unwrap();
            assert!(
                Reader::open(&path, format, Limits::default()).is_err(),
                "{name}: {len}"
            );
        }
        let mut bad = bytes.clone();
        bad.push(0);
        std::fs::write(&path, bad).unwrap();
        assert!(
            Reader::open(&path, format, Limits::default()).is_err(),
            "{name}: trailing byte"
        );
        for limits in [
            Limits {
                frame_bytes: 1,
                ..Limits::default()
            },
            Limits {
                header_bytes: 1,
                ..Limits::default()
            },
            Limits {
                metadata_bytes: 1,
                ..Limits::default()
            },
        ] {
            assert!(
                Reader::open(fixture(name), format, limits).is_err(),
                "{name}: limits"
            );
        }
    }
    fn npy_header(text: &str) -> Vec<u8> {
        let mut b = b"\x93NUMPY\x01\x00".to_vec();
        b.extend((text.len() as u16).to_le_bytes());
        b.extend(text.as_bytes());
        b
    }
    for text in [
        "{'descr':'<u8','fortran_order':False,'shape':(18446744073709551615,2)}",
        "{'descr':'<u8','fortran_order':False,'shape':(0,3)}",
        "{'descr':'|O','fortran_order':False,'shape':(2,3)}",
        "{'descr':'<f16','fortran_order':False,'shape':(2,3)}",
        "{'descr':'<u8','descr':'<i8','fortran_order':False,'shape':(2,3)}",
        "{'descr':'<u8','fortran_order':False,'shape':(2,3)}; print('no')",
        "{'descr':[('x','<u8')],'fortran_order':False,'shape':(2,3)}",
    ] {
        std::fs::write(&path, npy_header(text)).unwrap();
        assert!(Reader::open(&path, Format::Npy, Limits::default()).is_err());
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn masks_headers_and_ascii_errors_preserve_explicit_meaning() {
    let reader = Reader::open(fixture("hipic.img"), Format::HiPic, Limits::default()).unwrap();
    assert_eq!(reader.metadata().fields["Dim_1_offset"], "7");
    assert_eq!(reader.metadata().fields["Exposure"], "12.5");
    assert_eq!(reader.metadata().fields["Label"], "Synthetic");
    assert!(
        reader
            .metadata()
            .header
            .windows(b"Label=\"First".len())
            .any(|s| s == b"Label=\"First")
    );
    let dtrek = Reader::open(
        fixture("dtrek-int32-big.img"),
        Format::Dtrek,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        dtrek.metadata().fields["COMMENT"],
        "{ Stored axes {nested} and counts }"
    );
    let path = std::env::temp_dir().join(format!("dioptrin-pnm-bad-{}.pnm", std::process::id()));
    let mut buffer = FrameBuffer::default();
    for text in [
        "P2\n3 1\n255\n1 2",
        "P2\n3 1\n255\n1 2 256",
        "P2\n3 1\n255\n1 2 3 4",
        "P1\n3 1\n0 -1 1",
    ] {
        std::fs::write(&path, text).unwrap();
        let reader = Reader::open(&path, Format::Pnm, Limits::default()).unwrap();
        assert!(reader.read_frame_into(0, &mut buffer).is_err());
        assert!(buffer.pixels().is_none());
        drop(reader);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn spreadsheet_requires_complete_numeric_rows_without_invented_values() {
    let path = tempfile_path();
    let mut buffer = FrameBuffer::default();
    for text in [
        "3 2\n1 2 3\n4 5",
        "3 1\n1 2 3 4\n",
        "3 1\n1 2 x\n",
        "3 1\n1 2 3\n4 5 6\n",
    ] {
        std::fs::write(&path, text).unwrap();
        let reader = Reader::open(&path, Format::Fit2dSpreadsheet, Limits::default()).unwrap();
        assert!(reader.read_frame_into(0, &mut buffer).is_err());
        assert!(buffer.pixels().is_none());
    }
    std::fs::remove_file(path).unwrap();
}
fn tempfile_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "dioptrin-spreadsheet-bad-{}.spr",
        std::process::id()
    ))
}
