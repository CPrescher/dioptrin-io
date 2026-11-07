# Upstream ARM NEON backport

This is the hdf5-bitshuffle 1.0.0 registry crate, retaining its original Rust
wrapper, Bitshuffle/LZ4 sources, MIT notices, and HDF5 compression-level bounds
patch. `UPSTREAM.json` records every baseline file hash and the exact upstream
commits backported here.

The build copies the original Bitshuffle core into OUT_DIR and applies:

- Sebastian Pop's upstream Bitshuffle commit
  `45af6537796e15099959529899edbd606002c3f5` (bulk NEON movemask).
- Christoph Gohlke's upstream follow-up
  `aa9bd80f076ccde6320d81833b19b835ecf2acfe` (MSVC alias-type guard).

These change ARM NEON bit shuffling/unshuffling, without changing the encoded
format, block-size convention, filter metadata, output sample types, or stored
orientation. Other architectures retain their existing kernels. No Jon Wright
integration/decoder code is included by this backport. The optimization and
its attribution come directly from the MIT-licensed upstream Bitshuffle project.

The codec is shared by general HDF5 filter reads and the indexed raw reader.
The latter retains checked/bounded Rust LZ4 decoding and immutable-source checks.
Neither path introduces an integration kernel, Python dependency, or image
conversion. Independent stock-Bitshuffle fixtures and original Eiger9M count
hashes qualify the changed decoder.

The optional path dependency enables this backport in source/Git consumers.
As with the existing patched TIFF backend, registry publication would require a
published patched backend or another packaging design; this task does not
publish a crate or change consuming applications.

The build forces standalone Git patch application, even when Cargo output is
inside a consumer worktree, and verifies that each patch changes its target.
This also makes the retained HDF5 bounds patch effective in that build layout.

`dioptrin_unshuffle.c` is local MIT-licensed adapter code. It calls the same
upstream primitive pair selected by Bitshuffle for NEON, SSE2, AVX2, AVX512
or scalar builds, using disjoint scratch supplied by the Rust frame worker.
It handles one validated block without generic blocking or heap allocations.
Compressed parsing, checked LZ4, tail handling and resource admission remain in
Rust. Original upstream source files and patches remain unchanged.

## Windows SIMD and runtime AVX2

Local patch `0004-msvc-simd-selection.patch` recognizes MSVC x64 and ARM64
macros, selects `arm64_neon.h` with MSVC, and uses a byte-array load for the
NEON mask constant. It does not change the mask values or kernel arithmetic.

The build extracts the two exact upstream AVX2 decode primitives from the
pinned patched core and compiles them separately with `/arch:AVX2` or `-mavx2`.
Their SSE remainder helpers remain in the baseline object. The raw reader uses
Rust CPU/OS feature detection before allowing AVX2; the rest of the library is
not compiled with that flag. General HDF5 filter reads use the baseline SSE2 or
NEON kernels; runtime AVX2 selection applies to the indexed raw reader.

The raw reader exposes Auto/Scalar/Sse2/Avx2/Neon selection for reproducible
comparisons, rejecting explicit unsupported choices. Auto prefers AVX2 where
available, then NEON, SSE2 and scalar. Each worker keeps its reusable scratch.
