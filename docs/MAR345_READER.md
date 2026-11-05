# MAR345 / MAR555 native PCK V1 input

The optional `mar345` feature depends only on the existing `native` feature.
`mar345::Reader` exposes `open(path, Limits)`, `metadata()`, `frames()` and
`read_frame_into(index, &mut FrameBuffer)`. One source image produces a U32
buffer of original unsigned counts in stored row order. Metadata distinguishes
`base_pixel_type=U16` from the U32 output/high-value counts. No orientation,
intensity multiplier, geometry, correction or neighboring-file substitution is
applied. There is no Python runtime, new native codec dependency or decoded cache.

## Source layout and metadata

The [manufacturer's format description](https://marxperts.com/man/html/mar345_formats.html)
defines unsigned16 base pixels with a one-based address/value table for wider
counts, a fixed 4096-byte header and the first stored pixel at the upper left.
The native reader retains every header byte, all 16 original binary words,
byte order, source text assignments/repeated entries, the exact PCK marker and
original high-count records/padding. Geometry and units remain original metadata.
Text uses a reversible Latin-1 mapping because the real older file contains
non-ASCII DATE bytes. Its original bytes remain authoritative.

The header marker must decode exactly as 1234 in one byte order. Positive shape
and exact pixel-product division, collection mode, compressed storage format,
text terminator and matching HIGH/FORMAT declarations are checked. Text FORMAT
admits corpus `PCK` and documented `MAR345`/`PCK345` spellings. A non-square
MAR555 shape is retained rather than forced square. Width/height below two are
explicitly unsupported because the admitted original predictor requires its
initial neighborhood. Raw spiral and unknown formats fail rather than retry.

Two high-record layouts are explicit: the manufacturer-specified table padded
to groups of eight pairs, and the compact table in the real Mar555 source. The
marker must start at exactly one of these calculated boundaries. It is not
searched for arbitrarily inside bytes. Marker dimensions must agree with the
binary header. Every used one-based index must be inside the image and unique.
Unused padded pairs must be zero, and are retained in raw table metadata.
Unsigned32 value bits remain exact, including values above signed32 range.

## Existing MIT decoder adaptation

The bounded PCK bit reader and predictor adapt the existing MIT
`ext/mar345_IO.pyx` V1 implementation by ESRF contributors. Its copyright,
authors and full MIT notice are retained in
`licenses/native-readers/FabIO-MAR345-MIT.txt`. No LGPL `ccp4_pack.c` code or
GPL/LGPL runtime decoder is incorporated. The previous backend/library audit is
retained in `MAR345_XCALIBUR_AUDIT.md`.

A V1 block uses six header bits, a power-of-two sample count from 1 through 128,
and signed widths 0/4/5/6/7/8/16/32. All reads and complete output counts are
bounded before indexing or writing samples. No block can overrun the image.
The original flattened four-neighbor predictor, C-style division toward zero
and modulo16 reconstruction are explicit integer operations. High counts are
then applied from the original checked table. The encoded stream must end at
exactly the final used byte with zero unused bits; truncation, extra bytes and
nonzero bit padding fail without exposing partial output.

The source dtype is authoritative where the pinned reference has defects:

- The reference sign-extends signed16 values into U32 for the first width+1
  pixels. Native output widens their original unsigned16 counts instead.
- The pinned compiled V1 decoder loses nonzero 32-bit fields through an
  overflowing C integer mask. Native extraction uses a bounded u64 mask.
- The reference trims whitespace before packed bytes. A legal first packed
  byte can be ASCII space; native input starts immediately after the marker's
  one delimiter and retains that byte.
- The reference big-endian header branch assigns `byte_order`, while overflow
  parsing uses `byteorder`. Independent big-endian oracle reads explicitly
  select the source order before reading; production uses the validated marker.

These edge differences are independently exercised and recorded, not silently
presented as reference equality. Both actual corpus sources match the reference
without count changes. No diagnostic interpretation or automatic retry exposes
misread reference counts as source data.

## PCK V2 audit and unsupported scope

The pinned MIT `unpack_pck` docstring explicitly states V1 only. Its surrounding
function recognizes V2 markers but still calls that V1 function by default;
only the separate LGPL path selects the older V2 routine. Recognizing the
marker is therefore not evidence of a working permissive V2 backend.

The manufacturer's public description establishes the surrounding MAR345
layout but does not provide an authoritative separate V2 grammar. No already
bounded, permissively licensed standalone native V2 backend was located in the
recorded reader evaluation. Native `open` rejects a V2 marker explicitly and
never searches onward for a V1 alternative. V2, raw spiral/old IMAGE variants,
other high-record framing, degenerate predictor shapes and automatic file-series
traversal remain visible gaps. Supporting these requires its own source/codec
contract and independent reference files; no guessed V2 decoder is included.

## Bounds, snapshots and verification

Limits admit raw/parsed header metadata, high records, encoded bytes and output
frame bytes before corresponding payload allocation. Immutable snapshots check
path/open-handle identity, length and modification metadata around indexing and
every read. Ordinary mutation/replacement invalidates the reader and public
output; adversarial writes preserving all source metadata are outside this
contract. Worker-owned FrameBuffer allocations are reused. Budgets cover payload
and conservative metadata admission rather than total process RSS.

Twenty-six small independent CC0 files verify complete source/sample hashes,
field widths, runs, predictor edge behavior, both endian tables, compact/padded
framing, header aliases/reserved bytes and the qualified oracle differences.
Tests reject malformed headers, high indices/counts/duplicates/padding,
unsupported V2/spiral, invalid block sizes, truncated streams, extra encoded
bytes, nonzero unused bits, resource limits and changed sources. Failed reads
never expose partially reconstructed counts.

The real `example.mar2300` (2300x2300) and `Fe3O4_023_101.mar2560` (3072x2560),
plus both endian rewrites emitted by the pinned writer for each source, match
native U32 dtype, shape and every count. Complete source/sample hashes are in
`tests/data/mar345/upstream-provenance.json`; original large files remain external.
Outer gzip/bzip2 staging is the separate wrapper integration's responsibility.
These are correctness comparisons, not throughput benchmarks or a universal
MAR345-variant claim.
