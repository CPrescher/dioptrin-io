# Content discovery and captured stream input

`discovery::probe` inspects a bounded original byte prefix without a decoder or
Cargo feature dependency. It returns format candidates, evidence and reasons.
`Candidate::codec.available()` reports optional decoder availability separately.
`Detection::require_unique()` rejects unknown, conflicting or ambiguous evidence
and disabled features. A detected signature is not proof of a valid payload.
There is no extension lookup or retry through another decoder after failure.

With `wrappers` enabled, `wrapped::Input` owns a source before decoder selection:

```rust,ignore
use dioptrin_io::{native, tiff, wrapped::{Input, Limits, FrameBuffer}};
let input = Input::from_reader(socket, None, Limits::default())?;
let evidence = input.discover()?;
let mut reader = input.read_discovered(native::Limits::default(), tiff::Limits::default())?;
let mut frame = FrameBuffer::default();
reader.read_frame_into(0, &mut frame)?;
```

`Input::open(path, transport_limits)` prepares an immutable path.
`from_reader(read, origin_directory, limits)` captures from the current position
through EOF. It requires only `Read`, does not seek, and does not retain the
caller's object. The snapshot describes bytes received from that stream, without
claiming a backing-file identity or coherence of a concurrently changing input.
`from_bytes` makes an owned bounded snapshot of a byte slice.
`Reader::from_reader` is a convenience for a caller-selected existing `Format`.
`HeaderReader::from_reader` provides the same transport for EDF/TIFF/d*TREK metadata
without selecting a sample codec or constructing an image.

Encoded input is copied to a private temporary file under `encoded_bytes`.
Gzip/bzip2 decoding uses the existing streaming transport under `source_bytes`.
Both limits must be positive and finite. A failed read, corrupt complete-reader
wrapper or exhausted budget produces an error and releases the temporary files.
A stream clone shares its immutable capture; readers can independently open
stored counts and selected views without consuming the stream again or copying
its input twice. The last owner releases the capture after decoder drop.
`provenance()` distinguishes captured streams from path input and reports the
explicit origin, encoded/decoded sizes and outer compression.

Discovery and decoding reuse the native parsers and backends. Stored counts,
source dtypes, reserved values and orientation retain their existing contracts.
Automatic opening uses native defaults: it does not infer the legacy CBF escape
dialect, a KCD readout sum, a R-AXIS intensity expansion, a TIFF display view,
compatibility metadata, missing-source recovery or a sparse background model.
Call `Input::read(explicit_format, ...)` when one of those policies is required.
`Format::Raw` takes a complete explicit `native::RawLayout`; raw samples are not
content-discoverable.

## Ambiguity and container conventions

A blank GE prefix is also a possible prefix of arbitrary binary data. Numerical
spreadsheet text has no unique signature. These remain explicit-format inputs.
SPE version/type/dimensions, DM3 version/length/byte order and MRC's MAP marker
provide structured evidence beyond a short prefix. The DM3/MRC collision when a
big-endian MRC width equals three is covered by a regression test. The fixed
prefix budget is 8,192 bytes, including HDF5 superblocks after user blocks up to
4,096 bytes; larger user blocks require explicit HDF5 selection.

TIFF is discovered as its container family. Detector-specific MARCCD/Pilatus
metadata remains available through an explicit `Format::DetectorTiff` selection;
filename suffixes do not silently change the detector interpretation. EDF-prefixed
GE files similarly retain an explicit detector choice when source content does
not distinguish the acquisition convention.

With `hdf5` enabled, `Input::discover` inspects creator/default/description
metadata through the existing explicit link resolver, with cumulative metadata
and traversal limits. The existing variable-length HDF5 text helper checks its
text limit after native allocation; this pass does not establish a preallocation
bound inside the native library for those strings. HDF5 signatures remain
recognizable when the optional feature is disabled. Generic HDF5 does not imply
an arbitrary dataset selection. `Input::read_hdf5(selection, options, vds_options)`
selects the original native typed reader and optional exact VDS bindings.
`read_sparse(selection, options)` exposes the original sparse reader. Its default
retains stored components/events; dense background reconstruction needs the
existing explicit opt-in. The owning wrapper retains the capture while
`hdf5_reader()`/`sparse_reader()` expose their specialized metadata/component APIs.

## Origins, links and source changes

An anonymous stream has no relative-file origin. A staging filename or a stream
object's claimed name is not used as one. Following EDF/HDF5 external references
requires both an explicit source policy and an explicit existing origin directory
for a stream. Missing targets remain errors. Exact VDS filename bindings retain
their separate explicit contract. An origin is resolution context, not a claim
that the captured stream is the contents of a user file at that location.

Only root-relative links use the supplied directory. HDF5 intermediate files
resolve their relative links from their own tracked actual parents. Every
traversed local file remains an immutable snapshot. Source replacement/growth and
private capture changes invalidate subsequent reads and clear visible output
buffers. No captured counts are silently substituted for changing sources.

`Input::source_path()` exposes the checked physical snapshot for specialized
existing parsers. Keep an `Input` clone alive until those parsers are dropped.
That path describes transport storage, not the stream's original filename.

`edf::recovery::Reader::from_reader` captures **encoded** bytes directly, then
uses the existing member-aware recovery parser. It preserves encoded offsets,
verified spans, missing samples and checksum/truncation diagnostics. The strict
complete-reader transport is not used to sanitize an incomplete gzip/bzip2 source
before recovery. Its explicit source policy and origin rules still apply.

The separate EDF header facade retains original local/general headers and can
index complete headers when samples are truncated. It exposes no recovery
reader or sample methods. The upstream lazy BytesIO EDF object can fail its
`.data` getter despite supplying shape/dtype/header fields; that reference
limitation is recorded explicitly, and metadata access alone proves no counts.

Discovery's positive Eiger layout evidence uses conventional `data` or numbered
`data_<digits>` arrays. The existing explicit typed Eiger reader retains its
broader `starts_with("data")` source enumeration. Containers mixing detector
arrays with ancillary `data...` objects are outside this corpus qualification;
select an exact dataset for those inputs pending a separately qualified naming
policy. This pass does not silently change existing explicit-reader enumeration.

## Qualification on the unchanged FabIO files

The [schema 10 report](fabio-parity/2026-11-06-input-apis.json) runs **237 unchanged
upstream tests**: the original 213 codec tests plus 23 opening tests and one
header-opening test. There are no failures/errors and one existing slow-test
skip. Both imported reference helpers and added test modules match the pinned
FabIO 2026.6.0 source hashes. No reference objects or assertions are replaced.

All **626 native stdin record comparisons match** their corresponding path
reads. Each subprocess receives the same encoded file on stdin through the
public capture APIs, the same explicit format/read/view/recovery options, and
an explicitly supplied origin directory. The comparison includes entire JSON
records: counts/type/shape hashes, auxiliary arrays, source metadata, explicit
views, VDS mappings, sparse components and integrity/missing-span reports. Only
exact private root paths declared by the executable are mapped to the original
transport root. Every replacement is recorded; other source paths and metadata
are retained. macOS canonical aliases are declared by the executable rather
than guessed by the bridge.

Of those events, 26 are metadata-only; 600 compare complete native JSON records,
including recovery/view records. That does not turn partial spans into complete
images. The original eleven partial-source events and two independently
qualified reference decoder defects keep their prior classification. All 580
original codec events retain their previous pixel/reference outcomes.

| Content decision | Events | Evidence/contract |
|---|---:|---|
| Selected codec family matches | 610 | Original content signature or structured header |
| Generic HDF5/container selection alias | 6 | Explicit dataset/convention remains separate |
| GE with EDF-prefixed acquisition header | 1 | Actual native `ApsEdf8192` source kind and original EDF fields |
| Conservative ambiguity | 4 | Blank GE or spreadsheet text requires explicit format |
| Explicit raw configuration | 2 | Original dimensions/type/order/offset retained |
| Strict outer-integrity rejection | 3 | Independent recovery report proves the original wrapper is incomplete/corrupt |

Every content decision is accounted for. Actual pinned FabIO path/BytesIO and
magic-only routing choices are recorded separately, including filename fallback,
container conventions and reference stream limitations. This is qualification
of explicit content/input contracts, not identical Python object types or a
claim that malformed wrappers pass strict discovery.

Three additional original EDF `readheader` calls have independent local/general
header comparisons and matching native stdin reads. The EDF lazy BytesIO pixel
getter error remains explicit; only header declarations are compared at that
read event. Header-only `declared_shape`/`declared_dtype` fields cannot be used
unless `header_only=true` and `pixels_available=false`. Incorrect, absent or
contradictory declarations fail comparison.

[Native validation](fabio-parity/2026-11-06-input-apis-native-checks.json) passes
240 Rust tests, 36 Python controls, formatting, strict all-feature Clippy,
no-default checks/eight tests and all 21 isolated feature checks. Separate
[feature-disabled evidence](fabio-parity/2026-11-06-input-apis-feature-disabled.json)
proves positive content recognition still reports unavailable JPEG/JPEG2000,
HDF5 and MRC decoders, while available NPY stdin reads continue to match.
All source, executable, dependency, library and policy fingerprints remain
unchanged during the full run.

`input_api_parity`, `header_only_parity` and
`metadata_attributes_accounted_for` are true for this measured scope. Exact
metadata and full reading parity remain false. Remaining API work includes
neighboring-file series/frame navigation and companion readers such as Xcalibur
`.run`; variants outside the corpus and the native HDF5 qualifications above
remain separate. Existing reference bugs, display conventions and absent source
samples are retained as such.

File-series/frame navigation and run/CIF companion access are subsequently
qualified in [SERIES_COMPANIONS_PARITY.md](SERIES_COMPANIONS_PARITY.md). That
continuation also strengthens source-path alias checks without changing this
input API's count/provenance contracts.

## Streaming raw HDF5 codec blocks

`hdf5::raw::RawReader::read_frame_blocks` accepts the same immutable, full-frame
chunk snapshots as `read_frame_into`. It delivers borrowed `PixelBlock::U16` or
`PixelBlock::U32` slices with a linear offset in stored image order. Reuse one
`BlockBuffer` per worker. The compressed frame is read into memory, while decoded
storage is bounded by the admitted codec block size. This API does not infer
orientation, discard reserved counts, apply masks, or normalize intensities.

Callbacks are provisional. A later corrupt block, consumer error or final source
change check can fail after earlier blocks have been delivered. Consumers must
abort their frame accumulator on any error and publish a result only after the
reader returns `Ok(())`. `BlockBuffer::shape()` is set only after success.

Dioptrin's `Fixed1dPlan` supplies a fixed-mask float64 accumulator for this API.
The two crates remain independent: applications connect the callback to the
accumulator. See the [matched fused integration benchmark](benchmarks/2026-11-07-fused-fixed/README.md)
for a complete bridge and measured scope. This API does not implement dynamic
mask plugins.

`RawReader::fold_frame_blocks` splits **one frame** into contiguous codec-block
ranges and decodes those ranges concurrently. Reuse a `ParallelBlockBuffer` and
supply `PipelineOptions` with the core count and decoder payload budget. An init
closure creates private consumer state for each range; a consume closure receives
its ordered blocks. The returned states cover the image exactly in range order.
They are returned only after all workers and the final source check succeed. On
any failure every state is discarded; callbacks must not publish partial results.
The compressed frame is read once, with separate decoded blocks and scratch per
core. Payload budgets exclude consumer state and block-descriptor metadata.

Connect these ranges to Dioptrin's `begin_range`, then call `finish_ranges` only
after `fold_frame_blocks` succeeds. This merges float64 partial histograms and
normalizes once. Thread launch/join happens within each frame call; do not combine
8 frame workers with 8 block workers without considering CPU oversubscription.

Each call processes one frame. Applications can reuse the reader, buffers and
fixed integration plan for successive frames, or apply the same approach to
compatible frames from other files. The decoder is mask-agnostic; the fused
Dioptrin accumulator currently requires a fixed mask. No Dioptas batch/map
selection or dynamic-mask behavior changes as part of these Rust APIs.
