#![cfg(feature = "hdf5")]
use dioptrin_io::{
    PixelBuffer, PixelType,
    hdf5::typed::{ExternalLinks, FrameBuffer, Limits, Options, Reader, Selection},
};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/hdf5-native")
        .join(name)
}
fn hash(pixels: &PixelBuffer) -> String {
    let mut hash = Sha256::new();
    match pixels {
        PixelBuffer::C32(v) => {
            for pair in v {
                for x in pair {
                    hash.update(x.to_le_bytes());
                }
            }
            return format!("{:x}", hash.finalize());
        }
        PixelBuffer::C64(v) => {
            for pair in v {
                for x in pair {
                    hash.update(x.to_le_bytes());
                }
            }
            return format!("{:x}", hash.finalize());
        }
        PixelBuffer::CI16(v) => {
            for pair in v {
                for x in pair {
                    hash.update(x.to_le_bytes());
                }
            }
            return format!("{:x}", hash.finalize());
        }
        _ => {}
    }

    macro_rules! types {($($v:ident);*)=>{match pixels {$(PixelBuffer::$v(v)=>for x in v {hash.update(x.to_le_bytes());}),*,_=>unreachable!()}};}
    types!(U8;U16;U32;U64;I8;I16;I32;I64;F32;F64);
    format!("{:x}", hash.finalize())
}
#[test]
fn independent_h5py_types_endianness_partial_chunks_and_ieee_bits() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    let mut buffer = FrameBuffer::default();
    for (key, case) in manifest["datasets"].as_object().unwrap() {
        let (file, path) = key.split_once("::").unwrap();
        let reader =
            Reader::open_snapshot(fixture(file), Selection::Dataset(path), Options::default())
                .unwrap_or_else(|e| panic!("{key}: {e}"));
        let hashes = case["frame_sha256"].as_array().unwrap();
        assert_eq!(reader.frames(), hashes.len());
        for (i, expected) in hashes.iter().enumerate().rev() {
            reader.read_frame_into(i, &mut buffer).unwrap();
            assert_eq!(
                hash(buffer.pixels().unwrap()),
                expected.as_str().unwrap(),
                "{key}: {i}"
            );
            assert_eq!(
                reader.metadata(i).unwrap().little_endian,
                case["little_endian"].as_bool().unwrap(),
                "{key}"
            );
        }
        assert!(
            reader
                .read_frame_into(reader.frames(), &mut buffer)
                .is_err()
        );
        assert!(buffer.pixels().is_none());
    }
    let reader = Reader::open_snapshot(
        fixture("scalars.h5"),
        Selection::Dataset("explicit-fill"),
        Options::default(),
    )
    .unwrap();
    assert_eq!(
        reader.metadata(0).unwrap().fill_value,
        Some(PixelBuffer::I16(vec![-123]))
    );
    let grouped = Reader::open_snapshot(
        fixture("scalars.h5"),
        Selection::Dataset("group"),
        Options::default(),
    )
    .unwrap();
    assert_eq!(grouped.metadata(0).unwrap().pixel_type, PixelType::U64);
}
fn external() -> Options {
    Options {
        external_links: ExternalLinks::FollowSnapshot,
        ..Options::default()
    }
}
#[test]
fn detector_discovery_and_complete_source_graph_preserve_frame_order_and_types() {
    assert!(
        Reader::open_snapshot(
            fixture("eiger-master.h5"),
            Selection::Eiger,
            Options::default()
        )
        .is_err()
    );
    let reader =
        Reader::open_snapshot(fixture("eiger-master.h5"), Selection::Eiger, external()).unwrap();
    assert_eq!(reader.frames(), 3);
    assert_eq!(reader.sources().len(), 4);
    assert_eq!(reader.links().len(), 4);
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::U32(vec![
            0,
            70000,
            u32::MAX - 1,
            u32::MAX,
            5,
            6
        ]))
    );
    reader.read_frame_into(2, &mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::I16(vec![i16::MIN, 0, i16::MAX, 7, 8, 9]))
    );
    let lima =
        Reader::open_snapshot(fixture("lima.h5"), Selection::Lima, Options::default()).unwrap();
    assert_eq!(lima.frames(), 2);
    assert_eq!(lima.fields()["detector"], "Pilatus");
    let lambda =
        Reader::open_snapshot(fixture("lambda.h5"), Selection::Lambda, Options::default()).unwrap();
    assert_eq!(lambda.frames(), 1);
    assert_eq!(lambda.fields()["description"], "Lambda");
    assert_eq!(lambda.fields()["detector"], "λ-detector");
}
#[test]
fn missing_cycles_unsupported_storage_custom_precision_and_limits_fail_closed() {
    for path in [
        "rank1",
        "compound",
        "custom-precision",
        "virtual",
        "external-raw",
        "undefined-unwritten",
    ] {
        assert!(
            Reader::open_snapshot(
                fixture("scalars.h5"),
                Selection::Dataset(path),
                Options::default()
            )
            .is_err(),
            "{path}"
        );
    }
    assert!(
        Reader::open_snapshot(fixture("dangling-master.h5"), Selection::Eiger, external()).is_err()
    );
    assert!(
        Reader::open_snapshot(
            fixture("cycle.h5"),
            Selection::Dataset("first"),
            Options::default()
        )
        .is_err()
    );
    for limits in [
        Limits {
            frames: 1,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 1,
            ..Limits::default()
        },
        Limits {
            chunk_bytes: 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
        Limits {
            link_traversals: 1,
            ..Limits::default()
        },
        Limits {
            source_files: 1,
            ..Limits::default()
        },
        Limits {
            datasets: 1,
            ..Limits::default()
        },
        Limits {
            group_members: 1,
            ..Limits::default()
        },
    ] {
        assert!(
            Reader::open_snapshot(
                fixture("eiger-master.h5"),
                Selection::Eiger,
                Options {
                    limits,
                    ..external()
                }
            )
            .is_err()
        );
    }
    assert!(
        Reader::open_snapshot(
            fixture("lambda.h5"),
            Selection::Lambda,
            Options {
                limits: Limits {
                    text_bytes: 4,
                    ..Limits::default()
                },
                ..Options::default()
            }
        )
        .is_err()
    );
    assert!(
        Reader::open_snapshot(
            fixture("lima.h5"),
            Selection::Lima,
            Options {
                limits: Limits {
                    text_bytes: 4,
                    ..Limits::default()
                },
                ..Options::default()
            }
        )
        .is_err()
    );
}
struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "dioptrin-typed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        for item in std::fs::read_dir(fixture("")).unwrap() {
            let item = item.unwrap();
            std::fs::copy(item.path(), path.join(item.file_name())).unwrap();
        }
        Self(path)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn resizable_datasets_are_frozen_and_changes_in_any_tracked_file_invalidate_output() {
    let temp = Temporary::new();
    let path = temp.0.join("growing.h5");
    let file = hdf5_backend::File::create(&path).unwrap();
    let data = file
        .new_dataset::<u16>()
        .shape((1.., 2, 3))
        .chunk((1, 2, 3))
        .create("data")
        .unwrap();
    data.write_raw(&[1u16, 2, 3, 4, 5, 6]).unwrap();
    file.flush().unwrap();
    let reader =
        Reader::open_snapshot(&path, Selection::Dataset("data"), Options::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    data.resize((2, 2, 3)).unwrap();
    file.flush().unwrap();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    drop(reader);
    drop(data);
    drop(file);
    // An intermediate external-link file belongs to the manifest even when
    // another file owns the actual dataset. Change it through a live native
    // write handle so this check is portable across OS file-sharing rules.
    let intermediate = hdf5_backend::File::open_rw(temp.0.join("intermediate.h5")).unwrap();
    let reader =
        Reader::open_snapshot(temp.0.join("eiger-master.h5"), Selection::Eiger, external())
            .unwrap();
    reader.read_frame_into(2, &mut buffer).unwrap();
    intermediate
        .new_dataset::<u32>()
        .shape(10000)
        .create("change")
        .unwrap()
        .write_raw(&vec![7u32; 10000])
        .unwrap();
    intermediate.flush().unwrap();
    assert!(reader.read_frame_into(2, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
