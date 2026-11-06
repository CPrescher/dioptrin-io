#![cfg(feature = "native")]
use dioptrin_io::{
    PixelBuffer, PixelType,
    native::{Format, FrameBuffer, Limits, Reader},
};
use std::io::Write;
fn source(dtype: &str, shape: &str, fortran: bool, payload: &[u8]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    let mut h = format!(
        "{{'descr': '{dtype}', 'fortran_order': {}, 'shape': {shape}, }}",
        if fortran { "True" } else { "False" }
    )
    .into_bytes();
    let len = (10 + h.len() + 1).div_ceil(64) * 64 - 10;
    h.resize(len - 1, b' ');
    h.push(b'\n');
    f.write_all(b"\x93NUMPY\x01\x00").unwrap();
    f.write_all(&(h.len() as u16).to_le_bytes()).unwrap();
    f.write_all(&h).unwrap();
    f.write_all(payload).unwrap();
    f.flush().unwrap();
    f
}
#[test]
fn booleans_preserve_noncanonical_bytes_and_explicit_truth_view() {
    let f = source("|b1", "(2, 3)", false, &[0, 1, 2, 127, 128, 255]);
    let r = Reader::open(f.path(), Format::Npy, Limits::default()).unwrap();
    let mut b = FrameBuffer::default();
    r.read_frame_into(0, &mut b).unwrap();
    assert!(matches!(b.pixels(),Some(PixelBuffer::Bool(v)) if v==&[0,1,2,127,128,255]));
    assert_eq!(
        b.pixels()
            .unwrap()
            .logical_booleans()
            .unwrap()
            .collect::<Vec<_>>(),
        [false, true, true, true, true, true]
    );
    assert!(r.read_frame_into(1, &mut b).is_err());
    assert!(b.pixels().is_none());
    assert!(b.shape().is_none());
}
#[test]
fn half_words_endian_and_fortran_indexing_preserve_nan_payloads() {
    let words = [0x8000u16, 0x0001, 0x7c00, 0x7e42, 0xfc00, 0x3c00];
    let bytes: Vec<_> = [words[0], words[3], words[1], words[4], words[2], words[5]]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect();
    let f = source(">f2", "(2, 3)", true, &bytes);
    let r = Reader::open(f.path(), Format::Npy, Limits::default()).unwrap();
    assert_eq!(r.metadata().pixel_type, PixelType::F16);
    assert!(!r.metadata().little_endian);
    let mut b = FrameBuffer::default();
    r.read_frame_into(0, &mut b).unwrap();
    assert_eq!(b.shape(), Some([2, 3]));
    assert!(matches!(b.pixels(),Some(PixelBuffer::F16(v)) if v==&words));
}
#[test]
fn empty_spatial_and_leading_axes_have_distinct_frame_contracts() {
    let mut b = FrameBuffer::default();
    let huge_empty = format!("({}, 0)", usize::MAX);
    for (shape, frames, plane) in [
        ("(2, 0)", 1, [2, 0]),
        ("(0, 3)", 1, [0, 3]),
        ("(2, 3, 0)", 2, [3, 0]),
        ("(0, 2, 3)", 0, [2, 3]),
        ("(4, 0, 2, 3)", 0, [2, 3]),
        ("(0,)", 1, [1, 0]),
        (huge_empty.as_str(), 1, [usize::MAX, 0]),
    ] {
        let f = source("<f2", shape, true, &[]);
        let r = Reader::open(f.path(), Format::Npy, Limits::default()).unwrap();
        assert_eq!(r.frames(), frames);
        for i in 0..frames {
            r.read_frame_into(i, &mut b).unwrap();
            assert_eq!(b.shape(), Some(plane));
            assert!(b.pixels().unwrap().is_empty());
        }
        assert!(r.read_frame_into(frames, &mut b).is_err());
        assert!(b.pixels().is_none());
    }
}
#[test]
fn malformed_unsupported_bounds_and_mutation_never_expose_stale_counts() {
    for (dtype, shape, payload) in [
        ("|b1", "(1, 2)", vec![0]),
        ("<f2", "(1, 1)", vec![0, 0, 0]),
        ("|O8", "(1, 1)", vec![0; 8]),
        ("<f16", "(1, 1)", vec![0; 16]),
        ("<f2", "(18446744073709551615, 2)", vec![]),
    ] {
        let f = source(dtype, shape, false, &payload);
        assert!(Reader::open(f.path(), Format::Npy, Limits::default()).is_err());
    }
    let f = source("<f2", "(1, 2)", false, &[0, 0, 1, 0]);
    assert!(
        Reader::open(
            f.path(),
            Format::Npy,
            Limits {
                frame_bytes: 3,
                ..Default::default()
            }
        )
        .is_err()
    );
    let r = Reader::open(f.path(), Format::Npy, Limits::default()).unwrap();
    let mut b = FrameBuffer::default();
    r.read_frame_into(0, &mut b).unwrap();
    std::fs::write(f.path(), b"replaced").unwrap();
    assert!(r.read_frame_into(0, &mut b).is_err());
    assert!(b.pixels().is_none());
}
#[cfg(feature = "wrappers")]
#[test]
fn captured_stream_keeps_the_same_half_bits() {
    let f = source(">f2", "(1, 2)", false, &[0x80, 0, 0x7e, 0x42]);
    let bytes = std::fs::read(f.path()).unwrap();
    let input =
        dioptrin_io::wrapped::Input::from_reader(&bytes[..], None, Default::default()).unwrap();
    let mut r = input
        .read(
            dioptrin_io::wrapped::Format::Native(Format::Npy),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let mut b = dioptrin_io::wrapped::FrameBuffer::default();
    r.read_frame_into(0, &mut b).unwrap();
    assert!(matches!(b.pixels(),Some(PixelBuffer::F16(v)) if v==&[0x8000,0x7e42]));
}

#[test]
fn independent_fixture_matrix_preserves_every_original_byte_and_axis() {
    use sha2::{Digest, Sha256};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/npy-types");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("provenance.json")).unwrap()).unwrap();
    let mut buffer = FrameBuffer::default();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let bytes = std::fs::read(root.join(name)).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            case["sha256"].as_str().unwrap(),
            "{name}"
        );
        let reader = Reader::open(root.join(name), Format::Npy, Limits::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let m = reader.metadata();
        assert_eq!(
            serde_json::json!(m.source_shape),
            case["source_shape"],
            "{name}"
        );
        assert_eq!(serde_json::json!(m.shape), case["frame_shape"], "{name}");
        assert_eq!(
            m.fortran_order,
            case["fortran_order"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(
            m.little_endian,
            case["little_endian"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(
            format!("{:?}", m.pixel_type),
            case["pixel_type"].as_str().unwrap(),
            "{name}"
        );
        assert_eq!(
            reader.frames(),
            case["frames"].as_u64().unwrap() as usize,
            "{name}"
        );
        for (frame, expected) in case["frame_hex"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .rev()
        {
            reader.read_frame_into(frame, &mut buffer).unwrap();
            let bytes = match buffer.pixels().unwrap() {
                PixelBuffer::Bool(v) => v.clone(),
                PixelBuffer::F16(v) => v.iter().flat_map(|v| v.to_le_bytes()).collect(),
                _ => panic!("wrongtype"),
            };
            let actual = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
            assert_eq!(&actual, expected.as_str().unwrap(), "{name}: {frame}");
        }
        assert!(
            reader
                .read_frame_into(reader.frames(), &mut buffer)
                .is_err()
        );
        assert!(buffer.pixels().is_none());
    }
    for category in ["unsupported", "malformed"] {
        for name in manifest[category].as_object().unwrap().keys() {
            assert!(
                Reader::open(root.join(name), Format::Npy, Limits::default()).is_err(),
                "{category}: {name}"
            );
        }
    }
}
