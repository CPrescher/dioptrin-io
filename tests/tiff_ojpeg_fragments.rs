#![cfg(all(feature = "tiff", feature = "jpeg"))]
use dioptrin_io::tiff::{Limits, Reader, ojpeg::Reconstruction, ycbcr};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/ojpeg-fragments")
        .join(name)
}
fn temporary(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file
}
fn entry(bytes: &[u8], tag: u16) -> usize {
    let ifd = get(bytes, 4) as usize;
    let count = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
    (0..count)
        .map(|i| ifd + 2 + 12 * i)
        .find(|&p| bytes[p..p + 2] == tag.to_le_bytes())
        .unwrap()
}
fn get(bytes: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap())
}
fn put(bytes: &mut [u8], p: usize, value: u32) {
    bytes[p..p + 4].copy_from_slice(&value.to_le_bytes());
}
fn scalar(bytes: &[u8], tag: u16) -> u32 {
    get(bytes, entry(bytes, tag) + 8)
}
fn decode(bytes: &[u8]) -> Result<(), String> {
    let file = temporary(bytes);
    let mut reader = Reader::open(file.path(), Limits::default()).map_err(|e| e.to_string())?;
    reader
        .read_ycbcr_into(0, &mut ycbcr::Buffer::default())
        .map_err(|e| e.to_string())
}
#[test]
fn fragmented_source_planes_restart_geometry_and_original_windows_match_native_libtiff() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let original = std::fs::read(fixture(name)).unwrap();
        let mut reader = Reader::open(fixture(name), Limits::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(reader.metadata(0).unwrap().orientation, 6);
        assert_eq!(reader.metadata(0).unwrap().compression, 6);
        let info = reader.ojpeg_metadata(0).unwrap();
        let count = case["fragments"].as_u64().unwrap() as usize;
        assert_eq!(info.fragments.len(), count);
        assert_eq!(
            u64::from(info.restart_interval),
            case["restart_interval"].as_u64().unwrap()
        );
        let expected = match case["mode"].as_str().unwrap() {
            "complete" => Reconstruction::CompleteInterchange,
            "table" => Reconstruction::TableDefinedFragments,
            _ => Reconstruction::FragmentedInterchange,
        };
        assert_eq!(info.reconstruction, expected);
        assert_eq!(
            info.restart_markers_inserted,
            if expected == Reconstruction::CompleteInterchange {
                0
            } else {
                count - 1
            }
        );
        assert_eq!(
            info.baseline_sos_normalized,
            name == "legacy-prefix-strips.tif"
        );
        for fragment in &info.fragments {
            assert!(fragment.source_offset + fragment.source_bytes as u64 <= original.len() as u64);
            assert!(fragment.entropy_offset >= fragment.source_offset);
            assert!(
                fragment.entropy_offset + fragment.entropy_bytes as u64
                    <= fragment.source_offset + fragment.source_bytes as u64
            );
        }
        let mut output = ycbcr::Buffer::default();
        for _ in 0..2 {
            reader
                .read_ycbcr_into(0, &mut output)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(serde_json::json!(output.shape().unwrap()), case["shape"]);
            let chunks = output.chunks().unwrap();
            assert_eq!(chunks.len(), count);
            for (index, chunk) in chunks.iter().enumerate() {
                let wanted = &case["reference"]["chunks"][index];
                assert_eq!(serde_json::json!(chunk.origin), wanted["origin"]);
                assert_eq!(serde_json::json!(chunk.image_shape), wanted["image_shape"]);
                for (plane, wanted) in chunk
                    .planes
                    .iter()
                    .zip(wanted["planes"].as_array().unwrap())
                {
                    assert_eq!(serde_json::json!(plane.shape), wanted["shape"], "{name}");
                    assert_eq!(serde_json::json!(plane.valid_shape), wanted["valid_shape"]);
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
        assert_eq!(std::fs::read(fixture(name)).unwrap(), original);
    }
}
#[test]
fn complete_interchange_cannot_hide_wrong_declared_fragment_contributions() {
    let original = std::fs::read(fixture("complete-interchange-strips.tif")).unwrap();
    let offsets = scalar(&original, 273) as usize;
    let lengths = scalar(&original, 279) as usize;
    for (p, value) in [
        (offsets, get(&original, offsets) + 1),
        (offsets, get(&original, offsets + 4)),
        (lengths, get(&original, lengths) - 1),
        (lengths, get(&original, lengths) + 1),
        (offsets + 4, get(&original, offsets + 4) + 1),
    ] {
        let mut bad = original.clone();
        put(&mut bad, p, value);
        assert!(decode(&bad).is_err(), "p={p} value={value}");
    }
    let mut bad = original.clone();
    let offset = get(&bad, offsets) as usize;
    let length = get(&bad, lengths) as usize;
    bad[offset + length + 1] = 0xd5;
    assert!(decode(&bad).is_err());
}
#[test]
fn fragmented_boundaries_markers_truncation_and_restart_declarations_are_strict() {
    let original = std::fs::read(fixture("table-strips.tif")).unwrap();
    let offsets = scalar(&original, 273) as usize;
    let lengths = scalar(&original, 279) as usize;
    let mut cases = Vec::new();
    for tag in [515, 278] {
        let mut bad = original.clone();
        let p = entry(&bad, tag) + 8;
        put(&mut bad, p, 1);
        cases.push(bad);
    }
    let mut bad = original.clone();
    let p = entry(&bad, 273) + 4;
    put(&mut bad, p, 2);
    cases.push(bad);
    let mut bad = original.clone();
    let first = get(&bad, offsets);
    put(&mut bad, offsets + 4, first);
    cases.push(bad);
    let mut bad = original.clone();
    put(&mut bad, offsets + 4, u32::MAX);
    cases.push(bad);
    for marker in [0xd0, 0xd9, 0xc0, 0xff] {
        let mut bad = original.clone();
        let start = get(&bad, offsets + 4) as usize;
        bad[start] = 255;
        bad[start + 1] = marker;
        cases.push(bad);
    }
    let mut bad = original.clone();
    let amount = get(&bad, lengths);
    put(&mut bad, lengths, amount / 2);
    cases.push(bad);
    let mut bad = original.clone();
    let start = get(&bad, offsets + 4) as usize;
    let length = get(&bad, lengths + 4) as usize;
    bad[start + length - 1] = 255;
    cases.push(bad);
    for (index, bad) in cases.iter().enumerate() {
        assert!(decode(bad).is_err(), "case{index}");
    }
}
#[test]
fn fragmented_scratch_source_changes_and_failed_reads_invalidate_all_components() {
    let original = std::fs::read(fixture("split-header-strips.tif")).unwrap();
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
            metadata_bytes: 1000,
            ..Limits::default()
        },
    ] {
        let file = temporary(&original);
        if let Ok(mut reader) = Reader::open(file.path(), limits) {
            let mut output = ycbcr::Buffer::default();
            assert!(reader.read_ycbcr_into(0, &mut output).is_err());
            assert!(output.chunks().is_none());
        }
    }
    let file = temporary(&original);
    let mut reader = Reader::open(file.path(), Limits::default()).unwrap();
    let mut output = ycbcr::Buffer::default();
    reader.read_ycbcr_into(0, &mut output).unwrap();
    std::fs::write(file.path(), [0; 8]).unwrap();
    assert!(reader.read_ycbcr_into(0, &mut output).is_err());
    assert!(output.chunks().is_none());
    assert!(output.shape().is_none());
    assert!(output.storage().is_none());
}
#[test]
fn tile_grid_requires_source_sof_and_keeps_stream_and_spatial_origins_separate() {
    let original = std::fs::read(fixture("prefix-grid-tiles.tif")).unwrap();
    let file = temporary(&original);
    let reader = Reader::open(file.path(), Limits::default()).unwrap();
    let info = reader.ojpeg_metadata(0).unwrap();
    assert_eq!(info.encoded_shape, [64, 32]);
    assert_eq!(info.fragments[1].origin, [0, 32]);
    assert_eq!(info.fragments[1].stream_origin, [16, 0]);
    assert_eq!(info.fragments[2].origin, [16, 0]);
    assert_eq!(info.fragments[2].stream_origin, [32, 0]);
    assert_eq!(info.fragments[3].image_shape, [13, 19]);
    // Table-only metadata does not prove how a multi-across tile raster is
    // stacked into a single obsolete JPEG frame. Never invent that SOF.
    let mut no_header = original.clone();
    for tag in [513, 514] {
        let p = entry(&no_header, tag);
        no_header[p..p + 2].copy_from_slice(&(tag + 1000).to_le_bytes());
    }
    assert!(
        decode(&no_header)
            .unwrap_err()
            .contains("explicit source SOF")
    );
    let prefix = scalar(&original, 513) as usize;
    let length = scalar(&original, 514) as usize;
    let sof = (prefix..prefix + length - 4)
        .find(|&p| original[p..p + 2] == [255, 192])
        .unwrap();
    let mut wrong = original.clone();
    wrong[sof + 5..sof + 7].copy_from_slice(&32u16.to_be_bytes());
    assert!(decode(&wrong).is_err());
    let dri = (prefix..prefix + length - 4)
        .find(|&p| original[p..p + 2] == [255, 221])
        .unwrap();
    let mut wrong = original.clone();
    wrong[dri + 4..dri + 6].copy_from_slice(&1u16.to_be_bytes());
    assert!(decode(&wrong).is_err());
}
