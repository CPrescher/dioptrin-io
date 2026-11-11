//! Matched native file-read/decode/integration comparisons; no Python in timing.
mod codecs;
use dioptrin::{
    geometry::Geometry,
    integration::{
        Fixed1dPlan, Fixed1dResult, Fixed1dWorker, InputOrder, IntegrationMethod, Integrator,
        RadialUnit,
    },
    mask::Mask,
    poni::Poni,
};
use dioptrin_io::{
    PixelBuffer,
    hdf5::{
        Limits,
        raw::{
            BlockBuffer, FrameBuffer, ParallelBlockBuffer, PipelineOptions, PixelBlock, RawReader,
        },
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

thread_local! {
    static COMPUTE: rayon::ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(1).use_current_thread().build().unwrap();
}
struct Worker<'a> {
    plan: &'a Fixed1dPlan,
    reader: &'a RawReader,
    frame: FrameBuffer,
    block: BlockBuffer,
    parallel: ParallelBlockBuffer,
    floats: Vec<f64>,
    sums: Fixed1dWorker<'a>,
    errors: bool,
}
impl<'a> Worker<'a> {
    fn new(plan: &'a Fixed1dPlan, reader: &'a RawReader, errors: bool) -> Self {
        COMPUTE.with(|_| {});
        Self {
            plan,
            reader,
            frame: FrameBuffer::default(),
            block: BlockBuffer::default(),
            parallel: ParallelBlockBuffer::default(),
            floats: Vec::new(),
            sums: plan.worker(errors, InputOrder::FlipVertical),
            errors,
        }
    }
    fn process(&mut self, mode: &str, frame: usize) -> Result<Fixed1dResult, String> {
        if let Some(count) = mode.strip_prefix("fused-blocks-") {
            let count: usize = count.parse().unwrap();
            let (plan, errors) = (self.plan, self.errors);
            let mut states = self
                .reader
                .fold_frame_blocks(
                    frame,
                    &mut self.parallel,
                    PipelineOptions {
                        workers: count,
                        memory_bytes: 512 * 1024 * 1024,
                    },
                    |_, range| {
                        let mut state = plan.worker(errors, InputOrder::FlipVertical);
                        state
                            .begin_range(range)
                            .map_err(dioptrin_io::hdf5::Error::from)?;
                        Ok(state)
                    },
                    |state, offset, block| {
                        let PixelBlock::U32(values) = block else {
                            return Err(String::from("expected u32").into());
                        };
                        state.push_u32_block(offset, values).map_err(Into::into)
                    },
                )
                .map_err(|e| e.to_string())?;
            return plan.finish_ranges(&mut states);
        }
        if mode == "fused-u32" {
            self.sums.begin_frame();
            let sums = &mut self.sums;
            if let Err(error) =
                self.reader
                    .read_frame_blocks(frame, &mut self.block, |offset, block| {
                        let PixelBlock::U32(values) = block else {
                            return Err(String::from("expected u32").into());
                        };
                        sums.push_u32_block(offset, values).map_err(Into::into)
                    })
            {
                self.sums.abort_frame();
                return Err(error.to_string());
            }
            return self.sums.finish_frame();
        }
        self.reader
            .read_frame_into(frame, &mut self.frame)
            .map_err(|e| e.to_string())?;
        let Some(PixelBuffer::U32(values)) = self.frame.pixels() else {
            return Err("expected u32".into());
        };
        if mode == "full-u32" {
            self.sums.begin_frame();
            self.sums.push_u32_block(0, values)?;
            return self.sums.finish_frame();
        }
        assert!(matches!(mode, "current-f64" | "current-f64-8"));
        let width = self.plan.shape().1;
        self.floats.resize(values.len(), 0.);
        for (input, output) in values
            .chunks_exact(width)
            .zip(self.floats.rchunks_exact_mut(width))
        {
            for (&x, y) in input.iter().zip(output) {
                *y = x as f64;
            }
        }
        if mode == "current-f64-8" {
            // Invoke outside Rayon: the existing kernel suppresses nested work.
            std::thread::scope(|scope| {
                scope
                    .spawn(|| self.plan.integrate_frame(&self.floats, self.errors))
                    .join()
                    .unwrap()
            })
        } else {
            COMPUTE
                .with(|pool| pool.install(|| self.plan.integrate_frame(&self.floats, self.errors)))
        }
    }

    fn payload(&self, mode: &str) -> usize {
        if mode.starts_with("fused-blocks-") {
            self.parallel.payload_capacity_bytes()
        } else if mode == "fused-u32" {
            self.block.payload_capacity_bytes()
        } else {
            self.frame.payload_capacity_bytes() + self.floats.capacity() * 8
        }
    }
}

type Results = Vec<(usize, Fixed1dResult)>;
fn run(
    plan: &Fixed1dPlan,
    reader: &RawReader,
    mode: &str,
    workers: usize,
    errors: bool,
) -> (Results, usize) {
    let next = AtomicUsize::new(0);
    let frames = reader.shape()[0];
    let results = Mutex::new(Vec::with_capacity(frames));
    let payload = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let (next, results, payload) = (&next, &results, &payload);
            scope.spawn(move || {
                let mut worker = Worker::new(plan, reader, errors);
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= frames {
                        break;
                    }
                    let result = worker.process(mode, i).unwrap();
                    results.lock().unwrap().push((i, result));
                }
                payload.fetch_add(worker.payload(mode), Ordering::Relaxed);
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|x| x.0);
    assert_eq!(results.len(), frames);
    (results, payload.load(Ordering::Relaxed))
}

fn compare(result: &Fixed1dResult, expected: &Fixed1dResult) -> f64 {
    assert_eq!(result.radial, expected.radial);
    let mut max_relative: f64 = 0.;
    for (a, b) in [Some(&result.intensity), result.errors.as_ref()]
        .into_iter()
        .zip([Some(&expected.intensity), expected.errors.as_ref()])
    {
        if let Some(a) = a {
            let b = b.unwrap();
            assert_eq!(a.len(), b.len());
            for (&x, &y) in a.iter().zip(b) {
                assert!(x.is_finite() && y.is_finite());
                assert!((x - y).abs() <= 1e-10 + 2e-12 * y.abs(), "{x} != {y}");
                max_relative = max_relative.max((x - y).abs() / y.abs().max(1.));
            }
        }
    }
    max_relative
}

fn save(path: &str, report: &Value) {
    fs::write(path, serde_json::to_vec_pretty(report).unwrap()).unwrap();
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 7 && args.len() != 8 {
        return Err("FILE PONI MASK_U8 COUNT_REFERENCE_JSON OUTPUT_JSON PASSES".into());
    }
    let passes: usize = args[6].parse()?;
    if args.get(7).is_some_and(|x| x == "single") {
        rayon::ThreadPoolBuilder::new()
            .num_threads(8)
            .build_global()?;
    }
    let start = Instant::now();
    let reader = RawReader::open_snapshot(
        &args[1],
        "entry_0000/instrument/eiger/data",
        Limits::default(),
    )?;
    let index_seconds = start.elapsed().as_secs_f64();
    let [frames, height, width] = reader.shape();
    let mask = fs::read(&args[3])?;
    assert_eq!(mask.len(), height * width);
    let integrator = Integrator::new(Geometry::new(height, width, Poni::load(&args[2])?))
        .with_method(IntegrationMethod::PixelSplit)
        .with_radial_unit(RadialUnit::TthDeg)
        .with_polarization_factor(0.965)
        .with_mask(Mask {
            data: mask,
            dim1: height,
            dim2: width,
        });
    let start = Instant::now();
    let plan = integrator.prepare_fixed1d((height, width), 2000)?;
    let setup_seconds = start.elapsed().as_secs_f64();
    eprintln!("index {index_seconds:.3}s; fixed float64 CSC setup {setup_seconds:.3}s");
    if args.get(7).is_some_and(|x| x == "codecs") {
        return codecs::benchmark(&plan, &args, passes);
    }
    if args.get(7).is_some_and(|x| x == "single") {
        return single_image(&plan, &reader, &args[5], passes);
    }
    let reference: Value = serde_json::from_slice(&fs::read(&args[4])?)?;
    let mut baseline = Worker::new(&plan, &reader, true);
    let mut expected = Vec::with_capacity(frames);
    let mut block_buffer = BlockBuffer::default();
    for i in 0..frames {
        let result = baseline.process("current-f64", i)?;
        let Some(PixelBuffer::U32(v)) = baseline.frame.pixels() else {
            unreachable!()
        };
        let hash = format!("{:x}", Sha256::digest(bytemuck::cast_slice(v)));
        assert_eq!(hash, reference["frame_sha256"][i].as_str().unwrap());
        let mut hash = Sha256::new();
        let mut next = 0;
        reader.read_frame_blocks(i, &mut block_buffer, |offset, block| {
            assert_eq!(offset, next);
            let PixelBlock::U32(v) = block else {
                unreachable!()
            };
            next += v.len();
            hash.update(bytemuck::cast_slice(v));
            Ok(())
        })?;
        assert_eq!(next, height * width);
        assert_eq!(
            format!("{:x}", hash.finalize()),
            reference["frame_sha256"][i].as_str().unwrap()
        );
        expected.push(result);
    }
    drop(baseline);
    drop(block_buffer);
    eprintln!(
        "All {frames} full-frame and streamed count hashes match independent h5py references"
    );
    let modes = ["current-f64", "full-u32", "fused-u32"];
    let mut report = json!({"file":args[1],"dataset":"entry_0000/instrument/eiger/data","shape":reader.shape(),
        "bins":2000,"method":"pixel_split","polarization":0.965,"mask":"fixed","weights":"float64", "sums":"float64",
        "backend":format!("{:?}",reader.unshuffle_backend()),"index_seconds":index_seconds,"plan_seconds":setup_seconds,
        "cache":"warm OS cache; no decoded-image cache", "independent_frame_hashes":frames,
        "input_order":"vertical flip mapped into the block consumer", "validation":[],"latency":[],"timings":[]});
    for workers in [1, 8] {
        for mode in modes {
            let (results, bytes) = run(&plan, &reader, mode, workers, true);
            let mut max: f64 = 0.;
            for (i, result) in results {
                max = max.max(compare(&result, &expected[i]));
            }
            report["validation"]
                .as_array_mut()
                .unwrap()
                .push(json!({"mode":mode,"workers":workers,"frames":frames,
                "max_scaled_error":max,"decoder_and_image_capacity_bytes":bytes}));
            eprintln!(
                "Validated {mode} workers={workers}: max scaled error {max:e}; payload {bytes}"
            );
            save(&args[5], &report);
        }
    }
    // Serial single-image latency: sampled acquired frames, no thread creation.
    for errors in [false, true] {
        let mut worker = Worker::new(&plan, &reader, errors);
        for sample in 0..24 {
            let frame = sample * (frames - 1) / 23;
            for j in 0..3 {
                let mode = modes[(sample + j) % 3];
                let start = Instant::now();
                let result = worker.process(mode, frame)?;
                let seconds = start.elapsed().as_secs_f64();
                compare(&result, &expected[frame]);
                report["latency"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"frame":frame,"mode":mode,"poisson":errors,"seconds":seconds}));
            }
        }
    }
    save(&args[5], &report);
    for errors in [false, true] {
        for workers in [1, 8] {
            for pass in 0..passes {
                for j in 0..3 {
                    let mode = modes[(pass + j) % 3];
                    let start = Instant::now();
                    let (results, bytes) = run(&plan, &reader, mode, workers, errors);
                    let seconds = start.elapsed().as_secs_f64();
                    // Validate EVERY timed result outside the timed interval.
                    let mut max: f64 = 0.;
                    for (i, result) in results {
                        max = max.max(compare(&result, &expected[i]));
                    }
                    report["timings"].as_array_mut().unwrap().push(
                        json!({"mode":mode,"workers":workers,
                        "poisson":errors,"pass":pass,"seconds":seconds,"fps":frames as f64/seconds,
                        "decoder_and_image_capacity_bytes":bytes,"max_scaled_error":max}),
                    );
                    eprintln!(
                        "{mode} workers={workers} poisson={errors} pass={pass}: {:.2} fps",
                        frames as f64 / seconds
                    );
                    save(&args[5], &report);
                }
            }
        }
    }
    Ok(())
}

fn single_image(
    plan: &Fixed1dPlan,
    reader: &RawReader,
    output: &str,
    passes: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let [frames, _, _] = reader.shape();
    let mut baseline = Worker::new(plan, reader, true);
    let mut scratch = ParallelBlockBuffer::default();
    let mut expected = Vec::new();
    let mut max: f64 = 0.;
    for frame in 0..frames {
        let target = baseline.process("current-f64", frame)?;
        let control = std::thread::scope(|scope| {
            scope
                .spawn(|| plan.integrate_frame(&baseline.floats, true))
                .join()
                .unwrap()
        })
        .unwrap();
        max = max.max(compare(&control, &target));
        let Some(PixelBuffer::U32(original)) = baseline.frame.pixels() else {
            unreachable!()
        };
        for count in [1, 8] {
            let mut states = reader.fold_frame_blocks(
                frame,
                &mut scratch,
                PipelineOptions {
                    workers: count,
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
                    let PixelBlock::U32(values) = block else {
                        unreachable!()
                    };
                    assert_eq!(values, &original[offset..offset + values.len()]);
                    state.push_u32_block(offset, values).map_err(Into::into)
                },
            )?;
            max = max.max(compare(&plan.finish_ranges(&mut states)?, &target));
        }
        expected.push(target);
    }
    eprintln!(
        "Parallel 1/8-core counts and float64/Poisson results validated for all {frames} frames; max scaled error {max:e}"
    );
    drop(baseline);
    drop(scratch);
    let modes = [
        "current-f64",
        "current-f64-8",
        "fused-u32",
        "fused-blocks-1",
        "fused-blocks-8",
    ];
    let mut report = json!({"scope":"one frame at a time; parallel blocks within each frame", "shape":reader.shape(),
        "backend":format!("{:?}",reader.unshuffle_backend()), "validation_frames":frames,
        "parallel_count_checks":"every block compared exactly with previously independently hash-validated full-frame decoder",
        "max_scaled_error":max,"passes":passes,"latency":[]});
    for errors in [false, true] {
        let mut worker = Worker::new(plan, reader, errors);
        for pass in 0..passes {
            for sample in 0..24 {
                let frame = sample * (frames - 1) / 23;
                for j in 0..modes.len() {
                    let mode = modes[(sample + pass + j) % modes.len()];
                    let start = Instant::now();
                    let result = worker.process(mode, frame)?;
                    let seconds = start.elapsed().as_secs_f64();
                    assert_eq!(result.errors.is_some(), errors);
                    compare(&result, &expected[frame]);
                    report["latency"].as_array_mut().unwrap().push(json!({"mode":mode,"frame":frame,"poisson":errors,
                        "pass":pass,"seconds":seconds,"decoder_and_image_capacity_bytes":worker.payload(mode)}));
                }
            }
            save(output, &report);
            eprintln!("Completed single-frame pass {pass}, poisson={errors}");
        }
    }
    save(output, &report);
    Ok(())
}
