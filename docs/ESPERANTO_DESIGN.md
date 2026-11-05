# Esperanto native signed32 reader

The optional `esperanto` feature depends only on the existing `native` feature.
`esperanto::Reader` exposes `open(path, Limits)`, `metadata()`, `frames()` and
`read_frame_into(index, &mut FrameBuffer)`. Both `4BYTE_LONG` and `AGI_BITFIELD`
return original signed32 samples, preserving stored dimensions, row order,
negative/reserved values and all counter bits. No writer-style padding,
centering, clipping, binning or correction is applied. There is one image per
source file and no neighboring-file navigation or Python runtime.

The complete original ASCII header is retained, including spaces, CRLF and
optional final CR/SUB. All named header assignments, including unknown keys,
remain in metadata. The fixed 256-byte line declaration, version 1, line count,
line delimiters, IMAGE shape/encoding and resource bounds are checked before
pixel allocation. Header shape is authoritative even for small/rectangular
sources; the reference's data property setter can pad these nonstandard images.
Unknown versions/encodings, malformed declarations and duplicate keys fail
explicitly. Numeric detector/calibration fields remain unmodified source text
rather than being applied to counts.

Uncompressed `4BYTE_LONG` requires exactly the declared little-endian signed32
payload. AGI uses a checked u32 data-block length, original per-row offsets and
one bounded slice per declared row. The first row offset must be zero and later
offsets strictly increase inside the data block. Every row must produce exactly
the declared columns and consume exactly its indexed byte range. Each pair of
eight-value fields admits widths 1..8 only; integer offsets, two/four-byte escapes
and trailing pixels follow the existing MIT reference algorithm. All range and
escape accesses are checked. Malformed indices, unsupported widths, truncation
and unexplained data are errors rather than zero fill.

The full AGI data/index payload must exactly describe the source. The real
reference.esperanto additionally has 584 zero bytes after its index table,
aligning the encoded payload to 1024 bytes. This explicit convention is admitted
only for fewer than 1024 zero bytes ending at the next 1024-byte boundary. The
padding count and original row offsets remain metadata; nonzero/other trailing
data fails explicitly. Validation occurs when indexing and decoding; corrupt
row contents can fail at read time without exposing partial pixels.

AGI stores int32 differences, which can wrap at signed32 limits. Reconstruction
uses explicit modulo32 addition. Although NumPy's AGI primitive temporarily
promotes its cumulative sums to int64, EsperantoImage.data immediately casts
back to int32. Native reconstruction therefore matches the full pinned file
reader, including extreme counters. There is no promoted-output interpretation
or retry policy. The independent fixture generator retains intermediate NumPy
hashes only to make this reference implementation detail inspectable.

Sources are immutable snapshots. Path/open-handle identity, source length and
modification metadata are checked around indexing and every read. Changed
sources require reopening, and failed reads invalidate public output. These
checks detect ordinary changes, not adversarial writes preserving all metadata.
Payload/header/index bounds are conservative admission limits rather than RSS
limits; allocator/library bookkeeping adds memory. A worker-owned FrameBuffer
reuses the encoded allocation and native int32 output without a decoded cache.

## Existing implementation reuse and licensing

Registry search found cryiorust's Esperanto implementation under GPL-3.0+, which
is not incorporated. The pinned FabIO `compression/agi_bitfield.py` and
`esperantoimage.py` are explicitly MIT. The bounded native decoder adapts this
existing algorithm rather than adding another codec/dialect. Its ESRF copyright,
authors and full MIT notice are retained under
`licenses/native-readers/FabIO-Esperanto-AGI-MIT.txt`. The full pinned FabIO
notice also remains. Python/NumPy/FabIO are external fixture/reference tools only.

## Verification

Twenty small independent CC0 files cover all packed field widths, two/four-byte
escapes, first/tail pixels, multiple groups, original dimensions, integer extrema
and wrapping. A separate Python bit packer generates source bytes, compares
against the existing AGI decoder with its signed32 cast and records complete
source/native-sample hashes. Rust tests read these exact bytes and verify every
sample bit/shape plus original headers and unknown fields. Malformed header
lines, dimensions, indices, fields, alignment padding, truncated data, resource
limits and changed sources fail without exposing partial output.

Actual upstream source files `reference.esperanto` and
`sucrose_1s__1_1.esperanto` match the pinned full file reader in original int32
dtype, shape and all counts. Four additional files produced by its own writer
(two original images times both encodings) also match. Source/sample hashes are
retained in `tests/data/esperanto/upstream-provenance.json`; the source payloads
remain external. Outer wrappers are handled by the separate wrapper API during
integration. These correctness comparisons do not establish throughput claims.
