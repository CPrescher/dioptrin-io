#![cfg(feature = "hdf5")]
mod common;
use dioptrin_io::{
    PixelBuffer,
    hdf5::typed::{
        ExternalLinks, FilenameBinding, FrameBuffer, Options, Reader, Selection, UnmappedRegions,
        VdsOptions, VdsSelection,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/vds-unlimited")
}
fn manifest() -> serde_json::Value {
    serde_json::from_slice(&fs::read(directory().join("provenance.json")).unwrap()).unwrap()
}
fn bindings(case: &serde_json::Value, dir: &Path) -> Vec<FilenameBinding> {
    case["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| FilenameBinding {
            stored_filename: b["stored_filename"].as_str().unwrap().into(),
            actual_file: dir.join(b["actual_file"].as_str().unwrap()),
        })
        .collect()
}
fn open(case: &serde_json::Value, dir: &Path) -> Result<Reader, dioptrin_io::hdf5::Error> {
    let bindings = bindings(case, dir);
    Reader::open_snapshot_with_vds(
        dir.join(case["master"].as_str().unwrap()),
        Selection::Dataset("/data"),
        Options {
            external_links: ExternalLinks::FollowSnapshot,
            ..Options::default()
        },
        VdsOptions {
            filenames: &bindings,
            printf_mappings: &[],
            unmapped_regions: if case["unmapped_regions"] == "declared_fill" {
                UnmappedRegions::DeclaredFill
            } else {
                UnmappedRegions::Reject
            },
        },
    )
}
fn hex(words: &[u64]) -> String {
    words
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .map(|v| format!("{v:02x}"))
        .collect()
}
#[test]
fn independent_unlimited_offsets_strides_partial_blocks_and_interleaves_preserve_exact_words() {
    let source = manifest();
    let mut buffer = FrameBuffer::default();
    let mut accepted = 0;
    let mut rejected = 0;
    for (name, case) in source["cases"].as_object().unwrap() {
        let result = open(case, &directory());
        if case["expect"] == "reject" {
            assert!(
                result.is_err(),
                "{name} must reject exhausted mapping rather than fill its missing samples"
            );
            rejected += 1;
            continue;
        }
        let reader = result.unwrap_or_else(|e| panic!("{name}: {e}"));
        accepted += 1;
        let expected = case["frame_hex"].as_array().unwrap();
        assert_eq!(reader.frames(), expected.len(), "{name}");
        for frame in (0..reader.frames()).rev().chain(0..reader.frames()) {
            reader
                .read_frame_into(frame, &mut buffer)
                .unwrap_or_else(|e| panic!("{name}/{frame}: {e}"));
            let PixelBuffer::U64(words) = buffer.pixels().unwrap() else {
                panic!("source type changed")
            };
            assert_eq!(
                hex(words),
                expected[frame].as_str().unwrap(),
                "{name}/{frame}"
            );
            assert_eq!(
                buffer.shape(),
                Some([
                    case["shape"][1].as_u64().unwrap() as usize,
                    case["shape"][2].as_u64().unwrap() as usize
                ])
            );
        }
        let meta = reader
            .metadata(0)
            .unwrap()
            .virtual_dataset
            .as_ref()
            .unwrap();
        assert_eq!(meta.frozen_shape[0], reader.frames());
        assert_eq!(meta.declared_shape[0], 0);
        assert_eq!(
            meta.mappings.len(),
            case["mappings"].as_array().unwrap().len()
        );
        for (mapping, declaration) in meta
            .mappings
            .iter()
            .zip(case["mappings"].as_array().unwrap())
        {
            assert!(mapping.storage_allocated);
            assert!(mapping.printf_expansion.is_none());
            let VdsSelection::Regular {
                start,
                stride,
                count,
                block,
                ..
            } = &mapping.source_selection
            else {
                panic!("lost source selection")
            };
            for (actual, key) in [(start, 0), (stride, 1), (count, 2), (block, 3)] {
                for axis in 0..3 {
                    assert_eq!(
                        actual[axis],
                        declaration["stored_source"][key][axis].as_u64().unwrap(),
                        "{name}"
                    );
                }
            }
            for effective in [
                &mapping.frozen_source_selection,
                &mapping.frozen_virtual_selection,
            ] {
                match effective {
                    VdsSelection::Regular {
                        shape,
                        count,
                        block,
                        ..
                    } => {
                        assert!(!shape.contains(&u64::MAX));
                        assert!(!count.contains(&u64::MAX));
                        assert!(!block.contains(&u64::MAX));
                    }
                    VdsSelection::All { shape, .. } => {
                        assert!(!shape.contains(&u64::MAX))
                    }
                    VdsSelection::Irregular { shape, .. } => {
                        assert!(!shape.contains(&u64::MAX))
                    }
                }
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
    assert_eq!(accepted, 6);
    assert_eq!(rejected, 1);
}
fn copied() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for item in fs::read_dir(directory()).unwrap() {
        let item = item.unwrap();
        fs::copy(item.path(), dir.path().join(item.file_name())).unwrap();
    }
    dir
}
#[test]
fn every_bound_source_is_frozen_and_growth_or_replacement_invalidates_prior_output() {
    let source = manifest();
    let case = &source["cases"]["interleaved"];
    for growth in [false, true] {
        let dir = copied();
        let target = dir.path().join("interleaved-1.h5");
        let mutable = if growth {
            Some(hdf5_backend::File::open_rw(&target).unwrap())
        } else {
            None
        };
        let reader = open(case, dir.path()).unwrap();
        let mut buffer = FrameBuffer::default();
        reader.read_frame_into(0, &mut buffer).unwrap();
        let changed = if let Some(file) = mutable.as_ref() {
            file.dataset("/data").unwrap().resize((3, 2, 3)).unwrap();
            true
        } else {
            common::mutate_source(&target, || {
                fs::rename(&target, target.with_extension("old"))?;
                fs::copy(dir.path().join("interleaved-0.h5"), &target).map(|_| ())
            })
        };
        assert_eq!(reader.read_frame_into(0, &mut buffer).is_err(), changed);
        assert_eq!(buffer.pixels().is_none(), changed);
        assert_eq!(buffer.shape().is_none(), changed);
    }
}
#[test]
fn native_hdf5_rejects_unlimited_unions_and_preserves_finite_intersection_semantics() {
    use hdf5_sys::h5s::{H5S_seloper_t, H5Sget_select_npoints, H5Sselect_hyperslab};
    hdf5_backend::sync::sync(|| {
        for operation in [
            H5S_seloper_t::H5S_SELECT_OR,
            H5S_seloper_t::H5S_SELECT_XOR,
            H5S_seloper_t::H5S_SELECT_NOTB,
            H5S_seloper_t::H5S_SELECT_AND,
        ] {
            let space = hdf5_backend::Dataspace::try_new((0.., 2, 3)).unwrap();
            let status = unsafe {
                H5Sselect_hyperslab(
                    space.id(),
                    H5S_seloper_t::H5S_SELECT_SET,
                    [0, 0, 0].as_ptr(),
                    [2, 2, 3].as_ptr(),
                    [u64::MAX, 1, 1].as_ptr(),
                    [1, 2, 3].as_ptr(),
                )
            };
            assert!(status >= 0);
            let status = unsafe {
                H5Sselect_hyperslab(
                    space.id(),
                    operation,
                    [1, 0, 0].as_ptr(),
                    [1, 2, 3].as_ptr(),
                    [1, 1, 1].as_ptr(),
                    [1, 2, 3].as_ptr(),
                )
            };
            if operation == H5S_seloper_t::H5S_SELECT_AND {
                assert!(status >= 0);
                assert_eq!(unsafe { H5Sget_select_npoints(space.id()) }, 0);
            } else {
                assert!(status < 0);
            }
        }
        let space = hdf5_backend::Dataspace::try_new((0.., 2, 3)).unwrap();
        assert!(
            unsafe {
                H5Sselect_hyperslab(
                    space.id(),
                    H5S_seloper_t::H5S_SELECT_SET,
                    [0, 0, 0].as_ptr(),
                    [2, 2, 3].as_ptr(),
                    [u64::MAX, 1, 1].as_ptr(),
                    [1, 2, 3].as_ptr(),
                )
            } >= 0
        );
        assert!(
            unsafe {
                H5Sselect_hyperslab(
                    space.id(),
                    H5S_seloper_t::H5S_SELECT_OR,
                    [1, 0, 0].as_ptr(),
                    [2, 2, 3].as_ptr(),
                    [u64::MAX, 1, 1].as_ptr(),
                    [1, 2, 3].as_ptr(),
                )
            } < 0
        );
    });
}
