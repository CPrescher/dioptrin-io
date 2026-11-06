#![cfg(all(feature = "tiff", feature = "jpeg"))]
use dioptrin_io::tiff::{Limits, OjpegTiles, Options, Reader, ojpeg::Partition, ycbcr};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/ojpeg-advanced")
        .join(name)
}
fn file(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(bytes).unwrap();
    f
}
fn entry(bytes: &[u8], tag: u16) -> usize {
    let root = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(bytes[root..root + 2].try_into().unwrap()) as usize;
    (0..count)
        .map(|i| root + 2 + i * 12)
        .find(|&p| bytes[p..p + 2] == tag.to_le_bytes())
        .unwrap()
}
fn get(bytes: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap())
}
fn put(bytes: &mut [u8], p: usize, v: u32) {
    bytes[p..p + 4].copy_from_slice(&v.to_le_bytes());
}
fn value(bytes: &[u8], tag: u16) -> u32 {
    get(bytes, entry(bytes, tag) + 8)
}
fn options(policy: &str) -> Options {
    Options {
        ojpeg_tiles: if policy == "independent-reset-tiles" {
            OjpegTiles::IndependentResetTiles
        } else {
            OjpegTiles::Declared
        },
        ..Options::default()
    }
}
fn decode(bytes: &[u8], opts: Options) -> Result<(), String> {
    let file = file(bytes);
    let mut reader = Reader::open_with_options(file.path(), Limits::default(), opts)
        .map_err(|e| e.to_string())?;
    reader
        .read_ycbcr_into(0, &mut ycbcr::Buffer::default())
        .map_err(|e| e.to_string())
}
#[test]
fn independently_encoded_original_jpeg_source_words_match_every_component_and_padding() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let before = std::fs::read(fixture(name)).unwrap();
        let opts = options(case["policy"].as_str().unwrap());
        let mut reader = Reader::open_with_options(fixture(name), Limits::default(), opts)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let meta = reader.ojpeg_metadata(0).unwrap();
        assert_eq!(meta.tile_policy, opts.ojpeg_tiles);
        assert_eq!(reader.metadata(0).unwrap().orientation, 6);
        if name.starts_with("planar") {
            assert_eq!(meta.partition, Partition::PlanarScans);
            assert_eq!(meta.scan_sources.len(), 3);
            for scan in &meta.scan_sources {
                assert!(!scan.header_windows.is_empty());
                assert!(!scan.entropy_windows.is_empty());
            }
        }
        if name.starts_with("header-") {
            assert_eq!(meta.partition, Partition::GlobalStream);
            assert!(
                meta.fragments
                    .iter()
                    .any(|f| f.entropy_bytes == 0 && f.header_bytes == f.source_bytes)
            );
        }
        if name == "table-grid-explicit-reset.tif" {
            assert_eq!(meta.partition, Partition::IndependentChunks);
            assert!(Reader::open(fixture(name), Limits::default()).is_err());
        }
        let mut buffer = ycbcr::Buffer::default();
        for _ in 0..2 {
            reader
                .read_ycbcr_into(0, &mut buffer)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let expected = &case["source_word_reference"];
            assert_eq!(
                serde_json::json!(buffer.shape().unwrap()),
                expected["shape"]
            );
            let chunks = buffer.chunks().unwrap();
            assert_eq!(chunks.len(), expected["chunks"].as_array().unwrap().len());
            for (index, (chunk, wanted)) in chunks
                .iter()
                .zip(expected["chunks"].as_array().unwrap())
                .enumerate()
            {
                assert_eq!(serde_json::json!(chunk.origin), wanted["origin"]);
                assert_eq!(serde_json::json!(chunk.image_shape), wanted["image_shape"]);
                for (plane, wanted) in chunk
                    .planes
                    .iter()
                    .zip(wanted["planes"].as_array().unwrap())
                {
                    assert_eq!(serde_json::json!(plane.shape), wanted["shape"], "{name}");
                    assert_eq!(serde_json::json!(plane.valid_shape), wanted["valid_shape"]);
                    assert_eq!(serde_json::json!(plane.sampling), wanted["sampling"]);
                    let label = wanted["component"].as_str().unwrap();
                    let stem = name.strip_suffix(".tif").unwrap();
                    assert_eq!(
                        plane.samples,
                        std::fs::read(fixture(&format!("{stem}.chunk{index}.{label}"))).unwrap(),
                        "{name} chunk{index} {label}"
                    );
                }
            }
        }
        assert_eq!(std::fs::read(fixture(name)).unwrap(), before);
    }
}
#[test]
fn single_headers_and_internal_restarts_require_exact_source_fields() {
    let original = std::fs::read(fixture("single-prefix-restarts.tif")).unwrap();
    let source = value(&original, 513) as usize;
    let length = value(&original, 514) as usize;
    let dri = (source..source + length - 5)
        .find(|&p| original[p..p + 2] == [255, 221])
        .unwrap();
    for marker in [0xd1, 0xc0, 0x00] {
        let mut bad = original.clone();
        let start = value(&bad, 273) as usize;
        let n = value(&bad, 279) as usize;
        let rst = (start..start + n - 1)
            .find(|&p| bad[p..p + 2] == [255, 208])
            .unwrap();
        bad[rst + 1] = marker;
        assert!(decode(&bad, Options::default()).is_err());
    }
    let mut bad = original.clone();
    bad[dri + 4..dri + 6].copy_from_slice(&1u16.to_be_bytes());
    assert!(decode(&bad, Options::default()).is_err());
    let mut bad = original.clone();
    let p = entry(&bad, 279) + 8;
    put(&mut bad, p, 1);
    assert!(decode(&bad, Options::default()).is_err());
    let mut bad = original.clone();
    let p = entry(&bad, 513) + 8;
    let offset = value(&bad, 273);
    put(&mut bad, p, offset);
    assert!(decode(&bad, Options::default()).is_err());
}
#[test]
fn header_only_stream_windows_are_not_silently_substituted_or_reordered() {
    let original = std::fs::read(fixture("header-across-three.tif")).unwrap();
    let table = value(&original, 273) as usize;
    let lengths = value(&original, 279) as usize;
    for (p, v) in [
        (table, get(&original, table + 4)),
        (table + 4, get(&original, table)),
        (lengths, get(&original, lengths) - 1),
        (lengths + 4, get(&original, lengths + 4) + 1),
        (table + 8, u32::MAX),
    ] {
        let mut bad = original.clone();
        put(&mut bad, p, v);
        assert!(decode(&bad, Options::default()).is_err(), "p{p} v{v}");
    }
}
#[test]
fn planar_scans_reject_duplicate_ids_shifted_complete_windows_and_unsupported_restarts() {
    let original = std::fs::read(fixture("planar444-complete.tif")).unwrap();
    let table = value(&original, 273) as usize;
    let lengths = value(&original, 279) as usize;
    for (p, v) in [
        (table, get(&original, table) + 1),
        (lengths, get(&original, lengths) - 1),
        (table + 4, get(&original, table)),
    ] {
        let mut bad = original.clone();
        put(&mut bad, p, v);
        assert!(decode(&bad, Options::default()).is_err());
    }
    let mut bad = original.clone();
    let offset = get(&bad, table + 4) as usize;
    bad[offset - 5] = 1;
    assert!(decode(&bad, Options::default()).is_err());
    let mut bad = original.clone();
    let p = entry(&bad, 515) + 8;
    put(&mut bad, p, 1);
    assert!(decode(&bad, Options::default()).is_err());
}
#[test]
fn explicit_tile_policy_never_retries_source_markers_or_short_windows() {
    let original = std::fs::read(fixture("table-grid-explicit-reset.tif")).unwrap();
    let opts = options("independent-reset-tiles");
    let table = value(&original, 324) as usize;
    let lengths = value(&original, 325) as usize;
    for (p, v) in [
        (table + 4, get(&original, table)),
        (lengths + 8, 1),
        (table + 8, u32::MAX),
    ] {
        let mut bad = original.clone();
        put(&mut bad, p, v);
        assert!(decode(&bad, opts).is_err());
    }
    let mut bad = original.clone();
    let start = get(&bad, table) as usize;
    bad[start] = 255;
    bad[start + 1] = 216;
    assert!(decode(&bad, opts).is_err());
    assert!(decode(&std::fs::read(fixture("single-prefix.tif")).unwrap(), opts).is_err());
}
#[test]
fn advanced_frame_scratch_metadata_and_source_failures_clear_every_accessor() {
    for name in [
        "header-across-three.tif",
        "planar420-complete.tif",
        "table-grid-explicit-reset.tif",
    ] {
        let bytes = std::fs::read(fixture(name)).unwrap();
        let opts = if name.starts_with("table") {
            options("independent-reset-tiles")
        } else {
            Options::default()
        };
        for limits in [
            Limits {
                frame_bytes: 100,
                ..Limits::default()
            },
            Limits {
                intermediate_bytes: 1000,
                ..Limits::default()
            },
            Limits {
                metadata_bytes: 200,
                ..Limits::default()
            },
        ] {
            let f = file(&bytes);
            if let Ok(mut reader) = Reader::open_with_options(f.path(), limits, opts) {
                let mut buffer = ycbcr::Buffer::default();
                assert!(reader.read_ycbcr_into(0, &mut buffer).is_err());
                assert!(buffer.chunks().is_none());
            }
        }
        let f = file(&bytes);
        let mut reader = Reader::open_with_options(f.path(), Limits::default(), opts).unwrap();
        let mut buffer = ycbcr::Buffer::default();
        reader.read_ycbcr_into(0, &mut buffer).unwrap();
        std::fs::write(f.path(), [0; 8]).unwrap();
        assert!(reader.read_ycbcr_into(0, &mut buffer).is_err());
        assert!(buffer.shape().is_none());
        assert!(buffer.chunks().is_none());
        assert!(buffer.storage().is_none());
    }
}
#[test]
fn literal_restart_stream_requires_every_source_marker_and_global_sequence() {
    let original = std::fs::read(fixture("stored-restarts-prefix.tif")).unwrap();
    assert!(decode(&original, Options::default()).is_ok());
    let table = value(&original, 273) as usize;
    let start = get(&original, table) as usize;
    let lengths = value(&original, 279) as usize;
    let length = get(&original, lengths) as usize;
    let positions: Vec<_> = (start..start + length - 1)
        .filter(|&p| original[p] == 255 && (0xd0..=0xd7).contains(&original[p + 1]))
        .collect();
    assert!(!positions.is_empty());
    for marker in [0xd3, 0x00, 0xd9, 0xc0] {
        let mut bad = original.clone();
        bad[positions[0] + 1] = marker;
        assert!(decode(&bad, Options::default()).is_err());
    }
    let mut bad = original.clone();
    let offset = get(&bad, table + 4);
    put(&mut bad, table + 4, offset + 1);
    assert!(decode(&bad, Options::default()).is_err());
}
