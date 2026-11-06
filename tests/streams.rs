#![cfg(feature = "wrappers")]
use dioptrin_io::{
    discovery::Codec,
    native, tiff,
    wrapped::{self, Compression, Format, FrameBuffer, Input, Limits, Reader},
};
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}
struct ShortReads<'a>(&'a [u8]);
impl Read for ShortReads<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let count = out.len().min(17).min(self.0.len());
        out[..count].copy_from_slice(&self.0[..count]);
        self.0 = &self.0[count..];
        Ok(count)
    }
}
fn read(input: Input, format: Format) -> Reader {
    input
        .read(format, native::Limits::default(), tiff::Limits::default())
        .unwrap()
}
fn compare(a: &mut Reader, b: &mut Reader) {
    assert_eq!(a.frames(), b.frames());
    let mut aa = FrameBuffer::default();
    let mut bb = FrameBuffer::default();
    for i in (0..a.frames()).rev() {
        a.read_frame_into(i, &mut aa).unwrap();
        b.read_frame_into(i, &mut bb).unwrap();
        assert_eq!(aa.sample_shape(), bb.sample_shape());
        assert_eq!(aa.pixels(), bb.pixels());
    }
    assert!(b.read_frame_into(b.frames(), &mut bb).is_err());
    assert!(bb.pixels().is_none());
}
#[test]
fn existing_image_backends_read_nonseekable_captures_with_the_same_counts() {
    for (name, format, codec) in [
        (
            "native/npy-uint64-be-F.npy",
            Format::Native(native::Format::Npy),
            Codec::Npy,
        ),
        (
            "native/dtrek-int32-big.img",
            Format::Native(native::Format::Dtrek),
            Codec::Dtrek,
        ),
        (
            "native/mask-33.msk",
            Format::Native(native::Format::Fit2dMask),
            Codec::Fit2dMask,
        ),
        (
            "native/hipic.img",
            Format::Native(native::Format::HiPic),
            Codec::HiPic,
        ),
        (
            "native/p5-uint16.pgm",
            Format::Native(native::Format::Pnm),
            Codec::Pnm,
        ),
        ("edf/compressed-stack.edf", Format::Edf, Codec::Edf),
        (
            "cbf/int16.cbf",
            Format::Cbf(dioptrin_io::cbf::ByteOffsetMode::Extended64),
            Codec::Cbf,
        ),
        ("tiff/le-bigtiff.tif", Format::Tiff, Codec::Tiff),
        #[cfg(feature = "mar345")]
        (
            "mar345/field32-nonzero.mar345",
            Format::Mar345,
            Codec::Mar345,
        ),
        #[cfg(feature = "bruker")]
        ("legacy/bruker86-linear.sfrm", Format::Bruker, Codec::Bruker),
        #[cfg(feature = "bruker")]
        (
            "legacy/bruker100-baseline.sfrm",
            Format::Bruker,
            Codec::Bruker,
        ),
        #[cfg(feature = "esperanto")]
        (
            "esperanto/agi-extrema-cols34.esperanto",
            Format::Esperanto,
            Codec::Esperanto,
        ),
        #[cfg(feature = "fit2d")]
        ("records/fit2d-512-big.f2d", Format::Fit2d, Codec::Fit2d),
        #[cfg(feature = "kcd")]
        (
            "records/readouts.kcd",
            Format::Kcd(dioptrin_io::kcd::Interpretation::StoredReadouts),
            Codec::Kcd,
        ),
        #[cfg(feature = "mpa")]
        ("records/sections.mpa", Format::Mpa, Codec::Mpa),
        #[cfg(feature = "jpeg")]
        (
            "images/rgb.jpg",
            Format::Jpeg(Default::default()),
            Codec::Jpeg,
        ),
        #[cfg(feature = "jpeg2000")]
        (
            "images/uint16.jp2",
            Format::Jpeg2000(Default::default()),
            Codec::Jpeg2000,
        ),
        #[cfg(feature = "fixed")]
        ("fixed/ge-uint16.ge", Format::Ge, Codec::Ge),
        #[cfg(feature = "containers")]
        ("containers/spe-v3-int32.spe", Format::Spe, Codec::Spe),
        #[cfg(feature = "containers")]
        ("containers/mrc-mode3-be.mrc", Format::Mrc, Codec::Mrc),
        #[cfg(feature = "containers")]
        ("containers/dm3-uint16-be.dm3", Format::Dm3, Codec::Dm3),
        #[cfg(feature = "oxford")]
        ("legacy/oxford-ty1-i32.img", Format::Oxford, Codec::Oxford),
        #[cfg(feature = "raxis")]
        (
            "legacy/raxis-big.img",
            Format::Raxis(dioptrin_io::raxis::Interpretation::Stored),
            Codec::Raxis,
        ),
        #[cfg(feature = "xsd")]
        ("xsd/uint64-base32.xml", Format::Xsd, Codec::Xsd),
    ] {
        let path = fixture(name);
        let bytes = std::fs::read(&path).unwrap();
        let input = Input::from_reader(ShortReads(&bytes), None, Limits::default()).unwrap();
        let d = input.discover().unwrap();
        assert_eq!(d.require_unique().unwrap(), codec, "{name}: {d:?}");
        let mut automatic = Input::from_bytes(&bytes, None, Limits::default())
            .unwrap()
            .read_discovered(native::Limits::default(), tiff::Limits::default())
            .unwrap();
        let mut a = read(Input::open(&path, Limits::default()).unwrap(), format);
        let mut b = read(input, format);
        compare(&mut a, &mut b);
        compare(&mut a, &mut automatic);
    }
}
fn compressed(bytes: &[u8], mode: Compression) -> Vec<u8> {
    match mode {
        Compression::Gzip => {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), Default::default());
            e.write_all(bytes).unwrap();
            e.finish().unwrap()
        }
        Compression::Bzip2 => {
            let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
            e.write_all(bytes).unwrap();
            e.finish().unwrap()
        }
        Compression::None => bytes.to_vec(),
    }
}
#[test]
fn stream_wrappers_bounds_short_reads_and_owned_copy() {
    let path = fixture("native/npy-uint64-be-F.npy");
    let mut bytes = std::fs::read(&path).unwrap();
    let mut a = read(
        Input::open(&path, Limits::default()).unwrap(),
        Format::Native(native::Format::Npy),
    );
    for mode in [Compression::None, Compression::Gzip, Compression::Bzip2] {
        let payload = compressed(&bytes, mode);
        let input = Input::from_reader(ShortReads(&payload), None, Limits::default()).unwrap();
        let p = input.provenance().unwrap();
        assert!(p.stream);
        assert_eq!(p.origin_directory, None);
        assert_eq!(p.encoded_bytes, payload.len() as u64);
        assert_eq!(p.decoded_bytes, bytes.len() as u64);
        assert_eq!(p.compression, mode);
        compare(
            &mut a,
            &mut read(input, Format::Native(native::Format::Npy)),
        );
        assert!(
            Input::from_bytes(
                &payload,
                None,
                Limits {
                    encoded_bytes: payload.len() as u64 - 1,
                    ..Limits::default()
                }
            )
            .is_err()
        );
        assert!(
            Input::from_bytes(
                &payload,
                None,
                Limits {
                    source_bytes: bytes.len() as u64 - 1,
                    ..Limits::default()
                }
            )
            .is_err()
        );
    }
    let input = Input::from_bytes(&bytes, None, Limits::default()).unwrap();
    bytes.fill(255);
    compare(
        &mut a,
        &mut read(input, Format::Native(native::Format::Npy)),
    );
    for limits in [
        Limits {
            encoded_bytes: 0,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: u64::MAX,
            ..Limits::default()
        },
        Limits {
            source_bytes: u64::MAX,
            ..Limits::default()
        },
    ] {
        assert!(Input::from_bytes(b"", None, limits).is_err());
    }
}
#[test]
fn io_errors_corrupt_wrappers_and_current_position_are_explicit() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("test read failure"))
        }
    }
    assert!(Input::from_reader(Broken, None, Limits::default()).is_err());
    let bytes = std::fs::read(fixture("native/npy-v1.npy")).unwrap();
    for mode in [Compression::Gzip, Compression::Bzip2] {
        let mut bad = compressed(&bytes, mode);
        bad.truncate(bad.len() - 4);
        assert!(Input::from_bytes(&bad, None, Limits::default()).is_err());
    }
    let mut with_prefix = b"skip".to_vec();
    with_prefix.extend(&bytes);
    let mut cursor = io::Cursor::new(with_prefix);
    cursor.set_position(4);
    let input = Input::from_reader(&mut cursor, None, Limits::default()).unwrap();
    assert_eq!(
        input.discover().unwrap().require_unique().unwrap(),
        Codec::Npy
    );
    assert_eq!(cursor.position(), cursor.get_ref().len() as u64);
    assert!(
        Input::from_bytes(
            &bytes,
            Some(&fixture("native/npy-v1.npy")),
            Limits::default()
        )
        .is_err()
    );
}
#[test]
fn capture_cleanup_and_failed_read_invalidate_output() {
    let bytes = std::fs::read(fixture("native/npy-v1.npy")).unwrap();
    let mut r = Reader::from_reader(
        &bytes[..],
        None,
        Format::Native(native::Format::Npy),
        Limits::default(),
        native::Limits::default(),
        tiff::Limits::default(),
    )
    .unwrap();
    let capture = r.source_paths()[0].to_path_buf();
    assert!(capture.exists());
    let mut b = FrameBuffer::default();
    r.read_frame_into(0, &mut b).unwrap();
    std::fs::write(&capture, b"changed").unwrap();
    assert!(r.read_frame_into(0, &mut b).is_err());
    assert!(b.pixels().is_none());
    drop(r);
    assert!(!capture.exists());
}
#[test]
fn renamed_content_and_header_only_streams_do_not_need_an_extension() {
    let bytes = std::fs::read(fixture("tiff/le-bigtiff.tif")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let wrong = dir.path().join("misleading.npy");
    std::fs::write(&wrong, &bytes).unwrap();
    assert_eq!(
        Input::open(&wrong, Limits::default())
            .unwrap()
            .discover()
            .unwrap()
            .require_unique()
            .unwrap(),
        Codec::Tiff
    );
    let header = wrapped::HeaderReader::from_reader(
        ShortReads(&bytes),
        wrapped::HeaderFormat::Tiff,
        Limits::default(),
        native::Limits::default(),
        tiff::Limits::default(),
    )
    .unwrap();
    let wrapped::HeaderMetadata::Tiff(pages) = header.metadata().unwrap() else {
        panic!()
    };
    assert!(!pages.is_empty());
    assert_eq!(header.source_prefix().unwrap(), bytes[..64]);
}
#[cfg(feature = "hdf5")]
#[test]
fn hdf5_streams_select_datasets_conventions_and_explicit_link_origins() {
    use dioptrin_io::hdf5::typed::{ExternalLinks, Options, Selection};
    for (name, selection, codec) in [
        (
            "scalars.h5",
            Selection::Dataset("/uint16-be-2d"),
            Codec::Hdf5,
        ),
        ("lima.h5", Selection::Lima, Codec::Lima),
        ("lambda.h5", Selection::Lambda, Codec::Lambda),
    ] {
        let path = fixture(&format!("hdf5-native/{name}"));
        let bytes = std::fs::read(&path).unwrap();
        let input = Input::from_bytes(&bytes, None, Limits::default()).unwrap();
        assert_eq!(input.discover().unwrap().require_unique().unwrap(), codec);
        let mut a = Input::open(&path, Limits::default())
            .unwrap()
            .read_hdf5(selection, Options::default(), None)
            .unwrap();
        let mut b = input
            .read_hdf5(selection, Options::default(), None)
            .unwrap();
        compare(&mut a, &mut b);
    }
    let path = fixture("hdf5-native/eiger-master.h5");
    let bytes = std::fs::read(&path).unwrap();
    let options = Options {
        external_links: ExternalLinks::FollowSnapshot,
        ..Options::default()
    };
    assert!(
        Input::from_bytes(&bytes, None, Limits::default())
            .unwrap()
            .read_hdf5(Selection::Eiger, options, None)
            .is_err()
    );
    let input = Input::from_bytes(&bytes, Some(path.parent().unwrap()), Limits::default()).unwrap();
    assert_eq!(
        input.discover().unwrap().require_unique().unwrap(),
        Codec::Eiger
    );
    let mut b = input.read_hdf5(Selection::Eiger, options, None).unwrap();
    let mut a = Input::open(&path, Limits::default())
        .unwrap()
        .read_hdf5(Selection::Eiger, options, None)
        .unwrap();
    compare(&mut a, &mut b);
    assert!(b.source_paths().len() > 1);
}
#[cfg(feature = "sparse")]
#[test]
fn discovered_sparse_retains_source_events_and_requires_explicit_dense_view() {
    let path = fixture("sparse/float-events.h5");
    let bytes = std::fs::read(&path).unwrap();
    let input = Input::from_bytes(&bytes, None, Limits::default()).unwrap();
    assert_eq!(
        input.discover().unwrap().require_unique().unwrap(),
        Codec::Sparse
    );
    let mut r = input
        .read_discovered(native::Limits::default(), tiff::Limits::default())
        .unwrap();
    assert!(r.sparse_reader().is_some());
    let mut dense = FrameBuffer::default();
    assert!(r.read_frame_into(0, &mut dense).is_err());
    assert!(dense.pixels().is_none());
    let mut events = dioptrin_io::sparse::SparseFrameBuffer::default();
    r.sparse_reader()
        .unwrap()
        .read_sparse_frame_into(0, &mut events)
        .unwrap();
    assert!(events.index().is_some());
}

#[test]
fn external_edf_streams_require_origin_and_track_linked_source_changes() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data.bin");
    std::fs::write(&data, [99, 98, 0, 0, 255, 255, 1, 128]).unwrap();
    let header=b"\r\n{\nEDF_BinarySize = 0; EDF_BinaryFileName = data.bin; EDF_BinaryFilePosition = 2; EDF_BinaryFileSize = 6; Dim_1 = 3; Dim_2 = 1; DataType = UnsignedShort; ByteOrder = LowByteFirst;}\r\n";
    let format = Format::EdfExternal {
        policy: dioptrin_io::edf::ExternalPayloads::FollowSnapshot,
        basenames: false,
    };
    assert!(
        Input::from_bytes(header, None, Limits::default())
            .unwrap()
            .read(format, native::Limits::default(), tiff::Limits::default())
            .is_err()
    );
    assert!(
        Input::from_bytes(header, Some(dir.path()), Limits::default())
            .unwrap()
            .read_discovered(native::Limits::default(), tiff::Limits::default())
            .is_err()
    );
    let mut r = read(
        Input::from_bytes(header, Some(dir.path()), Limits::default()).unwrap(),
        format,
    );
    let mut b = FrameBuffer::default();
    r.read_frame_into(0, &mut b).unwrap();
    assert_eq!(
        b.pixels(),
        Some(&dioptrin_io::PixelBuffer::U16(vec![0, 65535, 32769]))
    );
    assert_eq!(r.source_paths().len(), 2);
    std::fs::rename(&data, dir.path().join("original.bin")).unwrap();
    std::fs::write(&data, [99, 98, 0, 0, 255, 255, 1, 128]).unwrap();
    assert!(r.read_frame_into(0, &mut b).is_err());
    assert!(b.pixels().is_none());
}

#[test]
fn cloned_input_keeps_both_transport_roots_until_the_last_owner_and_failed_decoder_drop() {
    let source = std::fs::read(fixture("native/npy-v1.npy")).unwrap();
    let bytes = compressed(&source, Compression::Gzip);
    let input = Input::from_bytes(&bytes, None, Limits::default()).unwrap();
    let roots: Vec<_> = input
        .transport_paths()
        .unwrap()
        .into_iter()
        .map(Path::to_path_buf)
        .collect();
    assert_eq!(roots.len(), 2);
    assert_eq!(std::fs::read(&roots[0]).unwrap(), bytes);
    assert_eq!(std::fs::read(&roots[1]).unwrap(), source);
    assert_eq!(input.source_path().unwrap(), roots[1]);
    let retained = input.clone();
    let mut first = read(input, Format::Native(native::Format::Npy));
    let mut second = read(retained.clone(), Format::Native(native::Format::Npy));
    compare(&mut first, &mut second);
    drop(first);
    drop(second);
    assert!(roots.iter().all(|p| p.exists()));
    drop(retained);
    assert!(roots.iter().all(|p| !p.exists()));
    let input = Input::from_bytes(&bytes, None, Limits::default()).unwrap();
    let paths: Vec<_> = input
        .transport_paths()
        .unwrap()
        .into_iter()
        .map(Path::to_path_buf)
        .collect();
    assert!(
        input
            .read(
                Format::Edf,
                native::Limits::default(),
                tiff::Limits::default()
            )
            .is_err()
    );
    assert!(paths.iter().all(|p| !p.exists()));
}

#[test]
fn anonymous_raw_stream_and_named_reader_captures_preserve_owned_bytes_and_current_position() {
    let values = [0u64, u64::MAX, (1 << 53) + 1, 1 << 63];
    let mut bytes = b"raw".to_vec();
    for value in values {
        bytes.extend(value.to_be_bytes());
    }
    let format = Format::Raw(native::RawLayout {
        shape: [2, 2],
        pixel_type: dioptrin_io::PixelType::U64,
        little_endian: false,
        offset: native::Offset::Absolute(3),
    });
    let mut r = read(
        Input::from_reader(ShortReads(&bytes), None, Limits::default()).unwrap(),
        format,
    );
    let mut b = FrameBuffer::default();
    r.read_frame_into(0, &mut b).unwrap();
    assert_eq!(
        b.pixels(),
        Some(&dioptrin_io::PixelBuffer::U64(values.to_vec()))
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original.npy");
    let source = std::fs::read(fixture("native/npy-v1.npy")).unwrap();
    std::fs::write(&path, &source).unwrap();
    let mut file = std::fs::File::open(&path).unwrap();
    let input = Input::from_reader(&mut file, None, Limits::default()).unwrap();
    assert!(input.provenance().unwrap().stream);
    assert_eq!(input.provenance().unwrap().origin_directory, None);
    std::fs::write(&path, b"changed original backing file").unwrap();
    let mut owned = read(input, Format::Native(native::Format::Npy));
    owned.read_frame_into(0, &mut b).unwrap();
    assert!(b.pixels().is_some());
    let mut bounded = io::Cursor::new(&source);
    assert!(
        Input::from_reader(
            &mut bounded,
            None,
            Limits {
                encoded_bytes: 3,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert_eq!(bounded.position(), 4);
    let mut untouched = io::Cursor::new(&source);
    assert!(Input::from_reader(&mut untouched, Some(&path), Limits::default()).is_err());
    assert_eq!(untouched.position(), 0);
}

#[cfg(feature = "hdf5")]
#[test]
fn compressed_hdf5_streams_reuse_dataset_counts_without_implicit_origin_or_backing_file() {
    use dioptrin_io::hdf5::typed::{Options, Selection};
    let path = fixture("hdf5-native/scalars.h5");
    let bytes = std::fs::read(&path).unwrap();
    let mut direct = Input::open(&path, Limits::default())
        .unwrap()
        .read_hdf5(
            Selection::Dataset("/uint64-be-2d"),
            Options::default(),
            None,
        )
        .unwrap();
    for codec in [Compression::Gzip, Compression::Bzip2] {
        let encoded = compressed(&bytes, codec);
        let input = Input::from_reader(ShortReads(&encoded), None, Limits::default()).unwrap();
        assert_eq!(
            input.discover().unwrap().require_unique().unwrap(),
            Codec::Hdf5
        );
        let provenance = input.provenance().unwrap();
        assert_eq!(provenance.origin_directory, None);
        assert_eq!(provenance.compression, codec);
        let roots: Vec<_> = input
            .transport_paths()
            .unwrap()
            .into_iter()
            .map(Path::to_path_buf)
            .collect();
        let mut stream = input
            .read_hdf5(
                Selection::Dataset("/uint64-be-2d"),
                Options::default(),
                None,
            )
            .unwrap();
        compare(&mut direct, &mut stream);
        drop(stream);
        assert!(roots.iter().all(|p| !p.exists()));
    }
}

#[cfg(feature = "hdf5")]
#[test]
fn hdf5_nested_stream_links_anchor_each_source_and_track_intermediate_and_leaf_replacement() {
    use dioptrin_io::hdf5::typed::{ExternalLinks, Options, Selection};
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let master = dir.path().join("master.h5");
    let relay = nested.join("relay.h5");
    let leaf = nested.join("leaf.h5");
    let decoy = dir.path().join("leaf.h5");
    for (path, values) in [(&leaf, [0u16, 65535, 32769]), (&decoy, [17u16, 18, 19])] {
        let file = hdf5_backend::File::create(path).unwrap();
        file.new_dataset::<u16>()
            .shape((1, 3))
            .create("pixels")
            .unwrap()
            .write_raw(&values)
            .unwrap();
        file.flush().unwrap();
    }
    {
        let file = hdf5_backend::File::create(&relay).unwrap();
        file.link_external("leaf.h5", "/pixels", "relay").unwrap();
        file.flush().unwrap();
    }
    {
        let file = hdf5_backend::File::create(&master).unwrap();
        file.link_external("nested/relay.h5", "/relay", "data")
            .unwrap();
        file.flush().unwrap();
    }
    let bytes = std::fs::read(&master).unwrap();
    let options = Options {
        external_links: ExternalLinks::FollowSnapshot,
        ..Default::default()
    };
    // An explicit origin does not itself authorize an external link.
    assert!(
        Input::from_bytes(&bytes, Some(dir.path()), Limits::default())
            .unwrap()
            .read_hdf5(Selection::Dataset("/data"), Options::default(), None)
            .is_err()
    );
    let wrong = tempfile::tempdir().unwrap();
    assert!(
        Input::from_bytes(&bytes, Some(wrong.path()), Limits::default())
            .unwrap()
            .read_hdf5(Selection::Dataset("/data"), options, None)
            .is_err()
    );
    for changed in [&relay, &leaf] {
        let mut reader = Input::from_bytes(&bytes, Some(dir.path()), Limits::default())
            .unwrap()
            .read_hdf5(Selection::Dataset("/data"), options, None)
            .unwrap();
        let mut frame = FrameBuffer::default();
        reader.read_frame_into(0, &mut frame).unwrap();
        assert_eq!(
            frame.pixels(),
            Some(&dioptrin_io::PixelBuffer::U16(vec![0, 65535, 32769]))
        );
        assert_eq!(reader.source_paths().len(), 3);
        assert!(!reader.source_paths().contains(&decoy.as_path()));
        let sources = reader.hdf5_reader().unwrap();
        assert_eq!(sources.links().len(), 2);
        assert!(sources.links().iter().all(|l| l.external));
        let old = changed.with_extension("old");
        std::fs::rename(changed, &old).unwrap();
        std::fs::copy(&old, changed).unwrap();
        assert!(reader.read_frame_into(0, &mut frame).is_err());
        assert!(frame.pixels().is_none());
        drop(reader);
        std::fs::remove_file(old).unwrap();
    }
}

#[cfg(feature = "hdf5")]
#[test]
fn vds_streams_require_explicit_bindings_and_detect_replaced_bound_sources_without_fill() {
    use dioptrin_io::hdf5::typed::{FilenameBinding, Options, Selection, VdsOptions};
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("module-a.h5");
    std::fs::copy(fixture("vds/module-a.h5"), &a).unwrap();
    let bytes = std::fs::read(fixture("vds/full.h5")).unwrap();
    assert!(
        Input::from_bytes(&bytes, Some(dir.path()), Limits::default())
            .unwrap()
            .read_hdf5(
                Selection::Dataset("/data"),
                Options::default(),
                Some(VdsOptions::default())
            )
            .is_err()
    );
    let maps = [FilenameBinding {
        stored_filename: "/missing/acquisition/module-a.h5".into(),
        actual_file: a.clone(),
    }];
    let policy = VdsOptions {
        filenames: &maps,
        ..Default::default()
    };
    let mut reader = Input::from_bytes(&bytes, None, Limits::default())
        .unwrap()
        .read_hdf5(
            Selection::Dataset("/data"),
            Options::default(),
            Some(policy),
        )
        .unwrap();
    let mut direct = Input::open(fixture("vds/full.h5"), Limits::default())
        .unwrap()
        .read_hdf5(
            Selection::Dataset("/data"),
            Options::default(),
            Some(policy),
        )
        .unwrap();
    compare(&mut direct, &mut reader);
    assert!(
        reader
            .hdf5_reader()
            .unwrap()
            .metadata(0)
            .unwrap()
            .virtual_dataset
            .is_some()
    );
    let mut frame = FrameBuffer::default();
    reader.read_frame_into(0, &mut frame).unwrap();
    let old = dir.path().join("prior-module-a.h5");
    std::fs::rename(&a, &old).unwrap();
    std::fs::copy(&old, &a).unwrap();
    assert!(reader.read_frame_into(0, &mut frame).is_err());
    assert!(frame.pixels().is_none());
}
