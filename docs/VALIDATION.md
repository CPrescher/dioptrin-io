# Validation, 2026-11-05

## Four TIFF codec/layout sources, 2026-11-06

All 156 Rust tests pass, with formatting, strict all-target/all-feature Clippy,
no-default check/tests and 20 isolated optional-feature checks (25 commands).
The full pinned 213 reference tests have no failures/errors and one upstream skip.
Native results: 482 direct events / 2,404 frames, 26 other explicit views, four
independently verified component/view events and one 100-frame sparse reconstruction.
Rejections fall 44→40; matching events rise 509→513. The two known Pillow planar16
mismatches remain explicit. Source, vendor, Cargo, bridge, executable and independent
reference-library fingerprints are stable. No compared header/component differences
or unexpected comparison failures occur.

Legacy LZW keeps the mature vendored TIFF raster/predictor pipeline. YCbCr native
grids, full stored padding and valid extents are explicit; scalar reads reject
expansion. OJPEG source tables/windows/headers and documented legacy SOS behavior
remain auditable through existing native JPEG decoding. RGB/grayscale is a
separate exact view. The modern JPEG component path also retains truthful grids;
a reproduced libtiff RAW reference defect is qualified and independently avoided.

See `tiff-codec-gaps/README.md`, `fabio-parity/2026-11-06-tiff-codecs.json`,
`fabio-parity/2026-11-06-tiff-codecs-native-checks.json`, and the verified all 91-event
`rejection-audits/2026-11-06-after-tiff-codecs.json`. Earlier sections below retain
historical results. This is local correctness evidence, without expanded platform
CI, publication, universal format parity or new throughput qualification.

## Rejection classification and native gaps, 2026-11-06

All 139 Rust tests pass, including external container/fixed/Esperanto/MAR345
manifests. Formatting, strict all-target/all-feature Clippy, no-default check/tests
and 20 isolated optional-feature checks pass (25 commands total).

The full pinned FabIO suite runs 213 tests with no failures/errors and one upstream
skip. Results: 482 direct format comparisons / 2,404 frames, 26 additional explicit
reference views, and one deterministic sparse reconstruction / 100 frames.
Native rejections decrease 91→44; the two known Pillow planar16 defects remain
mismatches. There are no missing adapters/decoders, unexpected comparison failures
or compared-header differences. Both full and minimal-feature source/bridge/probe
hashes remain stable. The minimal proof runs 47 tests and identifies 129 events as
feature-disabled, with four matches and one known malformed EDF rejection.

Every preceding rejection event has a classification and verified current outcome:
79 inconsistent/malformed/truncated events, eight codec/layout or legacy variants,
two unavailable exact-source events, one mixed-tag legacy dialect and one
ThunderScan integrity qualification. Five events close through native reader
changes; 42 more require an explicit EDF window policy. `Exact` remains default;
complete samples/codecs and unchanged sources are mandatory. No missing counts
or EOL markers are supplied. New count readers retain original signed/packed
values and metadata; reference dtype/display views are explicit.

See `rejection-audits/README.md`,
`fabio-parity/2026-11-06-rejection-continuation.json`, and
`fabio-parity/2026-11-06-rejection-native-checks.json` for exact evidence. Complete
format parity, expanded platform CI, publication and new throughput claims remain
unestablished. Older sections below retain their original historical results.

## MAR345, sparse and explicit reference views

The current local macOS continuation passes formatting, strict all-target /
all-feature Clippy and all **124 Rust tests**, with container/fixed/Esperanto/
MAR345 external manifests enabled. No-default check/tests and isolated strict
all-target Clippy for all 20 optional feature selections pass (22 checks).

The complete pinned FabIO suite runs 213 reference tests with no failures/errors
and one upstream skip. Native results are 439 direct format comparisons / 1,563
frames (including existing explicit R-AXIS/KCD policies), 22 additional
display/encoding view matches, and one deterministic sparse
reconstruction event / 100 frames. The sparse file contains zero events;
independent nonempty/extreme-event fixtures supply separate source evidence.
Two known Pillow planar16 view defects and 91 explicit input/layout rejections
remain. There are no missing-decoder or test-adapter events in the full build.
Seventeen reference input errors agree with native rejection; one deliberately
disabled Python JPEG backend is an environment difference.

The minimal-feature proof runs 42 reference tests, with 117 feature-disabled read
events, three source matches and no missing adapters/decoders. Both reports have
stable source/bridge/probe hashes. TIFF window regressions also document a reused
Deflate footer-integrity limitation; bounds do not imply validation of every
compressed footer. Full parity remains incomplete. See
`fabio-parity/2026-11-05-mar-sparse.json`,
`fabio-parity/2026-11-05-mar-sparse-native-checks.json` and the current
`fabio-parity/README.md` for exact scope and remaining differences.

This is local correctness validation. Expanded platform CI, publication and new
performance claims are not established. The following sections retain historical
validation results.

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
