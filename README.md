# dioptrin-io

Native detector-file input for Dioptrin, PhaseSmith and other Rust consumers.
No Python, NumPy, FabIO, h5py or integration-kernel runtime dependency.

Readers preserve original samples, source dtype metadata, reserved values,
headers and stored orientation within their documented scope. Consumers own
flips, masks, corrections, normalization and integration. `PixelBuffer` keeps
integer and floating samples in their native types, without a full-frame f64
conversion. Worker buffers reuse allocations and invalidate their public data
when a read fails.

## Current reading coverage

Every format dependency is optional; default features are empty.

| Feature | Implemented scope | Important remaining gaps |
|---|---|---|
| `hdf5` | Explicit 3D u16/u32 datasets; bundled HDF5/gzip/LZF/Bitshuffle-LZ4; u32 compatibility output; opt-in indexed raw full-frame chunks with reusable native u16/u32 buffers and bounded parallel consumers | Master discovery/linked sequences, VDS, live refresh, signed/float/2D datasets; raw path rejects unsupported layouts/pipelines explicitly |
| `tiff` | TIFF/BigTIFF; all ten u8–u64/i8–i64/f32/f64 scalar types; both byte orders; primary IFD stacks; strips/tiles; none/PackBits/LZW/Deflate; typed tags | WhiteIsZero, packed/color/lossy samples, float16, subIFDs and detector-specific semantic headers |
| `cbf` | Single 2D integer byte-offset images, eight scalar types, explicit extended64/legacy32 dialects, Content-MD5 verification, original CIF/Pilatus text | Semantic CIF/Pilatus header parsing, other conversions, multiple binary sections, outer wrappers |
| `edf` | Inline 2D frames and concatenated stacks; ten scalar types; both byte orders; internal none/gzip/zlib/bzip2 and legacy spellings; optional uncompressed Size and binary padding; original headers | General/external headers/payloads, volumes, float128, byte-offset EDF |
| `native` | NPY v1/v2/v3 C/Fortran scalar arrays and stacks; d*TREK/ADSC; Fit2D packed masks/spreadsheets; HiPiC; PNM P1/P2/P5; explicitly configured binary input | See the inventory for every dtype, dimension and reference-behavior gap; no content autodetection |
| `wrappers` | gzip/bzip2 files around all `native` formats, EDF, CBF and TIFF; bounded disk staging, source identity checks; includes those reader features | HDF5 wrappers, direct file-like streams, neighboring-file series |

**Full FabIO reading parity is not established.** The complete version-pinned
[coverage inventory](docs/FORMAT_COVERAGE.md) tracks all 36 FabIO 2026.6.0
registry entries (35 distinct codecs), subformats, compression and stack targets,
independent fixtures and remaining work. Generic TIFF/HDF5 success does not
establish detector-specific metadata or master-file coverage.

This development crate is not published. Use a local path dependency:

```toml
[dependencies]
dioptrin-io = { path = "../dioptrin-io", features = ["hdf5", "tiff", "cbf", "edf"] }
```

## Reusable HDF5 input

```rust
use dioptrin_io::hdf5::{Limits, raw::{RawReader, FrameBuffer}};

// Explicit contract: acquisition has stopped and this source is immutable.
let reader = RawReader::open_snapshot("scan.h5", "/entry/data/data", Limits::default())?;
let mut buffer = FrameBuffer::default();
reader.read_frame_into(0, &mut buffer)?;
let native_pixels = buffer.pixels().expect("successful read");
assert_eq!(native_pixels.len(), reader.shape()[1] * reader.shape()[2]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`raw::PipelineOptions` supplies a worker count and decoder payload budget for
`for_each_frame`. Callbacks receive the frame index, stored shape and borrowed
native pixels in unspecified completion order. One reusable frame buffer belongs
to each worker; no unbounded full-scan decoded queue is created. Consumers account
for their own retained results and numerical workspace separately.

Raw snapshots check source identity/size/modification metadata before and after
reads. Frozen resizable datasets are allowed; active acquisition is not. Changes
require reopening. Ordinary filesystem changes are detected; adversarial writes
preserving metadata are outside this contract. Unsupported sources do not
silently fall back or substitute neighboring files.

The general `hdf5::Reader` remains available for its documented layouts and
filters under the upstream HDF5 mutex. Its `read_frame` API still returns exact
u32 counts. The source-bound optional decoded-count cache retains its explicit
budget and original compatibility API.

## TIFF, CBF and EDF

Each module provides `Reader`, `Limits`, `FrameBuffer`, metadata access and
`read_frame_into`. TIFF/EDF take a frame index; CBF admits one binary image.
Header orientation remains metadata. TIFF WhiteIsZero is rejected because the
underlying decoder otherwise performs a count inversion.

CBF dialects can be ambiguous at INT32_MIN. `cbf::Reader::open` uses the extended
8/16/32/64-bit dialect. For FabIO/Pilatus terminal signed32 streams, explicitly
use `open_with_mode(..., ByteOffsetMode::Legacy32)`. Failed decoding never tries
another interpretation. See the [reader contracts](docs/READER_DESIGN.md) for
resource limits, source mutation, dtype and unsupported-subformat details.

## Additional native formats and compressed files

```rust
use dioptrin_io::native::{Reader, Format, Limits, FrameBuffer};
let reader = Reader::open("scan.npy", Format::Npy, Limits::default())?;
let mut frame = FrameBuffer::default();
reader.read_frame_into(0, &mut frame)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Format::Dtrek` also reads ADSC's legacy `TYPE=unsigned_short` header.
`Format::Fit2dMask` returns unpacked 0/1 u8 values without changing row order;
`Format::Fit2dSpreadsheet` exposes the format's f32 numeric rows.
`Format::HiPic` retains binary header offsets and original comments; source
values above 4095 are retained as u16. NPY Fortran storage is converted into
logical row-major frames with the same axes, and leading dimensions determine
C-order frame numbering. Structured, complex and other unsupported NPY dtypes
are rejected; object arrays never execute pickle.

`Reader::open_raw` requires `RawLayout` with shape, scalar type, byte order and
an explicit absolute or end-relative offset. No guessed raw-input defaults are
applied. `Offset::FromEnd { trailing_bytes: 0 }` matches FabIO's offset `-1`.

For compressed files, `wrapped::Reader::open` takes an explicit backend format,
image limits and separate transport limits. It identifies gzip/bzip2 by magic
bytes and streams into a private temporary file. `source_bytes` bounds staged
disk output; decoding does not retain the whole file in RAM. The original file
is checked around staging and every frame read. There is no automatic source,
CBF dialect or backend substitution.

## Validation and performance

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo check --no-default-features
cargo test --no-default-features
```

Small independent FabIO/tifffile/h5py fixtures retain generation provenance under
`tests/data`; Python is used only to generate references, never by production
readers or Rust tests. The real 625-frame Eiger dataset stays outside Git. Full
original-count hashes and complete read-to-Dioptrin measurements are documented
in [the Eiger benchmark](docs/benchmarks/2026-11-05-native-eiger/README.md).
These are warm-cache results, not cold-disk throughput.

The [upstream FabIO suite harness](docs/FABIO_PARITY.md) runs all pinned codec
test modules and compares the same actual files, including generated cases,
against a Rust probe. Unsupported readers, rejected cases, reference failures,
skips and missing metadata coverage remain explicit and keep the parity gate red.

## Provenance and build

The initial HDF5 backend is adapted from `phasesmith-hdf5` 0.8.0. Its original
MIT notice, independent fixtures and native-codec licenses remain unchanged;
`SOURCE_PROVENANCE.json` records the extraction source revision. New optional
readers reuse permissive Rust libraries, as described in the reader design.
HDF5, zlib, LZF and Bitshuffle/LZ4 build from pinned native Cargo dependencies.
The HDF5 feature requires a C compiler, CMake >=3.26 and Git in addition to Rust.
TIFF, CBF and EDF use Rust libraries without an installed HDF5/Python requirement.

Dioptrin and PhaseSmith remain independent consumers. A small opt-in typed
Dioptrin adapter is tested separately for the benchmark; no PhaseSmith migration
or Dioptas changes are included.
