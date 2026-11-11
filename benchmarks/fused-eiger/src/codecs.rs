//! Factorial decoder comparison on one unchanged file and consumer plan.
use super::*;
use dioptrin_io::hdf5::raw::{Lz4Backend, UnshuffleBackend};
use std::collections::BTreeMap;

fn decode(reader: &RawReader, workers: usize) {
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut buffer = FrameBuffer::default();
                    let mut count = 0;
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= reader.shape()[0] {
                            break;
                        }
                        reader.read_frame_into(i, &mut buffer).unwrap();
                        std::hint::black_box(buffer.pixels().unwrap());
                        count += 1;
                    }
                    count
                })
            })
            .collect();
        let count: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
        assert_eq!(count, reader.shape()[0]);
    });
}

fn digest(results: &Results) -> String {
    let mut hash = Sha256::new();
    for (_, result) in results {
        for values in [
            Some(&result.radial),
            Some(&result.intensity),
            result.errors.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            hash.update(bytemuck::cast_slice(values));
        }
    }
    format!("{:x}", hash.finalize())
}

pub(super) fn benchmark(
    plan: &Fixed1dPlan,
    args: &[String],
    passes: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if passes == 0 {
        return Err("positive pass count required".into());
    }
    // Preserve the dc6e303 Auto policy even if a winning alternative later
    // changes the production default. The baseline remains reproducible.
    let original = [
        UnshuffleBackend::Avx2,
        UnshuffleBackend::Neon,
        UnshuffleBackend::Sse2,
        UnshuffleBackend::Scalar,
    ]
    .into_iter()
    .find(|b| b.is_available())
    .unwrap();
    let variants = [
        ("main", original, Lz4Backend::Rust),
        ("native-lz4", original, Lz4Backend::Native),
    ];
    let readers: Vec<_> = variants
        .iter()
        .map(|(_, unshuffle, lz4)| {
            RawReader::open_snapshot(
                &args[1],
                "entry_0000/instrument/eiger/data",
                Limits::default(),
            )
            .and_then(|r| r.with_unshuffle_backend(*unshuffle))
            .map(|r| r.with_lz4_backend(*lz4))
        })
        .collect::<Result<_, _>>()?;
    let [frames, height, width] = readers[0].shape();
    let reference: Value = serde_json::from_slice(&fs::read(&args[4])?)?;
    assert_eq!(reference["frame_sha256"].as_array().unwrap().len(), frames);
    let mut report = json!({
        "file":args[1], "dataset":"entry_0000/instrument/eiger/data", "shape":readers[0].shape(),
        "source_sha256":reference["sha256"], "reference":args[4], "passes":passes,
        "bins":2000,"method":"pixel_split","polarization":0.965,"input_order":"FlipVertical",
        "arithmetic":"float64 weights, sums, normalization and Poisson terms",
        "cache":"warm OS file cache; every pass reads and decodes all frames; no decoded cache",
        "timing_scope":"reads, decompression, orientation mapping, integration, normalization, worker/buffer allocation and output collection; setup, validation and hashing excluded",
        "variants":variants.iter().zip(&readers).map(|((name,_,_),r)|json!({"name":name,"unshuffle":format!("{:?}",r.unshuffle_backend()),"lz4":format!("{:?}",r.lz4_backend())})).collect::<Vec<_>>(),
        "validation":[], "timings":[], "single_frame":[]
    });
    let mut reference_worker = Worker::new(plan, &readers[0], true);
    let mut expected = Vec::with_capacity(frames);
    for i in 0..frames {
        let result = reference_worker.process("current-f64", i)?;
        let Some(PixelBuffer::U32(v)) = reference_worker.frame.pixels() else {
            unreachable!()
        };
        assert_eq!(
            format!("{:x}", Sha256::digest(bytemuck::cast_slice(v))),
            reference["frame_sha256"][i].as_str().unwrap()
        );
        expected.push(result);
    }
    drop(reference_worker);
    eprintln!("Current-main float64 reference and all {frames} independent count hashes verified");
    for ((name, _, _), reader) in variants.iter().zip(&readers) {
        let mut full = Worker::new(plan, reader, true);
        let mut streamed = BlockBuffer::default();
        let mut parallel = ParallelBlockBuffer::default();
        let mut max: f64 = 0.;
        for i in 0..frames {
            let result = full.process("full-u32", i)?;
            max = max.max(compare(&result, &expected[i]));
            let Some(PixelBuffer::U32(original)) = full.frame.pixels() else {
                unreachable!()
            };
            assert_eq!(
                format!("{:x}", Sha256::digest(bytemuck::cast_slice(original))),
                reference["frame_sha256"][i].as_str().unwrap()
            );
            let mut hash = Sha256::new();
            let mut next = 0;
            reader.read_frame_blocks(i, &mut streamed, |offset, block| {
                let PixelBlock::U32(v) = block else {
                    unreachable!()
                };
                assert_eq!(offset, next);
                assert_eq!(v, &original[offset..offset + v.len()]);
                next += v.len();
                hash.update(bytemuck::cast_slice(v));
                Ok(())
            })?;
            assert_eq!(next, height * width);
            assert_eq!(
                format!("{:x}", hash.finalize()),
                reference["frame_sha256"][i].as_str().unwrap()
            );
            let mut states = reader.fold_frame_blocks(
                i,
                &mut parallel,
                PipelineOptions {
                    workers: 8,
                    memory_bytes: 512 * 1024 * 1024,
                },
                |_, range| {
                    let mut state = plan.worker(true, InputOrder::FlipVertical);
                    state
                        .begin_range(range)
                        .map_err(dioptrin_io::hdf5::Error::from)?;
                    Ok(state)
                },
                |state, offset, block| {
                    let PixelBlock::U32(v) = block else {
                        unreachable!()
                    };
                    assert_eq!(v, &original[offset..offset + v.len()]);
                    state.push_u32_block(offset, v).map_err(Into::into)
                },
            )?;
            max = max.max(compare(&plan.finish_ranges(&mut states)?, &expected[i]));
            if (i + 1) % 125 == 0 {
                eprintln!(
                    "{name}: validated full/stream/8-core counts and Poisson integration {}/{frames}",
                    i + 1
                );
            }
        }
        report["validation"].as_array_mut().unwrap().push(json!({"variant":name,"frames":frames,"routes":["full-frame","serial-blocks","parallel-blocks-8"],"all_independent_counts_match":true,"max_scaled_consumer_error":max}));
        save(&args[5], &report);
    }

    let mut output_hashes = BTreeMap::new();
    for (mode, errors) in [
        ("decode", false),
        ("full-u32", false),
        ("full-u32", true),
        ("fused-u32", false),
        ("fused-u32", true),
    ] {
        // Complete-file warmup for each variant before the alternating passes.
        for reader in &readers {
            if mode == "decode" {
                decode(reader, 8);
            } else {
                std::hint::black_box(run(plan, reader, mode, 8, errors));
            }
        }
        for pass in 0..passes {
            let mut order: Vec<_> = (0..variants.len())
                .map(|j| (pass + j) % variants.len())
                .collect();
            if pass % 2 == 1 && variants.len() > 2 {
                order.reverse();
            }
            for k in order {
                let start = Instant::now();
                let results = if mode == "decode" {
                    decode(&readers[k], 8);
                    None
                } else {
                    Some(run(plan, &readers[k], mode, 8, errors).0)
                };
                let seconds = start.elapsed().as_secs_f64();
                let mut hash = None;
                let mut max: f64 = 0.;
                if let Some(results) = results {
                    for (i, result) in &results {
                        max = max.max(compare(result, &expected[*i]));
                    }
                    let got = digest(&results);
                    let control = output_hashes
                        .entry((mode, errors))
                        .or_insert_with(|| got.clone());
                    assert_eq!(
                        &got, control,
                        "decoder variant changed complete consumer outputs"
                    );
                    hash = Some(got);
                }
                let fps = frames as f64 / seconds;
                report["timings"].as_array_mut().unwrap().push(json!({"variant":variants[k].0,"mode":mode,"workers":8,"poisson":errors,"pass":pass,"seconds":seconds,"fps":fps,"output_sha256":hash,"max_scaled_consumer_error":max}));
                eprintln!(
                    "{} {mode} poisson={errors} pass={pass}: {fps:.2} fps",
                    variants[k].0
                );
                save(&args[5], &report);
            }
        }
    }
    // One frame at a time: eight codec/integration workers within that frame.
    let mut single_hashes = BTreeMap::new();
    for errors in [false, true] {
        for pass in 0..passes {
            for sample in 0..24 {
                let frame = sample * (frames - 1) / 23;
                for j in 0..variants.len() {
                    let k = (pass + sample + j) % variants.len();
                    let mut worker = Worker::new(plan, &readers[k], errors);
                    // Warm the selected frame and worker, excluding its first allocation.
                    worker.process("fused-blocks-8", frame)?;
                    let start = Instant::now();
                    let result = worker.process("fused-blocks-8", frame)?;
                    let seconds = start.elapsed().as_secs_f64();
                    let max = compare(&result, &expected[frame]);
                    let got = digest(&vec![(frame, result)]);
                    let control = single_hashes
                        .entry((errors, frame))
                        .or_insert_with(|| got.clone());
                    assert_eq!(
                        &got, control,
                        "decoder variant changed within-frame consumer outputs"
                    );
                    report["single_frame"].as_array_mut().unwrap().push(json!({"variant":variants[k].0,"mode":"fused-blocks-8","poisson":errors,"pass":pass,"frame":frame,"seconds":seconds,"output_sha256":got,"max_scaled_consumer_error":max}));
                }
            }
            save(&args[5], &report);
        }
    }
    Ok(())
}
