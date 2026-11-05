# dioptrin-io

Native detector-file input for Dioptrin, PhaseSmith and other Rust consumers.
The crate has its own version and does not depend on either application's
numerical kernels, Python, NumPy, FabIO or h5py.

The initial HDF5 backend is adapted from `phasesmith-hdf5` 0.8.0. Its original
MIT notice, independent fixtures and native-codec licenses are retained.
`SOURCE_PROVENANCE.json` records the source revision and extraction changes.
PhaseSmith's existing package and APIs remain unchanged during this first step.

## Initial scope

Enable feature `hdf5` for read-only `(frame, y, x)` uint16/uint32 datasets.
The reader returns original row-major counts as owned contiguous `u32` buffers;
uint16 promotion is exact and its source type remains available as metadata.
Reserved values, zeros and stored orientation are preserved. Masks, display
flips, normalization, interpolation and integration belong to consumers.

HDF5, zlib, LZF and Bitshuffle/LZ4 build from the pinned native dependencies.
No installed HDF5 or external Python compression plugin is required. Building
this feature requires a C compiler, CMake >=3.26 and Git in addition to Rust.
Unavailable filters and unsupported layouts fail explicitly. Virtual datasets
are not supported by this initial adapter.

```toml
[dependencies]
dioptrin-io = { version = "0.1", features = ["hdf5"] }
```

The version above describes the intended dependency interface; this development
crate has not been published. For local development before publication, use a path dependency pointing to this repository.

```rust
use dioptrin_io::hdf5::{Limits, Reader};

let reader = Reader::open("scan.h5", "/entry/data/data", Limits::default())?;
let counts = reader.read_frame(0)?;
assert_eq!(counts.pixels.len(), counts.shape[0] * counts.shape[1]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

An optional source-bound decoded-count cache retains only original counts and
has an explicit payload budget. It returns independent owned frames. Cached
reads reject changed source files and unsupported indirect storage; they are
intended for immutable scans rather than live acquisition.

## Validation and next performance step

```sh
cargo test -p dioptrin-io --features hdf5
cargo clippy -p dioptrin-io --all-targets --features hdf5 -- -D warnings
cargo run -p dioptrin-io --features hdf5 --release --example inspect_frames -- \
  scan.h5 /entry/data/data 0 1 2
```

The existing reader uses the upstream HDF5 mutex. This extraction does not add
parallel raw-chunk decoding or claim a new throughput gain. The next step is to
benchmark this baseline on the real Eiger file, then add reusable typed buffers
and a parallel raw-chunk path while retaining the general reader as a fallback.
Dioptrin integration adapters, PhaseSmith migration and additional formats can
then use the same independently tested I/O implementation.

Without format features, this crate builds without HDF5. Consumers can enable
only the format backends they need. Dioptrin and PhaseSmith remain independent
consumers; neither numerical library is a dependency of this package.
