#![cfg(feature = "wrappers")]
use dioptrin_io::{
    PixelType, native,
    navigation::{Cursor, FrameSource},
    series::{Limits, Numbered, Series},
    tiff,
    wrapped::{self, Compression, Format, FrameBuffer, Reader},
};
use std::{
    cell::Cell,
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}
fn open(path: &Path, format: Format) -> Result<Reader, wrapped::Error> {
    Reader::open(
        path,
        format,
        wrapped::Limits::default(),
        native::Limits::default(),
        tiff::Limits::default(),
    )
}
fn npy(path: &Path) -> Result<Reader, wrapped::Error> {
    open(path, Format::Native(native::Format::Npy))
}
fn pgm(path: &Path) -> Result<Reader, wrapped::Error> {
    open(path, Format::Native(native::Format::Pnm))
}
fn edf(path: &Path) -> Result<Reader, wrapped::Error> {
    open(path, Format::Edf)
}
fn copy(name: &str, path: &Path) {
    std::fs::copy(fixture(name), path).unwrap();
}

#[test]
fn exact_prefix_mapping_preserves_multiframe_mixed_shape_dtype_and_order() {
    let paths = [
        fixture("native/npy-uint64-be-F.npy"),
        fixture("native/p5-uint16.pgm"),
        fixture("edf/compressed-stack.edf"),
    ];
    let opener = |path: &Path| match path.extension().and_then(|v| v.to_str()).unwrap() {
        "npy" => npy(path),
        "pgm" => pgm(path),
        "edf" => edf(path),
        _ => panic!("unexpected path"),
    };
    let mut series = Series::open(&paths, opener, Limits::default()).unwrap();
    let mut prefix = 0;
    let mut shapes = Vec::new();
    let mut types = Vec::new();
    for (file_index, path) in paths.iter().enumerate() {
        let mut reader = opener(path).unwrap();
        assert_eq!(series.files()[file_index].first_frame, prefix);
        assert_eq!(series.files()[file_index].frames, reader.frames());
        for local_frame in 0..reader.frames() {
            let global = prefix + local_frame;
            let location = series.locate(global).unwrap();
            assert_eq!(
                (
                    location.file_index,
                    location.local_frame,
                    location.global_frame
                ),
                (file_index, local_frame, global)
            );
            assert_eq!(location.path, path);
            let mut expected = FrameBuffer::default();
            let mut actual = FrameBuffer::default();
            reader.read_frame_into(local_frame, &mut expected).unwrap();
            series.read_frame_into(global, &mut actual).unwrap();
            assert_eq!(actual.shape(), expected.shape());
            assert_eq!(actual.sample_shape(), expected.sample_shape());
            assert_eq!(actual.pixels(), expected.pixels());
            shapes.push(actual.shape().unwrap());
            types.push(actual.pixels().unwrap().pixel_type());
            assert_eq!(series.current_location().unwrap().local_frame, local_frame);
            assert!(series.current_metadata().unwrap().is_some());
        }
        prefix += reader.frames();
    }
    assert_eq!(FrameSource::frames(&series), prefix);
    assert!(prefix > paths.len());
    assert!(series.locate(prefix).is_none());
    assert!(shapes.windows(2).any(|v| v[0] != v[1]));
    assert!(types.contains(&PixelType::U64));
    assert!(types.contains(&PixelType::U16));
    let result = series
        .with_metadata(0, |m| match m {
            wrapped::Metadata::Native(m) => (m.pixel_type, m.shape),
            _ => panic!("wrong metadata"),
        })
        .unwrap();
    assert_eq!(result.0, PixelType::U64);
    // Repeated/backward/random reads do not assume equal frames per file.
    let mut output = FrameBuffer::default();
    for index in [prefix - 1, 0, prefix - 2, 0] {
        series.read_frame_into(index, &mut output).unwrap();
    }
    assert!(series.read_frame_into(prefix, &mut output).is_err());
    assert!(output.pixels().is_none());
    assert!(series.current_location().is_none());
    assert!(series.is_valid());
}

#[test]
fn outer_compression_and_navigation_cross_file_boundaries_without_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = std::fs::read(fixture("edf/compressed-stack.edf")).unwrap();
    let gz = dir.path().join("one.anything");
    let bz = dir.path().join("two.anything");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes).unwrap();
    std::fs::write(&gz, encoder.finish().unwrap()).unwrap();
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    encoder.write_all(&bytes).unwrap();
    std::fs::write(&bz, encoder.finish().unwrap()).unwrap();
    assert_eq!(edf(&gz).unwrap().compression(), Compression::Gzip);
    assert_eq!(edf(&bz).unwrap().compression(), Compression::Bzip2);
    let original = edf(&fixture("edf/compressed-stack.edf")).unwrap();
    let series = Series::open([gz, bz], edf, Limits::default()).unwrap();
    assert_eq!(series.frames(), 2 * original.frames());
    let mut cursor = Cursor::new(series);
    cursor.read_at(original.frames() - 1).unwrap();
    let last = cursor.buffer().unwrap().pixels().unwrap().clone();
    cursor.next().unwrap();
    assert!(matches!(
        cursor.source().current_metadata_ref().unwrap(),
        Some(wrapped::Metadata::Edf(_))
    ));
    assert!(cursor.buffer().unwrap().pixels().is_some());
    assert_eq!(cursor.source().current_location().unwrap().local_frame, 0);
    cursor.previous().unwrap();
    assert_eq!(cursor.buffer().unwrap().pixels().unwrap(), &last);
}

#[test]
fn generated_numbers_are_explicit_bounded_and_frozen_at_first_missing() {
    let dir = tempfile::tempdir().unwrap();
    let numbered = Numbered {
        stem: dir.path().join("mydata"),
        suffix: OsString::from(".mar2300.gz"),
        first: 1,
        step: 1,
        digits: 4,
    };
    assert!(
        numbered
            .path(99993)
            .unwrap()
            .ends_with("mydata99993.mar2300.gz")
    );
    assert!(numbered.path(3).unwrap().ends_with("mydata0003.mar2300.gz"));
    let numbered = Numbered {
        suffix: ".pgm".into(),
        ..numbered
    };
    for index in [1, 2, 4] {
        copy("native/p5-uint16.pgm", &numbered.path(index).unwrap());
    }
    let manifest = numbered.until_missing(Limits::default()).unwrap();
    assert_eq!(manifest.paths.len(), 2);
    assert_eq!(manifest.stopped_at, Some(numbered.path(3).unwrap()));
    let mut series = Series::open(manifest.paths, pgm, Limits::default()).unwrap();
    copy("native/p5-uint16.pgm", &numbered.path(3).unwrap());
    assert_eq!(series.frames(), 2); // A later file never extends a frozen manifest.
    let mut output = FrameBuffer::default();
    series.read_frame_into(1, &mut output).unwrap();
    assert!(series.read_frame_into(2, &mut output).is_err());
    let limited = Limits {
        files: 2,
        ..Limits::default()
    };
    assert!(numbered.until_missing(limited).is_err());
    let spaced = Numbered {
        first: 1,
        step: 2,
        digits: 0,
        ..numbered.clone()
    };
    assert_eq!(
        spaced.through(5, Limits::default()).unwrap(),
        [1, 3, 5].map(|n| spaced.path(n).unwrap())
    );
    assert!(spaced.through(0, Limits::default()).is_err());
    assert!(
        spaced
            .through(
                1_000_000,
                Limits {
                    files: usize::MAX - 1,
                    metadata_bytes: 512,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    assert!(
        Numbered {
            step: 0,
            ..spaced.clone()
        }
        .path(1)
        .is_err()
    );
    assert!(
        Numbered {
            digits: usize::MAX,
            ..spaced.clone()
        }
        .path(1)
        .is_err()
    );
    assert!(
        Numbered {
            first: 0,
            step: 1,
            ..spaced.clone()
        }
        .through(u64::MAX, Limits::default())
        .is_err()
    );
    let at_end = Numbered {
        first: u64::MAX,
        ..numbered
    };
    copy("native/p5-uint16.pgm", &at_end.path(u64::MAX).unwrap());
    assert!(at_end.until_missing(Limits::default()).is_err());
}

#[test]
fn filename_seed_selects_image_numbers_without_consuming_format_or_compression_digits() {
    for (seed, number, expected) in [
        ("data0001.edf", 2, "data0002.edf"),
        ("bob1.edf", 2, "bob2.edf"),
        ("1.edf", 123456, "123456.edf"),
        ("1.mar2300", 2, "2.mar2300"),
        ("mydata001.mar2300.gz", 3, "mydata003.mar2300.gz"),
        ("Brukerimage.0123", 124, "Brukerimage.0124"),
        ("detector9999.edf.bz2", 10000, "detector10000.edf.bz2"),
        ("scan0001", 0, "scan0000"),
        ("foo0004suffix.edf", 2, "foo0002suffix.edf"),
        (
            "/tmp/123-directory/1.mar2300",
            2,
            "/tmp/123-directory/2.mar2300",
        ),
        (
            "/tmp/123-directory/scan001.edf",
            2,
            "/tmp/123-directory/scan002.edf",
        ),
    ] {
        let pattern = Numbered::from_seed(seed, 1).unwrap();
        assert_eq!(pattern.path(number).unwrap(), Path::new(expected), "{seed}");
    }
    for path in [
        "ordinary.edf",
        "ordinary.mar2300.gz",
        "/tmp/123/ordinary.edf",
        "foo18446744073709551616.edf",
    ] {
        assert!(Numbered::from_seed(path, 1).is_err(), "{path}");
    }
    assert!(Numbered::from_seed("data0001.edf", 0).is_err());
}

#[test]
fn manifest_empty_missing_limits_and_opener_substitution_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.pgm");
    let missing = dir.path().join("missing.pgm");
    let b = dir.path().join("b.pgm");
    copy("native/p5-uint16.pgm", &a);
    copy("native/p5-uint16.pgm", &b);
    let mut empty = Series::open(Vec::<PathBuf>::new(), pgm, Limits::default()).unwrap();
    assert_eq!(empty.frames(), 0);
    assert!(empty.files().is_empty());
    assert!(empty.current_metadata().unwrap().is_none());
    assert!(Series::open([&a, &missing, &b], pgm, Limits::default()).is_err());
    assert!(
        Series::open(
            [&a, &b],
            pgm,
            Limits {
                files: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        Series::open(
            [&a],
            pgm,
            Limits {
                metadata_bytes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        Series::open(
            [fixture("edf/compressed-stack.edf")],
            edf,
            Limits {
                frames: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    for limits in [
        Limits {
            files: 0,
            ..Limits::default()
        },
        Limits {
            frames: usize::MAX,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 0,
            ..Limits::default()
        },
    ] {
        assert!(Series::open([&a], pgm, limits).is_err());
    }
    assert!(Series::open([&a], |_| pgm(&b), Limits::default()).is_err());
}

#[test]
fn growth_removal_or_rename_of_any_indexed_source_invalidates_all_output() {
    for action in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.pgm");
        let b = dir.path().join("b.pgm");
        copy("native/p5-uint16.pgm", &a);
        copy("native/p5-uint16.pgm", &b);
        let mut series = Series::open([&a, &b], pgm, Limits::default()).unwrap();
        let mut output = FrameBuffer::default();
        series.read_frame_into(0, &mut output).unwrap();
        match action {
            0 => std::fs::OpenOptions::new()
                .append(true)
                .open(&b)
                .unwrap()
                .write_all(&[0])
                .unwrap(),
            1 => std::fs::remove_file(&b).unwrap(),
            _ => std::fs::rename(&b, dir.path().join("renamed.pgm")).unwrap(),
        }
        assert!(series.read_frame_into(0, &mut output).is_err()); // inactive file too
        assert!(output.pixels().is_none());
        assert!(output.shape().is_none());
        assert!(output.sample_shape().is_none());
        assert!(!series.is_valid());
        assert!(series.current_location().is_none());
        assert!(series.current_metadata().is_err());
        assert!(series.read_frame_into(0, &mut output).is_err());
    }
}

#[test]
fn reopened_count_change_and_mutation_during_open_cannot_publish_counts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stack.edf");
    copy("edf/compressed-stack.edf", &path);
    let changed = Cell::new(false);
    let opener = |path: &Path| {
        if changed.get() {
            open(
                path,
                Format::Raw(native::RawLayout {
                    shape: [1, std::fs::metadata(path)?.len() as usize],
                    pixel_type: PixelType::U8,
                    little_endian: true,
                    offset: native::Offset::Absolute(0),
                }),
            )
        } else {
            edf(path)
        }
    };
    let mut series = Series::open([&path], opener, Limits::default()).unwrap();
    let mut output = FrameBuffer::default();
    series.read_frame_into(0, &mut output).unwrap();
    changed.set(true);
    assert!(series.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    assert!(!series.is_valid());
    // Mutation after Reader::open and before Series receives it is also rejected.
    copy("edf/compressed-stack.edf", &path);
    let opener = |path: &Path| {
        let reader = edf(path)?;
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)?
            .write_all(&[0])?;
        Ok(reader)
    };
    assert!(Series::open([&path], opener, Limits::default()).is_err());
}

#[cfg(unix)]
#[test]
fn root_symlink_retarget_and_broken_traversal_entries_are_errors() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.pgm");
    let b = dir.path().join("b.pgm");
    let alias = dir.path().join("alias.pgm");
    copy("native/p5-uint16.pgm", &a);
    copy("native/p5-uint16.pgm", &b);
    symlink(&a, &alias).unwrap();
    let mut series = Series::open([&alias], pgm, Limits::default()).unwrap();
    let mut output = FrameBuffer::default();
    series.read_frame_into(0, &mut output).unwrap();
    std::fs::remove_file(&alias).unwrap();
    symlink(&b, &alias).unwrap();
    assert!(series.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    let numbered = Numbered {
        stem: dir.path().join("broken"),
        suffix: ".pgm".into(),
        first: 0,
        step: 1,
        digits: 0,
    };
    symlink(dir.path().join("absent"), numbered.path(0).unwrap()).unwrap();
    assert!(numbered.until_missing(Limits::default()).is_err());
}

#[cfg(all(feature = "hdf5", unix))]
#[test]
fn inactive_hdf5_graph_and_duplicate_target_aliases_remain_bound_after_indexing() {
    use dioptrin_io::hdf5::typed::{ExternalLinks, Options, Selection};
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("plain.pgm");
    copy("native/p5-uint16.pgm", &plain);
    for name in ["original.h5", "replacement.h5"] {
        let file = hdf5_backend::File::create(dir.path().join(name)).unwrap();
        let pixels = file
            .new_dataset::<u16>()
            .shape((2, 2))
            .create("pixels")
            .unwrap();
        pixels.write_raw(&[1u16, 32768, 42, u16::MAX]).unwrap();
    }
    let one = dir.path().join("one.h5");
    let two = dir.path().join("two.h5");
    symlink(dir.path().join("original.h5"), &one).unwrap();
    symlink(dir.path().join("original.h5"), &two).unwrap();
    let master = dir.path().join("master.h5");
    {
        let file = hdf5_backend::File::create(&master).unwrap();
        let data = file
            .create_group("entry")
            .unwrap()
            .create_group("data")
            .unwrap();
        data.link_external("one.h5", "/pixels", "data_000001")
            .unwrap();
        data.link_external("two.h5", "/pixels", "data_000002")
            .unwrap();
    }
    let opener = |path: &Path| {
        if path.extension().is_some_and(|v| v == "pgm") {
            return pgm(path);
        }
        wrapped::Input::open(path, wrapped::Limits::default())?.read_hdf5(
            Selection::Eiger,
            Options {
                external_links: ExternalLinks::FollowSnapshot,
                ..Options::default()
            },
            None,
        )
    };
    let mut series = Series::open([&plain, &master], opener, Limits::default()).unwrap();
    assert_eq!(series.frames(), 3);
    let mut output = FrameBuffer::default();
    series.read_frame_into(1, &mut output).unwrap();
    assert_eq!(
        output.pixels(),
        Some(&dioptrin_io::PixelBuffer::U16(vec![1, 32768, 42, u16::MAX]))
    );
    series.read_frame_into(0, &mut output).unwrap();
    // The inactive reader was dropped. Both aliases led to the same physical
    // file at indexing, but retargeting the second must still invalidate it.
    std::fs::remove_file(&two).unwrap();
    symlink(dir.path().join("replacement.h5"), &two).unwrap();
    assert!(series.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    assert!(!series.is_valid());
}

#[test]
fn metadata_callbacks_cannot_return_success_after_mutating_an_inactive_source() {
    let dir = tempfile::tempdir().unwrap();
    let one = dir.path().join("one.pgm");
    let two = dir.path().join("two.pgm");
    copy("native/p5-uint16.pgm", &one);
    copy("native/p5-uint16.pgm", &two);
    let mut series = Series::open([&one, &two], pgm, Limits::default()).unwrap();
    let mut output = FrameBuffer::default();
    series.read_frame_into(0, &mut output).unwrap();
    let result = series.with_metadata(0, |_| {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&two)
            .unwrap()
            .write_all(&[0])
            .unwrap();
        42
    });
    assert!(result.is_err());
    assert!(!series.is_valid());
    assert!(series.current_location().is_none());
    assert!(series.current_metadata().is_err());
}
