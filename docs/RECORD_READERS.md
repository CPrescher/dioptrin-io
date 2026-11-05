# FIT2D binary, KCD and MPA source contracts

Optional `fit2d`, `kcd` and `mpa` features expose native immutable readers and
reuse the `native` feature's bounded byte/pixel utilities. No Python/runtime
codec or scientific correction is added. Each uses common source identity
checks and the same caller-owned `FrameBuffer` and `Limits` as the other native
readers. Every failure invalidates that buffer's public pixel/shape view.

Sources are frozen by canonical path, open handle and file metadata. Each read
checks the source before and after; ordinary mutation/replacement fails and
requires reopening. There is no decoded cache, series discovery, implicit
external source, live refresh, mask application, normalization or orientation
change. Identity checks do not detect adversarial writes preserving all source
metadata. Bounds admit header/text, retained metadata, encoded array and decoded
native frame allocations; metadata maps, indexing scratch, allocator overhead
and caller buffers are additional memory, not an RSS guarantee.

## FIT2D binary

`fit2d::Reader::open(path, Limits)` indexes all records and retains raw scalar
records, typed scalar fields and named array records/layouts. `frames()` is one;
`read_frame_into(&mut FrameBuffer)` reads `data_array`. The separate
`read_array_into(array_index, &mut FrameBuffer)` exposes original `data_mask`,
axis and other named arrays using metadata indices. Arrays remain independent.

The admitted stream begins with `$FFF_START` and an explicit `$BIG_ENDIAN`
record, ends with `$FFF_END`, and has physical records in multiples1..15 of
512 bytes. Both byte orders are supported. Scalar integer and IEEE f32 arrays
have rank2 and declared width32, with128 values per physical record; extra
record padding is skipped explicitly. Logical masks have declared width1 and
pack31 pixels into each32-bit word, low bit first; bit31 is storage padding.
Decoded masks are u8 zero/one and do not mask the source image. Stored dimensions,
record sizes, array blocks and decoded allocation must agree. Unknown metadata
and array encodings, duplicates, missing end marker, truncation and trailing
stacked records fail explicitly.

Array f32 bits, NaN payloads, infinities and signed zero survive byte-for-byte.
Float metadata is decoded from its actual hexadecimal IEEE bits: the pinned
FabIO `hex_to(..., "float")` helper returns a fixed unrelated constant. Raw
records make this difference auditable. Integer scalar metadata retains its
32 source bits as u32. FabIO ignores the array byte-order flag; native big-endian
fixtures preserve their source values and record this expected reference gap.
Only little-endian external FIT2D files have been qualified here.

## KCD / KappaCCD

`kcd::Reader::open(path, Limits)` preserves independent u16 readouts in file
order. `frames()` equals the source's declared readout count, and
`read_frame_into(readout_index, &mut FrameBuffer)` returns exactly that readout.
Raw ASCII header, fields, shape, source dtype and interpretation remain metadata.

Use `Reader::open_with_interpretation(path, limits,
Interpretation::SumReadouts)` explicitly to reproduce FabIO's combined image.
That reader has one i32 frame; it adds readouts using checked signed32 arithmetic.
Overflow fails instead of wrapping. The default remains capable of reading
original words even when a sum cannot fit signed32.

The currently admitted layout has exactly4100 ASCII prefix bytes followed by
exactly `readouts * rows * columns` little-endian u16 samples. Required dtype,
readout count and dimensions must be present; unknown types/orders, malformed
or duplicate fields, unsupported prefix layouts, short data, trailing bytes and
stacks fail. Fixed layout admission prevents an EOF-relative reader from silently
shifting all samples after tail truncation. Unknown KCD prefix variants and other
native sample types remain unqualified gaps. No repeats or neighboring files are
combined, and no exposure/gain corrections are applied.

## MPA / multiwire

`mpa::Reader::open(path, Limits)` indexes ASCII `DATA` spectra and `CDAT`
coincidence maps separately, retaining raw header/section records and all parsed
metadata strings. `frames()` is the number of CDAT maps, and
`read_frame_into(frame_index, &mut FrameBuffer)` reads those maps in source order.
`read_section_into(section_index, &mut FrameBuffer)` additionally exposes each
original DATA spectrum, shaped `[1, count]`; no spectra or maps are summed.

The source encoding is explicitly `AsciiF64`, with no source scalar dtype or
byte order. Integer-valued tokens outside inclusive `[-2^53, 2^53]` are
rejected before returning counts, including decimal/exponent forms such as
`9007199254740993.0` and `9007199254740993e0`. Wider integer interpretation is a
visible gap. Finite decimal overflow to infinity and nonzero underflow to zero
are rejected; explicitly written NaN/Inf tokens retain their IEEE meaning.
Other decimal/exponent values use ordinary IEEE f64 parsing. Scientific correction and
normalization remain outside this reader.

Declared section counts must match every numeric line, and CDAT size must equal
`ADC1_range * ADC2_range`. Multiple CDAT frames survive, while FabIO's primary
interface fails on the independent multi-map fixture. Non-ASCII/binary input,
external/separate headers, duplicate sections, unknown numeric lines, truncation,
extra counts and resource excess fail. Binary MPA encodings are not qualified or
silently treated as ASCII. This is a deliberate gap despite the upstream reader's
general binary-support description.

## Independent qualification

`scripts/generate_record_fixtures.py` uses only the Python standard library and
explicit arrays to generate seven CC0-1.0 fixtures. Their original-file and
canonical little-endian count hashes are in `tests/data/records/provenance.json`.
Tests cover512/2048-byte physical records, both byte orders, scalar/mask/axis
arrays,31-bit boundaries, padded scalar block boundaries, f32 nonfinite payloads,
readout ordering, explicit sums and overflow, multiple DATA/CDAT sections, exact
integer admission, malformed declarations, truncation, source mutation and bounds.

Pinned FabIO2026.6.0 commit `9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`
serves only as a test reference. Reproduce using:

```sh
python3 scripts/generate_record_fixtures.py
cargo build --no-default-features --features fit2d,kcd,mpa --example record_probe
/path/to/reference/python scripts/record_reference.py --corpus /path/to/fabio/testimages --output /tmp/record-reference.json
```

[Exact count evidence](legacy-readers/2026-11-05-record-reference.json) includes
four actual upstream files: `fit2d.f2d` and `Pilatus1M.f2d` match all primary,
mask and axis counts/types; `i01f0001.kcd` matches both independent original
readouts and the explicitly selected FabIO sum; `mpa_test.mpa` matches the FabIO
primary CDAT image and independently parsed DATA spectra. No display flips are
introduced when reproducing the upstream Fit2D/TIFF comparison. Large upstream
files from FabIO's test-image service stay outside Git. This is encoding/file
qualification and makes no arbitrary-family parity or performance claim.

Registry searches for FIT2D/KCD/MPA found no suitable detector reader.
The MIT `geddes`1.0.2 candidate was inspected; its FIT2D support is for1D `.chi`
patterns rather than the binary detector records here. GPL reader candidates
remain excluded. Parsers implement the bounded encodings
using MIT FabIO format references; original notices and named authors are
retained in `licenses/native-readers/legacy-layouts-MIT.txt` and the existing
FabIO copyright notice. Global wrapper/probe/ledger integration is separate.
