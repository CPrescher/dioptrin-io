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
        .join("tests/data/vds-nested")
        .join(name)
}
fn manifest() -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap()
}
fn bytes(p: &PixelBuffer) -> Vec<u8> {
    match p {
        PixelBuffer::U64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        PixelBuffer::F16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        PixelBuffer::Bool(v) => v.clone(),
        PixelBuffer::C32(v) => v
            .iter()
            .flatten()
            .flat_map(|x| x.to_bits().to_le_bytes())
            .collect(),
        _ => panic!("unexpected fixture type"),
    }
}
fn hex(p: &PixelBuffer) -> String {
    bytes(p).iter().map(|b| format!("{b:02x}")).collect()
}
fn maps(case: &serde_json::Value, root: &Path) -> Vec<FilenameBinding> {
    case["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let name = v.as_str().unwrap();
            FilenameBinding {
                stored_filename: name.into(),
                actual_file: root.join(name),
            }
        })
        .collect()
}
fn open(
    root: &Path,
    case: &serde_json::Value,
    fill: UnmappedRegions,
    limits: Limits,
) -> Result<Reader, dioptrin_io::hdf5::Error> {
    Reader::open_snapshot_with_vds(
        root.join(case["master"].as_str().unwrap()),
        Selection::Dataset(case["dataset"].as_str().unwrap()),
        Options {
            limits,
            external_links: ExternalLinks::FollowSnapshot,
        },
        VdsOptions {
            filenames: &maps(case, root),
            unmapped_regions: fill,
            printf_mappings: &[],
        },
    )
}
fn copy_graph() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for item in std::fs::read_dir(fixture("")).unwrap() {
        let p = item.unwrap().path();
        std::fs::copy(&p, dir.path().join(p.file_name().unwrap())).unwrap();
    }
    dir
}
#[test]
fn three_levels_preserve_exact_measurement_and_distinct_fill_words() {
    let m = manifest();
    let mut b = FrameBuffer::default();
    for (name, case) in m["cases"].as_object().unwrap() {
        let fill = if case["unmapped_regions"] == "reject" {
            UnmappedRegions::Reject
        } else {
            UnmappedRegions::DeclaredFill
        };
        let reader = open(&fixture(""), case, fill, Limits::default())
            .unwrap_or_else(|e| panic!("{name}:{e}"));
        let frames = case["frame_hex"].as_array().unwrap();
        assert_eq!(reader.frames(), frames.len());
        for index in [frames.len() - 1, 0, frames.len() - 1] {
            reader.read_frame_into(index, &mut b).unwrap();
            assert_eq!(
                hex(b.pixels().unwrap()),
                frames[index].as_str().unwrap(),
                "{name}:{index}"
            );
        }
        let v = reader
            .metadata(0)
            .unwrap()
            .virtual_dataset
            .as_ref()
            .unwrap();
        if name.starts_with("grand-") {
            assert_eq!(v.nested_nodes.len(), 2);
            let mut origins = v
                .effective_fill_regions
                .iter()
                .map(|r| r.origin_node)
                .collect::<Vec<_>>();
            origins.sort();
            origins.dedup();
            assert_eq!(origins.len(), 3);
        }
        if let Some(count) = case["effective_fill_regions"].as_u64() {
            assert_eq!(v.effective_fill_regions.len(), count as usize);
        }
        if let Some(count) = case["nested_nodes"].as_u64() {
            assert_eq!(v.nested_nodes.len(), count as usize);
            assert_eq!(v.mappings[0].nested_node, v.mappings[1].nested_node);
        }
        assert!(reader.read_frame_into(reader.frames(), &mut b).is_err());
        assert!(b.pixels().is_none());
    }
}
#[test]
fn reject_policy_is_applied_only_to_effective_selected_fill() {
    let m = manifest();
    open(
        &fixture(""),
        &m["cases"]["measured-only-undefined-child"],
        UnmappedRegions::Reject,
        Limits::default(),
    )
    .unwrap();
    for fill in [UnmappedRegions::Reject, UnmappedRegions::DeclaredFill] {
        assert!(
            open(
                &fixture(""),
                &m["invalid"]["undefined_used"],
                fill,
                Limits::default()
            )
            .is_err()
        );
    }
    assert!(
        open(
            &fixture(""),
            &m["cases"]["grand-u64"],
            UnmappedRegions::Reject,
            Limits::default()
        )
        .is_err()
    );
}
#[test]
fn cycles_depth_and_expanded_dag_limits_fail_before_exposing_counts() {
    let m = manifest();
    for name in ["cycle", "depth", "dag_expansion"] {
        let error = match open(
            &fixture(""),
            &m["invalid"][name],
            UnmappedRegions::DeclaredFill,
            Limits::default(),
        ) {
            Ok(_) => panic!("{name} admitted"),
            Err(e) => e,
        };
        let text = error.to_string();
        assert!(
            text.contains("cycle") || text.contains("limit") || text.contains("budget"),
            "{name}:{text}"
        );
    }
    let case = &m["cases"]["grand-u64"];
    for limits in [
        Limits {
            datasets: 2,
            ..Default::default()
        },
        Limits {
            source_files: 2,
            ..Default::default()
        },
        Limits {
            metadata_bytes: 2048,
            ..Default::default()
        },
        Limits {
            frame_bytes: 95,
            ..Default::default()
        },
    ] {
        assert!(open(&fixture(""), case, UnmappedRegions::DeclaredFill, limits).is_err());
    }
}
#[test]
fn inactive_nested_sources_and_hard_aliases_remain_part_of_snapshot() {
    let m = manifest();
    let dir = copy_graph();
    let writer = hdf5_backend::File::open_rw(dir.path().join("child.h5")).unwrap();
    let reader = open(
        dir.path(),
        &m["cases"]["physical-node-hard-alias-dag"],
        UnmappedRegions::DeclaredFill,
        Limits::default(),
    )
    .unwrap();
    let mut b = FrameBuffer::default();
    reader.read_frame_into(0, &mut b).unwrap();
    writer.unlink("alias-u64").unwrap();
    assert!(reader.read_frame_into(0, &mut b).is_err());
    assert!(b.pixels().is_none());
    let dir = copy_graph();
    let writer = hdf5_backend::File::open_rw(dir.path().join("source.h5")).unwrap();
    let reader = open(
        dir.path(),
        &m["cases"]["grand-u64"],
        UnmappedRegions::DeclaredFill,
        Limits::default(),
    )
    .unwrap();
    reader.read_frame_into(0, &mut b).unwrap();
    writer.dataset("u64").unwrap().resize((4, 3, 4)).unwrap();
    assert!(reader.read_frame_into(0, &mut b).is_err());
    assert!(b.pixels().is_none());
}
