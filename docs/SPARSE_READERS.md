# Sparse HDF5 source data and explicit reconstruction

The optional `sparse` feature includes `hdf5` and `native`. It exports the same
reader as `sparse` and `hdf5::sparse`, reusing the native HDF5 library, standard
dtype decoder, immutable source graph, lossless filter/chunk/layout admission and
fill policy. No Python, pyFAI, JSON parser or random-number generator is a runtime
dependency. The parity probe compares source components/events separately from
the selected deterministic background view. Outer HDF5 transport wrappers remain
unsupported.

## Stored source representation

`Reader::open(path, Options::default())` follows the exact root `default` entry
and the entry's `pyFAI_sparse_frames` (preferred when present) or `default` group.
`Reader::open_snapshot(path, Selection::Group(path), options)` selects a precise
group instead. Optional entry `sparsify`/`peaks` metadata is indexed for declared
default discovery; explicit group selection indexes that group only. No sibling
file, alternate detector group or partial background is silently substituted.

The default `Reconstruction::StoredSparse` exposes no invented full-frame pixels.
`read_sparse_frame_into(frame, &mut SparseFrameBuffer)` returns original index
and intensity words for that frame, preserving order, dtype, reserved values and
floating bit patterns. Its `source_range()` identifies the original frame_ptr
span; `shape()` identifies the stored detector geometry. Event indices remain
flattened in stored C order. `frames()` is exactly `frame_ptr.len() - 1`.

`read_component_into(name, &mut ArrayBuffer)` returns an entire numeric source
component under a separate byte limit. Mask/radius/background_avg/background_std,
normalization, dummy, frame_ptr, indices and intensities all remain available.
Additional numeric/text components are retained recursively, including declared
entry sparsify configuration and peak metadata. Text content is retained verbatim
as logical HDF5 strings, without parsing/reformatting JSON. Metadata records each
component's source file/path, original shape, standard dtype/byte order, storage,
filters, fill value/time and allocation status. Attributes retain original shape,
HDF5 type descriptors and native numeric or text values. Raw HDF5 physical heap,
compression bytes and fixed-string padding are not a second file copy.

## Explicit deterministic derived image

Dense reads require constructing the reader with
`Options { reconstruction: Reconstruction::DeterministicBackground, ..default }`.
`read_frame_into(frame, &mut FrameBuffer)` then returns a derived view and
`metadata().dense_is_derived` is true. The model background is not represented as
measured detector counts. Stored sparse reads/components remain available under
the same explicit reader, without applying model arithmetic to their samples.

The selected policy matches pinned FabIO's native Cython `NOISY=False` model:

1. Admit a finite increasing radial array, uniform within source float precision.
   The endpoint grid defines `fpos = (mask - radius[0]) * (bins-1)/(last-first)`.
2. Interpolate the stored f32 background in f64 using adjacent endpoint-grid bins.
   The terminal bin extends until `fpos == bins`; invalid/outside positions use
   the declared dummy (or float NaN/integer zero when absent).
3. If present, multiply the modeled background by the source normalization cast
   explicitly to f32, matching the reference. This affects modeled background
   only. Integer results round halfway away from zero, with range checks.
4. Copy each original event word last, using its native type. Events retain exact
   u64/i64 values above 2^53, NaN payloads and signed zero. No float roundtrip,
   normalization or integer rounding touches those source event words. Native
   reference semantics also preserve events at otherwise masked locations.

Without any background arrays, this explicit reference view starts from zeros
and overlays source events. It does not apply a dummy-only mask in that case,
matching the native reference. Partial background triples, irregular radial grids,
mismatched mask/radius float widths, non-f32 averages, inexact dummy conversions,
integer overflow and finite f32 overflow fail the requested derived read. Their
otherwise admitted raw components/events remain available. Stochastic noise,
background_std interpretation and cutoff-pick synthesis are not implemented.

These are deliberate source/model boundaries: the Python fallback and native
reference differ in mask/event precedence and out-of-grid behavior. Qualification
here explicitly names the native implementation. Unsupported big-endian buffers
in the reference remain visible reference gaps; native stored words and independent
hand-specified derived results still have exact comparisons.

## Bounds and source identity

The complete index/intensity lengths must agree; frame_ptr is a 1D integer array
starting at zero, nondecreasing, and ending at the exact event count. Indices must
be nonnegative, inside the detector and unique within each frame. Unsorted source
order is preserved; duplicate checks sort an independent event-sized scratch
vector. Scratch size is admitted under frame_bytes, so opening tiny-event frames
does not clear a detector-sized table per frame. Duplicate positions across
separate frames remain legal. Radius and background/normalization shapes are
checked independently; background_std is retained without enabling noise.

The existing `typed::ExternalLinks::Reject` policy is the default. Explicit
`FollowSnapshot` follows exact filenames and snapshots the master, intermediate
links and final data files through the existing `Sources` resolver. VDS, external
raw storage, custom precision/enums/compound types and unqualified filters fail.
Only the existing lossless HDF5 filter whitelist is admitted. Defined HDF5 fill
is a source logical value; undefined/unallocated numeric regions cannot invent
counts. String metadata must be allocated when nonempty.

Every source is checked before and after reads. All indexed dataset shapes stay
frozen; selected frame-pointer spans and event indices are revalidated on sparse
reads. Ordinary mutation/replacement or growing arrays invalidate reused sparse,
dense and component views and require reopening. No reconstructed image cache is
kept; mask/model/event buffers are freshly read for each dense request. Concurrent
reads use independent caller outputs through HDF5's native library mutex. This
contract does not promise detection of adversarial writes preserving all file
identity metadata or provide live/SWMR refresh.

`Limits.hdf5` bounds frames, output/event/scratch bytes, decoded chunks, retained
metadata/text, datasets, group/attribute names, linked sources and traversals.
`events` caps the complete stored event count. `component_bytes` bounds each
explicit whole-component read. The cached frame_ptr is checked metadata, not a
cached image; its allocation is charged before reading. Retained text vectors,
including empty strings and the metadata copy, are charged by element count.

These are admission/working-buffer limits, not a process RSS guarantee. HDF5
native caches/codecs, temporary conversion/model buffers, allocator overhead and
caller outputs coexist. Variable-length HDF5 text still requires the native
library to allocate its strings before their total length can be checked; this
inherits the existing native metadata limitation and is not a hard allocation
sandbox for arbitrary hostile HDF5 files.

## Independent evidence

`scripts/generate_sparse_fixtures.py` creates six independent CC0 valid files,
malformed numeric/storage/link cases, expandable control arrays and a two-hop
external graph using explicit arrays and independently specified dense outputs.
Tests preserve exact native event/component hashes, unsorted event order,
positive/negative halfway rounding, mask overrides, floating NaN payloads, u64
extrema, source byte order, all source links, growth/mutation, resource admission,
unsupported storage and parallel reads. No FabIO/pyFAI writer makes these arrays.

Reproduce exact source-array and derived comparisons using the pinned FabIO2026.6.0
commit `9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`:

```sh
cargo build --release --no-default-features --features sparse --example sparse_probe
/path/to/reference/python scripts/sparse_reference.py --corpus /path/to/fabio/testimages --output /tmp/sparse-reference.json
```

[Scoped reference evidence](sparse/2026-11-05-reference.json) records the probe
hash, immutable source hash, all numeric component hashes and all100 declared
frames of `output_sparse_0_00000.h5`. The actual file has **zero stored events**;
its mask is2164x2068 and its derived output is f32. All100 reconstructions match
native FabIO bit for bit, while original sparse component equality is reported
separately. Nonempty-event correctness, including exact u64 overlays, is qualified
by the independent fixtures; this actual file cannot establish it. This is a
count/reconstruction comparison, with no throughput or universal-format claim.

Large upstream files remain outside Git and come from FabIO's existing test-image
corpus. The native HDF5/Bitshuffle license notices remain intact. The MIT sparse
format and endpoint-grid reference are attributed to Jérôme Kieffer/ESRF in
`licenses/native-readers/FabIO-sparse-MIT.txt`. No GPL/LGPL implementation or random
background generator is incorporated.
