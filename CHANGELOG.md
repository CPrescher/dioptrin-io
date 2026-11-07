# Changelog

## Unreleased

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
