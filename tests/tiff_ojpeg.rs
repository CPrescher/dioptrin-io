#![cfg(all(feature = "tiff", feature = "jpeg"))]
use dioptrin_io::tiff::{
    FrameBuffer, Limits, Reader, SampleLayout,
    ojpeg::Reconstruction,
    ycbcr::{self, Storage},
};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/ojpeg")
        .join(name)
}
fn temporary(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file
}
fn entry(bytes: &[u8], tag: u16) -> usize {
    let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap());
    (0..usize::from(count))
        .map(|i| ifd + 2 + 12 * i)
        .find(|&p| bytes[p..p + 2] == tag.to_le_bytes())
        .unwrap()
}
fn scalar(bytes: &[u8], tag: u16) -> u32 {
    let p = entry(bytes, tag) + 8;
    u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap())
}
fn set(bytes: &mut [u8], tag: u16, value: u32) {
    let p = entry(bytes, tag) + 8;
    bytes[p..p + 4].copy_from_slice(&value.to_le_bytes());
}
fn decode(bytes: &[u8]) -> Result<(), String> {
    let file = temporary(bytes);
    let mut reader = Reader::open(file.path(), Limits::default()).map_err(|e| e.to_string())?;
    reader
        .read_ycbcr_into(0, &mut ycbcr::Buffer::default())
        .map_err(|e| e.to_string())
}
#[test]
fn independent_original_components_padding_and_legacy_headers_match_native_libtiff() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let original = std::fs::read(fixture(name)).unwrap();
        let mut reader = Reader::open(fixture(name), Limits::default()).unwrap();
        let meta = reader.metadata(0).unwrap();
        assert!(matches!(
            meta.sample_layout,
            SampleLayout::YCbCrComponents { .. }
        ));
        assert!(meta.uniform_sample_shape().is_none());
        assert_eq!(meta.compression, 6);
        assert_eq!(meta.orientation, 6);
        let info = reader.ojpeg_metadata(0).unwrap();
        assert_eq!(serde_json::json!(info.encoded_shape), case["encoded_shape"]);
        assert_eq!(
            info.baseline_sos_normalized,
            case["baseline_sos_normalized"]
        );
        if info.baseline_sos_normalized {
            assert_eq!(
                &info.original_jpeg_header[info.original_jpeg_header.len() - 3..],
                &[0, 0, 0]
            );
        }
        for window in &info.source_windows {
            assert!(window.offset + window.bytes as u64 <= original.len() as u64);
        }
        let mut scalar = FrameBuffer::default();
        assert!(reader.read_frame_into(0, &mut scalar).is_err());
        assert!(scalar.pixels().is_none());
        let mut buffer = ycbcr::Buffer::default();
        for _ in 0..2 {
            reader.read_ycbcr_into(0, &mut buffer).unwrap();
            assert_eq!(buffer.storage(), Some(Storage::JpegDecodedComponents));
            assert_eq!(
                serde_json::json!(buffer.subsampling().unwrap()),
                case["subsampling"]
            );
            let chunks = buffer.chunks().unwrap();
            assert_eq!(chunks.len(), 1);
            assert!(chunks[0].sampling_units.is_none());
            for (plane, expected) in chunks[0]
                .planes
                .iter()
                .zip(case["planes"].as_array().unwrap())
            {
                assert_eq!(serde_json::json!(plane.shape), expected["shape"]);
                assert_eq!(
                    serde_json::json!(plane.valid_shape),
                    expected["valid_shape"]
                );
                let label = expected["component"].as_str().unwrap();
                let stem = name.strip_suffix(".tif").unwrap();
                assert_eq!(
                    plane.samples,
                    std::fs::read(fixture(&format!("{stem}.{label}"))).unwrap(),
                    "{name} {label}"
                );
            }
            let sampling = buffer.subsampling().unwrap();
            assert_eq!(chunks[0].planes[1].sampling, [sampling[1], sampling[0]]);
        }
        assert_eq!(std::fs::read(fixture(name)).unwrap(), original);
    }
}
#[test]
fn malformed_tables_fragments_entropy_and_jpeg_tails_never_supply_partial_planes() {
    let original = std::fs::read(fixture("table-edge-tile.tif")).unwrap();
    let start = scalar(&original, 324) as usize;
    let length = scalar(&original, 325) as usize;
    let mut cases = Vec::new();
    let mut partial = original[..start + length / 2].to_vec();
    set(&mut partial, 325, (length / 2) as u32);
    cases.push(partial);
    let mut tail = original.clone();
    tail.extend([0x33; 32]);
    set(&mut tail, 325, (length + 32) as u32);
    cases.push(tail);
    let mut table = original.clone();
    let table_positions = scalar(&table, 520) as usize;
    let source_offset = u32::from_le_bytes(
        table[table_positions..table_positions + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    table[source_offset..source_offset + 16].fill(255);
    cases.push(table);
    let mut restart = original.clone();
    set(&mut restart, 512, 14);
    cases.push(restart);
    for (index, bytes) in cases.into_iter().enumerate() {
        assert!(decode(&bytes).is_err(), "case {index}");
    }
    let original = std::fs::read(fixture("interchange-standard.tif")).unwrap();
    let mut bytes = original.clone();
    let length = scalar(&bytes, 514);
    bytes.push(0);
    set(&mut bytes, 514, length + 1);
    assert!(decode(&bytes).is_err());
    let mut bytes = original.clone();
    set(&mut bytes, 256, 15);
    assert!(decode(&bytes).is_err());
    let mut bytes = original.clone();
    set(&mut bytes, 530, 0x00010002);
    assert!(decode(&bytes).is_err());
    let mut bytes = original.clone();
    let offset = scalar(&bytes, 513) as usize;
    let end = offset + scalar(&bytes, 514) as usize;
    let sos = (offset..end - 2)
        .find(|&p| bytes[p..p + 2] == [255, 218])
        .unwrap();
    let size = u16::from_be_bytes(bytes[sos + 2..sos + 4].try_into().unwrap()) as usize;
    bytes[sos + size] = 1;
    assert!(decode(&bytes).is_err());
}
#[test]
fn native_components_obey_resource_bounds_and_snapshot_failures_invalidate_reused_buffers() {
    let original = std::fs::read(fixture("table-edge-tile.tif")).unwrap();
    for limits in [
        Limits {
            frame_bytes: 400,
            ..Limits::default()
        },
        Limits {
            intermediate_bytes: 8,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 32,
            ..Limits::default()
        },
    ] {
        let source = temporary(&original);
        if let Ok(mut reader) = Reader::open(source.path(), limits) {
            let mut buffer = ycbcr::Buffer::default();
            assert!(reader.read_ycbcr_into(0, &mut buffer).is_err());
            assert!(buffer.chunks().is_none());
        }
    }
    // A tiny logical crop must not admit a detector-sized padded tile under
    // only the logical-raster byte budget.
    let mut crop = original.clone();
    set(&mut crop, 256, 2);
    set(&mut crop, 257, 2);
    let crop_file = temporary(&crop);
    if let Ok(mut crop_reader) = Reader::open(
        crop_file.path(),
        Limits {
            frame_bytes: 12,
            ..Limits::default()
        },
    ) {
        let mut cropped_output = ycbcr::Buffer::default();
        assert!(crop_reader.read_ycbcr_into(0, &mut cropped_output).is_err());
        assert!(cropped_output.chunks().is_none());
    }
    let mut file = temporary(&original);
    let mut reader = Reader::open(file.path(), Limits::default()).unwrap();
    let mut buffer = ycbcr::Buffer::default();
    reader.read_ycbcr_into(0, &mut buffer).unwrap();
    file.write_all(&[0]).unwrap();
    assert!(reader.read_ycbcr_into(0, &mut buffer).is_err());
    assert!(buffer.shape().is_none());
    assert!(buffer.chunks().is_none());
}
#[test]
fn actual_upstream_ojpeg_component_hashes_match_separate_libtiff_codec() {
    use sha2::{Digest, Sha256};
    for (name, kind, shape, hashes) in [
        (
            "zackthecat.tif",
            Reconstruction::TableDefinedSingleChunk,
            [213, 234],
            [
                "fc33ecfd3c999a9a22685b48683d45f4cd0909dfc513208c5901d096b7e59dbf",
                "7338a75a3cfa9ce129b2383aaba4ed3d80b3b7392143eba54631bcc976d9ee82",
                "fbea610b3308c04abae33a121175973932c2ac81b6eac120d2c578319b577197",
            ],
        ),
        (
            "smallliz.tif",
            Reconstruction::CompleteInterchange,
            [160, 160],
            [
                "14e90098dd4a4cb9ffafff7aa41deab8c88523f47f3f6534d6e34cce8b8a3416",
                "01c2b0e1b9c9043d5e67c88c9ecf49281577002fcf0e9559c73f97c5b4bacedf",
                "117c8677dc3a281e2d2517df5c03966b0792fa128323e5f65fa4677b5b40355f",
            ],
        ),
    ] {
        let path = Path::new("/tmp/dioptrin-io-fabio-corpus/libtiffpic.tar.gz__content").join(name);
        if !path.exists() {
            continue;
        }
        let mut reader = Reader::open(path, Limits::default()).unwrap();
        assert_eq!(reader.ojpeg_metadata(0).unwrap().reconstruction, kind);
        let mut buffer = ycbcr::Buffer::default();
        reader.read_ycbcr_into(0, &mut buffer).unwrap();
        assert_eq!(buffer.shape(), Some(shape));
        for (plane, expected) in buffer.chunks().unwrap()[0].planes.iter().zip(hashes) {
            assert_eq!(
                format!("{:x}", Sha256::digest(&plane.samples)),
                expected,
                "{name}"
            );
        }
    }
}
