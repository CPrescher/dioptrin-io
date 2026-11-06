#![cfg(feature = "hdf5")]
use dioptrin_io::{
    PixelBuffer,
    hdf5::typed::{
        ExternalLinks, FilenameBinding, FrameBuffer, Limits, Options, Reader, Selection,
        UnmappedRegions, VdsOptions,
    },
};
use std::path::{Path, PathBuf};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/vds-irregular")
        .join(name)
}
fn manifest() -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap()
}
fn hex(pixels: &PixelBuffer) -> String {
    let mut bytes = Vec::new();
    match pixels {
        PixelBuffer::Bool(v) => bytes.extend_from_slice(v),
        PixelBuffer::F16(v) => {
            for x in v {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
        }
        PixelBuffer::U64(v) => {
            for x in v {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
        }
        PixelBuffer::I16(v) => {
            for x in v {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
        }
        PixelBuffer::F32(v) => {
            for x in v {
                bytes.extend_from_slice(&x.to_bits().to_le_bytes());
            }
        }
        PixelBuffer::F64(v) => {
            for x in v {
                bytes.extend_from_slice(&x.to_bits().to_le_bytes());
            }
        }
        PixelBuffer::C32(v) => {
            for x in v.iter().flatten() {
                bytes.extend_from_slice(&x.to_bits().to_le_bytes());
            }
        }
        PixelBuffer::C64(v) => {
            for x in v.iter().flatten() {
                bytes.extend_from_slice(&x.to_bits().to_le_bytes());
            }
        }
        _ => panic!("unexpected fixture dtype"),
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn open(
    path: &Path,
    maps: &[FilenameBinding],
    fill: UnmappedRegions,
    options: Options,
) -> Result<Reader, dioptrin_io::hdf5::Error> {
    Reader::open_snapshot_with_vds(
        path,
        Selection::Dataset("/data"),
        options,
        VdsOptions {
            printf_mappings: &[],
            filenames: maps,
            unmapped_regions: fill,
        },
    )
}
fn bindings(root: &Path, name: &str) -> Vec<FilenameBinding> {
    vec![FilenameBinding {
        stored_filename: name.into(),
        actual_file: root.join(name),
    }]
}
#[test]
fn differing_source_and_virtual_block_shapes_follow_native_hdf_iteration_order_for_every_bit() {
    let manifest = manifest();
    let root = fixture("source.h5").parent().unwrap().to_path_buf();
    let mut output = FrameBuffer::default();
    for (name, case) in manifest["cases"].as_object().unwrap() {
        let filename = case["source_file"].as_str().unwrap();
        let maps = bindings(&root, filename);
        let options = Options {
            external_links: ExternalLinks::FollowSnapshot,
            ..Default::default()
        };
        let reader = open(
            &root.join(case["master"].as_str().unwrap()),
            &maps,
            UnmappedRegions::DeclaredFill,
            options,
        )
        .unwrap_or_else(|e| panic!("{name}:{e}"));
        let expected = case["frame_hex"].as_array().unwrap();
        assert_eq!(reader.frames(), expected.len());
        let meta = reader
            .metadata(0)
            .unwrap()
            .virtual_dataset
            .as_ref()
            .unwrap();
        assert_eq!(
            serde_json::json!(meta.unmapped_pixels_by_frame),
            case["unmapped_by_frame"],
            "{name}"
        );
        let constant = meta
            .unmapped_pixels_by_frame
            .first()
            .copied()
            .filter(|n| meta.unmapped_pixels_by_frame.iter().all(|v| v == n));
        assert_eq!(meta.unmapped_pixels_per_frame, constant);
        for (index, words) in expected.iter().enumerate().rev() {
            reader.read_frame_into(index, &mut output).unwrap();
            assert_eq!(
                hex(output.pixels().unwrap()),
                words.as_str().unwrap(),
                "{name}:{index}"
            );
        }
        assert!(
            reader
                .read_frame_into(reader.frames(), &mut output)
                .is_err()
        );
        assert!(output.pixels().is_none());
    }
}
#[test]
fn complete_coverage_needs_no_fill_but_variable_gaps_require_explicit_declared_fill() {
    let root = fixture("source.h5").parent().unwrap().to_path_buf();
    let maps = bindings(&root, "source.h5");
    open(
        &fixture("strided.h5"),
        &maps,
        UnmappedRegions::Reject,
        Options::default(),
    )
    .unwrap();
    assert!(
        open(
            &fixture("cross-frame.h5"),
            &maps,
            UnmappedRegions::Reject,
            Options::default()
        )
        .is_err()
    );
    let reader = open(
        &fixture("cross-frame.h5"),
        &maps,
        UnmappedRegions::DeclaredFill,
        Options::default(),
    )
    .unwrap();
    let m = reader
        .metadata(0)
        .unwrap()
        .virtual_dataset
        .as_ref()
        .unwrap();
    assert_eq!(m.unmapped_pixels_per_frame, None);
    assert_eq!(m.unmapped_pixels_by_frame, [18, 15, 17, 20]);
    let mut b = FrameBuffer::default();
    reader.read_frame_into(3, &mut b).unwrap();
    assert_eq!(b.pixels(), Some(&PixelBuffer::U64(vec![u64::MAX; 20])));
}
#[test]
fn partial_wrong_missing_overlapping_sources_and_complexity_limits_do_not_supply_counts() {
    let root = fixture("source.h5").parent().unwrap().to_path_buf();
    let mut maps = bindings(&root, "source.h5");
    assert!(
        Reader::open_snapshot(
            fixture("u64.h5"),
            Selection::Dataset("/data"),
            Options::default()
        )
        .is_err()
    );
    assert!(
        open(
            &fixture("u64.h5"),
            &[],
            UnmappedRegions::DeclaredFill,
            Options::default()
        )
        .is_err()
    );
    for name in ["partial.h5", "short.h5", "absent.h5"] {
        maps[0].actual_file = fixture(name);
        assert!(
            open(
                &fixture("u64.h5"),
                &maps,
                UnmappedRegions::DeclaredFill,
                Options::default()
            )
            .is_err(),
            "{name}"
        );
    }
    let maps = bindings(&root, "source.h5");
    assert!(
        open(
            &fixture("overlap.h5"),
            &maps,
            UnmappedRegions::DeclaredFill,
            Options::default()
        )
        .is_err()
    );
    for limits in [
        Limits {
            frames: 2,
            ..Default::default()
        },
        Limits {
            frame_bytes: 239,
            ..Default::default()
        },
        Limits {
            group_members: 1,
            ..Default::default()
        },
        Limits {
            metadata_bytes: 512,
            ..Default::default()
        },
    ] {
        assert!(
            open(
                &fixture("u64.h5"),
                &maps,
                UnmappedRegions::DeclaredFill,
                Options {
                    limits,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
    let maps = bindings(&root, "large-source.h5");
    let error = match open(
        &fixture("block-limit.h5"),
        &maps,
        UnmappedRegions::DeclaredFill,
        Options::default(),
    ) {
        Ok(_) => panic!("unbounded blocks admitted"),
        Err(e) => e,
    };
    assert!(error.to_string().contains("block"));
}
fn copy_graph() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "source.h5",
        "u64.h5",
        "nested-path.h5",
        "cross-frame.h5",
        "relay.h5",
        "bound.h5",
        "external-graph.h5",
    ] {
        std::fs::copy(fixture(name), dir.path().join(name)).unwrap();
    }
    dir
}
#[test]
fn cached_source_and_master_objects_cannot_hide_unflushed_unlink_rebind_or_growth() {
    for master in [false, true] {
        let dir = copy_graph();
        let target = dir.path().join(if master { "u64.h5" } else { "source.h5" });
        let writer = hdf5_backend::File::open_rw(&target).unwrap();
        let reader = open(
            &dir.path().join("u64.h5"),
            &bindings(dir.path(), "source.h5"),
            UnmappedRegions::DeclaredFill,
            Options::default(),
        )
        .unwrap();
        let mut b = FrameBuffer::default();
        reader.read_frame_into(0, &mut b).unwrap();
        let name = if master { "data" } else { "u64" };
        let cached = writer.dataset(name).unwrap();
        writer.unlink(name).unwrap();
        writer
            .new_dataset::<f64>()
            .shape(cached.shape())
            .create(name)
            .unwrap();
        assert!(reader.read_frame_into(0, &mut b).is_err());
        assert!(b.pixels().is_none());
    }
    let dir = copy_graph();
    let writer = hdf5_backend::File::open_rw(dir.path().join("cross-frame.h5")).unwrap();
    let reader = open(
        &dir.path().join("cross-frame.h5"),
        &bindings(dir.path(), "source.h5"),
        UnmappedRegions::DeclaredFill,
        Options::default(),
    )
    .unwrap();
    let mut b = FrameBuffer::default();
    reader.read_frame_into(0, &mut b).unwrap();
    writer.dataset("data").unwrap().resize((5, 4, 5)).unwrap();
    assert!(reader.read_frame_into(0, &mut b).is_err());
    assert!(b.pixels().is_none());
}
#[test]
fn unflushed_new_external_component_and_declared_link_retarget_are_rejected_before_counts() {
    let dir = copy_graph();
    let writer = hdf5_backend::File::open_rw(dir.path().join("source.h5")).unwrap();
    let reader = open(
        &dir.path().join("u64.h5"),
        &bindings(dir.path(), "source.h5"),
        UnmappedRegions::DeclaredFill,
        Options::default(),
    )
    .unwrap();
    let mut b = FrameBuffer::default();
    reader.read_frame_into(0, &mut b).unwrap();
    writer.unlink("u64").unwrap();
    writer
        .link_external("must-not-open.h5", "/u64", "u64")
        .unwrap();
    let error = reader.read_frame_into(0, &mut b).unwrap_err();
    assert!(error.to_string().contains("non-hard"), "{error}");
    assert!(b.pixels().is_none());
    let dir = copy_graph();
    let writer = hdf5_backend::File::open_rw(dir.path().join("source.h5")).unwrap();
    let reader = open(
        &dir.path().join("nested-path.h5"),
        &bindings(dir.path(), "source.h5"),
        UnmappedRegions::DeclaredFill,
        Options::default(),
    )
    .unwrap();
    reader.read_frame_into(0, &mut b).unwrap();
    writer.unlink("deep/actual").unwrap();
    writer
        .link_external("must-not-open.h5", "/actual", "deep/actual")
        .unwrap();
    let error = reader.read_frame_into(0, &mut b).unwrap_err();
    assert!(error.to_string().contains("non-hard"), "{error}");
    assert!(b.pixels().is_none());
    let dir = copy_graph();
    let writer = hdf5_backend::File::open_rw(dir.path().join("relay.h5")).unwrap();
    let reader = open(
        &dir.path().join("external-graph.h5"),
        &bindings(dir.path(), "bound.h5"),
        UnmappedRegions::DeclaredFill,
        Options {
            external_links: ExternalLinks::FollowSnapshot,
            ..Default::default()
        },
    )
    .unwrap();
    reader.read_frame_into(0, &mut b).unwrap();
    writer.unlink("data").unwrap();
    writer.link_external("source.h5", "/i16", "data").unwrap();
    assert!(reader.read_frame_into(0, &mut b).is_err());
    assert!(b.pixels().is_none());
}
