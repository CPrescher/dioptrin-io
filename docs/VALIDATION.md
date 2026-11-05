# Validation, 2026-11-05

The next native-reader expansion passes:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`: 32 Rust tests, including 71 small independently
  verified native-format fixtures and wrapper/padding/alias regressions.
- Separate no-default-feature check/tests, native-only tests and strict Clippy
  for every optional format feature and the wrapper feature.

The pinned upstream FabIO codec suite is now run against the same actual files
using a test-only Rust/Python comparison bridge. Full reading parity is **not**
established. Exact scope, reproduction, source/download hashes and remaining
reading-contract gaps are in `FABIO_PARITY.md` and `fabio-parity/`.
These local changes have not been pushed; expanded Windows/Linux/macOS CI is
still pending. No new performance claim is made for these additional readers.

## Previous expansion

Local macOS/aarch64 validation for the initial native-reader expansion:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`: 24 tests, including existing reader/cache tests,
  independent typed TIFF/BigTIFF/CBF/EDF fixtures and raw HDF5 pipeline checks.
- `cargo check --locked --no-default-features` and
  `cargo test --no-default-features`.
- Strict all-target Clippy with each of `hdf5`, `tiff`, `cbf`, `edf` enabled
  independently; `cargo check --no-default-features --features tiff,cbf,edf`.
  The three non-HDF5 readers do not require the HDF5 feature/runtime.
- Independent original-count SHA256 comparison over all 625 real Eiger frames;
  full read-to-consumer benchmark and bit-identical 625 × 2000 intensity/Poisson
  std validation against h5py → existing float64 Dioptrin.
- The isolated Dioptrin adapter's full library suite: 168 tests pass.

The initial GitHub Actions run 42049332725 passed Linux and macOS. Its Windows
job failed strict Clippy solely on Unix-only `AtomicUsize`/`Ordering` test imports.
Those imports now have `#[cfg(unix)]`. This branch is local/unpushed, so a new
Windows/Linux/macOS CI run has not yet validated the expanded readers.

The default chunk-size and skipped-filter regression uses independently encoded
h5py Bitshuffle/LZ4 payload bytes, with native HDF5 writing the explicit chunk
metadata/filter masks. Raw output is checked against original expected counts
and the general HDF5 reader. Full external counts are additionally compared to a
separately installed h5py/hdf5plugin reference; there is no production Python
runtime dependency.

Detailed limits/contracts and unsupported paths are in `READER_DESIGN.md`.
The version-pinned `FORMAT_COVERAGE.md` records every FabIO reader target and
remaining gaps; these checks do not establish full format-level FabIO parity.
