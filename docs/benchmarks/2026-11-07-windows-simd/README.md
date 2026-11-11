# Windows SIMD correctness and matched read/decode benchmark

[Benchmark index](../index.md) · Historical measurement: 2026-11-07.
Results and implementation descriptions apply to the recorded source revisions.

MSVC now recognizes x64 SSE2 and ARM64 NEON in the vendored Bitshuffle core.
The indexed raw reader additionally builds the exact upstream AVX2 decode
primitive pair in a separate object and selects it only after Rust's CPU/OS
feature detection. Other objects retain baseline compilation flags. An x64
machine without AVX2 uses SSE2; unsupported explicit backend choices fail.
General HDF5 filter reads use the baseline SSE2/NEON implementation, while the
indexed raw reader uses runtime Auto selection and retains checked Rust LZ4.

`RawReader::with_unshuffle_backend` supports reproducible explicit selection;
`RawReader::unshuffle_backend` reports the actual choice. Auto prefers AVX2,
then NEON, SSE2 and scalar. The scalar and SIMD paths share the same file IO,
framing/bounds checks, source checks, original uint32 output and worker-owned
scratch. No integration mathematics or consuming application is changed.

The local MSVC patch includes its ARM64 NEON header, recognizes its CPU macros,
and replaces compiler-specific vector casts/initializers with equivalent
standard reinterpret/load intrinsics. Original upstream files and license
notices remain unchanged. `vendor/hdf5-bitshuffle-1.0.0/UPSTREAM.json` records
patch/adapter hashes, and the build extracts the pinned AVX2 bodies rather than
maintaining a second copy of the SIMD algorithm.

## Native Windows measurement

The Rust workflow runs the release benchmark separately on `windows-latest`
x86-64 and `windows-11-arm` ARM64. Each runner compares all available explicit
backends **on the same VM**, at 1 and 8 workers, and records the automatically
selected backend and allocated CPU count. These standard runners may have fewer
than eight CPUs; eight workers can oversubscribe the allocation. Timings across
different runners are not a controlled CPU/OS performance comparison.

The benchmark uses **deterministic synthetic uint32 frames with Eiger9M geometry**
(3262 × 3108), not the original beamline file. No research images are uploaded.
Two patterns are retained as source algorithms: sparse nonzero counts at a rate
of 1/128, and dense counts spanning a 16-bit range. Each includes zero, high-bit
and reserved values. All 32 generated frames differ; their expected count hashes
are calculated from the original pixels before compression. The original
independent stock-Bitshuffle fixtures separately qualify codec compatibility.
Synthetic rates are not predictions for the earlier real Eiger file.

Each available backend first reads and verifies every frame, at both worker
counts. Only then do five alternating timed passes run. Each pass reads and
decodes four scans of the 32-frame file through fixed workers, using reusable
buffers and no decoded-frame cache. This extends timing without retaining a
large source-pixel allocation or queued decoded scan. Opening and chunk indexing,
generation and count hashing are excluded. File reads, checked Rust LZ4,
unshuffle, source checks, thread/buffer creation and uint32 delivery are timed.
Masking, float conversion, normalization and integration are excluded.

Validation warms the file cache. These are warm-cache read/decode results on
shared CI hardware, not cold-device bandwidth or universal detector performance.
The payload budget remains 2 GiB; it is not a process RSS cap.

Every CI job retains the full JSON, per-frame expected count hashes, validation
passes, all timings and median/ranges as a `simd-benchmark-<runner>` artifact.
The job summary reports scalar-relative gains. The GitHub run and target/compiler
logs identify the exact source and executing architecture.

## Reproduce

```sh
cargo test --locked --features hdf5 --test bitshuffle_neon -- --nocapture
cargo run --locked --release --features hdf5 --example benchmark_windows_simd -- /tmp/simd-benchmark.json
python scripts/report_simd_benchmark.py /tmp/simd-benchmark.json
```

On Windows, choose a Windows output path instead of `/tmp`. No Python packages
are needed for the report or CI compiler preflight. All generation, reading,
decoding, validation and measured work are native Rust/C.

MSVC compilation uses C11 mode, ensuring the upstream header includes complete
standard integer types instead of its old C89 fallback typedefs. The AVX2
extractor normalizes LF/CRLF in generated text; a regression checks identical
output for Unix and Windows checkouts. Original vendored files are not rewritten.
