//! Full-file validation and read-to-Dioptrin benchmark; no Python runtime.
use dioptrin::{
    geometry::Geometry,
    integration::{IntegrationMethod, Integrator, RadialUnit},
    mask::Mask,
    poni::Poni,
};
use dioptrin_io::{
    PixelBuffer,
    hdf5::{
        Limits, Reader,
        raw::{FrameBuffer, PipelineOptions, RawReader},
    },
};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    fs,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
thread_local! {
    static FLIPPED: RefCell<PixelBuffer> = RefCell::new(PixelBuffer::U32(Vec::new()));
    static POOL: rayon::ThreadPool = rayon::ThreadPoolBuilder::new().num_threads(1).use_current_thread().build().unwrap();
}
fn consume(
    integ: &Integrator,
    i: usize,
    shape: [usize; 2],
    pixels: &PixelBuffer,
    errors: bool,
    results: &Mutex<Vec<(usize, Vec<f64>, Vec<f64>)>>,
) {
    FLIPPED.with(|b| {
        let mut b = b.borrow_mut();
        let PixelBuffer::U32(out) = &mut *b else {
            unreachable!()
        };
        let PixelBuffer::U32(input) = pixels else {
            panic!("expected u32")
        };
        out.resize(input.len(), 0);
        for (source, dest) in input
            .chunks_exact(shape[1])
            .zip(out.rchunks_exact_mut(shape[1]))
        {
            dest.copy_from_slice(source);
        }
        let samples: &PixelBuffer = &b;
        let (_, y, e) = POOL
            .with(|p| {
                p.install(|| {
                    if errors {
                        integ.integrate_native1d::<true>(samples, shape, 2000)
                    } else {
                        integ.integrate_native1d::<false>(samples, shape, 2000)
                    }
                })
            })
            .unwrap();
        results.lock().unwrap().push((i, y, e));
    });
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 7 {
        return Err(
            "usage: eiger-bench FILE PONI MASK_U8 COUNT_REFERENCE_JSON OUTPUT_DIR WORKERS".into(),
        );
    }
    let workers: usize = args[6].parse()?;
    let dataset = "entry_0000/instrument/eiger/data";
    let before = Instant::now();
    let raw = RawReader::open_snapshot(&args[1], dataset, Limits::default())?;
    let index_seconds = before.elapsed().as_secs_f64();
    let [frames, h, w] = raw.shape();
    let reference: serde_json::Value = serde_json::from_slice(&fs::read(&args[4])?)?;
    let mut buffer = FrameBuffer::default();
    for i in 0..frames {
        raw.read_frame_into(i, &mut buffer)?;
        let Some(PixelBuffer::U32(v)) = buffer.pixels() else {
            panic!("dtype")
        };
        let hash = format!("{:x}", Sha256::digest(bytemuck::cast_slice(v)));
        assert_eq!(
            hash,
            reference["frame_sha256"][i].as_str().unwrap(),
            "count mismatch frame {i}"
        );
    }
    eprintln!("All {frames} original frame hashes match independent h5py");
    let mask = fs::read(&args[3])?;
    assert_eq!(mask.len(), h * w);
    let integ = Integrator::new(Geometry::new(h, w, Poni::load(&args[2])?))
        .with_method(IntegrationMethod::PixelSplit)
        .with_radial_unit(RadialUnit::TthDeg)
        .with_polarization_factor(0.965)
        .with_fast_math(true)
        .with_mask(Mask {
            data: mask,
            dim1: h,
            dim2: w,
        });
    let fallback = Reader::open(&args[1], dataset, Limits::default())?;
    let mut timings = Vec::new();
    fs::create_dir_all(&args[5])?;
    for errors in [false, true] {
        // Each workflow gets a complete warmup before alternating timed passes.
        for pass in 0..4 {
            for kind in ["library", "raw"] {
                let results = Mutex::new(Vec::with_capacity(frames));
                let start = Instant::now();
                if kind == "raw" {
                    raw.for_each_frame(
                        0..frames,
                        PipelineOptions {
                            workers,
                            memory_bytes: 2 * 1024 * 1024 * 1024,
                        },
                        |i, shape, pixels| {
                            consume(&integ, i, shape, pixels, errors, &results);
                            Ok(())
                        },
                    )?;
                } else {
                    let next = AtomicUsize::new(0);
                    std::thread::scope(|s| {
                        let mut handles = Vec::new();
                        for _ in 0..workers {
                            let (fallback, integ, next, results) =
                                (&fallback, &integ, &next, &results);
                            handles.push(s.spawn(move || {
                                loop {
                                    let i = next.fetch_add(1, Ordering::Relaxed);
                                    if i >= frames {
                                        break;
                                    }
                                    let frame = fallback.read_frame(i).unwrap();
                                    consume(
                                        integ,
                                        i,
                                        [h, w],
                                        &PixelBuffer::U32(frame.pixels),
                                        errors,
                                        results,
                                    );
                                }
                            }));
                        }
                        for handle in handles {
                            handle.join().unwrap();
                        }
                    });
                }
                let seconds = start.elapsed().as_secs_f64();
                let mut results = results.into_inner().unwrap();
                results.sort_by_key(|x| x.0);
                assert_eq!(results.len(), frames);
                if pass == 0 {
                    let suffix = if errors { "poisson" } else { "intensity" };
                    let y: Vec<f64> = results.iter().flat_map(|r| r.1.iter().copied()).collect();
                    fs::write(
                        format!("{}/{kind}-{suffix}.f64", args[5]),
                        bytemuck::cast_slice(&y),
                    )?;
                    if errors {
                        let e: Vec<f64> =
                            results.iter().flat_map(|r| r.2.iter().copied()).collect();
                        fs::write(
                            format!("{}/{kind}-std.f64", args[5]),
                            bytemuck::cast_slice(&e),
                        )?;
                    }
                } else {
                    timings.push(serde_json::json!({"reader":kind,"poisson":errors,"pass":pass,"seconds":seconds,"fps":frames as f64/seconds}));
                    eprintln!(
                        "{kind} poisson={errors} pass={pass}: {:.2} fps",
                        frames as f64 / seconds
                    );
                }
            }
        }
    }
    fs::write(
        format!("{}/timings.json", args[5]),
        serde_json::to_vec_pretty(
            &serde_json::json!({"frames":frames,"workers":workers,"index_seconds":index_seconds,"worker_decoder_payload_bound":raw.worker_payload_bytes(),"extra_u32_orientation_buffer_per_worker":h*w*4,"timings":timings}),
        )?,
    )?;
    Ok(())
}
