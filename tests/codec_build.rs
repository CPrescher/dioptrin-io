#![cfg(feature = "hdf5")]
#[path = "../vendor/hdf5-bitshuffle-1.0.0/avx2_source.rs"]
mod avx2_source;

#[test]
fn avx2_generation_is_identical_for_lf_and_windows_crlf_checkouts() {
    let core = include_str!("../vendor/hdf5-bitshuffle-1.0.0/ext/bitshuffle/src/bitshuffle_core.c")
        .replace("\r\n", "\n");
    let lf = avx2_source::generate(&core);
    let crlf = avx2_source::generate(&core.replace('\n', "\r\n"));
    assert_eq!(lf, crlf);
    assert!(lf.contains("_mm256_movemask_epi8"));
    assert_eq!(
        lf.matches("int64_t dioptrin_trans_byte_bitrow_avx2(")
            .count(),
        1
    );
    assert_eq!(
        lf.matches("int64_t dioptrin_shuffle_bit_eightelem_avx2(")
            .count(),
        1
    );
    assert!(!lf.contains("int64_t bshuf_untrans_bit_elem"));
}
