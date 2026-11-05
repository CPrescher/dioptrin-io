#![cfg(feature = "tiff")]
use dioptrin_io::{
    PixelBuffer, PixelType,
    tiff::{FrameBuffer, Limits, Reader, TagValue},
};
use std::path::PathBuf;
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/tiff")
        .join(name)
}

#[test]
fn independent_scalar_types_preserve_counts_precision_sentinels_and_stored_orientation() {
    macro_rules! check {
        ($name:literal,$variant:ident,$ty:ty) => {{
            let mut reader =
                Reader::open(fixture(concat!($name, ".tif")), Limits::default()).unwrap();
            let meta = reader.metadata(0).unwrap();
            assert_eq!(meta.shape, [2, 3]);
            assert_eq!(meta.orientation, 6);
            assert_eq!(meta.pixel_type, PixelType::$variant);
            assert_eq!(
                meta.tags.get(&65000),
                Some(&TagValue::Ascii("original header".into()))
            );
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
            reader.read_frame_into(0, &mut buffer).unwrap();
            assert!(reader.read_frame_into(1, &mut buffer).is_err());
            assert!(buffer.pixels().is_none());
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

#[test]
fn floats_keep_nonfinite_values_and_signed_zero() {
    macro_rules! check {
        ($name:literal,$variant:ident,$ty:ty) => {{
            let mut reader = Reader::open(fixture($name), Limits::default()).unwrap();
            let mut buffer = FrameBuffer::default();
            reader.read_frame_into(0, &mut buffer).unwrap();
            let PixelBuffer::$variant(v) = buffer.pixels().unwrap() else {
                panic!("wrong dtype")
            };
            assert_eq!(v[0].to_bits(), (0.0 as $ty).to_bits());
            assert_eq!(v[1].to_bits(), (-0.0 as $ty).to_bits());
            assert!(v[2].is_nan());
            assert_eq!(v[3], <$ty>::INFINITY);
            assert_eq!(v[4], <$ty>::NEG_INFINITY);
            assert_eq!(v[5], -1.25);
        }};
    }
    check!("float32.tif", F32, f32);
    check!("float64.tif", F64, f64);
}

#[test]
fn independent_multiframe_compressed_edge_tiles_and_bigtiff_endianness() {
    for name in [
        "deflate-tiles.tif",
        "lzw-tiles.tif",
        "packbits-tiles.tif",
        "le-bigtiff.tif",
        "be-bigtiff.tif",
    ] {
        let mut reader = Reader::open(fixture(name), Limits::default()).unwrap();
        assert_eq!(reader.frames(), 2);
        assert_eq!(
            reader.metadata(0).unwrap().little_endian,
            !name.starts_with("be-")
        );
        let mut buffer = FrameBuffer::default();
        for i in [1, 0, 1] {
            reader.read_frame_into(i, &mut buffer).unwrap();
            assert_eq!(buffer.shape(), Some([17, 19]));
            let expected: Vec<u32> = (i * 17 * 19..(i + 1) * 17 * 19)
                .map(|x| x as u32 * 70000)
                .collect();
            match buffer.pixels().unwrap() {
                PixelBuffer::U32(v) => assert_eq!(v, &expected),
                PixelBuffer::I32(v) => {
                    assert_eq!(*v, expected.iter().map(|&x| x as i32).collect::<Vec<_>>())
                }
                _ => panic!("wrong dtype"),
            }
        }
    }
}

#[test]
fn unsupported_modes_limits_and_truncation_are_explicit() {
    assert!(Reader::open(fixture("white-is-zero.tif"), Limits::default()).is_err());
    for limits in [
        Limits {
            frames: 1,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 10,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(Reader::open(fixture("deflate-tiles.tif"), limits).is_err());
    }
    let bytes = std::fs::read(fixture("deflate-tiles.tif")).unwrap();
    let path = std::env::temp_dir().join(format!(
        "dioptrin-io-tiff-truncated-{}.tif",
        std::process::id()
    ));
    for len in [0, 8, 100, bytes.len() / 2] {
        std::fs::write(&path, &bytes[..len]).unwrap();
        if let Ok(mut reader) = Reader::open(&path, Limits::default()) {
            assert!(
                reader
                    .read_frame_into(0, &mut FrameBuffer::default())
                    .is_err()
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}
