# FabIO rejection continuation, 2026-11-06

**All 91 preceding rejection events are classified; full reading parity remains
incomplete.** The complete inventory retains 36 registry entries / 35 unique codecs.
The full pinned suite runs 213 reference tests with no failures/errors and one
upstream skip. OpenJPEG 2.5.4 exercises the optional Glymur variants.

| Native outcome | Read events | Compared frames |
|---|---:|---:|
| `counts_match` under recorded format policy | 482 | 2,404 |
| Additional explicit display/encoding/dtype view | 26 | 26 |
| Explicit deterministic sparse reconstruction | 1 | 100 |
| Mismatch | 2 | 2 |
| Native input/layout rejection | 44 | — |
| Reference returns no pixels | 25 | — |
| Missing decoder / adapter / optional feature | 0 | — |

The 509 matching events / 2,530 frames span 254 distinct source hashes. Repeated
reads are events, not distinct files. Direct comparison policies retain the
existing three R-AXIS expanded-intensity and four KCD sum events; they are not all
default source-word reads. Original words/readouts remain available and tested.
TIFF/Bruker/Oxford reference views retain raw signatures separately. Sparse
backgrounds are derived, and the actual 100-frame sparse file has zero events;
independent fixtures provide separate nonempty/extreme-event evidence.

Compared with the [MAR345/sparse baseline](2026-11-05-mar-sparse.json), matching
events increase 462→509 and native rejections fall 91→44. The two known Pillow
planar16 display defects remain visible mismatches. No comparison errors,
timeouts, frame-count mismatches, source-component mismatches or compared header
differences occur. All installed reference-codec hashes match the pinned checkout;
production sources, bridge/inventory and copied executable remain unchanged
during the [full run](2026-11-06-rejection-continuation.json).

## What the 91 rejections meant

The [complete source audit](../rejection-audits/README.md) covers 31 distinct input
hashes with original event/test multiplicity. It finds 79 malformed, intentionally
truncated or inconsistent-header events; eight demonstrated codec/layout or legacy
compression variants; two unavailable Lambda source-graph events; one mixed-tag
legacy dialect; and one ThunderScan case whose compressed integrity is unqualified.
Input condition and reader action remain separate.

Native changes close five events: Oxford TY5 (two), Group3 TIFF (two), and opaque
preservation of an invalid descriptive TIFF field (one). TY5 signed32 counts match
an independently compiled pinned BSD3 dxtbx decoder; the Python float64 API is a
separate exact representation view. TIFF Group3 reuses mature fax Huffman tables
under strict row framing and source windows. No rows or EOL markers are invented.

Another 42 EDF events close only under explicit `ExternalWindow::UpToDeclared`.
The source contains every required sample but declares an oversized external
window. Default `Exact` stays strict; declared/effective sizes and source policy
are recorded. Missing pixels, incomplete codecs/checksums, source changes and
inline size conflicts remain errors. Both 20-frame source arrays and compared
headers match completely. The raw/gzip/bzip2 wrapper integration retains original
header/payload snapshots.

| Remaining rejected group | Events |
|---|---:|
| EDF inconsistent/truncated inputs | 26 |
| TIFF short/missing declared structural windows | 10 |
| Demonstrated TIFF codec/layout gaps: old-style TIFF JPEG (two files), subsampled YCbCr, old LZW | 4 |
| Mixed strip/tile legacy dialect | 1 |
| ThunderScan with unknown complete-source integrity | 1 |
| Lambda VDS layout and unavailable exact source graph | 2 |
| **Total** | **44** |

Seventeen reference input errors agree with native rejection; one deliberately
missing Python JPEG backend is an environment difference. Xcalibur remains
unexercised as a count reader because its pinned `.read` fabricates placeholder
zeros; its substantive mask-characteristics API is a separate target. Complete
semantic metadata, autodetection, direct streams, series and unsupported variants
also keep the full-parity gate red.

## Validation

[Recorded local checks](2026-11-06-rejection-native-checks.json): 139 Rust tests
pass, including opt-in actual container/fixed/Esperanto/MAR345 source checks;
formatting and strict all-target/all-feature Clippy pass. No-default check/tests
and isolated strict all-target Clippy for all 20 optional feature selections pass
(22 feature checks; 25 checks including the three required full-build checks).
The old missing-EOL reproducer now rejects, and complete four/eight-byte opaque
literals survive. Independent fixtures retain source/reference provenance.

The [minimal-feature proof](2026-11-06-rejection-feature-disabled.json) runs 47
reference tests with no failures/errors and one upstream skip: 129 events are
feature-disabled, four direct comparisons match, and the known stale-TY1 EDF
source stays rejected. Four reference rejection calls are feature-disabled.
There are no missing adapters/decoders; sources/bridge/probe remain stable.

Every baseline rejection has a verified current outcome in
[the combined audit JSON](../rejection-audits/2026-11-06-rejections.json). Exact
original bytes qualify 85 events / 25 hashes; six ephemeral cases use explicitly
labelled recreated pinned-test evidence. No case is discarded. Expanded platform
CI, publication and new throughput qualification are not established.

## Per-codec comparisons

| Codec | Direct match | Additional explicit view | Reconstruction | Mismatch | Rejected | No pixels |
|---|---:|---:|---:|---:|---:|---:|
| GEimage | 6 | 0 | 0 | 0 | 0 | 0 |
| HiPiCimage | 6 | 0 | 0 | 0 | 0 | 0 |
| OXDimage | 10 | 2 | 0 | 0 | 0 | 0 |
| binaryimage | 2 | 0 | 0 | 0 | 0 | 0 |
| bruker100image | 9 | 0 | 0 | 0 | 0 | 0 |
| brukerimage | 15 | 1 | 0 | 0 | 0 | 0 |
| cbfimage | 17 | 0 | 0 | 0 | 0 | 0 |
| dm3image | 6 | 0 | 0 | 0 | 0 | 0 |
| dtrekimage | 17 | 0 | 0 | 0 | 0 | 1 |
| edfimage | 175 | 0 | 0 | 0 | 26 | 0 |
| eigerimage | 7 | 0 | 0 | 0 | 0 | 0 |
| esperantoimage | 9 | 0 | 0 | 0 | 0 | 0 |
| fit2dimage | 4 | 0 | 0 | 0 | 0 | 0 |
| fit2dmaskimage | 8 | 0 | 0 | 0 | 0 | 0 |
| fit2dspreadsheetimage | 3 | 0 | 0 | 0 | 0 | 0 |
| hdf5image | 6 | 0 | 0 | 0 | 0 | 0 |
| jpeg2kimage | 8 | 0 | 0 | 0 | 0 | 0 |
| jpegimage | 3 | 0 | 0 | 0 | 0 | 0 |
| kcdimage | 4 | 0 | 0 | 0 | 0 | 0 |
| lambdaimage | 10 | 0 | 0 | 0 | 2 | 0 |
| limaimage | 6 | 0 | 0 | 0 | 0 | 0 |
| mar345image | 30 | 0 | 0 | 0 | 0 | 0 |
| marccdimage | 6 | 0 | 0 | 0 | 0 | 0 |
| mpaimage | 1 | 0 | 0 | 0 | 0 | 0 |
| mrcimage | 8 | 0 | 0 | 0 | 0 | 0 |
| numpyimage | 12 | 0 | 0 | 0 | 0 | 0 |
| pilatusimage | 13 | 0 | 0 | 0 | 0 | 0 |
| pixiimage | 6 | 0 | 0 | 0 | 0 | 0 |
| pnmimage | 2 | 0 | 0 | 0 | 0 | 0 |
| raxisimage | 3 | 0 | 0 | 0 | 0 | 0 |
| sparseimage | 0 | 0 | 1 | 0 | 0 | 0 |
| speimage | 42 | 0 | 0 | 0 | 0 | 0 |
| tifimage | 23 | 23 | 0 | 2 | 16 | 24 |
| xcaliburimage | 0 | 0 | 0 | 0 | 0 | 0 |
| xsdimage | 5 | 0 | 0 | 0 | 0 | 0 |

ADSC aliases d*TREK. Historical reports remain unchanged: [original baseline](2026-11-05.json),
[typed HDF5](2026-11-05-hdf5.json), [parallel readers](2026-11-05-parallel-readers.json),
and [MAR345/sparse](2026-11-05-mar-sparse.json). The [complete ledger](../FORMAT_COVERAGE.md)
and [comparison guide](../FABIO_PARITY.md) retain every target and reproduction details.
