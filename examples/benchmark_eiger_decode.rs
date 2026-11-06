//! Read/decode benchmark only: stored u32 pixels, no integration or conversion.
use dioptrin_io::{
    PixelBuffer,
    hdf5::{
        Limits,
        raw::{PipelineOptions, RawReader},
    },
};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6 {
        return Err(
            "usage: benchmark_eiger_decode FILE DATASET WORKERS bench|verify REFERENCE_JSON".into(),
        );
    }
    let workers: usize = args[3].parse()?;
    let verify = match args[4].as_str() {
        "verify" => true,
        "bench" => false,
        _ => return Err("mode must be bench or verify".into()),
    };
    let reference: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[5])?)?;
    let expected = reference["frame_sha256"]
        .as_array()
        .ok_or("missing reference hashes")?;
    let setup = Instant::now();
    let reader = RawReader::open_snapshot(&args[1], &args[2], Limits::default())?;
    let setup_seconds = setup.elapsed().as_secs_f64();
    let shape = reader.shape();
    if shape != [625, 3262, 3108] || expected.len() != shape[0] {
        return Err("this benchmark requires the complete reference Eiger9M scan".into());
    }
    let count = AtomicUsize::new(0);
    let hashes = Mutex::new(vec![String::new(); shape[0]]);
    let start = Instant::now();
    reader.for_each_frame(
        0..shape[0],
        PipelineOptions {
            workers,
            memory_bytes: 2 << 30,
        },
        |index, frame_shape, pixels| {
            let PixelBuffer::U32(values) = pixels else {
                return Err(dioptrin_io::hdf5::Error::from(
                    "expected native uint32 pixels".to_owned(),
                ));
            };
            if frame_shape != [shape[1], shape[2]] {
                return Err(dioptrin_io::hdf5::Error::from(
                    "frame shape changed".to_owned(),
                ));
            }
            if verify {
                let hash = format!("{:x}", Sha256::digest(bytemuck::cast_slice(values)));
                if expected[index].as_str() != Some(hash.as_str()) {
                    return Err(dioptrin_io::hdf5::Error::from(format!(
                        "frame {index} count hash differs"
                    )));
                }
                hashes.lock().unwrap()[index] = hash;
            } else {
                std::hint::black_box(values.as_slice());
            }
            count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    )?;
    let seconds = start.elapsed().as_secs_f64();
    if count.load(Ordering::Relaxed) != shape[0] {
        return Err("incomplete scan".into());
    }
    println!(
        "{}",
        serde_json::json!({
            "workers": workers, "mode": args[4], "frames": shape[0], "shape": shape,
            "seconds": seconds, "fps": shape[0] as f64 / seconds,
            "setup_seconds": setup_seconds, "worker_payload_bytes": reader.worker_payload_bytes(),
            "frame_sha256": if verify { Some(hashes.into_inner().unwrap()) } else { None },
        })
    );
    Ok(())
}
