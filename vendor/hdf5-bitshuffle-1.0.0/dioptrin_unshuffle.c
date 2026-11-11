/* SPDX-License-Identifier: MIT
 * Single-block adapters to unchanged upstream Bitshuffle primitives. The Rust
 * caller resolves CPU/OS capabilities before selecting a backend. AVX2 code is
 * compiled separately, so baseline/SSE2 callers never execute AVX instructions.
 */
#include <stddef.h>
#include <stdint.h>
#include "lz4.h"

int dioptrin_lz4_decompress_safe(const char *input, char *output,
                                int input_bytes, int output_bytes) {
    return LZ4_decompress_safe(input, output, input_bytes, output_bytes);
}

typedef int64_t (*kernel)(const void *, void *, size_t, size_t);
#define DECLARE_PAIR(tag) \
    int64_t bshuf_trans_byte_bitrow_##tag(const void *, void *, size_t, size_t); \
    int64_t bshuf_shuffle_bit_eightelem_##tag(const void *, void *, size_t, size_t)
DECLARE_PAIR(scal);
DECLARE_PAIR(SSE);
DECLARE_PAIR(NEON);
#ifdef DIOPTRIN_HAVE_AVX2
int64_t dioptrin_trans_byte_bitrow_avx2(const void *, void *, size_t, size_t);
int64_t dioptrin_shuffle_bit_eightelem_avx2(const void *, void *, size_t, size_t);
#endif

int dioptrin_bshuf_avx2_compiled(void) {
#ifdef DIOPTRIN_HAVE_AVX2
    return 1;
#else
    return 0;
#endif
}

/* All buffers are disjoint and contain elements * width bytes. Exactly one
 * block, a multiple of eight elements. Backend IDs match Rust's public enum:
 * 1 scalar, 2 SSE2, 3 AVX2 (requires runtime CPU/OS check), 4 NEON.
 */
int64_t dioptrin_bshuf_unshuffle_block(const void *input, void *output,
                                     void *scratch, size_t scratch_bytes,
                                     size_t elements, size_t width, int backend) {
    if (width == 0 || elements % 8 != 0 || elements > SIZE_MAX / width)
        return -80;
    const size_t bytes = elements * width;
    if (bytes > scratch_bytes || bytes > INT64_MAX)
        return -91;
    kernel first, second;
    switch (backend) {
    case 1:
        first = bshuf_trans_byte_bitrow_scal;
        second = bshuf_shuffle_bit_eightelem_scal;
        break;
    case 2:
        first = bshuf_trans_byte_bitrow_SSE;
        second = bshuf_shuffle_bit_eightelem_SSE;
        break;
#ifdef DIOPTRIN_HAVE_AVX2
    case 3:
        first = dioptrin_trans_byte_bitrow_avx2;
        second = dioptrin_shuffle_bit_eightelem_avx2;
        break;
#endif
    case 4:
        first = bshuf_trans_byte_bitrow_NEON;
        second = bshuf_shuffle_bit_eightelem_NEON;
        break;
    default:
        return -92;
    }
    const int64_t count = first(input, scratch, elements, width);
    if (count < 0)
        return count;
    return second(scratch, output, elements, width);
}
