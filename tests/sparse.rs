#![cfg(feature = "sparse")]
mod common;
use dioptrin_io::{PixelBuffer, PixelType, sparse::*};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/sparse")
        .join(name)
}
fn manifest() -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap()
}
fn hash(pixels: &PixelBuffer) -> String {
    let mut hash = Sha256::new();
    macro_rules! scalar {($($variant:ident);*)=>{match pixels{$(PixelBuffer::$variant(v)=>{for x in v{hash.update(x.to_le_bytes());}},)*_=>panic!("unexpected source type")}};}
    scalar!(U8;U16;U32;U64;I8;I16;I32;I64;F32;F64);
    format!("{:x}", hash.finalize())
}
fn dtype(ty: PixelType) -> &'static str {
    match ty {
        PixelType::U8 => "uint8",
        PixelType::U16 => "uint16",
        PixelType::U32 => "uint32",
        PixelType::U64 => "uint64",
        PixelType::I8 => "int8",
        PixelType::I16 => "int16",
        PixelType::I32 => "int32",
        PixelType::I64 => "int64",
        PixelType::F32 => "float32",
        PixelType::F64 => "float64",
        _ => panic!("source type"),
    }
}
fn derived() -> Options {
    Options {
        reconstruction: Reconstruction::DeterministicBackground,
        ..Options::default()
    }
}
#[test]
fn original_components_native_events_and_explicit_derived_views_remain_distinct() {
    let mut array = ArrayBuffer::default();
    let mut frame = SparseFrameBuffer::default();
    let mut dense = FrameBuffer::default();
    for (name, case) in manifest()["files"].as_object().unwrap() {
        let path = fixture(name);
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
            case["file_sha256"].as_str().unwrap()
        );
        let stored =
            Reader::open(&path, Options::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let model = Reader::open(&path, derived()).unwrap();
        assert!(!stored.metadata().dense_is_derived);
        assert!(model.metadata().dense_is_derived);
        assert_eq!(stored.frames(), case["dense"].as_array().unwrap().len());
        for (component, expected) in case["components"].as_object().unwrap() {
            stored.read_component_into(component, &mut array).unwrap();
            let pixels = array.pixels().unwrap();
            let metadata = stored
                .metadata()
                .components
                .iter()
                .find(|m| m.name == *component)
                .unwrap();
            assert_eq!(
                hash(pixels),
                expected["sha256"].as_str().unwrap(),
                "{name}:{component}"
            );
            assert_eq!(
                dtype(pixels.pixel_type()),
                expected["dtype"].as_str().unwrap()
            );
            assert_eq!(serde_json::json!(array.shape()), expected["shape"]);
            assert_eq!(metadata.little_endian, expected["little_endian"].as_bool());
        }
        let config = stored
            .metadata()
            .components
            .iter()
            .find(|m| m.name == "entry/sparsify/configuration/data")
            .unwrap();
        assert_eq!(
            config.text.as_deref(),
            Some(
                &["{\"sparsify\":{\"cutoff_pick\":5.5},\"source\":\"independent\"}".to_owned()][..]
            )
        );
        assert!(
            stored
                .metadata()
                .components
                .iter()
                .any(|m| m.name == "entry/peaks/position")
        );
        assert!(stored.metadata().attributes.iter().any(|a|matches!(a.values.get("axes"),Some(AttributeValue::Text(v))if v==&["y".to_owned(),"x".to_owned()])));
        for (index, expected) in case["dense"].as_array().unwrap().iter().enumerate().rev() {
            stored.read_sparse_frame_into(index, &mut frame).unwrap();
            assert_eq!(frame.shape(), Some(stored.metadata().shape));
            assert_eq!(
                frame.index().unwrap().len(),
                frame.intensity().unwrap().len()
            );
            assert_eq!(
                frame.intensity().unwrap().pixel_type(),
                stored.metadata().intensity_type
            );
            model.read_frame_into(index, &mut dense).unwrap();
            assert_eq!(
                hash(dense.pixels().unwrap()),
                expected["sha256"].as_str().unwrap(),
                "{name}:frame{index}"
            );
            assert!(stored.read_frame_into(index, &mut dense).is_err());
            assert!(dense.pixels().is_none());
        }
        assert!(
            stored
                .read_sparse_frame_into(stored.frames(), &mut frame)
                .is_err()
        );
        assert!(frame.index().is_none());
        assert!(frame.intensity().is_none());
        assert!(frame.source_range().is_none());
        assert!(stored.read_component_into("missing", &mut array).is_err());
        assert!(array.pixels().is_none());
    }
}
#[test]
fn invalid_sparse_arrays_cannot_hide_pointer_index_or_model_inconsistency() {
    let mut dense = FrameBuffer::default();
    for (name, case) in manifest()["invalid"].as_object().unwrap() {
        let path = fixture(name);
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
            case["file_sha256"].as_str().unwrap()
        );
        if case["failure"] == "open" {
            assert!(Reader::open(&path, Options::default()).is_err(), "{name}");
        } else {
            let reader = Reader::open(&path, derived()).unwrap();
            assert!(reader.read_frame_into(0, &mut dense).is_err(), "{name}");
            assert!(dense.pixels().is_none());
            let mut original = SparseFrameBuffer::default();
            reader.read_sparse_frame_into(0, &mut original).unwrap();
        }
    }
}
#[test]
fn exact_event_order_extrema_and_mask_override_do_not_pass_through_float_conversion() {
    let reader = Reader::open(fixture("signed-model.h5"), Options::default()).unwrap();
    let mut frame = SparseFrameBuffer::default();
    reader.read_sparse_frame_into(0, &mut frame).unwrap();
    assert_eq!(frame.index(), Some(&PixelBuffer::U32(vec![6, 1])));
    assert_eq!(frame.intensity(), Some(&PixelBuffer::I16(vec![-7, 123])));
    let reader = Reader::open(fixture("uint64-events.h5"), derived()).unwrap();
    let mut dense = FrameBuffer::default();
    reader.read_frame_into(0, &mut dense).unwrap();
    assert_eq!(
        dense.pixels(),
        Some(&PixelBuffer::U64(vec![0, u64::MAX, 1 << 63, (1 << 53) + 1]))
    );
}
#[test]
fn limits_are_checked_for_source_arrays_events_chunks_and_decoded_components() {
    let path = fixture("signed-model.h5");
    let limits = Limits::default();
    for limited in [
        Limits {
            events: 3,
            ..limits
        },
        Limits {
            component_bytes: 1,
            ..limits
        },
        Limits {
            hdf5: dioptrin_io::hdf5::typed::Limits {
                frames: 1,
                ..limits.hdf5
            },
            ..limits
        },
        Limits {
            hdf5: dioptrin_io::hdf5::typed::Limits {
                frame_bytes: 1,
                ..limits.hdf5
            },
            ..limits
        },
        Limits {
            hdf5: dioptrin_io::hdf5::typed::Limits {
                chunk_bytes: 1,
                ..limits.hdf5
            },
            ..limits
        },
        Limits {
            hdf5: dioptrin_io::hdf5::typed::Limits {
                metadata_bytes: 1,
                ..limits.hdf5
            },
            ..limits
        },
        Limits {
            hdf5: dioptrin_io::hdf5::typed::Limits {
                datasets: 1,
                ..limits.hdf5
            },
            ..limits
        },
        Limits {
            hdf5: dioptrin_io::hdf5::typed::Limits {
                group_members: 1,
                ..limits.hdf5
            },
            ..limits
        },
        Limits {
            hdf5: dioptrin_io::hdf5::typed::Limits {
                text_bytes: 1,
                ..limits.hdf5
            },
            ..limits
        },
    ] {
        assert!(
            Reader::open(
                &path,
                Options {
                    limits: limited,
                    ..Options::default()
                }
            )
            .is_err(),
            "{limited:?}"
        );
    }
    // A complete component can be larger than the independently admitted frame
    // slices; whole-component requests fail without invalidating sparse reads.
    let options = Options {
        limits: Limits {
            component_bytes: 4,
            ..limits
        },
        ..Options::default()
    };
    let reader = Reader::open(fixture("float-events-le.h5"), options).unwrap();
    let mut array = ArrayBuffer::default();
    assert!(reader.read_component_into("mask", &mut array).is_err());
    assert!(array.pixels().is_none());
    let mut frame = SparseFrameBuffer::default();
    reader.read_sparse_frame_into(0, &mut frame).unwrap();
}
#[test]
fn source_mutation_invalidates_reused_sparse_dense_and_component_views() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.h5");
    std::fs::copy(fixture("signed-model.h5"), &path).unwrap();
    let reader = Reader::open(&path, derived()).unwrap();
    let mut sparse = SparseFrameBuffer::default();
    let mut dense = FrameBuffer::default();
    let mut component = ArrayBuffer::default();
    reader.read_sparse_frame_into(0, &mut sparse).unwrap();
    reader.read_frame_into(0, &mut dense).unwrap();
    reader.read_component_into("mask", &mut component).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(128)
        .unwrap();
    assert!(reader.read_sparse_frame_into(0, &mut sparse).is_err());
    assert!(sparse.intensity().is_none());
    assert!(reader.read_frame_into(0, &mut dense).is_err());
    assert!(dense.pixels().is_none());
    assert!(reader.read_component_into("mask", &mut component).is_err());
    assert!(component.pixels().is_none());
}
#[test]
fn intermediate_external_links_require_opt_in_and_every_original_source_stays_tracked() {
    assert!(Reader::open(fixture("external-master.h5"), Options::default()).is_err());
    let temp = tempfile::tempdir().unwrap();
    for name in manifest()["links"].as_object().unwrap().keys() {
        std::fs::copy(fixture(name), temp.path().join(name)).unwrap();
    }
    let path = temp.path().join("external-master.h5");
    let reader = Reader::open(
        &path,
        Options {
            external_links: ExternalLinks::FollowSnapshot,
            ..derived()
        },
    )
    .unwrap();
    assert_eq!(reader.sources().len(), 3);
    assert_eq!(reader.links().iter().filter(|l| l.external).count(), 2);
    let mut frame = SparseFrameBuffer::default();
    reader.read_sparse_frame_into(0, &mut frame).unwrap();
    assert_eq!(frame.intensity(), Some(&PixelBuffer::U16(vec![11, 22])));
    // Replacing an intermediate link file cannot silently redirect later reads.
    let relay = temp.path().join("external-relay.h5");
    let changed = common::mutate_source(&relay, || {
        std::fs::rename(&relay, temp.path().join("old-relay.h5"))?;
        std::fs::copy(temp.path().join("old-relay.h5"), &relay).map(|_| ())
    });
    assert_eq!(
        reader.read_sparse_frame_into(0, &mut frame).is_err(),
        changed
    );
    assert_eq!(frame.index().is_none(), changed);
}
#[test]
fn parallel_sparse_reads_use_independent_output_without_stale_dense_caches() {
    let reader = Reader::open(fixture("signed-model-le.h5"), derived()).unwrap();
    std::thread::scope(|scope| {
        let mut tasks = Vec::new();
        for i in 0..8 {
            let reader = &reader;
            tasks.push(scope.spawn(move || {
                let mut frame = SparseFrameBuffer::default();
                let mut dense = FrameBuffer::default();
                for _ in 0..4 {
                    reader.read_sparse_frame_into(i % 2, &mut frame).unwrap();
                    reader.read_frame_into(i % 2, &mut dense).unwrap();
                }
                hash(dense.pixels().unwrap())
            }));
        }
        for (i, task) in tasks.into_iter().enumerate() {
            let manifest = manifest();
            assert_eq!(
                task.join().unwrap(),
                manifest["files"]["signed-model-le.h5"]["dense"][i % 2]["sha256"]
                    .as_str()
                    .unwrap()
            );
        }
    });
}

#[test]
fn growing_control_arrays_invalidate_every_frozen_sparse_view() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("growing.h5");
    std::fs::copy(fixture("growing.h5"), &path).unwrap();
    let writer = hdf5_backend::File::open_rw(&path).unwrap();
    let reader = Reader::open(&path, derived()).unwrap();
    let mut sparse = SparseFrameBuffer::default();
    let mut dense = FrameBuffer::default();
    let mut component = ArrayBuffer::default();
    reader.read_sparse_frame_into(0, &mut sparse).unwrap();
    reader.read_frame_into(0, &mut dense).unwrap();
    reader.read_component_into("mask", &mut component).unwrap();
    writer
        .dataset("/entry/data/frame_ptr")
        .unwrap()
        .resize((4,))
        .unwrap();
    assert!(reader.read_sparse_frame_into(0, &mut sparse).is_err());
    assert!(sparse.intensity().is_none());
    assert!(reader.read_frame_into(0, &mut dense).is_err());
    assert!(dense.pixels().is_none());
    assert!(reader.read_component_into("mask", &mut component).is_err());
    assert!(component.pixels().is_none());
}

#[test]
fn missing_external_sparse_sources_are_never_replaced_by_another_local_dataset() {
    assert!(
        Reader::open(
            fixture("invalid-missing-link.h5"),
            Options {
                external_links: ExternalLinks::FollowSnapshot,
                ..Options::default()
            }
        )
        .is_err()
    );
}

#[test]
fn retained_empty_string_vectors_are_charged_for_both_metadata_copies() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("text.h5");
    std::fs::copy(fixture("float-events-le.h5"), &path).unwrap();
    {
        let file = hdf5_backend::File::open_rw(&path).unwrap();
        let values = vec![hdf5_backend::types::VarLenUnicode::new(); 4000];
        file.new_dataset_builder()
            .with_data(&values)
            .create("/entry/data/text_vector")
            .unwrap();
    }
    let limits = Limits {
        hdf5: dioptrin_io::hdf5::typed::Limits {
            metadata_bytes: 128 * 1024,
            ..Limits::default().hdf5
        },
        ..Limits::default()
    };
    assert!(
        Reader::open(
            &path,
            Options {
                limits,
                ..Options::default()
            }
        )
        .is_err()
    );
}
