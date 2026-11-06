#![cfg(feature = "hdf5")]
use dioptrin_io::{
    PixelBuffer, PixelType,
    hdf5::{
        attributes::AttributeValue,
        nexus,
        typed::{
            ExternalRawOptions, FilenameBinding, FrameBuffer, Limits, Options, Reader, Selection,
        },
    },
};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/hdf5-layouts")
        .join(name)
}
fn hex(pixels: &PixelBuffer) -> String {
    let mut result = String::new();
    let mut bytes = |value: &[u8]| {
        for b in value {
            use std::fmt::Write;
            write!(&mut result, "{b:02x}").unwrap();
        }
    };
    match pixels {
        PixelBuffer::Bool(values) | PixelBuffer::U8(values) => bytes(values),
        PixelBuffer::F16(values) => {
            for v in values {
                bytes(&v.to_le_bytes());
            }
        }
        PixelBuffer::C32(values) => {
            for v in values.iter().flatten() {
                bytes(&v.to_bits().to_le_bytes());
            }
        }
        PixelBuffer::C64(values) => {
            for v in values.iter().flatten() {
                bytes(&v.to_bits().to_le_bytes());
            }
        }
        _ => panic!("unexpected output type"),
    }
    result
}
fn bindings(root: &Path) -> Vec<FilenameBinding> {
    ["external-a.bin", "external-b.bin"]
        .map(|name| FilenameBinding {
            stored_filename: name.into(),
            actual_file: root.join(name),
        })
        .to_vec()
}
fn raw(
    path: &Path,
    maps: &[FilenameBinding],
    options: Options,
) -> Result<Reader, dioptrin_io::hdf5::Error> {
    Reader::open_snapshot_with_storage(
        path,
        Selection::Dataset("data"),
        options,
        None,
        Some(ExternalRawOptions { filenames: maps }),
        None,
    )
}
#[test]
fn independent_complex_half_and_boolean_storage_bits_and_attributes() {
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    let graph = nexus::Reader::open_snapshot(fixture("types.h5"), Options::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    for (name, case) in expected["datasets"].as_object().unwrap() {
        if name == "external" {
            continue;
        }
        let reader = Reader::open_snapshot(
            fixture("types.h5"),
            Selection::Dataset(name),
            Options::default(),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let hashes = case["frame_hex"].as_array().unwrap();
        assert_eq!(reader.frames(), hashes.len());
        for (index, expected) in hashes.iter().enumerate().rev() {
            reader.read_frame_into(index, &mut buffer).unwrap();
            assert_eq!(
                hex(buffer.pixels().unwrap()),
                expected.as_str().unwrap(),
                "{name}:{index}"
            );
        }
        let ty = if name.starts_with("complex64") || name == "complex-fill" {
            PixelType::C32
        } else if name.starts_with("complex128") {
            PixelType::C64
        } else if name == "bool" {
            PixelType::Bool
        } else {
            PixelType::F16
        };
        assert_eq!(reader.metadata(0).unwrap().pixel_type, ty);
        assert_eq!(
            reader.metadata(0).unwrap().little_endian,
            !case["dtype"].as_str().unwrap().starts_with('>')
        );
        if let Some(expected) = case["attribute_hex"].as_str() {
            match graph
                .get_attr(&format!("/{name}"), "original")
                .unwrap()
                .unwrap()
            {
                AttributeValue::Numeric {
                    values, pixel_type, ..
                } => {
                    assert_eq!(pixel_type, ty);
                    assert_eq!(hex(&values), expected, "{name}");
                }
                _ => panic!("missing exact typed attribute"),
            }
        }
        assert!(
            reader
                .read_frame_into(reader.frames(), &mut buffer)
                .is_err()
        );
        assert!(buffer.pixels().is_none());
    }
    // NaN components compare by bits when the owned graph is revalidated.
    graph.check_sources().unwrap();
}
#[test]
fn exact_raw_windows_split_components_without_native_filename_lookup() {
    let parent = fixture("external.h5").parent().unwrap().to_path_buf();
    let maps = bindings(&parent);
    assert!(
        Reader::open_snapshot(
            fixture("external.h5"),
            Selection::Dataset("data"),
            Options::default()
        )
        .is_err()
    );
    let reader = raw(&fixture("external.h5"), &maps, Options::default()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    assert_eq!(reader.sources().len(), 3);
    assert_eq!(reader.source_bindings().len(), 3);
    let metadata = reader.metadata(0).unwrap();
    let windows = &metadata.external_raw.as_ref().unwrap().windows;
    assert_eq!(metadata.pixel_type, PixelType::C32);
    assert!(!metadata.little_endian);
    assert_eq!(windows[0].file_offset, 7);
    assert_eq!(windows[0].declared_bytes, Some(13));
    assert_eq!(windows[0].effective_bytes, 13);
    assert_eq!(windows[1].file_offset, 12);
    assert_eq!(windows[1].declared_bytes, None);
    assert_eq!(windows[1].effective_bytes, 83);
    let mut buffer = FrameBuffer::default();
    for index in [1, 0] {
        reader.read_frame_into(index, &mut buffer).unwrap();
        assert_eq!(
            hex(buffer.pixels().unwrap()),
            expected["datasets"]["external"]["frame_hex"][index]
                .as_str()
                .unwrap()
        );
    }
}
#[test]
fn raw_missing_partial_wrong_bindings_source_changes_and_limits_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["external.h5", "external-a.bin", "external-b.bin"] {
        std::fs::copy(fixture(name), dir.path().join(name)).unwrap();
    }
    let path = dir.path().join("external.h5");
    let maps = bindings(dir.path());
    assert!(raw(&path, &maps[..1], Options::default()).is_err());
    let mut duplicate = maps.clone();
    duplicate.push(maps[0].clone());
    assert!(raw(&path, &duplicate, Options::default()).is_err());
    let mut extra = maps.clone();
    extra.push(FilenameBinding {
        stored_filename: "wrong.bin".into(),
        actual_file: maps[0].actual_file.clone(),
    });
    assert!(raw(&path, &extra, Options::default()).is_err());
    for limits in [
        Limits {
            source_files: 2,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 47,
            ..Limits::default()
        },
        Limits {
            text_bytes: 5,
            ..Limits::default()
        },
    ] {
        assert!(
            raw(
                &path,
                &maps,
                Options {
                    limits,
                    ..Options::default()
                }
            )
            .is_err()
        );
    }
    let reader = raw(&path, &maps, Options::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&maps[1].actual_file)
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    assert!(reader.check_sources().is_err());
    std::fs::OpenOptions::new()
        .write(true)
        .open(&maps[1].actual_file)
        .unwrap()
        .set_len(94)
        .unwrap();
    assert!(raw(&path, &maps, Options::default()).is_err());
    std::fs::remove_file(&maps[1].actual_file).unwrap();
    assert!(raw(&path, &maps, Options::default()).is_err());
}
#[test]
fn unsupported_type_and_rank_variants_remain_explicit() {
    for name in ["renamed", "mixed-order", "padded", "enum", "rank1", "rank4"] {
        assert!(
            Reader::open_snapshot(
                fixture("types.h5"),
                Selection::Dataset(name),
                Options::default()
            )
            .is_err(),
            "{name}"
        );
    }
}

#[test]
fn raw_replacement_and_bound_symlink_changes_invalidate_reused_output() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["external.h5", "external-a.bin", "external-b.bin"] {
        std::fs::copy(fixture(name), dir.path().join(name)).unwrap();
    }
    let path = dir.path().join("external.h5");
    let maps = bindings(dir.path());
    let reader = raw(&path, &maps, Options::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    let alternate = dir.path().join("replacement.bin");
    std::fs::copy(&maps[1].actual_file, &alternate).unwrap();
    std::fs::rename(&alternate, &maps[1].actual_file).unwrap();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    #[cfg(unix)]
    {
        let alias = dir.path().join("payload-link.bin");
        std::os::unix::fs::symlink(&maps[1].actual_file, &alias).unwrap();
        let mut aliased = maps.clone();
        aliased[1].actual_file = alias.clone();
        let reader = raw(&path, &aliased, Options::default()).unwrap();
        reader.read_frame_into(1, &mut buffer).unwrap();
        std::fs::copy(&maps[1].actual_file, &alternate).unwrap();
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&alternate, &alias).unwrap();
        assert!(reader.read_frame_into(1, &mut buffer).is_err());
        assert!(buffer.pixels().is_none());
    }
}

#[test]
fn complex_vds_retains_original_fill_component_bits() {
    use dioptrin_io::hdf5::typed::{UnmappedRegions, VdsOptions};
    let maps = [FilenameBinding {
        stored_filename: "types.h5".into(),
        actual_file: fixture("types.h5"),
    }];
    let reader = Reader::open_snapshot_with_vds(
        fixture("complex-vds.h5"),
        Selection::Dataset("data"),
        Options::default(),
        VdsOptions {
            filenames: &maps,
            unmapped_regions: UnmappedRegions::DeclaredFill,
            ..Default::default()
        },
    )
    .unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    let original = Reader::open_snapshot(
        fixture("types.h5"),
        Selection::Dataset("complex64-be"),
        Options::default(),
    )
    .unwrap();
    let mut source = FrameBuffer::default();
    original.read_frame_into(0, &mut source).unwrap();
    let PixelBuffer::C32(out) = buffer.pixels().unwrap() else {
        panic!("wrong type")
    };
    let PixelBuffer::C32(input) = source.pixels().unwrap() else {
        panic!("wrong type")
    };
    for row in 0..2 {
        for col in 0..4 {
            for component in 0..2 {
                let expected = if col < 3 {
                    input[row * 3 + col][component].to_bits()
                } else {
                    [0x80000000, 0x7fc12345][component]
                };
                assert_eq!(out[row * 4 + col][component].to_bits(), expected);
            }
        }
    }
}

#[test]
fn unflushed_raw_dataset_unlink_invalidates_original_selection() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["external.h5", "external-a.bin", "external-b.bin"] {
        std::fs::copy(fixture(name), dir.path().join(name)).unwrap();
    }
    let path = dir.path().join("external.h5");
    let writer = hdf5_backend::File::open_rw(&path).unwrap();
    let reader = raw(&path, &bindings(dir.path()), Options::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    writer.unlink("data").unwrap();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}

#[cfg(feature = "sparse")]
#[test]
fn sparse_new_storage_types_stay_exact_and_dense_models_reject_explicitly() {
    use dioptrin_io::sparse;
    for (name, expected) in [
        ("complex", "000000804523c17f"),
        ("half", "557e"),
        ("bool", "ff"),
    ] {
        let reader = sparse::Reader::open_snapshot(
            fixture("sparse-types.h5"),
            sparse::Selection::Group(name),
            sparse::Options {
                reconstruction: sparse::Reconstruction::DeterministicBackground,
                ..Default::default()
            },
        )
        .unwrap();
        let mut stored = sparse::SparseFrameBuffer::default();
        reader.read_sparse_frame_into(0, &mut stored).unwrap();
        assert_eq!(hex(stored.intensity().unwrap()), expected);
        let mut dense = sparse::FrameBuffer::default();
        let error = reader.read_frame_into(0, &mut dense).unwrap_err();
        assert!(error.to_string().contains("stored components"));
        assert!(dense.pixels().is_none());
    }
}
