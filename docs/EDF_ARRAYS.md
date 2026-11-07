# EDF source arrays and explicit planes

An EDF image record can contain an array of rank other than two. The native
reader now preserves that array, using the pinned FabIO `EdfFrame.get_data_rank`,
`get_data_shape` and `get_data_counts` conventions. These conventions are tested
against unchanged FabIO 2026.6.0, commit
`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`, and independent source bytes.

`Reader::frames()` and its descriptive alias `image_records()` count EDF image
records. A record with source shape `[2, 3, 4]` remains one record containing 24
samples. Its two trailing-axis planes are selected explicitly. General metadata
headers do not become image records.

## Public contracts

`Metadata::source_shape` is authoritative for rank and stored axis order.
`source_samples` is its checked product, `record_index` identifies the image
record, and `plane_count` is the product of the leading axes. The existing
`shape: [usize; 2]` describes an explicit trailing-axis plane. It does not claim
the complete source is two-dimensional. Existing two-dimensional wire output
stays unchanged; the new probe command exposes these fields separately.

| Source rank | Source array shape | Explicit plane shape | Leading coordinates |
| --- | --- | --- | --- |
| 0 | `[]` (one scalar) | `[1, 1]` | `[]` |
| 1 | `[width]` | `[1, width]` | `[]` |
| 2 | `[rows, columns]` | `[rows, columns]` | `[]` |
| 3+ | All declared/effective axes | Last two axes | One coordinate for each leading axis |

`Reader::read_array_into(record, &mut ArrayBuffer)` reads every original typed
sample. `ArrayBuffer::shape()` returns `Some(&[])` for a valid scalar; `None`
means unavailable output. `pixels()` retains the original native scalar type,
integer words, floating-point bits, reserved values and stored order.

`Reader::read_plane_into(record, leading, &mut FrameBuffer)` selects a trailing
plane in source C order. For `[2, 3, 4]`, coordinates `[1]` select flat samples
`12..24`, with output shape `[3, 4]`. No orientation, calibration, masking,
normalization or intensity conversion is applied. Negative indices are absent
from this unsigned native API; wrong coordinate rank and out-of-bounds indices
are explicit errors.

The existing `read_frame_into` accepts native rank-two arrays. It also preserves
its prior acceptance of rank-three-or-higher arrays whose leading axes are all
one. Such metadata has `FrameLayout::SingletonLeadingAxes`, and the full source
shape remains available. Other source arrays have
`FrameLayout::ArrayOrPlaneRequired` and require one of the explicit APIs; they
are never silently flattened into additional frames. Ordinary rank-two sources
have `FrameLayout::Native2D`.

All read errors invalidate every public output getter, including after a prior
successful read. Output capacity can remain reusable. The existing immutable
root/external-file snapshots, requested-path bindings, wrapper capture and
before/after checks remain in effect for array and plane reads. External source
following and compatibility policies still require explicit caller selection.
A selected plane requires the complete admitted source record and valid codec
integrity; this API does not promise partial I/O or a decoding speedup.

## Dimension interpretation and bounds

Rank is the largest positive ASCII decimal suffix among `DIM_i` field names,
with case normalization. Non-numeric fields such as `Dim_H` remain original
metadata without becoming axes. Axis order is `Dim_rank` through `Dim_1`.
Missing canonical `DIM_i` for `i > 1` defaults to one and is listed by its
one-based EDF index in `implicit_dimensions`; no field is inserted into the raw
header or original field map. A key such as `Dim_003` raises the scanned rank to
three but does not supply canonical `Dim_3`, matching FabIO's helper behavior.
General-header inheritance uses the existing explicit inheritance rules.

Rank is bounded by `MAX_RANK = 32` before shape-vector growth. Axis products,
full decoded bytes and selected sample windows are checked for overflow. Zero
axes or missing `Dim_1` are explicitly unsupported; a rank-zero scalar is
separate and contains one sample. Native dimensions require exact unsigned
integer syntax. FabIO's `nice_int` also accepts floating-point strings and can
truncate fractional dimensions; that permissive conversion is not reproduced.

`Limits::frames` bounds source records, not the number of potential planes.
`Limits::frame_bytes` bounds the complete source array, even when only one plane
is requested or a recovery source contains only its header. Existing header,
aggregate metadata, encoded-window and transport/source limits still apply.
Full source geometry and implicit-axis metadata are charged to the metadata
budget. Empty layouts and oversized/overflowing geometry fail before allocating
a full pixel array.

The existing decoder is reused. Each read requests at most one full typed
source array and, for a compressed payload, one full decoded-byte scratch
buffer, plus the bounded encoded window. Plane selection copies/truncates the
typed vector in place; it allocates no second typed full array or plane. Thus
requested payload storage is bounded by `encoded_bytes + 2 * frame_bytes`
for compressed reads, with metadata and codec/allocator bookkeeping additional.
This is not an RSS limit. Reusable capacities and caller-retained buffers are
owned by the caller and are not a hidden shared decoded cache.

## Recovery retains source rank

`recovery::Reader` indexes source image records using the same checked source
geometry. `SpanReport::declared_source_shape`, `record_index` and
`complete_verified_record` make the full-record contract explicit. Existing
`declared_shape` remains the trailing plane shape; the older
`complete_verified_frame` field is a compatibility alias for the same whole
record status. Existing two-dimensional reports keep their wire schema.

Recovery spans are flat source C-order sample positions. A declared
`[2, 3, 4]` record with five complete words reports `present_samples = 0..5`
and `missing_samples = Some(5..24)`, retaining any partial word bytes separately.
It does not create a complete plane or dense zero-padded volume. Unsupported
semantic headers remain an explicit unparsed stop with their original bytes.
Resource failures remain errors rather than successful partial indexes.

Availability and integrity stay independent: every declared sample can be
present while a gzip footer is missing or its checksum fails. Such output is
available recovery data with an explicit integrity state, never a complete
verified record. Only verified physically available words enter verified sample
ranges. Original compression labels, header declarations and effective source
windows remain visible.

## Evidence and remaining type/codec scope

The reproducible [proof](edf-arrays/2026-11-06-layout-types.json) compares complete
arrays against unchanged FabIO and independently parsed source bytes. The
CC0 [fixtures](../tests/data/edf-arrays/provenance.json) and
[generator](../scripts/generate_edf_array_fixtures.py) cover ranks zero through
five, mixed-rank source records, singleton and missing axes, general headers,
both endiannesses, raw/gzip/zlib/bzip2 payloads, 64-bit integer extrema,
integers beyond `2^53`, NaN payloads, infinities and negative zero. The proof
also reads actual `MultiFrame.edf`, its outer gzip/bzip2 copies, and
`edfGzip_U16.edf`, including original physical slot padding. It checks every
explicit plane against the corresponding untouched reference slice.

Four independent malformed/recovery sources cover complete available-word
prefixes, partial elements, headers without samples, invalid gzip checksums and
short footers. Their oracle decodes raw DEFLATE, inspects CRC/ISIZE itself and
checks ranges/integrity without using FabIO's padded missing-data output.
Original source hashes, pinned/imported Python hashes, native source hashes and
both probe hashes are captured before and after the run. Nine Python controls
require all provenance, shape, word, plane, member, missing-range and source
fields and reject incomplete proof cardinality. Seven focused Rust array tests
plus the existing 22 source/recovery/window tests pass, with strict EDF-only
Clippy. The root owns the final all-feature and no-feature matrix.

No new scalar representation was guessed. The pinned `edfimage.py` conditionally
maps `FloatIEEE128`, `DoubleIEEE128` and `QuadrupleValue` to `numpy.float128`.
That name does not establish a portable source ABI: this pinned macOS environment
has no `numpy.float128`, and its `longdouble` is eight-byte binary64. These
labels remain explicit source-representation capability gaps; no F64 alias,
byte discard or assumed x87/IEEE binary128 conversion is introduced.

The pinned EDF `OFFSET` compression branch imports an external top-level
`byte_offset` module and calls `analyseCython`; that module is absent in the
reference environment. It is not evidence that the existing CBF codec is the
same EDF dialect. EDF byte-offset remains an unqualified legacy-codec gap;
there is no failed-codec retry or raw reinterpretation. Existing explicit
legacy raw-label policy remains separate from this array implementation.

The previous [MAR345 V2 audit](MAR345_READER.md#pck-v2-support-and-qualified-reference-scope)
was reviewed read-only: the pinned permissive `unpack_pck` path is explicitly
V1-only, while V2 selection lives in the separate legacy LGPL path. This lane
adds no speculative V2 decoder or changed MAR345 claim.

Reproduce the array proof after the root release build:

```sh
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/edf_array_reference.py \
  --probe target/release/examples/api_probe \
  --recovery-probe target/release/examples/edf_recovery_probe \
  --fabio-source /tmp/dioptrin-io-fabio-2026.6.0 \
  --corpus /tmp/dioptrin-io-fabio-corpus \
  --report docs/edf-arrays/2026-11-06-layout-types.json
```
