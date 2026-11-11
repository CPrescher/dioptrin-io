#![cfg(feature = "hdf5")]
use dioptrin_io::{
    PixelBuffer,
    hdf5::{
        Limits, Reader,
        raw::{
            BlockBuffer, FrameBuffer, Lz4Backend, ParallelBlockBuffer, PipelineOptions, PixelBlock,
            RawReader, UnshuffleBackend,
        },
    },
};
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "dioptrin-io-raw-{}-{}.h5",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn independent() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/counts-bitshuffle-lz4.h5")
}

#[test]
fn codec_backends_preserve_independent_counts_and_existing_endian_admission() {
    use sha2::{Digest, Sha256};
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/codec-backends");
    let source = root.join("counts.h5");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&source).unwrap())),
        manifest["sha256"].as_str().unwrap()
    );
    fn bytes(block: PixelBlock<'_>) -> Vec<u8> {
        match block {
            PixelBlock::U16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            PixelBlock::U32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        }
    }
    for case in manifest["cases"].as_array().unwrap() {
        // The existing legacy Reader used by RawReader admits native-endian
        // scalar types only. Keep its rejection contract; codec work must not
        // silently broaden which source types this entry point accepts.
        let native_order = if cfg!(target_endian = "little") {
            "<"
        } else {
            ">"
        };
        if case["byte_order"].as_str().unwrap() != native_order {
            assert!(
                RawReader::open_snapshot(
                    &source,
                    case["dataset"].as_str().unwrap(),
                    Limits::default()
                )
                .is_err()
            );
            continue;
        }
        for backend in [
            UnshuffleBackend::Scalar,
            UnshuffleBackend::Sse2,
            UnshuffleBackend::Avx2,
            UnshuffleBackend::Neon,
        ]
        .into_iter()
        .filter(|b| b.is_available())
        {
            for lz4 in [Lz4Backend::Rust, Lz4Backend::Native] {
                let reader = RawReader::open_snapshot(
                    &source,
                    case["dataset"].as_str().unwrap(),
                    Limits::default(),
                )
                .unwrap()
                .with_unshuffle_backend(backend)
                .unwrap()
                .with_lz4_backend(lz4);
                let mut frame = FrameBuffer::default();
                let mut blocks = BlockBuffer::default();
                let mut parallel = ParallelBlockBuffer::default();
                for i in [0, 1, 0] {
                    reader.read_frame_into(i, &mut frame).unwrap();
                    let decoded = match frame.pixels().unwrap() {
                        PixelBuffer::U16(v) => bytes(PixelBlock::U16(v)),
                        PixelBuffer::U32(v) => bytes(PixelBlock::U32(v)),
                        _ => panic!("original dtype changed"),
                    };
                    let expected = case["frame_sha256_le"][i].as_str().unwrap();
                    assert_eq!(format!("{:x}", Sha256::digest(&decoded)), expected);
                    let mut streamed = Vec::new();
                    reader
                        .read_frame_blocks(i, &mut blocks, |offset, block| {
                            assert_eq!(
                                offset * case["width"].as_u64().unwrap() as usize,
                                streamed.len()
                            );
                            streamed.extend(bytes(block));
                            Ok(())
                        })
                        .unwrap();
                    assert_eq!(streamed, decoded);
                    let states = reader
                        .fold_frame_blocks(
                            i,
                            &mut parallel,
                            PipelineOptions {
                                workers: 8,
                                memory_bytes: 1024 * 1024,
                            },
                            |_, _| Ok(Vec::new()),
                            |state, _, block| {
                                state.extend(bytes(block));
                                Ok(())
                            },
                        )
                        .unwrap();
                    assert_eq!(states.into_iter().flatten().collect::<Vec<u8>>(), decoded);
                }
            }
        }
    }
}

#[test]
fn independent_counts_are_native_reusable_and_parallel_without_orientation_changes() {
    let raw = RawReader::open_snapshot(independent(), "detector/data", Limits::default()).unwrap();
    let fallback = Reader::open(independent(), "detector/data", Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    for i in [0, 1, 0] {
        raw.read_frame_into(i, &mut buffer).unwrap();
        assert_eq!(buffer.shape(), Some([3, 4]));
        assert_eq!(
            buffer.pixels(),
            Some(&PixelBuffer::U32(fallback.read_frame(i).unwrap().pixels))
        );
    }
    assert!(raw.read_frame_into(2, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    let seen = Mutex::new(Vec::new());
    raw.for_each_frame(
        0..2,
        PipelineOptions {
            workers: 2,
            memory_bytes: 1024 * 1024,
        },
        |i, shape, pixels| {
            assert_eq!(shape, [3, 4]);
            assert_eq!(
                pixels,
                &PixelBuffer::U32(fallback.read_frame(i).unwrap().pixels)
            );
            seen.lock().unwrap().push(i);
            Ok(())
        },
    )
    .unwrap();
    seen.lock().unwrap().sort();
    assert_eq!(*seen.lock().unwrap(), [0, 1]);
    assert!(
        raw.for_each_frame(
            0..2,
            PipelineOptions {
                workers: 2,
                memory_bytes: 1
            },
            |_, _, _| Ok(())
        )
        .is_err()
    );
    assert!(
        raw.for_each_frame(
            0..2,
            PipelineOptions {
                workers: 0,
                memory_bytes: 1024
            },
            |_, _, _| Ok(())
        )
        .is_err()
    );
}

#[test]
fn u16_raw_and_bitshuffle_blocks_tails_and_sentinels_match_library() {
    hdf5_backend::sync::sync(hdf5_bitshuffle::register_bitshuffle_plugin);
    for count in [1, 7, 8, 9, 2047, 2048, 2049, 8193] {
        for filtered in [false, true] {
            let fixture = Fixture::new();
            let values: Vec<u16> = (0..2 * count).map(|i| (i * 257) as u16).collect();
            {
                let f = hdf5_backend::File::create(&fixture.0).unwrap();
                let mut builder = f
                    .new_dataset::<u16>()
                    .shape((2, 1, count))
                    .chunk((1, 1, count));
                if filtered {
                    builder =
                        builder.set_filters(&[hdf5_backend::filters::Filter::user(32008, &[0, 2])]);
                }
                builder.create("data").unwrap().write_raw(&values).unwrap();
            }
            let reader = RawReader::open_snapshot(&fixture.0, "data", Limits::default()).unwrap();
            let mut buffer = FrameBuffer::default();
            let mut blocks = BlockBuffer::default();
            for i in 0..2 {
                let mut restored = Vec::new();
                reader
                    .read_frame_blocks(i, &mut blocks, |offset, block| {
                        assert_eq!(offset, restored.len());
                        let PixelBlock::U16(samples) = block else {
                            panic!("dtype")
                        };
                        restored.extend_from_slice(samples);
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(restored, values[i * count..(i + 1) * count]);
                assert_eq!(blocks.shape(), Some([1, count]));
                if count == 8193 {
                    let mut parallel = ParallelBlockBuffer::default();
                    let states = reader
                        .fold_frame_blocks(
                            i,
                            &mut parallel,
                            PipelineOptions {
                                workers: 8,
                                memory_bytes: 1024 * 1024,
                            },
                            |_, range| Ok((range, Vec::<u16>::new())),
                            |(range, restored), offset, block| {
                                assert_eq!(offset, range.start + restored.len());
                                let PixelBlock::U16(v) = block else {
                                    panic!("dtype");
                                };
                                restored.extend_from_slice(v);
                                Ok(())
                            },
                        )
                        .unwrap();
                    assert_eq!(
                        states.into_iter().flat_map(|(_, x)| x).collect::<Vec<_>>(),
                        values[i * count..(i + 1) * count]
                    );
                }

                reader.read_frame_into(i, &mut buffer).unwrap();
                assert_eq!(
                    buffer.pixels(),
                    Some(&PixelBuffer::U16(
                        values[i * count..(i + 1) * count].to_vec()
                    ))
                );
            }
        }
    }
}

#[test]
fn unsupported_layouts_filters_missing_chunks_and_external_sources_fail_explicitly() {
    let fixture = Fixture::new();
    {
        let f = hdf5_backend::File::create(&fixture.0).unwrap();
        for (name, chunk) in [
            ("multiframe", (2, 2, 3)),
            ("partial", (1, 1, 3)),
            ("missing", (1, 2, 3)),
        ] {
            f.new_dataset::<u32>()
                .shape((2, 2, 3))
                .chunk(chunk)
                .create(name)
                .unwrap();
        }
        f.new_dataset::<u32>()
            .shape((2, 2, 3))
            .chunk((1, 2, 3))
            .deflate(1)
            .create("gzip")
            .unwrap()
            .write_raw(&[1; 12])
            .unwrap();
        f.link_external(independent().to_str().unwrap(), "/detector/data", "foreign")
            .unwrap();
    }
    for dataset in ["multiframe", "partial", "missing", "gzip", "foreign"] {
        assert!(
            RawReader::open_snapshot(&fixture.0, dataset, Limits::default()).is_err(),
            "{dataset}"
        );
    }
}

#[test]
fn source_mutation_and_truncation_invalidate_reused_output() {
    use std::io::Write;
    let fixture = Fixture::new();
    std::fs::copy(independent(), &fixture.0).unwrap();
    let reader = RawReader::open_snapshot(&fixture.0, "detector/data", Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    // Close no HDF5 handles are retained by the raw reader. Appending is allowed
    // by OS sharing modes and must never return stale indexed pixels.
    std::fs::OpenOptions::new()
        .append(true)
        .open(&fixture.0)
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    assert!(
        reader
            .read_frame_into(0, &mut buffer)
            .unwrap_err()
            .to_string()
            .contains("source")
    );
    assert!(buffer.pixels().is_none());
}

#[test]
fn independent_compressed_payload_with_default_block_and_skipped_filter_is_exact() {
    // This test reads through HDF5 directly before constructing our reader.
    // Register first; neither test order nor installed external plugins may
    // supply the filter implicitly.
    hdf5_backend::sync::sync(hdf5_bitshuffle::register_bitshuffle_plugin);
    use std::ffi::c_void;
    use std::io::{Read, Seek, SeekFrom};
    unsafe extern "C" {
        fn H5Dwrite_chunk(
            dataset: i64,
            dxpl: i64,
            filter_mask: u32,
            offset: *const u64,
            size: usize,
            data: *const c_void,
        ) -> i32;
    }
    let source = hdf5_backend::File::open(independent()).unwrap();
    let data = source.dataset("detector/data").unwrap();
    let info = data.chunk_info(0).unwrap();
    let counts = data.read_raw::<u32>().unwrap();
    let mut file = std::fs::File::open(independent()).unwrap();
    file.seek(SeekFrom::Start(info.addr)).unwrap();
    let mut compressed = vec![0; info.size as usize];
    file.read_exact(&mut compressed).unwrap();
    // Keep independent compressed payload; a zero header selects the same stable
    // codec default as the original explicit 8192-byte block field.
    compressed[8..12].fill(0);
    let fixture = Fixture::new();
    {
        let f = hdf5_backend::File::create(&fixture.0).unwrap();
        let ds = f
            .new_dataset::<u32>()
            .shape((2, 3, 4))
            .chunk((1, 3, 4))
            .set_filters(&[hdf5_backend::filters::Filter::user(32008, &[0, 2])])
            .create("data")
            .unwrap();
        let raw: Vec<u8> = counts[12..].iter().flat_map(|x| x.to_ne_bytes()).collect();
        for (i, mask, bytes) in [(0, 0, &compressed), (1, 1, &raw)] {
            let offset = [i, 0, 0];
            // SAFETY: live dataset, default property list, valid rank-3 offset
            // and disjoint slices with exactly the declared byte lengths. Same
            // upstream HDF5 mutex as all high-level calls; no unbounded decoder.
            let status = hdf5_backend::sync::sync(|| unsafe {
                H5Dwrite_chunk(
                    ds.id(),
                    0,
                    mask,
                    offset.as_ptr(),
                    bytes.len(),
                    bytes.as_ptr().cast(),
                )
            });
            assert_eq!(status, 0);
        }
    }
    let raw = RawReader::open_snapshot(&fixture.0, "data", Limits::default()).unwrap();
    let general = Reader::open(&fixture.0, "data", Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    for i in 0..2 {
        raw.read_frame_into(i, &mut buffer).unwrap();
        assert_eq!(
            buffer.pixels(),
            Some(&PixelBuffer::U32(counts[i * 12..(i + 1) * 12].to_vec()))
        );
        assert_eq!(
            general.read_frame(i).unwrap().pixels,
            counts[i * 12..(i + 1) * 12]
        );
    }
}

#[test]
fn streamed_independent_u32_counts_and_callback_failure_are_transactional() {
    let reader =
        RawReader::open_snapshot(independent(), "detector/data", Limits::default()).unwrap();
    let fallback = Reader::open(independent(), "detector/data", Limits::default()).unwrap();
    let mut buffer = BlockBuffer::default();
    for i in 0..2 {
        let mut samples = Vec::new();
        reader
            .read_frame_blocks(i, &mut buffer, |offset, block| {
                assert_eq!(offset, samples.len());
                let PixelBlock::U32(v) = block else {
                    panic!("dtype")
                };
                samples.extend_from_slice(v);
                Ok(())
            })
            .unwrap();
        assert_eq!(samples, fallback.read_frame(i).unwrap().pixels);
    }
    let mut called = 0;
    let error = reader
        .read_frame_blocks(0, &mut buffer, |_, _| {
            called += 1;
            Err(dioptrin_io::hdf5::Error::from(String::from(
                "consumer canceled",
            )))
        })
        .unwrap_err();
    assert!(error.to_string().contains("consumer canceled"));
    assert_eq!(called, 1);
    assert!(buffer.shape().is_none());
    assert!(
        reader
            .read_frame_blocks(2, &mut buffer, |_, _| Ok(()))
            .is_err()
    );
    assert!(buffer.shape().is_none());
    reader
        .read_frame_blocks(0, &mut buffer, |_, _| Ok(()))
        .unwrap();
    assert_eq!(buffer.shape(), Some([3, 4]));
}

#[test]
fn streamed_frame_is_invalidated_if_source_changes_during_callback() {
    use std::io::Write;
    let fixture = Fixture::new();
    std::fs::copy(independent(), &fixture.0).unwrap();
    let reader = RawReader::open_snapshot(&fixture.0, "detector/data", Limits::default()).unwrap();
    let mut buffer = BlockBuffer::default();
    let mut changed = false;
    let result = reader.read_frame_blocks(0, &mut buffer, |_, _| {
        if !changed {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&fixture.0)
                .unwrap()
                .write_all(&[0])
                .unwrap();
            changed = true;
        }
        Ok(())
    });
    assert!(changed && result.is_err());
    assert!(buffer.shape().is_none());
}

#[test]
fn corrupt_later_block_cannot_report_a_complete_frame() {
    use std::io::{Read, Seek, SeekFrom, Write};
    hdf5_backend::sync::sync(hdf5_bitshuffle::register_bitshuffle_plugin);
    let fixture = Fixture::new();
    let (address, size) = {
        let file = hdf5_backend::File::create(&fixture.0).unwrap();
        let data = file
            .new_dataset::<u32>()
            .shape((1, 1, 8193))
            .chunk((1, 1, 8193))
            .set_filters(&[hdf5_backend::filters::Filter::user(32008, &[0, 2])])
            .create("data")
            .unwrap();
        data.write_raw(&(0..8193_u32).collect::<Vec<_>>()).unwrap();
        let info = data.chunk_info(0).unwrap();
        (info.addr, info.size as usize)
    };
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&fixture.0)
        .unwrap();
    let mut encoded = vec![0; size];
    file.seek(SeekFrom::Start(address)).unwrap();
    file.read_exact(&mut encoded).unwrap();
    let mut position = 12;
    for _ in 0..3 {
        position +=
            4 + u32::from_be_bytes(encoded[position..position + 4].try_into().unwrap()) as usize;
    }
    file.seek(SeekFrom::Start(address + position as u64))
        .unwrap();
    file.write_all(&u32::MAX.to_be_bytes()).unwrap();
    drop(file);
    let reader = RawReader::open_snapshot(&fixture.0, "data", Limits::default()).unwrap();
    let mut buffer = BlockBuffer::default();
    let mut delivered = 0;
    let result = reader.read_frame_blocks(0, &mut buffer, |_, _| {
        delivered += 1;
        Ok(())
    });
    assert!(result.is_err() && delivered == 3);
    assert!(buffer.shape().is_none());
}

#[test]
fn parallel_blocks_preserve_order_counts_budget_and_failure_contracts() {
    let reader =
        RawReader::open_snapshot(independent(), "detector/data", Limits::default()).unwrap();
    let fallback = Reader::open(independent(), "detector/data", Limits::default()).unwrap();
    let mut buffer = ParallelBlockBuffer::default();
    for workers in [1, 8, 256] {
        for frame in 0..2 {
            let states = reader
                .fold_frame_blocks(
                    frame,
                    &mut buffer,
                    PipelineOptions {
                        workers,
                        memory_bytes: 1024 * 1024,
                    },
                    |_, range| Ok((range, Vec::<u32>::new())),
                    |(range, pixels), offset, block| {
                        assert_eq!(offset, range.start + pixels.len());
                        let PixelBlock::U32(values) = block else {
                            panic!("dtype");
                        };
                        pixels.extend_from_slice(values);
                        Ok(())
                    },
                )
                .unwrap();
            let mut next = 0;
            let mut all = Vec::new();
            for (range, values) in states {
                assert_eq!(range.start, next);
                assert_eq!(values.len(), range.len());
                next = range.end;
                all.extend(values);
            }
            assert_eq!(all, fallback.read_frame(frame).unwrap().pixels);
            assert_eq!(buffer.shape(), Some([3, 4]));
        }
    }
    for options in [
        PipelineOptions {
            workers: 0,
            memory_bytes: usize::MAX,
        },
        PipelineOptions {
            workers: 257,
            memory_bytes: usize::MAX,
        },
        PipelineOptions {
            workers: 8,
            memory_bytes: 1,
        },
    ] {
        assert!(
            reader
                .fold_frame_blocks(0, &mut buffer, options, |_, _| Ok(()), |_, _, _| Ok(()))
                .is_err()
        );
        assert!(buffer.shape().is_none());
    }
    assert!(
        reader
            .fold_frame_blocks(
                0,
                &mut buffer,
                PipelineOptions::default(),
                |_, _| Ok(()),
                |_, _, _| Err(String::from("cancel").into())
            )
            .is_err()
    );
    assert!(buffer.shape().is_none());
}

#[test]
fn parallel_block_failure_and_source_change_discard_all_states() {
    let fixture = Fixture::new();
    let data: Vec<u32> = (0..8193).map(|i| (i * 17) as u32).collect();
    let writer = hdf5_backend::File::create(&fixture.0).unwrap();
    let dataset = writer
        .new_dataset::<u32>()
        .shape((1, 1, data.len()))
        .chunk((1, 1, data.len()))
        .create("data")
        .unwrap();
    dataset.write_raw(&data).unwrap();
    drop(dataset);
    writer.flush().unwrap();
    drop(writer);
    let reader = RawReader::open_snapshot(&fixture.0, "data", Limits::default()).unwrap();
    let mut buffer = ParallelBlockBuffer::default();
    let states = reader
        .fold_frame_blocks(
            0,
            &mut buffer,
            PipelineOptions {
                workers: 8,
                memory_bytes: 1024 * 1024,
            },
            |_, range| Ok((range, Vec::<u32>::new())),
            |(range, pixels), offset, block| {
                assert_eq!(offset, range.start + pixels.len());
                let PixelBlock::U32(values) = block else {
                    panic!("dtype");
                };
                pixels.extend_from_slice(values);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(states.len(), 5); // five 8192-byte pieces, including the tail
    assert_eq!(
        states.into_iter().flat_map(|(_, x)| x).collect::<Vec<_>>(),
        data
    );
    let changed = std::sync::atomic::AtomicBool::new(false);
    let result = reader.fold_frame_blocks(
        0,
        &mut buffer,
        PipelineOptions {
            workers: 8,
            memory_bytes: 1024 * 1024,
        },
        |_, _| Ok(()),
        |_, _, _| {
            if !changed.swap(true, std::sync::atomic::Ordering::Relaxed) {
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&fixture.0)
                    .unwrap()
                    .write_all(&[0])
                    .unwrap();
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(buffer.shape().is_none());
}
