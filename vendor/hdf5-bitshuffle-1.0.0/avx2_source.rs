//! Generate a standalone AVX2 object from the retained upstream primitives.
pub fn generate(core: &str) -> String {
    // Git checkouts commonly use CRLF on Windows; extraction must not depend
    // on the host checkout policy. Original upstream source stays untouched.
    let text = core.replace("\r\n", "\n");
    let mut source = String::from(
        "/* MIT: generated from retained upstream Bitshuffle. */\n#include <immintrin.h>\n#include <stdint.h>\n#include <stddef.h>\n#define CHECK_MULT_EIGHT(n) do { if ((n) % 8) return -80; } while (0)\nint64_t bshuf_trans_byte_bitrow_SSE(const void *, void *, size_t, size_t);\nint64_t bshuf_shuffle_bit_eightelem_SSE(const void *, void *, size_t, size_t);\n",
    );
    for (name, renamed) in [
        (
            "bshuf_trans_byte_bitrow_AVX",
            "dioptrin_trans_byte_bitrow_avx2",
        ),
        (
            "bshuf_shuffle_bit_eightelem_AVX",
            "dioptrin_shuffle_bit_eightelem_avx2",
        ),
    ] {
        let start = text
            .find(&format!("int64_t {name}("))
            .expect("missing upstream AVX2 kernel");
        let end = start + text[start..].find("\n}\n").expect("missing kernel end") + 3;
        let function = &text[start..end];
        assert!(function.contains("__m256i"), "selected an AVX2 stub");
        source.push_str(&function.replace(name, renamed));
        source.push('\n');
    }
    source
}
