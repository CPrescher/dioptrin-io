//! Synthetic plaintext oracle encoded by the existing upstream HDF5 codec.
use sha2::{Digest, Sha256};
use std::{error::Error, ffi::c_void, path::Path};

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // Standalone benchmarks use different subsets of these workloads.
pub enum Pattern {
    Sparse,
    Dense16,
    DenseFull,
}

#[derive(Clone, Copy, Debug)]
pub struct Case {
    pub name: &'static str,
    pub rows: usize,
    pub cols: usize,
    pub frames: usize,
    pub width: usize,
    pub pattern: Pattern,
}

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

pub fn create(path: &Path, case: Case) -> Result<Vec<String>, Box<dyn Error>> {
    hdf5_backend::sync::sync(hdf5_bitshuffle::register_bitshuffle_plugin);
    let file = hdf5_backend::File::create(path)?;
    let shape = (case.frames, case.rows, case.cols);
    let chunk = (1, case.rows, case.cols);
    let filter = [hdf5_backend::filters::Filter::user(32008, &[0, 2])];
    let ds = match case.width {
        2 => file
            .new_dataset::<u16>()
            .shape(shape)
            .chunk(chunk)
            .set_filters(&filter)
            .create("data")?,
        4 => file
            .new_dataset::<u32>()
            .shape(shape)
            .chunk(chunk)
            .set_filters(&filter)
            .create("data")?,
        _ => return Err("synthetic width must be 2 or 4".into()),
    };
    let count = case
        .rows
        .checked_mul(case.cols)
        .ok_or("synthetic shape overflow")?;
    if count < 3
        || count
            .checked_mul(case.width)
            .is_none_or(|n| n > 64 * 1024 * 1024)
    {
        return Err("synthetic fixture size outside budget".into());
    }
    let mut raw = vec![0_u8; count * case.width];
    // SAFETY: supported positive width and bounded trusted input size.
    let bound = unsafe { bshuf_compress_lz4_bound(count, case.width, 0) };
    let mut encoded = vec![0_u8; bound + 12];
    let mut hashes = Vec::new();
    for frame in 0..case.frames {
        for (i, bytes) in raw.chunks_exact_mut(case.width).enumerate() {
            let mut x = (i as u32).wrapping_add((frame as u32).wrapping_mul(0x9E3779B9));
            x ^= x >> 16;
            x = x.wrapping_mul(0x7FEB352D);
            x ^= x >> 15;
            let mut value = match case.pattern {
                Pattern::Sparse => {
                    if x & 127 == 0 {
                        1 + ((x >> 8) % 2500)
                    } else {
                        0
                    }
                }
                Pattern::Dense16 => (x >> 8) & 65535,
                Pattern::DenseFull => x,
            };
            value = match i {
                0 => {
                    if case.width == 2 {
                        u16::MAX as u32
                    } else {
                        u32::MAX
                    }
                }
                1 => 0,
                2 => 1 << (case.width * 8 - 1),
                _ => value,
            };
            match case.width {
                2 => bytes.copy_from_slice(&(value as u16).to_ne_bytes()),
                4 => bytes.copy_from_slice(&value.to_ne_bytes()),
                _ => unreachable!(),
            }
        }
        hashes.push(format!("{:x}", Sha256::digest(&raw)));
        encoded[..8].copy_from_slice(&(raw.len() as u64).to_be_bytes());
        encoded[8..12].fill(0);
        // SAFETY: trusted plaintext and allocated upstream-declared capacity.
        let size = unsafe {
            bshuf_compress_lz4(
                raw.as_ptr().cast(),
                encoded[12..].as_mut_ptr().cast(),
                count,
                case.width,
                0,
            )
        };
        if size < 0 || size as usize > bound {
            return Err(
                format!("stock fixture encoder failed: {} frame {frame}", case.name).into(),
            );
        }
        let offset = [frame as u64, 0, 0];
        // SAFETY: exact rank-3 dataset and codec-compatible bounded payload.
        let rc = hdf5_backend::sync::sync(|| unsafe {
            H5Dwrite_chunk(
                ds.id(),
                0,
                0,
                offset.as_ptr(),
                size as usize + 12,
                encoded.as_ptr().cast(),
            )
        });
        if rc < 0 {
            return Err("direct fixture chunk write failed".into());
        }
    }
    file.flush()?;
    Ok(hashes)
}
