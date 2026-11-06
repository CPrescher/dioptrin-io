# Explicit EDF compatibility and available-sample recovery

The ordinary `edf::Reader` keeps its strict defaults. The explicit policies and
separate recovery API add access without manufacturing missing detector values.
[The source proof](edf-recovery/2026-11-06-proof.json) covers the remaining 26 EDF
baseline events across 14 source hashes: 16 complete-source compatibility reads
and 10 deliberately truncated sources. The full
[68-event EDF audit](edf-recovery/2026-11-06-updated-edf-audit.json) retains the previous
42 external-window closures and all original source/test multiplicities.

## Complete legacy sources

`Options.inline_layout = InlineLayout::LegacySizeAtParsedBoundary` selects the
actual parsed closing newline and requires legacy `Size` to describe complete
uncompressed inline scalar storage. It deliberately chooses that physical block
size for navigation. It does not activate after another interpretation fails,
does not shorten external windows, and does not reinterpret compressed bytes as
raw samples. Actual header/resource bounds remain mandatory. The source's stale
`EDF_HeaderSize` and `EDF_BinarySize` remain visible.

`Options.payload_interpretation = PayloadInterpretation::StoredRawForLabel("TY1".into())`
is an explicit assertion about stored encoding. The declared label must match
(ASCII case ignored); no TY1 or other codec is attempted before reading raw
words. This admits the actual `d80_60s.edf` raw I32 payload while retaining its
stale TY1 declaration. It does not claim that the bytes are valid TY1 bytecode.
There are no runtime filename/hash exceptions or automatic raw fallbacks.

Metadata retains original header bytes and fields, original
`declared_stored_bytes`, effective `stored_bytes`, `declared_compression`,
`payload_interpretation`, `inline_layout_policy`, `declared_header_bytes`,
`parsed_header_bytes` and `legacy_size_bytes`. The existing `compression` field
identifies the effective codec under the selected explicit interpretation.
Original counts, dtype, scalar bit patterns and stored row order are unchanged.

All three split MultiFrame source hashes (15 events) match pinned FabIO exactly
in full sample bytes, dtype and shape under the selected Size policy. The raw
TY1-labelled source (one event) also matches exactly. The independent parser
agrees with the same source-byte interpretation. Their conflicting declarations
remain source inconsistencies rather than being relabelled well-formed files.

## Prefix recovery instead of dense padding

`edf::recovery::Reader::open(original_path, limits, options)` opens the original
bytes explicitly. It must receive original gzip/bzip2 bytes rather than a staged
strict transport view: strict transport correctly refuses incomplete envelopes.
Recovery source staging is private to EDF and reuses the existing mature native
codecs and source-snapshot primitives. Shared transport defaults are unchanged.

`image_records()` counts complete image headers, including records with zero
available samples. `metadata(i)` exposes declared source metadata.
`read_samples_into(i, &mut SampleBuffer)` exposes exactly the physically present
complete scalar words in a contiguous linear prefix, preserving dtype and byte
order interpretation. It never allocates the declared dense image with missing
zeros, whites, NaNs or neighboring data. Header-only sources have a correctly
typed zero-length sample vector, not a shape-sized zero array.

`SampleBuffer.pixels()`, `report()` and `partial_element_bytes()` are valid only
after a successful read; all return `None` after any failed read. The partial
element bytes retain their original source byte order and are never zero-extended
into another sample. The report contains:

- Original declared shape/count and the present linear sample range.
- Exact missing sample range and independently verified sample prefix.
- Expected/available payload window bytes and availability status.
- Outer/payload integrity, payload-member ranges and diagnostics.
- `complete_verified_frame`, independent of mere sample availability.

A complete header followed by no data reports `HeadersOnly`, an empty typed
prefix and the complete missing range. A truncated next header is retained as
exact bytes with its source offset; no missing frame shape is invented.
Semantic failure in a later complete header retains that original header as an
`UnparsedRecord`. Successfully indexed earlier images remain accessible.

## Compression integrity and complete earlier members

Integrity is separate from availability:

| State | Meaning |
| --- | --- |
| `ContainerComplete` | Complete compression envelope, or original unwrapped bytes; this does not assert a complete EDF sequence |
| `VerifiedMember` | This span is within completed verified members of an otherwise incomplete envelope |
| `UnverifiedTruncated` | A member/header/footer ends prematurely; decoded bytes remain visibly unverified |
| `ChecksumFailure` | The codec reports a checksum failure |
| `DecodeFailure` | Other compression/framing failure; no verified-member claim |

Gzip member headers are admitted under a separate bounded header budget before
native decoding, including optional extra/name/comment fields. Native member
completion verifies framing/footer checks. Verified member boundaries are
retained in encoded and decoded coordinates. A missing footer can leave every
declared pixel available while `complete_verified_frame` remains false. Earlier
complete verified members can still supply complete earlier image records.
No resynchronization scan skips past a failed member.

Decoded word ranges from incomplete members are labelled unverified even when
the independent known-array proof demonstrates their values. Consumers can
inspect/recover those bytes under this explicit API; they are not default
complete-source count parity. Storage windows must also be complete for a frame
to receive `complete_verified_frame`. Encoded-slot padding has the same bounded
zero/whitespace 512-byte-slot admission as the ordinary EDF codec.

## Bounds and immutable sources

The source must be a regular file, checked before opening a potentially blocking
read handle, with post-open identity checks retained. Original and staged files
are snapshotted and checked around every read. Growing/replaced/truncated sources
require reopening; there is no live refresh, decoded sample cache or source retry.
Recovery currently rejects external payload sources rather than inventing an
unqualified partial external-source graph.

Encoded source/staging output, actual headers, member count, cumulative metadata,
frame shape and payload windows have explicit bounds. Resource failures remain
hard errors rather than being softened into successful partial outcomes.
Overlong decoded output also fails and invalidates every output accessor.
Working payloads use separately bounded encoded bytes, decoded bytes and typed
sample storage; codec dictionaries and allocation bookkeeping are additional.
These are payload limits rather than an RSS/throughput claim.

## Independent actual-source evidence

The import-safe `scripts/edf_recovery_reference.py` uses independent Python
zlib/bz2 and NumPy. It parses gzip header/DEFLATE/footer CRC and ISIZE separately,
and checks exact typed prefix hashes, partial bytes, missing/verified ranges,
member integrity/ranges and stop/header records. It never imports FabIO or uses
its zero-padded arrays. The complete-source compatibility proof additionally
compares the same immutable bytes with pinned FabIO 2026.6.0.

All ten original/captured truncated cases match this independent oracle with no
differences. Available words and partial bytes also match the pinned upstream
test's known original array (0 at `[0,0]`, 20 at `[1,1]`, otherwise 10). Two gzip
cases preserve 28,057 complete F32 samples and one extra byte; missing sample
ranges remain explicit. The complete earlier frames are independently verified;
missing later samples remain genuinely unavailable.

Five original baseline gzip envelopes had been deleted by upstream cleanup.
Their captured recreations have different compressed timestamps/hashes. Both
original baseline and current proof hashes remain recorded, with explicit
recreated provenance. The other sources match exact baseline bytes. Preserved
copies are preferred so upstream test writers cannot mutate the proof subject.
No input is repaired or overwritten, and large source files remain outside Git.

Small independent CC0 fixture bytes are generated inside `tests/edf_recovery.rs`.
They cover both byte orders, unsigned64/IEEE bits, stale declarations, exact
label assertions, partial words, absent data, incomplete/malformed headers,
member/footer/CRC states, bounded padding, overlong rollback, source mutation,
resource/member/header limits and a bounded Unix FIFO regression.

```sh
cargo build --no-default-features --features edf --example edf_recovery_probe
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/audit_edf_recovery.py
```

The scoped audit generator uses the preserved `/tmp` rejection captures and
the original baseline audit pinned at Git ref `017afba`. It verifies exact
68-event source/test multiplicity before writing the updated scoped audit.
