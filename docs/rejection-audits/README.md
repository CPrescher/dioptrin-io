# Audit of the 91 remaining FabIO rejections

The subsequent [TIFF codec continuation](../tiff-codec-gaps/README.md) closes four
more events: the current total is 40 rejections. [Updated verified outcomes](2026-11-06-after-tiff-codecs.json) retain every original case. The remaining tables below
record the preceding audit stage and its then-current 44 rejections.

All **91 baseline read events / 31 distinct source hashes** are classified in
[the complete audit](2026-11-06-rejections.json). Repeated reads remain counted.
The baseline is the immutable [MAR345/sparse report](../fabio-parity/2026-11-05-mar-sparse.json).
Input condition and reader action are separate: an explicit compatibility read
does not make a contradictory header well formed.

| Input condition | Events | Distinct source hashes |
|---|---:|---:|
| Inconsistent headers, malformed metadata or intentional truncation | 79 | 21 |
| Demonstrated codec/layout or legacy compression variants | 8 | 7 |
| Valid Lambda VDS descriptor with unavailable exact source graph | 2 | 1 |
| Mixed strip/tile legacy dialect requiring an explicit layout policy | 1 | 1 |
| ThunderScan with unqualified compressed integrity | 1 | 1 |
| **Total** | **91** | **31** |

## What became readable

Five events across four source hashes now have native support:

- Oxford TY5 (two events): bounded adaptation of the existing BSD3 dxtbx decoder.
  Every actual count and five independent fixtures agree with the separately
  compiled pinned native authority. Production output remains signed32, with
  original row offsets/header/trailer retained. The pinned Python API's float64
  representation is a separate exact test view.
- Group3 TIFF (two events): mature fax Huffman tables under strict bounded TIFF
  framing, exact row widths, FillOrder and complete tail/padding checks. No rows,
  pixels or EOL markers are invented. Independent source samples and the explicit
  Pillow view agree. Packed uncompressed FillOrder2 is also supported.
- Generated TIFF with invalid inline description (one event): every original
  description byte and descriptor is preserved as opaque metadata. The complete
  declared pixel window is decoded unchanged; descriptive text is not repaired.

Another **42 events across two source hashes** become readable only with explicit
`edf::ExternalWindow::UpToDeclared`. The pinned test itself calls the 140000-byte
external size wrong: the source contains 29184 bytes at offset8192, including all
28800 sample bytes. Every frame in both 20-frame sources matches exactly, including
original dtype, sample bits and compared header fields. Native `Exact` remains the
default. Metadata exposes declared/effective bytes and the selected policy.
Incomplete pixels, incomplete codecs/checksums, inline size conflicts and source
changes remain errors. The same explicit choice works through gzip/bzip2 wrappers
while tracking both original header and payload files.

These changes close **47 baseline rejection events** under the documented selected
policies. They comprise four codec events, one metadata-admission event and 42
explicit compatibility events; they are not 47 newly demonstrated well-formed
files. The combined suite report is
[the rejection continuation](../fabio-parity/2026-11-06-rejection-continuation.json).

## What remains and why

| Remaining group | Events | Boundary / next work |
|---|---:|---|
| EDF inconsistent or truncated inputs | 26 | 15 split-file reads have stale header/payload lengths; ten are deliberately truncated; one contains raw i32 pixels with a stale TY1 compression tag. No implicit fallback or zero fill. |
| TIFF malformed structural metadata | 10 | Nine repeated reads declare half the required uncompressed byte window; one omits required StripByteCounts. Complete pixels elsewhere in a file do not authorize reading beyond its declaration. |
| Demonstrated TIFF codec/layout gaps | 4 | Two old-style TIFF JPEG files (Compression=6), one LZW/subsampled-YCbCr layout and one old LSB/late-change LZW dialect. Reuse a bounded mature implementation; preserve component-grid/source semantics before display conversion. |
| Mixed strip/tile legacy dialect | 1 | Twelve tile-sized chunks use StripOffsets/Counts with TileWidth/Length, rather than tile offset/count tags. No silent reinterpretation; any future legacy mapping must be explicit. |
| ThunderScan integrity not established | 1 | A fresh Pillow/libtiff load fails at scanline 320. FabIO's broad array-conversion fallback can access the same partially loaded image. A reference array does not prove complete source counts; retain both the codec gap and integrity qualification. |
| Lambda virtual layout and source graph | 2 | Stored absolute module paths are unavailable. HDF5 basename lookup and missing-module fill change returned counts. Require explicit filename mappings, complete snapshots, validated selections/extents and an unmapped-region fill contract. |
| **Total** | **44** | No case dropped from the baseline ledger. |

The two previously documented Pillow planar16 display mismatches are separate
from these 91 rejected events. Metadata, direct-stream, discovery and series API
gaps also remain outside this count comparison; full reading parity is incomplete.

## Evidence and reproduction

The scoped audits retain each baseline hash, event count, test identities,
source conditions, original numeric declarations and independent verification:

- [EDF JSON](2026-11-06-edf.json) and [analysis](2026-11-06-edf.md).
- [TIFF JSON](2026-11-05-tiff.json) and [reader contracts](../IMAGE_READERS.md).
- [Lambda/TY5 JSON](2026-11-05-lambda-ty5.json) and [analysis](LAMBDA_TY5.md).

Exact original bytes are available for 85 events across 25 hashes. Five deleted
BadGz envelopes and the generated TIFF have explicitly labelled recreated
pinned-test evidence: gzip timestamps and the TIFF date prevent claiming that
those new bytes are the original baseline input. Source samples and format
construction are checked independently. Large detector files and captured test
inputs remain outside Git; small independent fixtures retain their provenance.

Rebuild the combined classification:

```sh
python3 scripts/aggregate_rejection_audits.py \
  --baseline docs/fabio-parity/2026-11-05-mar-sparse.json \
  --audits docs/rejection-audits/2026-11-05-lambda-ty5.json \
    docs/rejection-audits/2026-11-06-edf.json \
    docs/rejection-audits/2026-11-05-tiff.json \
  --comparison docs/fabio-parity/2026-11-06-rejection-continuation.json \
  --output docs/rejection-audits/2026-11-06-rejections.json
```

The aggregator rejects unclassified or duplicate hashes, changed baselines,
wrong event counts and mismatched test identities/multiplicities. Rebuilding
from these scoped reports is deterministic. The comparison flag verifies every
baseline event against the complete suite, recording exact source identity or an
explicitly recreated ephemeral test case, actual outcomes and compatibility
window policies. The comparison harness's optional
`--rejected-source-dir /tmp/dioptrin-rejected-inputs` preserves newly rejected
bytes before upstream cleanup and verifies their recorded hashes.
