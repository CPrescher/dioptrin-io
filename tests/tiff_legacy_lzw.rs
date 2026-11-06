#![cfg(feature = "tiff")]
//! CC0 independent legacy LZW streams, plus byte-level source-window regressions.
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
        .join("tests/data/tiff-legacy-lzw")
        .join(name)
}
fn file(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(bytes).unwrap();
    f
}
fn pixels(values: &PixelBuffer) -> Vec<u8> {
    match values {
        PixelBuffer::U8(v) => v.clone(),
        PixelBuffer::U16(v) => v.iter().flat_map(|v| v.to_le_bytes()).collect(),
        PixelBuffer::F32(v) => v.iter().flat_map(|v| v.to_bits().to_le_bytes()).collect(),
        _ => panic!("fixture type"),
    }
}
fn tag(bytes: &[u8], wanted: u16) -> usize {
    let n = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize;
    (0..n)
        .map(|i| 10 + 12 * i)
        .find(|&p| bytes[p..p + 2] == wanted.to_le_bytes())
        .unwrap()
}
fn value(bytes: &[u8], wanted: u16) -> u32 {
    let p = tag(bytes, wanted) + 8;
    u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap())
}
fn set(bytes: &mut [u8], wanted: u16, value: u32) {
    let p = tag(bytes, wanted) + 8;
    bytes[p..p + 4].copy_from_slice(&value.to_le_bytes());
}
fn set_last_count(bytes: &mut [u8], wanted: u16, value: u32) -> usize {
    let p = tag(bytes, wanted);
    let n = u32::from_le_bytes(bytes[p + 4..p + 8].try_into().unwrap()) as usize;
    let table = u32::from_le_bytes(bytes[p + 8..p + 12].try_into().unwrap()) as usize;
    let position = table + 4 * (n - 1);
    let original = u32::from_le_bytes(bytes[position..position + 4].try_into().unwrap());
    if wanted == 325 {
        bytes[position..position + 4].copy_from_slice(&value.to_le_bytes());
    }
    original as usize
}
fn decode(bytes: &[u8]) -> Result<PixelBuffer, String> {
    let f = file(bytes);
    let mut r = Reader::open(f.path(), Limits::default()).map_err(|e| e.to_string())?;
    let mut b = FrameBuffer::default();
    r.read_frame_into(0, &mut b).map_err(|e| e.to_string())?;
    Ok(b.pixels().unwrap().clone())
}
#[test]
fn independent_counts_dictionary_widths_clears_kwkwk_and_predictors() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    assert!(
        manifest["files"]["dictionary-widths.tif"]["stream_statistics"][0]["width12"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        manifest["files"]["clears-after-growth.tif"]["stream_statistics"][0]["clears"]
            .as_u64()
            .unwrap()
            > 1
    );
    assert!(
        manifest["files"]["kwkwk.tif"]["stream_statistics"][0]["kwkwk"]
            .as_u64()
            .unwrap()
            > 0
    );
    for (name, case) in manifest["files"].as_object().unwrap() {
        let before = std::fs::read(fixture(name)).unwrap();
        let expected = std::fs::read(fixture(&(name.clone() + ".raw"))).unwrap();
        let mut r = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut b = FrameBuffer::default();
        for _ in 0..2 {
            r.read_frame_into(0, &mut b)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(pixels(b.pixels().unwrap()), expected, "{name}");
        }
        let metadata = r.metadata(0).unwrap();
        assert_eq!(metadata.compression, 5);
        assert_eq!(serde_json::json!(metadata.sample_shape()), case["shape"]);
        if name.starts_with("rgb-") {
            assert_eq!(metadata.orientation, 6);
            assert_eq!(metadata.shape, [7, 19]);
        }
        if name == "packed2.tif" {
            assert_eq!(metadata.bits_per_sample, 2);
        }
        if case.get("pillow_error").is_none() && metadata.orientation == 1 {
            let image = view::convert(metadata, &b, Policy::Pillow13, 100000).unwrap();
            let bytes = match &image.pixels {
                Pixels::Scalar(v) => pixels(v),
                Pixels::Bool(v) => v.iter().map(|&v| u8::from(v)).collect(),
            };
            assert_eq!(
                bytes,
                std::fs::read(fixture(&(name.clone() + ".pillow"))).unwrap(),
                "{name}"
            );
        }
        assert_eq!(std::fs::read(fixture(name)).unwrap(), before);
    }
    assert!(decode(&std::fs::read(fixture("invalid-lsb-early.tif")).unwrap()).is_err());
}
// Literal-only streams and explicit clears stay nine-bit throughout; no test
// dictionary decoder/width tracker mirrors the production framing adapter.
fn codes9(codes: &[u16]) -> Vec<u8> {
    let (mut pending, mut count) = (0u32, 0usize);
    let mut bytes = Vec::new();
    for &code in codes {
        pending |= u32::from(code) << count;
        count += 9;
        while count >= 8 {
            bytes.push(pending as u8);
            pending >>= 8;
            count -= 8;
        }
    }
    if count > 0 {
        bytes.push(pending as u8);
    }
    bytes
}
fn literals(bytes: &[u8]) -> Vec<u8> {
    let mut codes = Vec::new();
    for part in bytes.chunks(100) {
        codes.push(256);
        codes.extend(part.iter().map(|&v| u16::from(v)));
    }
    codes.push(257);
    codes9(&codes)
}
fn image(payload: &[u8], width: u32) -> Vec<u8> {
    let mut source = std::fs::read(fixture("kwkwk.tif")).unwrap();
    let start = value(&source, 273) as usize;
    source.truncate(start);
    set(&mut source, 256, width);
    set(&mut source, 257, 1);
    set(&mut source, 278, 1);
    set(&mut source, 279, payload.len() as u32);
    source.extend_from_slice(payload);
    source
}
#[test]
fn legacy_end_output_and_tail_integrity_are_strict() {
    let good = codes9(&[256, 65, 66, 257]);
    let mut white = image(&good, 2);
    set(&mut white, 262, 0);
    assert_eq!(decode(&white).unwrap(), PixelBuffer::U8(vec![65, 66]));
    assert_eq!(
        decode(&image(&good, 2)).unwrap(),
        PixelBuffer::U8(vec![65, 66])
    );
    assert_eq!(
        decode(&image(&codes9(&[256, 65, 258, 257]), 3)).unwrap(),
        PixelBuffer::U8(vec![65; 3])
    );
    for codes in [
        vec![256, 65, 400, 257],
        vec![256, 257],
        vec![256, 65, 66],
        vec![256, 65, 257, 66],
        vec![256, 65, 257, 257],
    ] {
        assert!(decode(&image(&codes9(&codes), 2)).is_err(), "{codes:?}");
    }
    assert!(
        decode(&image(&good, 1))
            .unwrap_err()
            .contains("beyond declared")
    );
    assert!(
        decode(&image(&good, 3))
            .unwrap_err()
            .contains("byte count mismatch")
    );
    for prefix in 1..good.len() {
        assert!(decode(&image(&good[..prefix], 2)).is_err());
    }
    let mut extra = good.clone();
    extra.push(0);
    assert!(
        decode(&image(&extra, 2))
            .unwrap_err()
            .contains("after end code")
    );
    let mut extra = good.clone();
    *extra.last_mut().unwrap() |= 0x80;
    assert!(
        decode(&image(&extra, 2))
            .unwrap_err()
            .contains("after end code")
    );
}
#[test]
fn declared_windows_and_all_tile_padding_are_validated() {
    let good = codes9(&[256, 65, 66, 257]);
    let mut source = image(&good, 2);
    set(&mut source, 279, good.len() as u32 - 1);
    // Missing EOI bytes remain physically present immediately past the window.
    assert!(decode(&source).is_err());
    let mut source = image(&good, 2);
    let offset = value(&source, 273);
    set(&mut source, 273, offset + 1);
    assert!(decode(&source).is_err());
    for count in [48usize, 144, 769] {
        let mut source = std::fs::read(fixture("rgb8-edge-tiles.tif")).unwrap();
        let last = set_last_count(&mut source, 324, 0);
        let compressed = literals(&vec![205; count]);
        set_last_count(&mut source, 325, compressed.len() as u32);
        source.truncate(last);
        source.extend_from_slice(&compressed);
        assert!(
            decode(&source).is_err(),
            "short/long source tile bytes={count}"
        );
    }
    // A codec view must not let an aliased source range expose adapted metadata.
    let mut source = image(&good, 2);
    set(&mut source, 262, 0);
    let photo = tag(&source, 262) + 8;
    set(&mut source, 273, photo as u32 - 1);
    assert!(decode(&source).unwrap_err().contains("overlaps adapted"));
}
#[test]
fn legacy_scratch_limits_source_identity_and_failed_buffers_are_explicit() {
    let source = std::fs::read(fixture("kwkwk.tif")).unwrap();
    let f = file(&source);
    let limits = Limits {
        intermediate_bytes: value(&source, 279) as usize,
        ..Limits::default()
    };
    let mut r = Reader::open(f.path(), limits).unwrap();
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
    r.read_frame_into(0, &mut b).unwrap();
    assert!(b.pixels().is_some());
    std::fs::write(f.path(), [0; 8]).unwrap();
    assert!(r.read_frame_into(0, &mut b).is_err());
    assert!(b.pixels().is_none());
    let f = file(&source);
    let mut r = Reader::open(f.path(), Limits::default()).unwrap();
    let replacement = file(&source);
    let replacement = replacement.into_temp_path();
    std::fs::rename(&replacement, f.path()).unwrap();
    assert!(r.read_frame_into(0, &mut b).is_err());
    assert!(b.pixels().is_none());
}
