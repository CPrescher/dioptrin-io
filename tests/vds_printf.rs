#![cfg(feature = "hdf5")]
mod common;
use dioptrin_io::{
    PixelBuffer,
    hdf5::typed::{
        ExternalLinks, FilenameBinding, FrameBuffer, Limits, Options, PrintfMapping, Reader,
        Selection, UnmappedRegions, VdsOptions, VdsSelection,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/vds-printf")
        .join(name)
}
fn manifest() -> serde_json::Value {
    serde_json::from_slice(&fs::read(fixture("provenance.json")).unwrap()).unwrap()
}
fn policies(
    case: &serde_json::Value,
    directory: &Path,
) -> (Vec<FilenameBinding>, Vec<PrintfMapping>) {
    let bindings = case["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| FilenameBinding {
            stored_filename: v["stored_filename"].as_str().unwrap().into(),
            actual_file: directory.join(v["actual_file"].as_str().unwrap()),
        })
        .collect();
    let plans = case["printf_mappings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| PrintfMapping {
            source_file: v["source_file"].as_str().map(|p| directory.join(p)),
            dataset: v["dataset"].as_str().unwrap().into(),
            mapping_index: v["mapping_index"].as_u64().unwrap() as usize,
            stored_filename: v["stored_filename"].as_str().unwrap().into(),
            declared_dataset: v["declared_dataset"].as_str().unwrap().into(),
            blocks: v["blocks"].as_u64().unwrap() as usize,
        })
        .collect();
    (bindings, plans)
}
fn open(
    path: impl AsRef<Path>,
    bindings: &[FilenameBinding],
    plans: &[PrintfMapping],
    fill: UnmappedRegions,
    limits: Limits,
) -> Result<Reader, dioptrin_io::hdf5::Error> {
    Reader::open_snapshot_with_vds(
        path,
        Selection::Dataset("/data"),
        Options {
            external_links: ExternalLinks::FollowSnapshot,
            limits,
        },
        VdsOptions {
            filenames: bindings,
            printf_mappings: plans,
            unmapped_regions: fill,
        },
    )
}
fn bytes(pixels: &PixelBuffer) -> Vec<u8> {
    match pixels {
        PixelBuffer::Bool(v) | PixelBuffer::U8(v) => v.clone(),
        PixelBuffer::F16(v) | PixelBuffer::U16(v) => {
            v.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        PixelBuffer::U64(v) => v.iter().flat_map(|v| v.to_le_bytes()).collect(),
        PixelBuffer::I64(v) => v.iter().flat_map(|v| v.to_le_bytes()).collect(),
        PixelBuffer::I16(v) => v.iter().flat_map(|v| v.to_le_bytes()).collect(),
        PixelBuffer::C32(v) => v
            .iter()
            .flat_map(|v| v.iter().flat_map(|v| v.to_bits().to_le_bytes()))
            .collect(),
        _ => panic!("unexpected fixture scalar type"),
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn copied() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    for item in fs::read_dir(fixture("")).unwrap() {
        let item = item.unwrap();
        fs::copy(item.path(), directory.path().join(item.file_name())).unwrap();
    }
    directory
}
#[test]
fn explicit_printf_blocks_preserve_every_typed_word_and_source_declaration() {
    let source = manifest();
    let mut buffer = FrameBuffer::default();
    for (name, case) in source["cases"].as_object().unwrap() {
        let (bindings, plans) = policies(case, &fixture(""));
        let fill = if case["unmapped_regions"] == "declared_fill" {
            UnmappedRegions::DeclaredFill
        } else {
            UnmappedRegions::Reject
        };
        let reader = open(
            fixture(case["master"].as_str().unwrap()),
            &bindings,
            &plans,
            fill,
            Limits::default(),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let expected = case["frame_hex"].as_array().unwrap();
        assert_eq!(reader.frames(), expected.len(), "{name}");
        for index in (0..reader.frames()).rev().chain(0..reader.frames()) {
            reader
                .read_frame_into(index, &mut buffer)
                .unwrap_or_else(|e| panic!("{name}/{index}: {e}"));
            assert_eq!(
                buffer.shape(),
                Some([
                    case["shape"][1].as_u64().unwrap() as usize,
                    case["shape"][2].as_u64().unwrap() as usize
                ])
            );
            assert_eq!(
                hex(&bytes(buffer.pixels().unwrap())),
                expected[index].as_str().unwrap(),
                "{name}/{index}"
            );
        }
        let virtual_ = reader
            .metadata(0)
            .unwrap()
            .virtual_dataset
            .as_ref()
            .unwrap();
        let expansions: Vec<_> = virtual_
            .mappings
            .iter()
            .chain(virtual_.nested_nodes.iter().flat_map(|n| n.mappings.iter()))
            .filter_map(|m| m.printf_expansion.as_ref())
            .collect();
        let expected_blocks = if plans.is_empty() {
            1
        } else {
            plans.iter().map(|p| p.blocks).sum()
        };
        assert_eq!(expansions.len(), expected_blocks, "{name}");
        for expanded in expansions {
            let declaration = &expanded.declaration;
            assert!(expanded.block_index < declaration.blocks);
            if let Some(plan) = &declaration.plan {
                assert_eq!(declaration.stored_filename, plan.stored_filename);
                assert_eq!(declaration.declared_dataset, plan.declared_dataset);
                assert_eq!(declaration.dataset, plan.dataset);
                assert_eq!(declaration.mapping_index, plan.mapping_index);
            }
            if name == "predeclared-prefix" {
                let VdsSelection::Regular { shape, count, .. } = &declaration.virtual_selection
                else {
                    panic!()
                };
                assert_eq!(shape[0], 100);
                assert_eq!(count[0], u64::MAX);
                assert_eq!(virtual_.frozen_shape, [6, 2, 3]);
            }
        }
        assert!(
            reader
                .read_frame_into(reader.frames(), &mut buffer)
                .is_err()
        );
        assert!(buffer.pixels().is_none());
        assert!(buffer.shape().is_none());
    }
}
#[test]
fn block_scope_is_explicit_with_no_missing_source_fill_or_filename_fallback() {
    let m = manifest();
    let c = &m["cases"]["files-u64"];
    let (bindings, mut plans) = policies(c, &fixture(""));
    let path = fixture("files-u64.h5");
    assert!(
        open(
            &path,
            &bindings,
            &[],
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err()
    );
    for blocks in [0, Limits::default().datasets + 1] {
        plans[0].blocks = blocks;
        assert!(
            open(
                &path,
                &bindings,
                &plans,
                UnmappedRegions::DeclaredFill,
                Limits::default()
            )
            .is_err()
        );
    }
    plans[0].blocks = 3;
    let mut missing = bindings.clone();
    missing.remove(1);
    assert!(
        open(
            &path,
            &missing,
            &plans,
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err()
    );
    let directory = tempfile::tempdir().unwrap();
    let mut wrong = bindings.clone();
    wrong[1].actual_file = directory.path().join("absent.h5");
    assert!(
        open(
            &path,
            &wrong,
            &plans,
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err()
    );
    // Explicit prefix2 stays a four-frame view even though source block2 exists.
    plans[0].blocks = 2;
    let reader = open(
        &path,
        &bindings[..2],
        &plans,
        UnmappedRegions::Reject,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(reader.frames(), 4);
    let mut output = FrameBuffer::default();
    reader.read_frame_into(3, &mut output).unwrap();
    assert!(reader.read_frame_into(4, &mut output).is_err());
    assert!(output.pixels().is_none());
}
#[test]
fn owner_ordinal_pattern_conflicts_unused_plans_and_resources_fail_before_counts() {
    let m = manifest();
    let (bindings, plans) = policies(&m["cases"]["files-u64"], &fixture(""));
    let path = fixture("files-u64.h5");
    for change in 0..4 {
        let mut p = plans.clone();
        match change {
            0 => p[0].dataset = "/different".into(),
            1 => p[0].mapping_index = 1,
            2 => p[0].stored_filename = "different-%b.h5".into(),
            _ => p[0].declared_dataset = "/different-%b".into(),
        }
        assert!(
            open(
                &path,
                &bindings,
                &p,
                UnmappedRegions::Reject,
                Limits::default()
            )
            .is_err()
        );
    }
    let mut duplicate = plans.clone();
    duplicate.push(plans[0].clone());
    assert!(
        open(
            &path,
            &bindings,
            &duplicate,
            UnmappedRegions::Reject,
            Limits::default()
        )
        .is_err()
    );
    for limits in [
        Limits {
            metadata_bytes: 1024,
            ..Default::default()
        },
        Limits {
            frame_bytes: 47,
            ..Default::default()
        },
        Limits {
            frames: 5,
            ..Default::default()
        },
        Limits {
            datasets: 2,
            ..Default::default()
        },
    ] {
        assert!(open(&path, &bindings, &plans, UnmappedRegions::Reject, limits).is_err());
    }
}
#[test]
fn all_indexed_sources_are_frozen_even_for_a_read_from_another_block() {
    let directory = copied();
    let m = manifest();
    let (bindings, plans) = policies(&m["cases"]["files-u64"], directory.path());
    let reader = open(
        directory.path().join("files-u64.h5"),
        &bindings,
        &plans,
        UnmappedRegions::Reject,
        Limits::default(),
    )
    .unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    let target = directory.path().join("files-u64-2.h5");
    let replacement = directory.path().join("replacement.h5");
    fs::copy(&target, &replacement).unwrap();
    let changed = common::mutate_source(&target, || fs::rename(replacement, &target));
    assert_eq!(reader.read_frame_into(0, &mut buffer).is_err(), changed);
    assert_eq!(buffer.pixels().is_none(), changed);
    assert_eq!(buffer.shape().is_none(), changed);
    let directory = copied();
    let source = hdf5_backend::File::open_rw(directory.path().join("files-u64-0.h5")).unwrap();
    let data = source.dataset("/data").unwrap();
    let (bindings, plans) = policies(&m["cases"]["files-u64"], directory.path());
    let reader = open(
        directory.path().join("files-u64.h5"),
        &bindings,
        &plans,
        UnmappedRegions::Reject,
        Limits::default(),
    )
    .unwrap();
    reader.read_frame_into(0, &mut buffer).unwrap();
    data.resize((3, 2, 3)).unwrap();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
#[cfg(unix)]
#[test]
fn nested_printf_owner_alias_retarget_invalidates_the_frozen_policy() {
    use std::os::unix::fs::symlink;
    let directory = copied();
    let m = manifest();
    let (bindings, mut plans) = policies(&m["cases"]["nested-owner"], directory.path());
    let alias = directory.path().join("owner-alias.h5");
    symlink(directory.path().join("nested-child.h5"), &alias).unwrap();
    plans[0].source_file = Some(alias.clone());
    let reader = open(
        directory.path().join("nested-owner.h5"),
        &bindings,
        &plans,
        UnmappedRegions::Reject,
        Limits::default(),
    )
    .unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    fs::remove_file(&alias).unwrap();
    symlink(directory.path().join("self-datasets.h5"), &alias).unwrap();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    assert!(buffer.shape().is_none());
}
#[cfg(feature = "wrappers")]
#[test]
fn same_file_printf_capture_needs_no_private_filename_binding_or_relative_origin() {
    use dioptrin_io::wrapped::{FrameBuffer as WrappedBuffer, Input, Metadata};
    let m = manifest();
    let c = &m["cases"]["self-datasets"];
    let (_, plans) = policies(c, &fixture(""));
    let source = Input::from_bytes(
        &fs::read(fixture("self-datasets.h5")).unwrap(),
        None,
        Default::default(),
    )
    .unwrap();
    let mut reader = source
        .read_hdf5(
            Selection::Dataset("/data"),
            Options::default(),
            Some(VdsOptions {
                filenames: &[],
                printf_mappings: &plans,
                unmapped_regions: UnmappedRegions::Reject,
            }),
        )
        .unwrap();
    let Some(Metadata::Hdf5(meta)) = reader.metadata(0) else {
        panic!()
    };
    let v = meta.virtual_dataset.as_ref().unwrap();
    assert!(v.mappings.iter().all(|m| {
        m.printf_expansion
            .as_ref()
            .unwrap()
            .declaration
            .plan
            .as_ref()
            .unwrap()
            .source_file
            .is_none()
    }));
    let mut output = WrappedBuffer::default();
    for index in (0..reader.frames()).rev() {
        reader.read_frame_into(index, &mut output).unwrap();
        assert_eq!(
            hex(&bytes(output.pixels().unwrap())),
            c["frame_hex"][index].as_str().unwrap()
        );
    }
    assert!(
        reader
            .read_frame_into(reader.frames(), &mut output)
            .is_err()
    );
    assert!(output.pixels().is_none());
}
#[test]
fn unequal_printf_prefixes_cannot_hide_original_mapped_tails_inside_global_domain() {
    let source = manifest();
    let case = &source["cases"]["files-u64"];
    let (bindings, mut plans) = policies(case, &fixture(""));
    plans[0].blocks = 2;
    let mut second = plans[0].clone();
    second.mapping_index = 1;
    second.blocks = 3;
    plans.push(second);
    assert!(
        open(
            fixture("exhausted-prefixes.h5"),
            &bindings,
            &plans,
            UnmappedRegions::DeclaredFill,
            Limits::default()
        )
        .is_err(),
        "the shorter original infinite mapping still owns samples within the six-frame domain; its missing third block cannot become fill"
    );
}
#[test]
fn unequal_interleaved_prefixes_cover_global_domain_without_an_extra_unbound_block() {
    let source = manifest();
    let case = &source["cases"]["files-u64"];
    let (bindings, mut plans) = policies(case, &fixture(""));
    let mut second = plans[0].clone();
    second.mapping_index = 1;
    second.blocks = 2;
    plans.push(second);
    let reader = open(
        fixture("interleaved-prefixes.h5"),
        &bindings,
        &plans,
        UnmappedRegions::Reject,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(reader.frames(), 5);
    let mut buffer = FrameBuffer::default();
    for frame in 0..5 {
        reader.read_frame_into(frame, &mut buffer).unwrap();
        assert_eq!(
            hex(&bytes(buffer.pixels().unwrap())),
            case["frame_hex"][frame].as_str().unwrap()
        );
    }
}
