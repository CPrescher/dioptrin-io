# dioptrin-io

Native detector-file input for Dioptrin, PhaseSmith and other Rust consumers.
No Python, NumPy, FabIO, h5py or integration-kernel runtime dependency.

Readers preserve original samples, source dtype metadata, reserved values,
headers and stored orientation within their documented scope. Consumers own
flips, masks, corrections, normalization and integration. `PixelBuffer` keeps
integer and floating samples in their native types, without a full-frame f64
conversion. Worker buffers reuse allocations and invalidate their public data
when a read fails.

## Inspiration and acknowledgments

[FabIO](https://github.com/silx-kit/fabio) is our inspiration and a central
reference for this project. Its broad format support, readers, test suite and
reference files provide an invaluable foundation for understanding and checking
scientific detector-file input. We gratefully acknowledge the FabIO authors
and contributors for that work.

Our aim is to be as reliable and useful as FabIO for reading detector files,
through a native Rust reader. We use FabIO to guide format coverage and compare
reading behavior while preserving original samples and metadata. Adapted code
and format-specific reference work retain their attribution and license notices
under [licenses/native-readers](licenses/native-readers).

## Current reading coverage

Every format dependency is optional; default features are empty.

| Feature | Implemented scope | Boundaries / explicit policies |
|---|---|---|
| `hdf5` | Typed 2D/3D images/stacks with standard scalar, binary16, Boolean and exact packed complex types, both byte orders, Eiger/LImA/Lambda discovery, explicit external-file/raw-window snapshots and bound finite/nested VDS, frame-axis unlimited patterns and explicitly bounded printf mappings; original u32 compatibility reader; opt-in indexed raw u16/u32 chunks and bounded parallel consumers | Additional VDS layouts/custom types are file-triggered; cyclic graphs are rejected, other ranks and live refresh are outside snapshots; raw fast path retains its narrower contract |
| `tiff` | Scalar/packed/WhiteIsZero/palette/CFA/color/planar TIFF and BigTIFF; primary pages and explicit SubIFD selection, strips/tiles, lossless codecs, CCITT RLE/Group3 1D/2D/Group4, legacy LZW, native YCbCr grids/units, modern JPEG and bounded OJPEG interchange/table fragments, global streams, planar scans and explicit reset tiles; explicit legacy metadata policies; ThunderScan complete/available-row reads; Pilatus/MAR headers | OJPEG/JPEG component reads need `jpeg`; ambiguous legacy OJPEG profiles and other sampled layouts remain gaps; F16 widening and reference views qualified |
| `jpeg`, `jpeg2000` | Mature libjpeg-turbo/OpenJPEG backends; lossless high-precision JPEG; original channels, precision, lossy provenance and headers | Unsupported precision/component layouts fail explicitly; see image contracts |
| `cbf` | Eight integer types, explicit byte-offset dialects, original encoded payload/integrity access, raw CIF headers and separate source-preserving CIF 1.1/2.0 syntax trees | Dictionary semantics, other decoded codecs and multiple binary sections |
| `edf` | Ten scalar types, full-rank source arrays/explicit planes and concatenated records/general headers, internal/outer compression and bounded padding, explicit external-source snapshots, explicit legacy layout/label policies and typed-prefix recovery | ABI-dependent float128, obsolete byte-offset; complete-image APIs stay strict; recovery exposes missing spans and member integrity separately |
| `native` | NPY v1/2/3 scalar/complex/Boolean/binary16 and empty arrays, C/Fortran storage; d*TREK/ADSC, Fit2D masks/spreadsheets, HiPiC, PNM and explicit raw input | Remaining dtype/dimension variants; general discovery is available through `wrapped::Input` |
| `columnfile` | Separate labeled ASCII tables, exact labels/comments/numeric tokens and explicit float64 values | Binary decoder is a stub in pinned FabIO; no binary schema is inferred |
| `chiplot` | Separate FIT2D/CHIPLOT profiles, stored X coordinates, multiple Y curves and exact numeric tokens | Unspecified uncertainty layouts and declaration flags fail explicitly |
| `containers` | SPE v2/v3/ROI/frames, typed MRC stacks and nested DM3 image/tag selection | Unsupported layouts/types; pinned reference quirks documented |
| `bruker`, `oxford`, `raxis` | Bruker86/100 tables/baselines, Oxford raw/TY1/TY5, R-AXIS words and explicit expanded-intensity view | Bruker LINEAR float encoding has an explicit decoded view; unknown record layouts fail |
| `fixed` | GE intact/blank/EDF-prefixed headers and Pixi fixed frame records | Explicit source manifests and fixed-layout variants outside the corpus |
| `fit2d`, `kcd`, `mpa` | FIT2D primary/mask/axis arrays, separate KCD readouts/explicit sum, MPA CDAT maps and DATA spectra | Binary MPA and unsupported record layouts/types |
| `esperanto` | Original signed32 Long32/AGI bitfield counts, row indexes, headers and strict source bounds | Unsupported header variants fail; no writer-style reshaping on read |
| `mar345` | MAR345/MAR555 PCK V1/V2, original unsigned counts, high-count tables and headers, both byte orders | Spiral/old IMAGE, degenerate predictor shapes; file series use explicit manifests |
| `sparse` | Original HDF5 sparse events/components/attributes and explicit deterministic background reconstruction | Stochastic noise; native reference and Python fallback have separate semantics; reconstructed backgrounds are derived |
| `xcalibur` | CCD characteristics/source records, explicit mask reconstruction and native lossless binary-mask decomposition and bounded .run acquisition records | No fabricated source images; truncated machine tails require an explicit policy |
| `xsd` | Explicit 2D XSData XML arrays, base64/base32/base16, ten scalar/complex types and optional MD5 | Other ranks/types/encodings and namespaces |
| `wrappers` | Bounded path/nonseekable/buffer input, gzip/bzip2 staging, conservative content discovery and EDF/TIFF/d*TREK header access; source checks on every read | File series/navigation use explicit frozen manifests; variants outside the corpus; HDF5/sparse input requires its optional feature |

The [current gap inventory](docs/READING_GAPS.md) separates confirmed
compatibility cases, optional variants, intentional policies and acquired-file
validation needs. The [reader catalog](docs/FORMAT_COVERAGE.md) retains all 36
pinned registry entries (35 distinct codecs). Its boundary labels are not a
count of release blockers. Full universal FabIO equality is not claimed.

The [current pinned corpus run](docs/fabio-parity/README.md) qualifies all 626
observed input events, including metadata-only and 11 partial-source cases, with
no native rejection of a successful reference read. Expected failures and missing
counts remain explicit. The [two Pillow display differences](docs/pillow-planar16/README.md)
are independently qualified decoder defects. Related compressed-planar display
and native partial-edge tile gaps are fixed; no unclassified mismatch remains
in that pinned corpus. Six supplemental enum/compound/structured/string fixture
cases remain documented compatibility gaps; acquired-detector relevance must
be established separately. The next priority is the [real-file validation
matrix](docs/READING_GAPS.md#next-acquired-detector-files).

Version 0.1.0 is distributed as a GitHub source release. Rust consumers can pin
the release tag; format dependencies stay optional and defaults are empty:

```toml
[dependencies]
dioptrin-io = { git = "https://github.com/CPrescher/dioptrin-io.git", tag = "v0.1.0", features = ["wrappers", "hdf5", "jpeg", "jpeg2000", "mar345"] }
```

The source release includes the patched TIFF/fax backends with explicit relative
dependencies, so consumers need no Cargo patch section. Registry publication is
disabled until those patched backends have a registry packaging strategy; this
is a Git dependency, not a crates.io version. See [release scope and consumption](docs/RELEASING.md)
and [documented unsupported variants](docs/READING_GAPS.md).

No Python scientific package is needed by the readers. HDF5, JPEG/JPEG2000 and
the small ThunderScan adapter compile bundled native code; the full-feature
crate is not a pure-Rust codec implementation. A native compiler and CMake are
build-time requirements for the relevant optional features.

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

## Typed HDF5 and detector discovery

```rust
use dioptrin_io::hdf5::typed::{Reader, Selection, Options, FrameBuffer};
let reader = Reader::open_snapshot("scan.h5", Selection::Dataset("/entry/data"), Options::default())?;
let mut frame = FrameBuffer::default();
reader.read_frame_into(0, &mut frame)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Selection::Eiger`, `Selection::Lima` and `Selection::Lambda` use their native
discovery conventions. The output preserves u8–u64, i8–i64 and f32/f64, with no
full-frame float conversion. Metadata retains source shape/type/endian, filters,
layout and declared fill values. Per-part shape/type metadata remains explicit
when an Eiger sequence contains differing datasets.

External links require `Options { external_links: ExternalLinks::FollowSnapshot,
..Options::default() }`. The reader tracks every master, intermediate link file
and data file, checks all of them around every frame read, and exposes the source
manifest and original link values. Missing sources fail the complete scan. File
search, neighboring-file substitution and live refresh are not performed.
Datasets with undefined unwritten regions are rejected; defined HDF5 fill values
remain part of the source array's documented storage semantics.

`typed::Reader::open_snapshot_with_vds` admits bounded, nonoverlapping 3D
mappings using caller-provided exact `FilenameBinding`s. Finite selections can
be strided or irregular and compose through nested VDS nodes. Frame-axis
unlimited patterns freeze from complete bound sources; `%b` mappings require
an explicit owner-qualified `PrintfMapping` block count. The reader tracks the
complete source graph and reads concrete leaves directly. Original and frozen
selections, nested fill origins and printf declarations remain metadata.
`UnmappedRegions::DeclaredFill` permits defined fill visible in the selected
output; absent/unallocated mapped sources remain errors. Ordinary opens continue
to reject VDS. See [VDS contracts](docs/VDS_READER.md) and
[source recovery](docs/source-recovery/README.md).

## TIFF, CBF and EDF

Each module provides `Reader`, `Limits`, `FrameBuffer`, metadata access and
`read_frame_into`. TIFF/EDF take a frame index; CBF admits one binary image.
Header orientation remains metadata. TIFF WhiteIsZero samples bypass the
backend's display inversion; source photometric metadata is retained. Original color channels are exposed through `sample_shape()` alongside
spatial `shape()`. YCbCr pages expose native component grids and stored sampling units through
`read_ycbcr_into`, retaining padding and valid extents. `uniform_sample_shape()`
returns `None` for these layouts; scalar reads reject implicit expansion.
`sample_shape()` remains a logical shape and is not a component allocation size.
Consumers explicitly choose display/grayscale conversions through `tiff::view`;
raw samples remain available. Palette indices and original ColorMap are retained.
Old-style JPEG requires `tiff,jpeg` and reuses the existing bundled codec.
See [the image contracts](docs/IMAGE_READERS.md) for bounded chunk windows,
opaque legacy description metadata and qualified reference display defects.

`tiff::Options` explicitly selects bounded uncompressed windows derived from
shape or legacy strip tables used as tiles; metadata retains original IFD
entries and effective decisions. ThunderScan uses the original permissively
licensed codec through a bounded row adapter. `read_thunderscan_available_into`
exposes validated row spans and missing ranges without failed-row padding.
`edf::recovery::Reader` similarly exposes only available typed prefixes,
partial-element bytes, headers and compression-member integrity. These outputs
are separate from complete dense frames; absent counts are never invented.

CBF dialects can be ambiguous at INT32_MIN. `cbf::Reader::open` uses the extended
8/16/32/64-bit dialect. For FabIO/Pilatus terminal signed32 streams, explicitly
use `open_with_mode(..., ByteOffsetMode::Legacy32)`. Failed decoding never tries
another interpretation. See the [reader contracts](docs/READER_DESIGN.md) for
resource limits, source mutation, dtype and unsupported-subformat details.

## MAR345 and sparse HDF5

`mar345::Reader` reuses the bounded MIT-derived PCK decoder for V1 and V2,
with separately qualified V2 block framing and original unsigned counts.
It returns U32 source counts, including exact high-count replacements. See
[MAR345 contracts and oracle differences](docs/MAR345_READER.md).

`sparse::Reader` defaults to original typed sparse events and source components.
Dense output requires explicit `Reconstruction::DeterministicBackground`; its
metadata identifies the background as derived. The pinned native reference
model matches all 100 actual frames, while that file contains zero stored
events; independent fixtures cover nonempty and extreme-valued events. See
[the sparse source/reconstruction contract](docs/SPARSE_READERS.md).

## Additional native formats and compressed files

```rust
use dioptrin_io::native::{Reader, Format, Limits, FrameBuffer};
let reader = Reader::open("scan.npy", Format::Npy, Limits::default())?;
let mut frame = FrameBuffer::default();
reader.read_frame_into(0, &mut frame)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Format::Dtrek` also reads ADSC's legacy `TYPE=unsigned_short` header.
Ranks 1 through 32 retain their source shape and expose C-order 2D planes.
`Format::Fit2dMask` returns unpacked 0/1 u8 values without changing row order;
`Format::Fit2dSpreadsheet` exposes the format's f32 numeric rows.
`Format::HiPic` retains binary header offsets and original comments; source
values above 4095 are retained as u16. NPY Fortran storage is converted into
logical row-major frames with the same axes, and leading dimensions determine
C-order frame numbering. Complex64/complex128 retain both components and their original IEEE bits.
Structured and other unsupported NPY dtypes are rejected; object arrays never execute pickle.

`Reader::open_raw` requires `RawLayout` with shape, scalar type, byte order and
an explicit absolute or end-relative offset. No guessed raw-input defaults are
applied. `Offset::FromEnd { trailing_bytes: 0 }` matches FabIO's offset `-1`.

For compressed files, `wrapped::Reader::open` takes an explicit backend format,
image limits and separate transport limits. It identifies gzip/bzip2 by magic
bytes and streams into a private temporary file. `source_bytes` bounds staged
disk output; decoding does not retain the whole file in RAM. The original file
is checked around staging and every frame read. There is no automatic source,
CBF dialect or backend substitution.

See [file series, navigation and companions](docs/SERIES_COMPANIONS_PARITY.md)
for frozen manifests, checked global/local frame mapping and acquisition records.

See [CIF 2.0 and low-level reader APIs](docs/CIF2_LOWLEVEL_PARITY.md) for versioned
CIF syntax, TIFF controls, Pilatus documents, read-only NeXus navigation and
original CBF encoded bytes. The earlier [table/profile audit](docs/CIF_LEGACY_PARITY.md)
qualifies exact ASCII source tokens. Documents/tables/profiles and explicit
reference display views retain separate source contracts.

See [content discovery and stream input](docs/INPUT_APIS.md) for bounded
nonseekable reads, explicit relative origins and conservative format decisions.

See [Xcalibur mask APIs](docs/XCALIBUR_MASKS.md) for caller-shaped derived masks,
explicit truncated-metadata diagnostics and same-file qualification.

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
The [rejection audit](docs/rejection-audits/README.md) separates malformed or
inconsistent inputs, real codec/layout gaps and unavailable source contracts.

## Provenance and build

TIFF uses a [minimally patched vendored backend](docs/TIFF_LEGACY_LZW.md) so
legacy LZW retains the existing raster/predictor implementation. Original upstream
source hashes, the complete patch and licenses are retained.

The initial HDF5 backend is adapted from `phasesmith-hdf5` 0.8.0. Its original
MIT notice, independent fixtures and native-codec licenses remain unchanged;
`SOURCE_PROVENANCE.json` records the extraction source revision. New optional readers reuse permissive native libraries and retain their notices.
SPE/MRC/DM3, fixed/legacy records, XML and image codecs have separate scoped
contracts under `docs/`; the complete inventory links their validation evidence.
HDF5, zlib, LZF and Bitshuffle/LZ4 build from pinned native Cargo dependencies.
The vendored `hdf5-bitshuffle` backend backports the upstream ARM NEON bulk
movemask kernels and their MSVC portability fix. Its original sources and MIT
notices are retained; `vendor/hdf5-bitshuffle-1.0.0/UPSTREAM.json` records the
baseline hashes and upstream commits, and `PATCHES.md` describes the build.
The indexed reader retains checked Rust LZ4 decoding and original pixel types,
and reuses worker-owned scratch for native bit unshuffle.
Matched Eiger9M read/decode measurements are documented in
[`docs/benchmarks/2026-11-06-neon-eiger/`](docs/benchmarks/2026-11-06-neon-eiger/);
the subsequent scratch-reuse comparison is in
[`docs/benchmarks/2026-11-06-scratch-eiger/`](docs/benchmarks/2026-11-06-scratch-eiger/).
Windows SSE2/NEON support, runtime AVX2 dispatch and matched native CI benchmarks
are described in [`docs/benchmarks/2026-11-07-windows-simd/`](docs/benchmarks/2026-11-07-windows-simd/).
The HDF5 feature requires a C compiler, CMake >=3.26 and Git in addition to Rust.
TIFF, CBF and EDF use Rust libraries without an installed HDF5/Python requirement.
JPEG/JPEG2000 build their vendored C codecs; those optional features also require
a native compiler and CMake. No installed Python is required.

Dioptrin and PhaseSmith remain independent consumers. A small opt-in typed
Dioptrin adapter is tested separately for the benchmark; no PhaseSmith migration
or Dioptas changes are included.

See [layout and type qualification](docs/LAYOUT_TYPE_PARITY.md) for source-rank
EDF arrays, NumPy Boolean/binary16/empty arrays, HDF5 exact types/raw windows
and TIFF directory/fax layouts. Source correctness, reference failures and
valid unsupported variants are recorded separately.

The [MAR345 V2, VDS and OJPEG continuation](docs/MARV2_VDS_OJPEG_PARITY.md)
records the added source contracts, independent original-word comparisons and
separate unchanged FabIO failures or display behavior.

The [nested/printf VDS and OJPEG continuation](docs/VDS_ADVANCED_OJPEG_PARITY.md)
records bounded source expansion, nested selection composition and additional
legacy JPEG profiles, with independent counts and reference limitations kept
separate.
