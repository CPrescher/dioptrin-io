#![cfg(feature = "hdf5")]
use dioptrin_io::{
    discovery::Codec,
    hdf5::{
        discovery::discover_snapshot,
        typed::{ExternalLinks, Limits, Options},
    },
};
use hdf5_backend::{File, Location, types::VarLenUnicode};
use std::{path::PathBuf, str::FromStr};
fn attr(location: &Location, name: &str, value: &str) {
    let value = VarLenUnicode::from_str(value).unwrap();
    location
        .new_attr::<VarLenUnicode>()
        .shape(())
        .create(name)
        .unwrap()
        .write_scalar(&value)
        .unwrap();
}
fn array(file: &File, path: &str) {
    file.new_dataset::<u16>()
        .shape((1, 2, 3))
        .create(path)
        .unwrap();
}
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/hdf5-native")
        .join(name)
}
#[test]
fn existing_detector_conventions_and_generic_hdf5_are_metadata_only() {
    for (name, codec) in [
        ("eiger-master.h5", Codec::Eiger),
        ("lima.h5", Codec::Lima),
        ("lambda.h5", Codec::Lambda),
        ("scalars.h5", Codec::Hdf5),
    ] {
        assert_eq!(
            discover_snapshot(fixture(name), Options::default(), None).unwrap(),
            codec,
            "{name}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metadata.h5");
    {
        let file = File::create(&path).unwrap();
        file.create_group("entry").unwrap();
        file.create_group("entry/data").unwrap();
        array(&file, "entry/data/unrelated");
        file.flush().unwrap();
    }
    assert_eq!(
        discover_snapshot(&path, Options::default(), None).unwrap(),
        Codec::Hdf5
    );
    {
        let file = File::create(&path).unwrap();
        file.create_group("entry").unwrap();
        file.new_dataset::<u16>()
            .shape((1_000_000, 2, 3))
            .create("entry/data")
            .unwrap();
        file.flush().unwrap();
    }
    let options = Options {
        limits: Limits {
            frame_bytes: 1,
            frames: 1,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert_eq!(
        discover_snapshot(&path, options, None).unwrap(),
        Codec::Eiger
    );
    {
        let file = File::create(&path).unwrap();
        file.new_dataset::<u16>()
            .shape((2, 3))
            .create("entry")
            .unwrap();
        file.flush().unwrap();
    }
    assert_eq!(
        discover_snapshot(&path, Options::default(), None).unwrap(),
        Codec::Hdf5
    );
}
#[test]
fn conflicting_strong_markers_require_an_explicit_layout() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("conflict.h5");
    {
        let file = File::create(&path).unwrap();
        attr(&file, "creator", "LIMA");
        attr(&file, "default", "entry");
        file.create_group("entry/measurement").unwrap();
        array(&file, "entry/measurement/data");
        file.create_group("entry/instrument/detector").unwrap();
        array(&file, "entry/instrument/detector/data");
        file.new_dataset::<VarLenUnicode>()
            .shape(())
            .create("entry/instrument/detector/description")
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str("Lambda").unwrap())
            .unwrap();
        file.flush().unwrap();
    }
    assert!(
        discover_snapshot(&path, Options::default(), None)
            .unwrap_err()
            .to_string()
            .contains("conflicting")
    );
}
#[test]
fn external_metadata_paths_use_their_resolved_source_and_explicit_origin() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let master = dir.path().join("master.h5");
    {
        let file = File::create(nested.join("leaf.h5")).unwrap();
        let plot = file.create_group("plot").unwrap();
        attr(&plot, "dataformat", "sparsify-Bragg");
        file.flush().unwrap();
    }
    {
        let file = File::create(dir.path().join("leaf.h5")).unwrap();
        let plot = file.create_group("plot").unwrap();
        attr(&plot, "dataformat", "dense");
        file.flush().unwrap();
    }
    {
        let file = File::create(nested.join("relay.h5")).unwrap();
        let entry = file.create_group("entry").unwrap();
        attr(&entry, "default", "target/plot");
        entry.link_external("leaf.h5", "/", "target").unwrap();
        file.flush().unwrap();
    }
    {
        let file = File::create(&master).unwrap();
        attr(&file, "creator", "LIMA");
        attr(&file, "default", "relay");
        file.link_external("nested/relay.h5", "/entry", "relay")
            .unwrap();
        file.flush().unwrap();
    }
    assert!(
        discover_snapshot(&master, Options::default(), None)
            .unwrap_err()
            .to_string()
            .contains("FollowSnapshot")
    );
    let staged = tempfile::tempdir().unwrap();
    let path = staged.path().join("capture");
    std::fs::copy(&master, &path).unwrap();
    let options = Options {
        external_links: ExternalLinks::FollowSnapshot,
        ..Options::default()
    };
    assert_eq!(
        discover_snapshot(&path, options, Some(dir.path())).unwrap(),
        Codec::Sparse
    );
    assert!(
        discover_snapshot(
            &path,
            Options {
                limits: Limits {
                    source_files: 1,
                    ..Limits::default()
                },
                ..options
            },
            Some(dir.path())
        )
        .is_err()
    );
}
#[test]
fn metadata_text_members_and_traversal_limits_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("limits.h5");
    {
        let file = File::create(&path).unwrap();
        attr(&file, "creator", "LIMA");
        attr(&file, "default", "entry");
        let entry = file.create_group("entry").unwrap();
        attr(&entry, "default", "plot");
        let plot = entry.create_group("plot").unwrap();
        attr(&plot, "dataformat", &format!("{}Bragg", "x".repeat(1000)));
        file.flush().unwrap();
    }
    assert!(
        discover_snapshot(
            &path,
            Options {
                limits: Limits {
                    text_bytes: 64,
                    ..Limits::default()
                },
                ..Options::default()
            },
            None
        )
        .is_err()
    );
    assert!(
        discover_snapshot(
            &path,
            Options {
                limits: Limits {
                    metadata_bytes: 1400,
                    ..Limits::default()
                },
                ..Options::default()
            },
            None
        )
        .is_err()
    );
    assert!(
        discover_snapshot(
            &path,
            Options {
                limits: Limits {
                    link_traversals: 1,
                    ..Limits::default()
                },
                ..Options::default()
            },
            None
        )
        .is_err()
    );
    {
        let file = File::create(&path).unwrap();
        file.create_group("entry").unwrap();
        file.create_group("unrelated").unwrap();
        file.flush().unwrap();
    }
    assert!(
        discover_snapshot(
            &path,
            Options {
                limits: Limits {
                    group_members: 1,
                    ..Limits::default()
                },
                ..Options::default()
            },
            None
        )
        .is_err()
    );
    {
        let file = File::create(&path).unwrap();
        file.link_soft("/entry", "entry").unwrap();
        file.flush().unwrap();
    }
    assert!(
        discover_snapshot(
            &path,
            Options {
                limits: Limits {
                    link_traversals: 4,
                    ..Limits::default()
                },
                ..Options::default()
            },
            None
        )
        .is_err()
    );
}
