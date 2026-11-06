#![cfg(feature = "tiff")]
//! Independent CC0 fixtures and malformed framing regressions.
use dioptrin_io::{
    PixelBuffer,
    tiff::{
        FrameBuffer, Limits, Reader,
        view::{self, Pixels, Policy},
    },
};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/tiff-rejections")
        .join(name)
}
fn file(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(bytes).unwrap();
    f
}
fn tag(bytes: &[u8], tag: u16) -> usize {
    let n = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize;
    (0..n)
        .map(|i| 10 + 12 * i)
        .find(|&p| bytes[p..p + 2] == tag.to_le_bytes())
        .unwrap()
}
fn set(bytes: &mut [u8], tagno: u16, value: u32) {
    let p = tag(bytes, tagno) + 8;
    bytes[p..p + 4].copy_from_slice(&value.to_le_bytes());
}
fn decode(bytes: &[u8]) -> Result<PixelBuffer, String> {
    let f = file(bytes);
    let mut r = Reader::open(f.path(), Limits::default()).map_err(|e| e.to_string())?;
    let mut b = FrameBuffer::default();
    r.read_frame_into(0, &mut b).map_err(|e| e.to_string())?;
    Ok(b.pixels().unwrap().clone())
}
#[test]
fn independent_group3_fillorder_and_packed_counts_match() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let original = std::fs::read(fixture(name)).unwrap();
        let expected = std::fs::read(fixture(&(name.clone() + ".raw"))).unwrap();
        let mut r = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut b = FrameBuffer::default();
        r.read_frame_into(0, &mut b)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            b.pixels(),
            Some(&PixelBuffer::U8(expected.clone())),
            "{name}"
        );
        let m = r.metadata(0).unwrap();
        assert_eq!(serde_json::json!(m.shape), case["shape"]);
        let image = view::convert(m, &b, Policy::Pillow13, 10000).unwrap();
        let actual = match image.pixels {
            Pixels::Bool(v) => v.into_iter().map(u8::from).collect(),
            Pixels::Scalar(PixelBuffer::U8(v)) => v,
            _ => panic!("unexpected view"),
        };
        assert_eq!(
            actual,
            std::fs::read(fixture(&(name.clone() + ".pillow"))).unwrap(),
            "{name}"
        );
        assert_eq!(b.pixels(), Some(&PixelBuffer::U8(expected)));
        assert_eq!(std::fs::read(fixture(name)).unwrap(), original);
    }
}
#[test]
fn malformed_inline_description_retains_every_literal_byte_and_original_entry() {
    for literal in [b"    ".as_slice(), b"\xaf", b"A\xffB\0"] {
        let name = format!(
            "opaque-{}.tif",
            literal
                .iter()
                .map(|x| format!("{x:02x}"))
                .collect::<String>()
        );
        let original = std::fs::read(fixture(&name)).unwrap();
        let r = Reader::open(fixture(&name), Limits::default()).unwrap();
        let opaque = &r.metadata(0).unwrap().opaque_tags[&270];
        let p = tag(&original, 270);
        assert_eq!(opaque.field_type, 2);
        assert_eq!(opaque.count, literal.len() as u64);
        assert_eq!(opaque.value_bytes, literal);
        assert_eq!(opaque.ifd_entry, &original[p..p + 12]);
    }
}
fn group3(bits: &str, height: u32) -> Vec<u8> {
    // Reuse only the independent fixture's structural IFD. Supply exact hand-coded
    // three-pixel white rows: EOL=000000000001, white3 terminating code=1000.
    let mut source = std::fs::read(fixture("group3-makeup.tif")).unwrap();
    let p = tag(&source, 273) + 8;
    let start = u32::from_le_bytes(source[p..p + 4].try_into().unwrap()) as usize;
    source.truncate(start);
    let mut padded = bits.to_string();
    padded.extend(std::iter::repeat_n('0', (8 - bits.len() % 8) % 8));
    let payload: Vec<u8> = padded
        .as_bytes()
        .chunks(8)
        .map(|chunk| chunk.iter().fold(0, |v, &b| (v << 1) | u8::from(b == b'1')))
        .collect();
    set(&mut source, 256, 3);
    set(&mut source, 257, height);
    set(&mut source, 278, height);
    set(&mut source, 279, payload.len() as u32);
    source.extend(payload);
    source
}
#[test]
fn strict_eol_row_width_and_tail_admission() {
    let eol = "000000000001";
    let row = format!("{eol}1000");
    assert_eq!(
        decode(&group3(&row, 1)).unwrap(),
        PixelBuffer::U8(vec![0; 3])
    );
    for zeros in 0..11 {
        let bad = format!("{}11000", "0".repeat(zeros));
        assert!(
            decode(&group3(&bad, 1)).is_err(),
            "initial EOL zeros={zeros}"
        );
        let bad = format!("{row}{}11000", "0".repeat(zeros));
        assert!(decode(&group3(&bad, 2)).is_err(), "row EOL zeros={zeros}");
    }
    for bad in [
        format!("{eol}000"),
        format!("{eol}1011"),
        format!("{row}{row}"),
        format!("{row}1"),
        format!("{row}{eol}{eol}"),
    ] {
        assert!(decode(&group3(&bad, 1)).is_err(), "{bad}");
    }
    assert_eq!(
        decode(&group3(&format!("{row}{}", eol.repeat(6)), 1)).unwrap(),
        PixelBuffer::U8(vec![0; 3])
    );
    assert!(decode(&group3(&format!("{row}{}1", eol.repeat(6)), 1)).is_err());
    // A makeup code must be followed by a terminating code and fit the row.
    assert!(decode(&group3(&format!("{eol}11011"), 1)).is_err());
}
#[test]
fn group3_source_bounds_options_resource_and_mutation_are_explicit() {
    let source = std::fs::read(fixture("group3-fill1-photo1.tif")).unwrap();
    let f = file(&source);
    let limit = Limits {
        intermediate_bytes: 1,
        ..Limits::default()
    };
    let mut r = Reader::open(f.path(), limit).unwrap();
    let mut b = FrameBuffer::default();
    assert!(
        r.read_frame_into(0, &mut b)
            .unwrap_err()
            .to_string()
            .contains("intermediate limit")
    );
    assert!(b.pixels().is_none());
    let f = file(&source);
    let mut r = Reader::open(f.path(), Limits::default()).unwrap();
    std::fs::write(f.path(), [0u8; 8]).unwrap();
    assert!(r.read_frame_into(0, &mut b).is_err());
    // Changing only the declared strip window never authorizes reads after it.
    let mut short = std::fs::read(fixture("group3-makeup.tif")).unwrap();
    set(&mut short, 279, 1);
    assert!(decode(&short).is_err());
    let mut bad = source.clone();
    set(&mut bad, 266, 3);
    assert!(decode(&bad).is_err());
    let mut bad = std::fs::read(fixture("group3-makeup.tif")).unwrap();
    set(&mut bad, 292, 1); // Declaring mixed mode cannot reinterpret this untagged 1D payload.
    assert!(decode(&bad).is_err());
    let mut bad = std::fs::read(fixture("group3-aligned.tif")).unwrap();
    set(&mut bad, 292, 0); // Actual byte-fill cannot contradict source options.
    assert!(decode(&bad).is_err());
}

#[test]
fn malformed_bigtiff_inline_ascii_keeps_all_eight_bytes() {
    let literal = *b"ABCDEFGH";
    let tags = [
        (256u16, 4u16, 1u64, 3u64),
        (257, 4, 1, 1),
        (258, 3, 1, 8),
        (259, 3, 1, 1),
        (262, 3, 1, 1),
        (270, 2, 8, u64::from_le_bytes(literal)),
        (273, 16, 1, 16 + 8 + 9 * 20 + 8),
        (278, 4, 1, 1),
        (279, 4, 1, 3),
    ];
    let mut bytes = b"II+\0\x08\0\0\0".to_vec();
    bytes.extend_from_slice(&16u64.to_le_bytes());
    bytes.extend_from_slice(&9u64.to_le_bytes());
    for (tag, ty, count, value) in tags {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&ty.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&[17, 18, 19]);
    assert_eq!(decode(&bytes).unwrap(), PixelBuffer::U8(vec![17, 18, 19]));
    let f = file(&bytes);
    let reader = Reader::open(f.path(), Limits::default()).unwrap();
    let opaque = &reader.metadata(0).unwrap().opaque_tags[&270];
    assert_eq!(opaque.value_bytes, literal);
    assert_eq!(opaque.count, 8);
    assert_eq!(opaque.ifd_entry.len(), 20);
    assert_eq!(std::fs::read(f.path()).unwrap(), bytes);
}
