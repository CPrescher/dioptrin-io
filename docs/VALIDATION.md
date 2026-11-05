# Validation, 2026-11-05

## Parallel native-reader continuation

The integrated continuation passes all required local macOS checks:

- `cargo fmt --check`.
- `cargo clippy --all-targets --all-features -- -D warnings`.
- `cargo test --all-features`: 93 tests, with container/fixed/Esperanto external
  parity manifests enabled (`CONTAINER_PARITY_MANIFEST`, `FIXED_PARITY_MANIFEST`,
  `ESPERANTO_PARITY_MANIFEST`).
- No-default check/tests plus isolated all-target strict Clippy for all 18
  optional feature selections: 20 separate feature checks.

The full pinned FabIO run executes 213 reference tests, no failures/errors,
one unconditional upstream skip. Native results match 413 events / 1,533 frames,
with zero missing test adapters. Thirteen source/display differences, 98 native
rejections and 31 missing-decoder events remain explicit; the full gate is red.
OpenJPEG 2.5.4 exercises all optional Glymur variants. Sources, comparison bridge
and copied probe remained unchanged during both full and feature-disabled runs.
See `fabio-parity/2026-11-05-parallel-readers.json`, the separate 64-event
feature-disabled proof and their summary in `fabio-parity/README.md`.

Cross-review fixes and independent fixtures cover container record ownership,
source substitution, signedness, table/count admission, exact ASCII input,
embedded codec layouts and source identity. This is local validation; expanded
platform CI, release/publication and new throughput claims are not established.

## Previous typed-HDF5 continuation

The typed HDF5 continuation passes formatting, strict all-target/all-feature
Clippy and all 36 Rust tests, plus separate no-default and HDF5-only checks.
Four new tests cover independent typed samples, source graphs, admission and
mutation. Full FabIO results: 202 matching read events / 1,049 frames;
213 reference tests, no failures/errors, five explicit skips. See
`fabio-parity/2026-11-05-hdf5.json` for exact source hashes and remaining gaps.
An HDF5-disabled test executable independently reports six generic HDF5 events
as `feature_disabled`, distinguishing build configuration from missing readers.
The changes remain local/unpushed; no new platform-CI or throughput claim is made.

## Native reader and wrapper expansion

The preceding native-reader expansion passed:

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
