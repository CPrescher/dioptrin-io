# TIFF codec continuation, 2026-11-06

**The four targeted TIFF gaps are closed; full reading parity remains incomplete.**
The complete inventory retains 36 registered entries / 35 unique codecs. The
pinned full suite runs 213 reference tests with no failures/errors and one upstream
skip. Detailed variant/source contracts are in [the TIFF continuation](../tiff-codec-gaps/README.md).

| Native outcome | Read events | Compared frames |
|---|---:|---:|
| `counts_match` under recorded format policy | 482 | 2,404 |
| Other explicit reference views | 26 | 26 |
| Independently verified component grids + explicit view | 4 | 4 |
| Deterministic sparse reconstruction | 1 | 100 |
| Mismatch (known Pillow planar16 defects) | 2 | 2 |
| Native input/layout rejection | 40 | — |
| Reference returns no pixels | 25 | — |
| Missing decoder / adapter / optional feature | 0 | — |

The 513 matching events / 2,534 frames span 258 distinct primary source hashes.
Events include repeated reads, not distinct files. Direct policies retain the
existing three R-AXIS expansion and four KCD sum events; they are not all default
source-word reads. Sparse background output is derived, and its actual 100-frame
source has zero events; independent nonempty/extreme fixtures provide separate
source evidence. Scalar arrays, component grids, transformations and reconstruction
remain separate statuses.

The [full report](2026-11-06-tiff-codecs.json) records independent component checks,
original unit/plane hashes, geometry/padding, source codec/header windows and
explicit view policies. All original decoded samples and selected views match for
the two OJPEG files, subsampled YCbCr and legacy LZW. Source headers/types/precision
and orientation remain original. Native scalar reads reject nonuniform grids;
callers explicitly select components or a derived display view.

The comparison now fingerprints every vendored runtime file and Cargo input in
addition to Rust, bridge and executable hashes. Native libtiff/imagecodecs reference
versions and binary hashes are recorded and unchanged. No comparison errors,
component mismatches, timeouts, frame-count mismatches or compared header differences
occur. No interpolation, source repair or partial zeros hide count differences.

[All 91 preceding rejection events remain classified and reverified](../rejection-audits/2026-11-06-after-tiff-codecs.json): nine native closures plus 42 explicit EDF
compatibility closures leave 40. Source conditions remain distinct from reader
support. The 40 remaining events are 26 EDF source inconsistencies/truncations,
ten TIFF structural-source errors, two Lambda VDS/source contracts, one mixed-tag
legacy dialect and one ThunderScan integrity qualification. Semantic metadata,
autodetection, direct streams, series, other unsupported variants and Xcalibur's
substantive mask API remain separate gaps. Its placeholder `.read` zero image is
not treated as source-count decoding.

Seventeen reference input errors agree with native rejection; one deliberately
missing Python JPEG backend is an environment difference. All 213 reference tests
and their original assertions remain unchanged.

## Validation

[Recorded native checks](2026-11-06-tiff-codecs-native-checks.json): 156 Rust tests,
formatting, strict all-target/all-feature Clippy, no-default check/tests and 20
isolated feature checks pass (25 commands total). External actual-file manifests
for containers/fixed/Esperanto/MAR345 are enabled. The [minimal proof](2026-11-06-tiff-codecs-feature-disabled.json)
runs 22 reference tests with no failures/errors: six feature-disabled events,
33 direct matches, 23 other view matches, one independent lossless component/view
match, 12 known strict rejections, 24 no-pixel events and two known Pillow mismatches.
Four reference rejection calls are feature-disabled. Source/runtime/reference
fingerprints stay stable in both runs. Expanded platform CI, publication and new
throughput qualification are not established.

## Per-codec comparisons

| Codec | Direct | Other view | Component/view | Reconstruction | Mismatch | Rejected | No pixels |
|---|---:|---:|---:|---:|---:|---:|---:|
| GEimage | 6 | 0 | 0 | 0 | 0 | 0 | 0 |
| HiPiCimage | 6 | 0 | 0 | 0 | 0 | 0 | 0 |
| OXDimage | 10 | 2 | 0 | 0 | 0 | 0 | 0 |
| binaryimage | 2 | 0 | 0 | 0 | 0 | 0 | 0 |
| bruker100image | 9 | 0 | 0 | 0 | 0 | 0 | 0 |
| brukerimage | 15 | 1 | 0 | 0 | 0 | 0 | 0 |
| cbfimage | 17 | 0 | 0 | 0 | 0 | 0 | 0 |
| dm3image | 6 | 0 | 0 | 0 | 0 | 0 | 0 |
| dtrekimage | 17 | 0 | 0 | 0 | 0 | 0 | 1 |
| edfimage | 175 | 0 | 0 | 0 | 0 | 26 | 0 |
| eigerimage | 7 | 0 | 0 | 0 | 0 | 0 | 0 |
| esperantoimage | 9 | 0 | 0 | 0 | 0 | 0 | 0 |
| fit2dimage | 4 | 0 | 0 | 0 | 0 | 0 | 0 |
| fit2dmaskimage | 8 | 0 | 0 | 0 | 0 | 0 | 0 |
| fit2dspreadsheetimage | 3 | 0 | 0 | 0 | 0 | 0 | 0 |
| hdf5image | 6 | 0 | 0 | 0 | 0 | 0 | 0 |
| jpeg2kimage | 8 | 0 | 0 | 0 | 0 | 0 | 0 |
| jpegimage | 3 | 0 | 0 | 0 | 0 | 0 | 0 |
| kcdimage | 4 | 0 | 0 | 0 | 0 | 0 | 0 |
| lambdaimage | 10 | 0 | 0 | 0 | 0 | 2 | 0 |
| limaimage | 6 | 0 | 0 | 0 | 0 | 0 | 0 |
| mar345image | 30 | 0 | 0 | 0 | 0 | 0 | 0 |
| marccdimage | 6 | 0 | 0 | 0 | 0 | 0 | 0 |
| mpaimage | 1 | 0 | 0 | 0 | 0 | 0 | 0 |
| mrcimage | 8 | 0 | 0 | 0 | 0 | 0 | 0 |
| numpyimage | 12 | 0 | 0 | 0 | 0 | 0 | 0 |
| pilatusimage | 13 | 0 | 0 | 0 | 0 | 0 | 0 |
| pixiimage | 6 | 0 | 0 | 0 | 0 | 0 | 0 |
| pnmimage | 2 | 0 | 0 | 0 | 0 | 0 | 0 |
| raxisimage | 3 | 0 | 0 | 0 | 0 | 0 | 0 |
| sparseimage | 0 | 0 | 0 | 1 | 0 | 0 | 0 |
| speimage | 42 | 0 | 0 | 0 | 0 | 0 | 0 |
| tifimage | 23 | 23 | 4 | 0 | 2 | 12 | 24 |
| xcaliburimage | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| xsdimage | 5 | 0 | 0 | 0 | 0 | 0 | 0 |

ADSC aliases d*TREK. Historical reports remain unchanged:
[initial baseline](2026-11-05.json), [typed HDF5](2026-11-05-hdf5.json),
[parallel readers](2026-11-05-parallel-readers.json), [MAR345/sparse](2026-11-05-mar-sparse.json),
and [91-rejection continuation](2026-11-06-rejection-continuation.json).
[The complete ledger](../FORMAT_COVERAGE.md) and [reproduction guide](../FABIO_PARITY.md)
retain every target and qualification.
