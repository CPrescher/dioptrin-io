#![cfg(feature = "hdf5")]
use dioptrin_io::{
    PixelBuffer, PixelType,
    hdf5::typed::{
        ExternalLinks, FilenameBinding, FrameBuffer, Limits, Options, Reader, Selection,
        UnmappedRegions, VdsOptions,
    },
};
use std::path::{Path, PathBuf};
const A: &str = "/missing/acquisition/module-a.h5";
const B: &str = "/missing/acquisition/module-b.h5";
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/vds")
        .join(name)
}
fn bindings(a: PathBuf, b: PathBuf) -> Vec<FilenameBinding> {
    vec![
        FilenameBinding {
            stored_filename: A.into(),
            actual_file: a,
        },
        FilenameBinding {
            stored_filename: B.into(),
            actual_file: b,
        },
    ]
}
fn standard() -> Vec<FilenameBinding> {
    bindings(fixture("module-a.h5"), fixture("module-b.h5"))
}
fn open(
    path: impl AsRef<Path>,
    maps: &[FilenameBinding],
    fill: UnmappedRegions,
    limits: Limits,
) -> Result<Reader, dioptrin_io::hdf5::Error> {
    Reader::open_snapshot_with_vds(
        path,
        Selection::Dataset("/data"),
        Options {
            limits,
            external_links: ExternalLinks::FollowSnapshot,
        },
        VdsOptions {
            filenames: maps,
            unmapped_regions: fill,
        },
    )
}
fn copied() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for file in std::fs::read_dir(fixture("")).unwrap() {
        let file = file.unwrap();
        std::fs::copy(file.path(), dir.path().join(file.file_name())).unwrap();
    }
    dir
}
fn expected_i16() -> Vec<Vec<i16>> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    manifest["expected_i16"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| {
            frame
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|row| row.as_array().unwrap())
                .map(|v| v.as_i64().unwrap() as i16)
                .collect()
        })
        .collect()
}
#[test]
fn independent_finite_and_unlimited_mosaics_preserve_counts_and_declared_fill_provenance() {
    let maps = standard();
    for name in ["finite.h5", "unlimited.h5"] {
        assert!(
            Reader::open_snapshot(
                fixture(name),
                Selection::Dataset("/data"),
                Options::default()
            )
            .is_err()
        );
        assert!(
            open(
                fixture(name),
                &maps,
                UnmappedRegions::Reject,
                Limits::default()
            )
            .is_err()
        );
        let reader = open(
            fixture(name),
            &maps,
            UnmappedRegions::DeclaredFill,
            Limits::default(),
        )
        .unwrap();
        assert_eq!(reader.frames(), 2);
        assert_eq!(reader.sources().len(), 3);
        let meta = reader.metadata(0).unwrap();
        assert_eq!(meta.pixel_type, PixelType::I16);
        assert_eq!(meta.fill_value, Some(PixelBuffer::I16(vec![-32768])));
        assert!(!meta.storage_allocated);
        let mapping = meta.virtual_dataset.as_ref().unwrap();
        assert_eq!(mapping.frozen_shape, [2, 4, 6]);
        assert_eq!(mapping.unmapped_pixels_per_frame, 16);
        assert_eq!(mapping.mappings.len(), 2);
        assert!(mapping.mappings.iter().all(|m| m.storage_allocated));
        let mut buffer = FrameBuffer::default();
        for _ in 0..2 {
            for (index, values) in expected_i16().into_iter().enumerate() {
                reader.read_frame_into(index, &mut buffer).unwrap();
                assert_eq!(buffer.shape(), Some([4, 6]));
                assert_eq!(
                    buffer.pixels(),
                    Some(&PixelBuffer::I16(values)),
                    "{name} frame{index}"
                );
            }
        }
        assert!(reader.read_frame_into(2, &mut buffer).is_err());
        assert!(buffer.pixels().is_none());
    }
}
#[test]
fn explicit_source_bindings_determine_frozen_frames_even_when_native_vds_extent_is_zero() {
    let dir = tempfile::tempdir().unwrap();
    let master = dir.path().join("master.h5");
    std::fs::copy(fixture("unlimited.h5"), &master).unwrap();
    let a = dir.path().join("renamed-a.h5");
    let b = dir.path().join("renamed-b.h5");
    std::fs::copy(fixture("module-a.h5"), &a).unwrap();
    std::fs::copy(fixture("module-b.h5"), &b).unwrap();
    // Original absolute names and their local basename candidates are absent.
    assert_eq!(
        hdf5_backend::File::open(&master)
            .unwrap()
            .dataset("/data")
            .unwrap()
            .shape()[0],
        0
    );
    let maps = bindings(a, b);
    let reader = open(
        master,
        &maps,
        UnmappedRegions::DeclaredFill,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(reader.frames(), 2);
    let mut frame = FrameBuffer::default();
    reader.read_frame_into(0, &mut frame).unwrap();
    assert_eq!(
        frame.pixels(),
        Some(&PixelBuffer::I16(expected_i16().remove(0)))
    );
}
#[test]
fn uint64_source_and_unmapped_fill_never_pass_through_float_or_stale_buffer_values() {
    let maps = vec![FilenameBinding {
        stored_filename: A.into(),
        actual_file: fixture("module-u64.h5"),
    }];
    let reader = open(
        fixture("uint64-fill.h5"),
        &maps,
        UnmappedRegions::DeclaredFill,
        Limits::default(),
    )
    .unwrap();
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    let mut frame = FrameBuffer::default();
    for (index, value) in expected["expected_u64"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        reader.read_frame_into(index, &mut frame).unwrap();
        let values: Vec<_> = value
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|row| row.as_array().unwrap())
            .map(|v| v.as_u64().unwrap())
            .collect();
        assert_eq!(frame.pixels(), Some(&PixelBuffer::U64(values)));
    }
    assert_eq!(
        reader.metadata(0).unwrap().fill_value,
        Some(PixelBuffer::U64(vec![u64::MAX]))
    );
}
#[test]
fn default_policies_missing_partial_wrong_or_ambiguous_bindings_never_fill_modules() {
    let maps = standard();
    assert!(
        open(
            fixture("finite.h5"),
            &[],
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err()
    );
    assert!(
        open(
            fixture("finite.h5"),
            &maps[..1],
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err()
    );
    for name in [
        "wrong-dtype.h5",
        "short-shape.h5",
        "unallocated-a.h5",
        "partial-a.h5",
        "finite.h5",
    ] {
        let mut bad = maps.clone();
        bad[0].actual_file = fixture(name);
        assert!(
            open(
                fixture("finite.h5"),
                &bad,
                UnmappedRegions::DeclaredFill,
                Limits::default()
            )
            .is_err(),
            "{name}"
        );
    }
    let mut bad = maps.clone();
    bad[1].actual_file = fixture("wrong-frames.h5");
    assert!(
        open(
            fixture("finite.h5"),
            &bad,
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err()
    );
    let mut bad = maps.clone();
    bad[0].actual_file = fixture("missing-file.h5");
    assert!(
        open(
            fixture("finite.h5"),
            &bad,
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err()
    );
    let mut duplicates = maps.clone();
    duplicates.push(maps[0].clone());
    assert!(
        open(
            fixture("finite.h5"),
            &duplicates,
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err()
    );
    let mut extra = maps.clone();
    extra.push(FilenameBinding {
        stored_filename: "unused".into(),
        actual_file: fixture("module-a.h5"),
    });
    assert!(
        open(
            fixture("finite.h5"),
            &extra,
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err()
    );
}
#[test]
fn unsupported_mapping_fill_and_metadata_layouts_are_explicit() {
    let maps = standard();
    for name in [
        "overlap.h5",
        "shifted.h5",
        "never-fill.h5",
        "frame-master-mismatch.h5",
    ] {
        assert!(
            open(
                fixture(name),
                &maps,
                UnmappedRegions::DeclaredFill,
                Limits::default()
            )
            .is_err(),
            "{name}"
        );
    }
    let strided = vec![FilenameBinding {
        stored_filename: A.into(),
        actual_file: fixture("stride-a.h5"),
    }];
    assert!(
        open(
            fixture("strided.h5"),
            &strided,
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err()
    );
    assert!(
        Reader::open_snapshot_with_vds(
            fixture("metadata-vds.h5"),
            Selection::Lambda,
            Options::default(),
            VdsOptions {
                filenames: &maps,
                unmapped_regions: UnmappedRegions::DeclaredFill
            }
        )
        .is_err()
    );
    let extra = open(
        fixture("spatial-extra-fill.h5"),
        &maps,
        UnmappedRegions::DeclaredFill,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(extra.metadata(0).unwrap().shape, [5, 7]);
    assert_eq!(
        extra
            .metadata(0)
            .unwrap()
            .virtual_dataset
            .as_ref()
            .unwrap()
            .unmapped_pixels_per_frame,
        27
    );
    let full = vec![maps[0].clone()];
    let reader = open(
        fixture("full.h5"),
        &full,
        UnmappedRegions::Reject,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        reader
            .metadata(0)
            .unwrap()
            .virtual_dataset
            .as_ref()
            .unwrap()
            .unmapped_pixels_per_frame,
        0
    );
}
#[test]
fn regular_datasets_ignore_irrelevant_global_bindings_including_their_source_limit() {
    let maps = standard();
    let reader = Reader::open_snapshot_with_vds(
        fixture("module-a.h5"),
        Selection::Dataset("/data"),
        Options {
            limits: Limits {
                source_files: 1,
                ..Limits::default()
            },
            ..Options::default()
        },
        VdsOptions {
            filenames: &maps,
            unmapped_regions: UnmappedRegions::DeclaredFill,
        },
    )
    .unwrap();
    assert!(reader.metadata(0).unwrap().virtual_dataset.is_none());
    assert_eq!(reader.sources().len(), 1);
}
#[test]
fn complete_external_graph_and_source_replacements_invalidate_every_reused_frame() {
    let dir = copied();
    let maps = bindings(
        dir.path().join("link-bound.h5"),
        dir.path().join("module-b.h5"),
    );
    assert!(
        Reader::open_snapshot_with_vds(
            dir.path().join("finite.h5"),
            Selection::Dataset("/data"),
            Options::default(),
            VdsOptions {
                filenames: &maps,
                unmapped_regions: UnmappedRegions::DeclaredFill
            }
        )
        .is_err()
    );
    let reader = open(
        dir.path().join("finite.h5"),
        &maps,
        UnmappedRegions::DeclaredFill,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(reader.sources().len(), 5);
    assert_eq!(reader.links().len(), 2);
    let mut frame = FrameBuffer::default();
    reader.read_frame_into(0, &mut frame).unwrap();
    let relay = dir.path().join("link-relay.h5");
    let mut bytes = std::fs::read(&relay).unwrap();
    bytes.push(0);
    std::fs::write(relay, bytes).unwrap();
    assert!(reader.read_frame_into(0, &mut frame).is_err());
    assert!(frame.pixels().is_none());
    let dir = copied();
    let maps = bindings(
        dir.path().join("module-a.h5"),
        dir.path().join("module-b.h5"),
    );
    let reader = open(
        dir.path().join("finite.h5"),
        &maps,
        UnmappedRegions::DeclaredFill,
        Limits::default(),
    )
    .unwrap();
    reader.read_frame_into(0, &mut frame).unwrap();
    std::fs::copy(fixture("module-b.h5"), dir.path().join("replacement.h5")).unwrap();
    std::fs::rename(
        dir.path().join("replacement.h5"),
        dir.path().join("module-b.h5"),
    )
    .unwrap();
    assert!(reader.read_frame_into(0, &mut frame).is_err());
    assert!(frame.pixels().is_none());
}
#[test]
fn growing_concrete_source_invalidates_frozen_extent_before_any_counts_are_exposed() {
    let dir = copied();
    let path = dir.path().join("growing-a.h5");
    let writer = hdf5_backend::File::open_rw(&path).unwrap();
    let dataset = writer.dataset("data").unwrap();
    let maps = bindings(path, dir.path().join("module-b.h5"));
    let reader = open(
        dir.path().join("unlimited.h5"),
        &maps,
        UnmappedRegions::DeclaredFill,
        Limits::default(),
    )
    .unwrap();
    let mut frame = FrameBuffer::default();
    reader.read_frame_into(1, &mut frame).unwrap();
    dataset.resize((3, 2, 3)).unwrap();
    assert!(reader.read_frame_into(0, &mut frame).is_err());
    assert!(frame.pixels().is_none());
}
#[test]
fn frames_bytes_chunks_sources_datasets_text_and_metadata_have_independent_bounds() {
    let maps = standard();
    for limits in [
        Limits {
            frames: 1,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 47,
            ..Limits::default()
        },
        Limits {
            source_files: 2,
            ..Limits::default()
        },
        Limits {
            datasets: 2,
            ..Limits::default()
        },
        Limits {
            text_bytes: 4,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 512,
            ..Limits::default()
        },
    ] {
        assert!(
            open(
                fixture("finite.h5"),
                &maps,
                UnmappedRegions::DeclaredFill,
                limits
            )
            .is_err()
        );
    }
    let maps = bindings(fixture("growing-a.h5"), fixture("module-b.h5"));
    assert!(
        open(
            fixture("finite.h5"),
            &maps,
            UnmappedRegions::DeclaredFill,
            Limits {
                chunk_bytes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
}
#[test]
fn every_scalar_and_endian_preserves_nan_payloads_negative_zero_integer_extrema_and_fill() {
    fn bytes(pixels: &PixelBuffer) -> Vec<u8> {
        macro_rules! values{($($v:ident);*)=>{match pixels{$(PixelBuffer::$v(values)=>values.iter().flat_map(|v|v.to_le_bytes()).collect(),)*_=>unreachable!()}};}
        values!(U8;I8;U16;I16;U32;I32;U64;I64;F32;F64)
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    let maps = vec![FilenameBinding {
        stored_filename: A.into(),
        actual_file: fixture("scalars-source.h5"),
    }];
    for (name, case) in manifest["scalar_cases"].as_object().unwrap() {
        let reader = Reader::open_snapshot_with_vds(
            fixture("scalars-master.h5"),
            Selection::Dataset(&format!("/{name}")),
            Options::default(),
            VdsOptions {
                filenames: &maps,
                unmapped_regions: UnmappedRegions::DeclaredFill,
            },
        )
        .unwrap();
        let mut frame = FrameBuffer::default();
        let mut actual = Vec::new();
        for index in 0..2 {
            reader.read_frame_into(index, &mut frame).unwrap();
            actual.extend(bytes(frame.pixels().unwrap()));
        }
        assert_eq!(
            actual,
            std::fs::read(fixture(case["expected_raw"].as_str().unwrap())).unwrap(),
            "{name}"
        );
        let fill = bytes(reader.metadata(0).unwrap().fill_value.as_ref().unwrap());
        assert_eq!(
            fill.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            case["fill_hex"].as_str().unwrap(),
            "{name} originalfill"
        );
    }
}
#[test]
fn growth_in_another_virtual_or_regular_part_invalidates_the_global_scan_index() {
    for ordinary_part in [false, true] {
        let dir = copied();
        let master = dir.path().join("multi-vds.h5");
        let writable = hdf5_backend::File::open_rw(if ordinary_part {
            master.clone()
        } else {
            dir.path().join("growing-b.h5")
        })
        .unwrap();
        let growing = writable
            .dataset(if ordinary_part {
                "/entry/data/data_000003"
            } else {
                "/data"
            })
            .unwrap();
        let maps = bindings(
            dir.path().join("module-a.h5"),
            dir.path().join("growing-b.h5"),
        );
        let reader = Reader::open_snapshot_with_vds(
            master,
            Selection::Eiger,
            Options {
                external_links: ExternalLinks::FollowSnapshot,
                ..Options::default()
            },
            VdsOptions {
                filenames: &maps,
                unmapped_regions: UnmappedRegions::Reject,
            },
        )
        .unwrap();
        assert_eq!(reader.frames(), 6);
        let mut frame = FrameBuffer::default();
        reader.read_frame_into(0, &mut frame).unwrap();
        growing.resize((3, 1, 2)).unwrap();
        assert!(
            reader.read_frame_into(0, &mut frame).is_err(),
            "change in another part must invalidate this frame"
        );
        assert!(frame.pixels().is_none());
    }
}
