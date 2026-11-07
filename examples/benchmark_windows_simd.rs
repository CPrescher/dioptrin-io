//! Matched native scalar/SIMD read+decode on deterministic synthetic 9M frames.
//! No detector data is uploaded. Run release mode; results are machine-local.
use dioptrin_io::{
    PixelBuffer,
    hdf5::{
        Limits,
        raw::{FrameBuffer, RawReader, UnshuffleBackend},
    },
};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    ffi::c_void,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
const ROWS: usize = 3262;
const COLS: usize = 3108;
const FRAMES: usize = 32;
const SCANS: usize = 4;
const REPEATS: usize = 5;
unsafe extern "C" {
    fn bshuf_compress_lz4_bound(size: usize, width: usize, block: usize) -> usize;
    fn bshuf_compress_lz4(
        input: *const c_void,
        output: *mut c_void,
        size: usize,
        width: usize,
        block: usize,
    ) -> i64;
    fn H5Dwrite_chunk(
        dataset: i64,
        dxpl: i64,
        mask: u32,
        offset: *const u64,
        size: usize,
        data: *const c_void,
    ) -> i32;
}
fn create(path: &std::path::Path, sparse: bool) -> Result<Vec<String>, Box<dyn Error>> {
    hdf5_backend::sync::sync(hdf5_bitshuffle::register_bitshuffle_plugin);
    let file = hdf5_backend::File::create(path)?;
    let ds = file
        .new_dataset::<u32>()
        .shape((FRAMES, ROWS, COLS))
        .chunk((1, ROWS, COLS))
        .set_filters(&[hdf5_backend::filters::Filter::user(32008, &[0, 2])])
        .create("data")?;
    let count = ROWS * COLS;
    let mut pixels = vec![0_u32; count];
    // SAFETY: positive supported width; trusted generated input of this size.
    let bound = unsafe { bshuf_compress_lz4_bound(count, 4, 0) };
    let mut encoded = vec![0_u8; bound + 12];
    let mut hashes = Vec::new();
    for frame in 0..FRAMES {
        for (i, pixel) in pixels.iter_mut().enumerate() {
            let mut x = (i as u32).wrapping_add((frame as u32).wrapping_mul(0x9E3779B9));
            x ^= x >> 16;
            x = x.wrapping_mul(0x7FEB352D);
            x ^= x >> 15;
            *pixel = if sparse {
                if x & 127 == 0 {
                    1 + ((x >> 8) % 2500)
                } else {
                    0
                }
            } else {
                (x >> 8) & 65535
            };
        }
        pixels[0] = u32::MAX;
        pixels[1] = 0;
        pixels[2] = 1 << 31;
        hashes.push(format!(
            "{:x}",
            Sha256::digest(bytemuck::cast_slice(&pixels))
        ));
        encoded[..8].copy_from_slice(&((count * 4) as u64).to_be_bytes());
        encoded[8..12].fill(0);
        // SAFETY: trusted native input, allocated codec-declared output bound.
        // This compression helper is benchmark generation only, never the raw
        // reader's untrusted compressed-input path (which stays checked Rust).
        let n = unsafe {
            bshuf_compress_lz4(
                pixels.as_ptr().cast(),
                encoded[12..].as_mut_ptr().cast(),
                count,
                4,
                0,
            )
        };
        if n < 0 || n as usize > bound {
            return Err("synthetic compression failed".into());
        }
        let offset = [frame as u64, 0, 0];
        // SAFETY: live rank-3 dataset, exact compressed extent, codec-compatible
        // header; HDF5 access serialized with the same upstream library mutex.
        let rc = hdf5_backend::sync::sync(|| unsafe {
            H5Dwrite_chunk(
                ds.id(),
                0,
                0,
                offset.as_ptr(),
                n as usize + 12,
                encoded.as_ptr().cast(),
            )
        });
        if rc < 0 {
            return Err("synthetic chunk write failed".into());
        }
    }
    file.flush()?;
    Ok(hashes)
}
fn pass(
    reader: &RawReader,
    workers: usize,
    scans: usize,
    hashes: Option<&[String]>,
) -> Result<f64, String> {
    if reader.worker_payload_bytes().saturating_mul(workers) > 2 << 30 {
        return Err("payload budget".into());
    }
    let next = AtomicUsize::new(0);
    let total = FRAMES * scans;
    let start = Instant::now();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| -> Result<usize, String> {
                    let mut buffer = FrameBuffer::default();
                    let mut done = 0;
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= total {
                            break;
                        }
                        let frame = i % FRAMES;
                        reader
                            .read_frame_into(frame, &mut buffer)
                            .map_err(|e| e.to_string())?;
                        let Some(PixelBuffer::U32(pixels)) = buffer.pixels() else {
                            return Err("wrong pixel type".into());
                        };
                        if let Some(hashes) = hashes {
                            if format!("{:x}", Sha256::digest(bytemuck::cast_slice(pixels)))
                                != hashes[frame]
                            {
                                return Err(format!("count mismatch in frame {frame}"));
                            }
                        } else {
                            std::hint::black_box(pixels.as_slice());
                        }
                        done += 1;
                    }
                    Ok(done)
                })
            })
            .collect();
        let mut count = 0;
        for handle in handles {
            count += handle.join().map_err(|_| "worker panicked")??;
        }
        if count != total {
            return Err("incomplete pass".to_owned());
        }
        Ok(())
    })?;
    Ok(start.elapsed().as_secs_f64())
}
fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args_os()
        .nth(1)
        .ok_or("usage: benchmark_windows_simd RESULTS_JSON")?;
    let temp = tempfile::tempdir()?;
    let mut samples = Vec::new();
    let mut validation = Vec::new();
    let mut sources = Vec::new();
    let backends: Vec<_> = [
        UnshuffleBackend::Scalar,
        UnshuffleBackend::Sse2,
        UnshuffleBackend::Avx2,
        UnshuffleBackend::Neon,
    ]
    .into_iter()
    .filter(|b| b.is_available())
    .collect();
    let mut selected = None;
    for (case, sparse) in [("sparse-1-in-128", true), ("dense-u16-range", false)] {
        let path = temp.path().join("synthetic.h5");
        let hashes = create(&path, sparse)?;
        let reader = RawReader::open_snapshot(&path, "data", Limits::default())?;
        selected = Some(format!("{:?}", reader.unshuffle_backend()));
        drop(reader);
        sources.push(serde_json::json!({"case":case,"file_bytes":std::fs::metadata(&path)?.len(),
            "ordered_counts_sha256":format!("{:x}",Sha256::digest(hashes.join(""))),"frame_count_sha256":hashes}));
        for &workers in &[1, 8] {
            for &backend in &backends {
                let reader = RawReader::open_snapshot(&path, "data", Limits::default())?
                    .with_unshuffle_backend(backend)?;
                pass(&reader, workers, 1, Some(&hashes))?;
                validation.push(serde_json::json!({"case":case,"workers":workers,"backend":format!("{backend:?}"),"frames":FRAMES,"all_counts_match":true}));
            }
        }
        for repeat in 0..REPEATS {
            for &workers in &[1, 8] {
                let order: Vec<_> = if repeat % 2 == 0 {
                    backends.clone()
                } else {
                    backends.iter().rev().copied().collect()
                };
                for backend in order {
                    let reader = RawReader::open_snapshot(&path, "data", Limits::default())?
                        .with_unshuffle_backend(backend)?;
                    let seconds = pass(&reader, workers, SCANS, None)?;
                    let fps = (FRAMES * SCANS) as f64 / seconds;
                    println!("{case} {backend:?} {workers} workers, repeat {repeat}: {fps:.2} fps");
                    samples.push(serde_json::json!({"case":case,"workers":workers,"backend":format!("{backend:?}"),"repeat":repeat,"seconds":seconds,"frames":FRAMES*SCANS,"fps":fps}));
                }
            }
        }
        std::fs::remove_file(path)?;
    }
    let mut summary = Vec::new();
    for case in ["sparse-1-in-128", "dense-u16-range"] {
        for workers in [1, 8] {
            for backend in &backends {
                let name = format!("{backend:?}");
                let mut fps: Vec<f64> = samples
                    .iter()
                    .filter(|s| {
                        s["case"] == case && s["workers"] == workers && s["backend"] == name
                    })
                    .map(|s| s["fps"].as_f64().unwrap())
                    .collect();
                fps.sort_by(f64::total_cmp);
                summary.push(serde_json::json!({"case":case,"workers":workers,"backend":name,"median_fps":fps[fps.len()/2],"min_fps":fps[0],"max_fps":fps[fps.len()-1]}));
            }
        }
    }
    let record = serde_json::json!({"workload":"deterministic synthetic uint32 frames, Eiger9M geometry; not original beamline data",
        "shape":[FRAMES,ROWS,COLS],"scans_per_pass":SCANS,"repeats":REPEATS,"logical_cpus":std::thread::available_parallelism()?.get(),
        "arch":std::env::consts::ARCH,"os":std::env::consts::OS,"processor":std::env::var("PROCESSOR_IDENTIFIER").ok(),"automatic_backend":selected,
        "boundaries":"Warm-cache positional file reads, checked Rust LZ4, unshuffle, source checks, thread/buffer creation, uint32 delivery included. Generation, opening/indexing and count hashing excluded. Fixed workers, no decoded cache. 8 workers may exceed hosted runner CPU allocation.",
        "sources":sources,"validation":validation,"samples":samples,"summary":summary});
    std::fs::write(output, serde_json::to_vec_pretty(&record)?)?;
    println!("{}", serde_json::to_string_pretty(&record["summary"])?);
    Ok(())
}
