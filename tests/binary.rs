#[cfg(any(feature = "cbf", feature = "edf"))]
use std::path::PathBuf;
#[cfg(any(feature = "cbf", feature = "edf"))]
fn fixture(format: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(format)
        .join(name)
}
#[cfg(feature = "cbf")]
#[test]
fn independent_cbf_offsets_preserve_integer_width_sign_and_escape_values() {
    use dioptrin_io::{
        PixelBuffer,
        cbf::{FrameBuffer, Limits, Reader},
    };
    macro_rules! check {
        ($name:literal,$variant:ident,$ty:ty) => {{
            let mode = if stringify!($ty) == "i32" {
                dioptrin_io::cbf::ByteOffsetMode::Legacy32
            } else {
                dioptrin_io::cbf::ByteOffsetMode::Extended64
            };
            let reader = Reader::open_with_mode(
                fixture("cbf", concat!($name, ".cbf")),
                Limits::default(),
                mode,
            )
            .unwrap();
            let mut buffer = FrameBuffer::default();
            reader.read_frame_into(&mut buffer).unwrap();
            let min = <$ty>::MIN as i128;
            let max = <$ty>::MAX as i128;
            let low = if min < 0 {
                if stringify!($ty) == "i64" {
                    -1_i128 << 40
                } else {
                    min
                }
            } else {
                1
            };
            let expected = [
                0,
                low,
                127,
                max.min(32768),
                max.min(1_i128 << 31),
                max.min((1_i128 << 53) + 1),
            ];
            assert_eq!(buffer.shape(), Some([2, 3]));
            assert_eq!(
                buffer.pixels(),
                Some(&PixelBuffer::$variant(
                    expected.iter().map(|&x| x as $ty).collect()
                ))
            );
            assert!(
                reader
                    .metadata()
                    .header
                    .windows(15)
                    .any(|w| w == b"Original header")
            );
        }};
    }
    check!("uint8", U8, u8);
    check!("uint16", U16, u16);
    check!("uint32", U32, u32);
    check!("uint64", U64, u64);
    check!("int8", I8, i8);
    check!("int16", I16, i16);
    check!("int32", I32, i32);
    check!("int64", I64, i64);
}
#[cfg(feature = "cbf")]
#[test]
fn cbf_corrupt_checksums_truncation_limits_and_unknown_conversions_fail() {
    use dioptrin_io::cbf::{FrameBuffer, Limits, Reader};
    let bytes = std::fs::read(fixture("cbf", "int64.cbf")).unwrap();
    let path = std::env::temp_dir().join(format!("dioptrin-cbf-bad-{}.cbf", std::process::id()));
    let start = bytes
        .windows(4)
        .position(|w| w == [12, 26, 4, 213])
        .unwrap()
        + 4;
    let mut bad = bytes.clone();
    bad[start] ^= 1;
    std::fs::write(&path, &bad).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    assert!(
        reader
            .read_frame_into(&mut buffer)
            .unwrap_err()
            .to_string()
            .contains("MD5")
    );
    assert!(buffer.pixels().is_none());
    drop(reader);
    for len in [0, 10, start, start + 1] {
        std::fs::write(&path, &bytes[..len]).unwrap();
        assert!(Reader::open(&path, Limits::default()).is_err());
    }
    let bad =
        String::from_utf8_lossy(&bytes[..start - 4]).replace("x-CBF_BYTE_OFFSET", "x-CBF_PACKED");
    let mut data = bad.into_bytes();
    data.extend_from_slice(&bytes[start - 4..]);
    std::fs::write(&path, data).unwrap();
    assert!(Reader::open(&path, Limits::default()).is_err());
    assert!(
        Reader::open(
            fixture("cbf", "int64.cbf"),
            Limits {
                frame_bytes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    std::fs::remove_file(path).unwrap();
}
#[cfg(feature = "edf")]
#[test]
fn independent_edf_scalars_both_byte_orders_keep_all_stored_bits() {
    use dioptrin_io::{
        PixelBuffer,
        edf::{FrameBuffer, Limits, Reader},
    };
    macro_rules! check {
        ($name:literal,$variant:ident,$ty:ty) => {{
            for order in ["le", "be"] {
                let reader = Reader::open(
                    fixture("edf", &format!("{}-{order}.edf", $name)),
                    Limits::default(),
                )
                .unwrap();
                let mut buffer = FrameBuffer::default();
                reader.read_frame_into(0, &mut buffer).unwrap();
                assert_eq!(
                    buffer.pixels(),
                    Some(&PixelBuffer::$variant(vec![
                        0,
                        <$ty>::MIN,
                        <$ty>::MAX,
                        2,
                        3,
                        4
                    ]))
                );
                assert_eq!(
                    reader.metadata(0).unwrap().fields["USERVALUE"],
                    "Original header"
                );
            }
        }};
    }
    check!("uint8", U8, u8);
    check!("uint16", U16, u16);
    check!("uint32", U32, u32);
    check!("uint64", U64, u64);
    check!("int8", I8, i8);
    check!("int16", I16, i16);
    check!("int32", I32, i32);
    check!("int64", I64, i64);
    macro_rules! floats {
        ($name:literal,$variant:ident,$ty:ty) => {{
            for order in ["le", "be"] {
                let reader = Reader::open(
                    fixture("edf", &format!("{}-{order}.edf", $name)),
                    Limits::default(),
                )
                .unwrap();
                let mut buffer = FrameBuffer::default();
                reader.read_frame_into(0, &mut buffer).unwrap();
                let PixelBuffer::$variant(v) = buffer.pixels().unwrap() else {
                    panic!("type")
                };
                assert_eq!(v[0].to_bits(), (0. as $ty).to_bits());
                assert_eq!(v[1].to_bits(), (-0. as $ty).to_bits());
                assert!(v[2].is_nan());
                assert_eq!(v[3], <$ty>::INFINITY);
                assert_eq!(v[4], <$ty>::NEG_INFINITY);
                assert_eq!(v[5], -1.25);
            }
        }};
    }
    floats!("float32", F32, f32);
    floats!("float64", F64, f64);
}
#[cfg(feature = "edf")]
#[test]
fn edf_compressed_stacks_and_truncation_limits() {
    use dioptrin_io::{
        PixelBuffer,
        edf::{FrameBuffer, Limits, Reader},
    };
    let path = fixture("edf", "compressed-stack.edf");
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(reader.frames(), 3);
    let mut buffer = FrameBuffer::default();
    for i in [2, 0, 1] {
        reader.read_frame_into(i, &mut buffer).unwrap();
        assert_eq!(
            buffer.pixels(),
            Some(&PixelBuffer::U32(
                (0..12).map(|x| x * 70000 + i as u32).collect()
            ))
        );
    }
    assert!(reader.read_frame_into(3, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    assert!(
        Reader::open(
            &path,
            Limits {
                frames: 2,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        Reader::open(
            &path,
            Limits {
                header_bytes: 10,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let bytes = std::fs::read(path).unwrap();
    let temp =
        std::env::temp_dir().join(format!("dioptrin-edf-truncated-{}.edf", std::process::id()));
    for n in [0, 10, 512, bytes.len() - 1] {
        std::fs::write(&temp, &bytes[..n]).unwrap();
        assert!(Reader::open(&temp, Limits::default()).is_err());
    }
    std::fs::remove_file(temp).unwrap();
}

#[cfg(feature = "edf")]
#[test]
fn corrupt_and_overlong_edf_streams_fail_without_exposing_partial_pixels() {
    use dioptrin_io::edf::{FrameBuffer, Limits, Reader};
    let source = std::fs::read(fixture("edf", "compressed-stack.edf")).unwrap();
    let path =
        std::env::temp_dir().join(format!("dioptrin-edf-corrupt-{}.edf", std::process::id()));
    for overlong in [false, true] {
        let mut bytes = source.clone();
        if overlong {
            let position = bytes.windows(9).position(|w| w == b"Dim_1 = 4").unwrap();
            bytes[position + 8] = b'3';
        } else {
            bytes[512] ^= 255;
        }
        std::fs::write(&path, bytes).unwrap();
        let reader = Reader::open(&path, Limits::default()).unwrap();
        let mut buffer = FrameBuffer::default();
        assert!(reader.read_frame_into(0, &mut buffer).is_err());
        assert!(buffer.pixels().is_none());
    }
    std::fs::remove_file(path).unwrap();
}
