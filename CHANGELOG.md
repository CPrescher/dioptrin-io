# Changelog

## Unreleased

## 0.4.0 — 2026-11-12 (Rust source)

- Add bounded native LZ4 selection to the indexed HDF5 `RawReader`. Use native
  LZ4 by default on Linux/Windows x86-64 and macOS/Linux/Windows ARM64, paired
  with the existing automatic AVX2/SSE2/NEON/scalar bit-unshuffle dispatch.
- Retain explicit Rust/native overrides, exact decoded-length checks and Rust
  fallback when configured chunk budgets exceed the native signed-size range.
  Other targets retain Rust LZ4 by default.
- Preserve original counts, native sample types, reserved values, stored
  orientation and existing source/resource contracts. Integration mathematics
  and numerical precision are unchanged.
- Expand the manual with acquired-file and dense/uint16/small-frame codec
  comparisons, fixed platform defaults, independent count checks and known
  workload-specific exceptions. KCB remains a preserved experiment.
- Make performance benchmarks manual-only: no runs on pushes, pull requests or
  a schedule. Keep ordinary correctness, lint, build and consumer CI checks.

## 0.2.1 — 2026-11-12 (Python distribution)

- Bundle Rust reader 0.4.0, including the qualified native LZ4 defaults for the
  indexed/fused HDF5 path. The basic Python reader API and fused float64 output
  contracts remain unchanged.
- Include the updated codec documentation and retained native-codec notices in
  the source distribution. Python and Rust versions remain independent.

## 0.3.0 — 2026-11-11 (Rust source)

- Resolve exact declared HDF5 master/VDS/raw companion paths with the native
  reader's shared dataset/convention and external-link rules.
- Expose structured missing-file and finite-printf requirements. Applications
  can select exact local companions without scanning neighboring filenames,
  inferring frame counts from availability or filling missing measurements.
- Add wrapped source-contract and external-raw storage entry points, retaining
  input ownership, original typed counts and immutable graph checks.
- Centralize bounded image candidate enumeration while retaining master aliases
  and avoiding HDF5 conditional VDS lookup.
- Preserve the existing explicit reader defaults, native storage admission,
  empty default features and independent Python 0.2.0 distribution.
- Qualify exact finite/nested/printf source words, raw windows, missing/changed
  companions, stream origins and contract ownership. See
  [source contracts](docs/HDF5_SOURCE_CONTRACTS.md) for APIs and limits.

## 0.2.0 — 2026-11-07

- Include the fixed-mask float64 Eiger integration bridge in the standard Python
  wheels, preserving the `dioptrin_io_fused` import used by Qt Dioptas. No local
  Rust checkout or separate adapter build is required for wheel users.
- Read original uint16/uint32 display pixels and integrate from one block decode,
  with scratch reuse and multicore integration within a single frame. Geometry,
  weights and normalization are shared; sums and Poisson terms remain float64.
- Preserve runtime-dispatched scalar/SSE2/AVX2/NEON decoding, platform fallbacks
  and original detector counts. The core Rust reader remains independent of the
  integration library. Existing reader API and application dynamic-mask/bulk
  paths are unchanged.
- Record the matched warm-cache Eiger 9M Qt improvement: median open-to-pattern
  latency 180–182 ms to 143–145 ms. All 625 frames match independent pixel hashes;
  see docs/benchmarks/2026-11-07-qt-combined for precision, timing and scope.


- Backport the upstream Bitshuffle NEON kernels and reuse worker-owned decoder
  scratch buffers for indexed HDF5 reads.
- Select scalar, SSE2, AVX2 or NEON at runtime according to compiled capabilities
  and CPU/OS support; keep AVX2 compilation separate from baseline code.
- Enable SSE2/AVX2 on Windows x86-64 and NEON on Windows ARM64, with independent
  count checks and matched synthetic read/decode benchmarks in CI.
- Extend native reader CI to Linux ARM64 and Windows ARM64 alongside Linux
  x86-64, macOS and Windows x86-64.

## 0.1.0

Initial source release of native detector-file input for Rust consumers.

- Typed counts, source dtype/axes, reserved values and original metadata; explicit
  consumer views, masks, normalization and sparse derived reconstruction.
- Optional TIFF/BigTIFF, CBF, EDF, HDF5 detector discovery/VDS, NPY, MAR345,
  container, legacy-detector, sparse, mask and companion readers.
- Bounded file/stream capture, content discovery, source snapshots, explicit
  external bindings, frozen file series and frame navigation.
- Qualified corpus/proof evidence and a classified inventory separating known
  compatibility cases, optional variants and intentional differences.
- Windows snapshots and decoded-count caches compare volume/file IDs as well
  as timestamps/size, detecting same-metadata file replacement.
- Explicit vendored TIFF/fax dependencies work when this crate is consumed by
  another application; no consumer-side Cargo patch is needed.

This release uses a Git tag/source distribution. It has no Python/FabIO/h5py
runtime dependency; optional codecs include bundled native C libraries.
Unsupported file types/layouts and source policies are documented in
[the gap inventory](docs/READING_GAPS.md). Universal FabIO behavior equality,
active-acquisition refresh and writer/Python-object APIs are not claimed.
