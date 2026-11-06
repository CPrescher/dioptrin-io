#![cfg(feature = "hdf5")]
//! Independent stock-encoded vectors cover vector loops, blocks and raw tails.
use sha2::{Digest, Sha256};
use std::ffi::c_void;

unsafe extern "C" {
    fn bshuf_using_NEON() -> i32;
    fn bshuf_using_SSE2() -> i32;
    fn bshuf_using_AVX2() -> i32;
    fn bshuf_using_AVX512() -> i32;
    fn bshuf_default_block_size(element_bytes: usize) -> usize;
    fn dioptrin_bshuf_unshuffle_block(
        input: *const c_void,
        output: *mut c_void,
        scratch: *mut c_void,
        scratch_bytes: usize,
        elements: usize,
        element_bytes: usize,
    ) -> i64;
    fn bshuf_bitshuffle(
        input: *const c_void,
        output: *mut c_void,
        elements: usize,
        element_bytes: usize,
        block_elements: usize,
    ) -> i64;
    fn bshuf_bitunshuffle(
        input: *const c_void,
        output: *mut c_void,
        elements: usize,
        element_bytes: usize,
        block_elements: usize,
    ) -> i64;
}

#[test]
fn linked_backend_reports_isa_and_uses_neon_on_unix_arm64() {
    hdf5_backend::sync::sync(hdf5_bitshuffle::register_bitshuffle_plugin);
    // SAFETY: these linked codec queries have no arguments or mutable state.
    let (neon, sse2, avx2, avx512) = unsafe {
        (
            bshuf_using_NEON(),
            bshuf_using_SSE2(),
            bshuf_using_AVX2(),
            bshuf_using_AVX512(),
        )
    };
    eprintln!("Linked Bitshuffle: NEON={neon}, SSE2={sse2}, AVX2={avx2}, AVX512={avx512}");
    #[cfg(all(target_arch = "aarch64", any(target_os = "macos", target_os = "linux")))]
    assert_eq!(
        neon, 1,
        "Unix ARM64 builds must exercise the optimized kernels"
    );
}

#[test]
fn reusable_block_scratch_matches_independent_vectors_and_preserves_guards() {
    hdf5_backend::sync::sync(hdf5_bitshuffle::register_bitshuffle_plugin);
    let encoded = include_bytes!("data/bitshuffle-neon/shuffled.bin");
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("data/bitshuffle-neon/manifest.json")).unwrap();
    let mut scratch = vec![0xCD_u8; 8192 + 32];
    for case in manifest["cases"].as_array().unwrap() {
        let width = case["element_bytes"].as_u64().unwrap() as usize;
        let count = case["elements"].as_u64().unwrap() as usize;
        let start = case["offset"].as_u64().unwrap() as usize;
        let length = case["length"].as_u64().unwrap() as usize;
        let input = &encoded[start..start + length];
        let mut expected = vec![0; length];
        // SAFETY: valid independent fixture, exact-size disjoint buffers.
        assert_eq!(
            unsafe {
                bshuf_bitunshuffle(
                    input.as_ptr().cast(),
                    expected.as_mut_ptr().cast(),
                    count,
                    width,
                    0,
                )
            },
            length as i64
        );
        // SAFETY: accepts a positive element width, no pointer arguments.
        let block_bytes = unsafe { bshuf_default_block_size(width) } * width;
        assert!(block_bytes <= 8192);
        let mut guarded = vec![0xA6; length + 32];
        for _ in 0..2 {
            let shuffled_bytes = count / 8 * 8 * width;
            let mut offset = 0;
            while offset < shuffled_bytes {
                let bytes = block_bytes.min(shuffled_bytes - offset);
                scratch.fill(0xCD);
                // SAFETY: one complete independent codec block, three disjoint
                // exact-size slices; 16-byte guard zones remain outside them.
                let result = unsafe {
                    dioptrin_bshuf_unshuffle_block(
                        input[offset..].as_ptr().cast(),
                        guarded[16 + offset..].as_mut_ptr().cast(),
                        scratch[16..].as_mut_ptr().cast(),
                        bytes,
                        bytes / width,
                        width,
                    )
                };
                assert_eq!(result, bytes as i64);
                assert!(scratch[..16].iter().all(|&v| v == 0xCD));
                assert!(scratch[16 + bytes..].iter().all(|&v| v == 0xCD));
                offset += bytes;
            }
            guarded[16 + offset..16 + length].copy_from_slice(&input[offset..]);
            assert_eq!(guarded[16..16 + length], expected);
            assert_eq!(guarded[..16], [0xA6; 16]);
            assert_eq!(guarded[16 + length..], [0xA6; 16]);
            guarded[16..16 + length].fill(0xA6);
        }
    }
    let input = [0_u8; 32];
    let mut output = [0xA6_u8; 32];
    for (count, width, capacity) in [(8, 4, 31), (7, 4, 32), (8, 0, 32), (8, usize::MAX, 32)] {
        // SAFETY: invalid lengths are rejected before touching these disjoint
        // buffers; even the overflow case cannot reach either primitive.
        let result = unsafe {
            dioptrin_bshuf_unshuffle_block(
                input.as_ptr().cast(),
                output.as_mut_ptr().cast(),
                scratch.as_mut_ptr().cast(),
                capacity,
                count,
                width,
            )
        };
        assert!(result < 0);
        assert_eq!(output, [0xA6; 32]);
    }
}

#[test]
fn upstream_kernels_decode_independent_stock_vectors_exactly() {
    // Reference the binding explicitly so Cargo links the native codec archive.
    hdf5_backend::sync::sync(hdf5_bitshuffle::register_bitshuffle_plugin);
    let encoded = include_bytes!("data/bitshuffle-neon/shuffled.bin");
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("data/bitshuffle-neon/manifest.json")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(encoded)),
        manifest["sha256"].as_str().unwrap()
    );
    for case in manifest["cases"].as_array().unwrap() {
        let width = case["element_bytes"].as_u64().unwrap() as usize;
        let count = case["elements"].as_u64().unwrap() as usize;
        let start = case["offset"].as_u64().unwrap() as usize;
        let length = case["length"].as_u64().unwrap() as usize;
        assert_eq!(length, count * width);
        let mask = u64::MAX >> (64 - 8 * width);
        let expected: Vec<u8> = (0..count)
            .flat_map(|i| {
                let value = ((i as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0xA5A5A5A5A5A5A5A5)
                    .rotate_left((i % 64) as u32);
                let value = if i % 17 == 0 {
                    0
                } else if i % 19 == 0 {
                    mask
                } else {
                    value & mask
                };
                value.to_le_bytes()[..width].to_vec()
            })
            .collect();
        let mut decoded = vec![0_u8; length];
        // SAFETY: the retained, independently encoded fixture and both buffers
        // contain exactly count*width bytes. Zero selects the codec default.
        let written = unsafe {
            bshuf_bitunshuffle(
                encoded[start..start + length].as_ptr().cast(),
                decoded.as_mut_ptr().cast(),
                count,
                width,
                0,
            )
        };
        assert_eq!(written, length as i64);
        assert_eq!(decoded, expected, "width={width}, elements={count}");
        let mut reencoded = vec![0_u8; length];
        // SAFETY: same exact-size buffers as above; the independent fixture
        // verifies format compatibility in the encode direction as well.
        let written = unsafe {
            bshuf_bitshuffle(
                expected.as_ptr().cast(),
                reencoded.as_mut_ptr().cast(),
                count,
                width,
                0,
            )
        };
        assert_eq!(written, length as i64);
        assert_eq!(reencoded, encoded[start..start + length]);
    }
}
