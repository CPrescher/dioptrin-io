/* SPDX-License-Identifier: MIT
 * Single-block adapter to upstream Bitshuffle primitives, with caller-owned
 * scratch. No codec algorithm is duplicated here. Compiled with the core's
 * flags, preserving its compile-time ISA selection and primitive pairing.
 */
#include <stddef.h>
#include <stdint.h>

#if defined(__AVX512F__) && defined(__AVX512BW__) && defined(__AVX2__) && defined(__SSE2__)
#define FIRST bshuf_trans_byte_bitrow_AVX
#define SECOND bshuf_shuffle_bit_eightelem_AVX512
#elif defined(__AVX2__) && defined(__SSE2__)
#define FIRST bshuf_trans_byte_bitrow_AVX
#define SECOND bshuf_shuffle_bit_eightelem_AVX
#elif defined(__SSE2__) || defined(NO_WARN_X86_INTRINSICS)
#define FIRST bshuf_trans_byte_bitrow_SSE
#define SECOND bshuf_shuffle_bit_eightelem_SSE
#elif (defined(__ARM_NEON__) || defined(__ARM_NEON)) && defined(__aarch64__)
#define FIRST bshuf_trans_byte_bitrow_NEON
#define SECOND bshuf_shuffle_bit_eightelem_NEON
#else
#define FIRST bshuf_trans_byte_bitrow_scal
#define SECOND bshuf_shuffle_bit_eightelem_scal
#endif

int64_t FIRST(const void *, void *, size_t, size_t);
int64_t SECOND(const void *, void *, size_t, size_t);

/* All three buffers must be disjoint and contain elements * width bytes.
 * Handles exactly one block, a multiple of eight elements; the Rust caller
 * validates compressed framing and handles the final unshuffled tail.
 */
int64_t dioptrin_bshuf_unshuffle_block(const void *input, void *output,
                                     void *scratch, size_t scratch_bytes,
                                     size_t elements, size_t width) {
    if (width == 0 || elements % 8 != 0 || elements > SIZE_MAX / width)
        return -80;
    const size_t bytes = elements * width;
    if (bytes > scratch_bytes || bytes > INT64_MAX)
        return -91;
    const int64_t count = FIRST(input, scratch, elements, width);
    if (count < 0)
        return count;
    return SECOND(scratch, output, elements, width);
}
