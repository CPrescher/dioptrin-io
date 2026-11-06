# Native reader contracts and implementation choices

`dioptrin-io` owns file interpretation and decompression. Numerical kernels stay
in consumers. A `PixelBuffer` variant owns exactly typed native samples; no
implicit full-frame float64 conversion occurs. Width/height and original storage
type remain explicit. Zeros, large integers, reserved values, signed values,
nonfinite floats and negative zero are retained when admitted by a backend.
Stored row order is never flipped. Header orientation is metadata, not an action.

Current backlog and release boundaries are classified in [READING_GAPS.md](READING_GAPS.md).
Windows snapshots and decoded-count cache stamps include the volume serial and
128-bit file ID from [FileIdInfo](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info).
File replacement must invalidate a snapshot even when size and timestamps match.
Failure to obtain identity is an explicit source-check error.
The raw fast-path restrictions below are separate from the broader typed reader.

## HDF5 general reader and raw snapshots

The original `hdf5::Reader` remains available for its validated 3D u16/u32 scope
and bundled HDF5 filter path. `read_frame` retains its exact u32 compatibility
output. The new `hdf5::raw::RawReader::open_snapshot` is opt-in, not an automatic
replacement. It indexes one allocated full-frame chunk per frame once, then
uses Unix/Windows positional reads, safe Rust LZ4 block decompression and the
already bundled native Bitshuffle unshuffle function outside the HDF5 mutex.
Only admitted decompressed slices reach native bit-unshuffle; native unbounded
compressed-input decoding is never called by this path.

Raw snapshots support unfiltered chunks and a sole Bitshuffle/LZ4 filter,
including explicitly skipped chunks. Both zero/default and explicit block-size
headers are validated. Encoded chunks, decoded frames, filter masks, dimensions,
addresses and exact output lengths are checked. Unsupported filters, partial or
multiframe chunks, missing/unallocated chunks, virtual datasets, external
storage/links and nonzero userblocks are rejected. Use the general reader
explicitly when its contracts fit; there is no retry that silently changes
source or decoding interpretation. The separate typed snapshot reader below
covers broader scalar types/layouts without expanding raw fast-path admission
or its performance claim.

A raw reader freezes dimensions and addresses. The caller asserts acquisition
has stopped, even when the HDF5 dataset is resizable. Path/open-handle identity,
size and modification metadata are checked before and after indexing and every
read. Reopen after changes. These checks detect ordinary mutation/replacement,
not adversarial writes preserving all metadata. Live/SWMR refresh, VDS source
validation and master-file source manifests are not implemented. No decoded
counts are cached by the raw reader.

`FrameBuffer` is worker-owned and reuses its native, compressed and block scratch
allocations. Failed reads invalidate its public pixels/shape. `for_each_frame`
uses at most 256 explicitly budgeted workers and one frame per worker. Callback
order is unspecified; the frame index travels with each callback. Borrowed
pixels expire when the callback returns. Errors stop unstarted work, join every
worker and return the error; already-running consumers can finish. There is no
unbounded decoded queue. `memory_bytes` bounds conservative decoder payload,
including native unshuffle temporary space; index storage, allocator/library
bookkeeping, thread stacks and caller-retained results are additional memory.
It is not an RSS limit.

## Typed HDF5 snapshots and detector discovery

`hdf5::typed::Reader` reuses the bundled HDF5 library/codecs for 2D images and
3D stacks. It admits standard u8/u16/u32/u64, i8/i16/i32/i64 and IEEE f32/f64
in either byte order, plus exact Boolean/binary16 and admitted packed complex
representations. Arbitrary enums, padded/renamed/mixed compounds and custom
precision remain outside the exact-type contract; see [type evidence](HDF5_LAYOUTS.md). Independent h5py fixtures verify
integer extrema, endian conversion, NaN payloads, infinities and signed zero.
Checked upstream dataspace selections feed native H5Dread into exactly sized,
reusable typed buffers under the HDF5 mutex. No full-frame float promotion,
parallel fast path or new performance claim is introduced.

Explicit selection accepts a dataset or its conventional parent group/data.
Eiger discovery handles entry/data, grouped data_N and legacy entry/data_N in
lexical order. LImA follows default-entry/measurement/data and retains detector
names. Lambda checks description and local-name/data paths. Each indexed part
retains source shape, frame range, native dtype/endian, filters, layout and
scalar fill. Differing part shapes/types are not homogenized.

External links default to rejection. Explicit `FollowSnapshot` resolves bounded
soft/external links, retains original link values, and snapshots every master,
intermediate file and data file. Relative filenames resolve exactly against the
referencing file. Native external-file search and basename fallback are not
used. Missing links fail the complete scan. Cycles/traversals, group members,
datasets, source files and retained metadata have separate admission limits.
Every tracked file is checked around every frame read, even when another file
owns that frame. Frozen resizable datasets are allowed; shape/source changes
invalidate reused output. There is no live refresh or decoded-count cache.

Defined HDF5 fill, including default zero, is part of the logical source array.
Unwritten regions with undefined fill or FillTime::Never are rejected so old
buffer contents cannot become counts. Fully allocated fill-disabled sources
remain readable. VDS graph/printf/unlimited mappings and raw external windows
use their explicit source contracts in [VDS_READER.md](VDS_READER.md) and
[HDF5_LAYOUTS.md](HDF5_LAYOUTS.md). The upstream Lambda VDS master contains
acquisition-system absolute paths; silently relocating them or filling missing
modules is not admitted.

Frame/chunk payload and retained metadata/text limits are not RSS limits; native
HDF5 codec/cache/internal allocations are additional memory. Fixed discovery
strings are checked before output allocation. Variable-length string allocation
is managed by the upstream reader, with length checked before retaining/copying
the result. This post-read admission for vlen attributes is an explicit remaining
resource-limit limitation. Admitted typed/text attributes and observed source
fields are qualified separately; arbitrary vendor-field semantics are a
real-file requirement/validation need, not a counted decoder defect.

## TIFF/BigTIFF

The MIT `tiff` crate is reused for native scalar/packed/color/planar samples,
primary IFD stacks and supported strip/tile codecs. WhiteIsZero backend display
inversion is bypassed to retain source counts. Packed widths retain their source
precision, while unpacked buffers expose integer values. Float16 widens exactly
to F32 with original bit width retained; this is explicit source/output type
metadata rather than raw dtype parity. EXIF/TIFF orientation is never applied.

Group3 one-dimensional strips reuse the pinned fax Huffman tables under bounded
TIFF row framing: real EOL markers, exact terminating runs, declared strip windows
and explicit end/padding admission. Output retains unpacked source 0/1 words.
FillOrder2 is admitted for Group3 and uncompressed packed grayscale; original
`fill_order` remains metadata. Other unsupported bit-order/codec combinations
fail. Invalid inline descriptive ASCII retains its complete original descriptor
and bytes as opaque metadata; structural source windows are never repaired.

`detector_tiff` reuses the same pixel reader and adds Pilatus semantic text maps
and MAR's typed binary acquisition fields/raw reserved bytes. Detector-specific
success is now checked independently of generic TIFF counts. Detailed admitted
codecs/layouts, palette/CFA source samples, explicit display views, image
fixtures and JPEG/component qualifications are in `IMAGE_READERS.md`.
CCITT RLE/Group3 2D and explicit reachable SubIFD selection are qualified in
[TIFF_LAYOUTS.md](TIFF_LAYOUTS.md); fragmented/global/planar legacy JPEG profiles
are qualified in [OJPEG_READER.md](OJPEG_READER.md).

Caller metadata/decoder limits apply before initial or selected IFD parsing. Admitted metadata,
page count, decoded output and intermediate scratch have separate bounds.
Unknown TIFF field *types* skipped by the backend are not represented in the tag
map. A bounded read-only IFD adapter retains source photometric tags while exposing
non-transforming tags to the existing codec. Every pixel codec reads inside its
declared chunk windows; metadata overlays cannot supply pixel bytes. Missing
Deflate footers after complete output can still be accepted by the reused
backend; this qualification is recorded in `IMAGE_READERS.md`.

## CBF and EDF

CBF retains the complete source CIF/Pilatus text header and parsed binary MIME
fields. It admits integer byte-offset images, validates Content-MD5 when present,
checks dimension/element counts and rejects missing/unknown fields or binary
conversions. It does not normalize malformed fields to defaults.

Byte-offset has incompatible escape dialects. The default `Extended64` follows
8/16/32/64-bit escapes and checks counts against their declared type. Callers
must explicitly select `ByteOffsetMode::Legacy32` for FabIO/Pilatus signed32
streams, where INT32_MIN is a terminal 32-bit difference and accumulation uses
32-bit wrapping. Failed reads never try another dialect. The small independent
FabIO fixtures exercise this distinction. Supporting all dialects does not
establish universal CBF/CIF header or multiple-array support.

EDF supports inline 2D frames (including concatenated frames with differing
metadata), all ten scalar types, both byte orders and bounded internal
none/gzip/zlib/bzip2, including GzipCompression/ZCompression spellings. The Size
field is optional for uncompressed dimensioned frames. Declared binary padding
is retained in stored-byte metadata and skipped after the exact image samples.
Original padded header bytes and parsed fields are retained. Legacy non-ASCII
text uses a reversible Latin-1 mapping; no bytes are silently dropped.
General headers and local overrides retain their original records. External
payloads require explicit snapshot options, optionally selecting basename
relocation or unique compressed counterparts. The source graph and expanded
staging bytes are bounded; every original/staged source and read handle is
checked around reads. Exact closing-newline boundaries and admitted bounded
storage padding are retained explicitly. Unexplained codec tails and incomplete
streams fail. External windows beyond EOF fail under the default `Exact` policy.
Explicit `ExternalWindow::UpToDeclared` caps only external reads to available
source bytes, retaining declared/effective sizes and still requiring complete
samples/full codecs. It does not change inline block navigation or repair missing
pixels. `wrapped::Format::EdfExternalWindow` exposes this choice through wrappers.
Source arrays/volumes and explicit planes are qualified in [EDF_ARRAYS.md](EDF_ARRAYS.md).
128-bit labels and obsolete byte-offset compression are optional extensions
with unresolved representation/reference authority, not current corpus blockers.
Outer wrappers also have the separate API below.
Neither reader pads truncated pixels with zeros or substitutes neighboring files.

## NPY, raw input and smaller detector formats

The optional `native` feature provides explicitly selected NPY, d*TREK/ADSC,
Fit2D mask/spreadsheet, HiPiC and PNM readers. They share bounded scalar decoding
and immutable source checks. Header bytes and type/endian metadata remain
available. Raw input requires explicit dimensions, scalar type, byte order and
absolute or EOF-relative location, with checked range arithmetic. FabIO negative
offset `-n` maps to `FromEnd { trailing_bytes: n - 1 }`.

NPY v1/v2/v3 accepts ten native scalar types plus complex64/complex128,
Boolean/binary16 and scalar/1D/2D/higher-dimensional arrays including empty
axes (at most32 axes). C-order leading axes are flattened into
frame numbers. Fortran storage is read in logical axis order into row-major
frames without a whole-array allocation; it currently performs one positional
seek/read per sample and has no performance claim. The original descriptor,
shape and storage-order flag survive. Structured/string arrays are confirmed
supplemental compatibility cases; float128 is a platform-dependent optional
variant, and object unpickling is intentionally excluded. Shape/byte arithmetic is checked
before reads. No Python expressions or pickle are executed.

d*TREK supports its seven scalar types, both byte orders and the ADSC legacy
unsigned_short TYPE spelling. Nested braces in header values are retained.
Ranks 1 through 32 retain their complete source shape; leading axes select
C-order 2D planes, and rank1 exposes a single `[1, length]` plane. Missing-type
repair remains unsupported.
Fit2D masks unpack little-endian LSB-first bits to 0/1 u8, skip row alignment
bits and retain all 1024 header bytes. They do not apply masks to detector data.
Fit2D spreadsheet rows require exact dimensions and parse decimal text through
f64 into the format's f32 representation, matching the reference.

HiPiC reads little-endian u16 IM images without the old 12-bit signed-intensity
reinterpretation. Binary offsets/header type and raw comments are retained.
UTF8 comment assignments are exposed as a map; repeated keys follow the last
assignment, with every original entry still present in raw header bytes.

PNM supports P1/P2 ASCII and big-endian P5 u8/u16/u32, preserving black/white
values without inversion. Comments and exact sample counts are checked. P5
admits a single raster delimiter, or extra line whitespace of a length proven
by the exact raster size (including FabIO's space-plus-newline writer). It does
not trim initial binary sample bytes. Color/packed variants remain unsupported.
The pinned FabIO P1/P2 loops never advance their row index; dioptrin-io retains
all original rows and records this reference bug as a behavior difference.

## Outer file wrappers

The optional `wrappers` feature includes the above readers and exposes
`wrapped::Reader` for magic-detected gzip/bzip2 around implemented native,
container, legacy and image backends when their optional features are compiled.
It streams decompressed bytes into a private tempfile, admitting concatenated
members and checking compression checksums. Transport limits independently bound
the encoded file and total uncompressed staging bytes. This is bounded disk
staging, not a whole-file RAM buffer or a decoded-pixel cache. Backend frame,
metadata and scratch limits still apply. The source snapshot and open handle
are checked before/after staging and every read; a changed original invalidates
already staged data. Backend handles are dropped before the temporary file for
Windows compatibility. Corrupt/truncated/trailing wrapper garbage fails closed.
`wrapped::Input` admits captured nonseekable/buffer input, conservative content
discovery and optional HDF5/sparse readers with explicit origins/source bindings.
File series and navigation use frozen manifests. See [INPUT_APIS.md](INPUT_APIS.md)
and [SERIES_API.md](SERIES_API.md). CBF dialect selection remains explicit,
including through this wrapper API.

## Container, record and image-specific contracts

The optional SPE/MRC/DM3 readers retain original types, image/ROI selections and
stored frame order (`CONTAINERS_DESIGN.md`). GE/Pixi fixed records retain every
source header (`FIXED_DESIGN.md`). Bruker/Oxford/R-AXIS preserve count encodings
and acquisition metadata (`LEGACY_READERS.md`). FIT2D/KCD/MPA retain auxiliary
arrays/readouts/spectra (`RECORD_READERS.md`). No source image is silently
replaced by a thumbnail or neighboring source.

R-AXIS expanded intensities and KCD summed readouts require explicit policies.
Bruker LINEAR float encoding has a separate explicit decoded view. Esperanto
Long32/AGI decoding returns original signed32 pixels (`ESPERANTO_DESIGN.md`).
XSData XML uses a single structurally owned record and explicit encoding/dtype,
rejecting ambiguous XML and checksums (`XSD_READER.md`). Native JPEG/JPEG2000
reuse mature codecs with precision, channel and lossy provenance retained
(`IMAGE_READERS.md`). No benchmark claim follows from count-parity evidence.

## Library evaluation and licensing

- `hdf5-metno` 0.15.1 and `hdf5-bitshuffle` 1.0.0: reuse the extracted reader,
  static HDF5/codecs and original MIT/native notices. HDF5 remains the metadata
  authority and general decoder; new raw code does not reimplement HDF5 indexes.
- `tiff` 0.11.3 (MIT): mature TIFF/BigTIFF, native typed buffers and lossless
  codecs, Rust 1.85 baseline. Explicit admission compensates for display-oriented
  inversions and planar/color behavior inappropriate for raw detector counts.
- `lz4_flex` (MIT): bounded Rust LZ4 blocks; only safe decoder features enabled.
  Bitshuffle is reused from the bundled native codec, rather than a second
  unvalidated bit-plane implementation.
- `flate2` (MIT/Apache-2.0, Rust backend) and `bzip2` 0.6.1 / `libbz2-rs-sys`
  0.2.6 (MIT/Apache-2.0 and retained bzip2 notices): independent native EDF
  decompression, without Python or subprocesses.
- `cryiorust` 0.6.0 was evaluated via its published package metadata. Its license
  is GPL-3.0+, so it is not incorporated into this MIT crate. No source was
  copied from it. CBF/EDF use independently written bounded parsers with
  independent test fixtures instead.
- `npyz` 0.9.1 (MIT) was evaluated for reuse. Its header interpretation computes
  unchecked shape/stride products before caller admission (`read.rs`), so it
  was not incorporated. The bounded numerical NPY grammar is independently
  implemented; broader dtypes remain visible gaps. Registry searches did not
  locate reusable permissive d*TREK/HiPiC/Fit2D detector readers. These small
  parsers use the pinned format reference and independent licensed fixtures.
- `tempfile` 3.27.0 (MIT/Apache-2.0) supplies private, automatically removed
  staging files. Existing flate2/bzip2 decoders are reused for wrappers.

The pinned FabIO source/test-suite copyright notice is retained under
`licenses/native-readers`. No GPL/LGPL FabIO native codec implementation was
incorporated into the MIT production crate. Upstream test data stays outside Git.

Cargo lockfiles pin exact build dependency resolution. New reader/codec notices
are under `licenses/native-readers`; original attribution remains unchanged.
FabIO, h5py, NumPy, tifffile and imagecodecs are test/reference tools only.

## MAR345 and sparse source/model boundaries

`MAR345_READER.md` specifies the bounded MIT PCK V1 adaptation, unsigned source
counts, high tables, retained headers and unsupported V2/raw variants.
`sparse` reuses the existing typed HDF5 source graph and storage admission.
It preserves event order/dtypes and source components by default. Dense model
output requires explicit deterministic reconstruction and is labelled derived;
stochastic noise is not synthesized. See `SPARSE_READERS.md` for source, bounds
and reference-backend qualifications.

## Native TIFF component layouts and legacy codecs

`Metadata.sample_layout` distinguishes uniform arrays from original YCbCr grids.
For component pages, `uniform_sample_shape()` returns None and ordinary scalar
reads invalidate output and reject implicit chroma expansion. `read_ycbcr_into`
returns source-order TIFF units or separately labelled decoded JPEG components,
with per-chunk plane dimensions, valid extents and source sampling. Padding is
retained, including complete nominal last strips. Logical `sample_shape()` is
not a native component allocation size. Explicit views carry source geometry and
record all conversion operations. Wrapped sources check the compressed original
around component reads and invalidate failed outputs.

Legacy LZW reuses the vendored mature TIFF raster/predictor implementation; source
manifest, complete minimal patch and license notices remain in `vendor`/`licenses`.
Full-suite provenance hashes every vendored file and Cargo build input. Original
TIFF JPEG table/header assembly reuses libtiff's documented container behavior and
the existing bundled libjpeg-turbo decoder. Details and unsupported contracts are
in `TIFF_LEGACY_LZW.md`, `OJPEG_READER.md` and `tiff-ycbcr/README.md`.
