# Upstream suite continuation, 2026-11-05: parallel native readers

**Full reading parity remains incomplete.** The complete 36-entry / 35-codec inventory is retained.

The full pinned suite executes 213 reference tests with no failures/errors and one upstream skip (the explicitly slow MAR345 memory-leak test). OpenJPEG 2.5.4 is now available to the test-only Glymur backend, so its four formerly skipped variants execute. Native comparisons match **413 read events / 1,533 frames**, up from 202 events / 1,049 frames in the typed-HDF5 baseline. Repeated reads are events, not distinct files; successful events cover 212 distinct input-file hashes.

| Native outcome | Events |
|---|---:|
| counts_match | 413 |
| mismatch | 13 |
| decoder_missing | 31 |
| test_adapter_missing | 0 |
| feature_disabled | 0 |
| native_rejected | 98 |
| reference_no_pixels | 25 |

The [full report](2026-11-05-parallel-readers.json) retains all source, executable, comparison-bridge and reference hashes. Production sources, bridge/inventory and the copied executable remained unchanged during the run; every reference codec source matches the pinned checkout. No comparison errors, timeouts or frame-count mismatches occurred. Compared EDF/d*TREK/Pilatus/MAR header fields have no differences. Other semantic-header schemas remain unqualified.

The 13 mismatching events remain visible: one Bruker LINEAR calibration; nine TIFF color/CMYK-to-grayscale display conversions; two TIFF packed 2/4-bit-to-byte display expansions; and one bool-versus-unpacked-u8 mask representation. Native source words, channels and packed values are preserved. Independent TIFF JPEG fixtures also retain the documented IDCT qualification; no comparison changes source counts to hide it.

The 31 absent-decoder events are MAR345 (30) and sparse reconstruction (1). Xcalibur remains unexercised as a count reader: its pinned `.read` returns a placeholder zero image rather than decoding source counts. The separate CCD-characteristics/mask API remains a target. MAR345 has a documented viable MIT V1 adaptation path; no LGPL/GPL runtime was introduced. See [the audit](../MAR345_XCALIBUR_AUDIT.md).

The 98 native rejections include intentionally incomplete EDF files/windows, unsupported TIFF encodings/photometric layouts, Oxford TY5, Lambda VDS and a d*TREK volume. They are not all missing decoders. Invalid input never authorizes invented counts or silent source substitution.

All implemented readers have adapters in the full build. A separate [minimal-feature run](2026-11-05-parallel-feature-disabled.json) executes 24 reference tests and classifies 64 events as `feature_disabled`, with three native matches. It has no missing adapters or decoders for those omitted optional features.

## What became readable

- Native SPE v2/v3, MRC and DM3; every declared SPE frame, selected ROIs, original complex components, thumbnails/main-image selection and source metadata.
- GE and Pixi fixed records; Bruker86/100, Oxford raw/TY1 and R-AXIS; explicit R-AXIS expansion and KCD summation preserve default raw views.
- FIT2D primary/mask/axis arrays, KCD separate readouts, MPA maps/spectra, Esperanto Long32/AGI and typed XSData XML.
- Mature JPEG/JPEG2000 backends, extended TIFF layouts, Pilatus semantic maps and all 125 MAR acquisition fields.
- EDF general records/external snapshots and bounded compressed padding; NPY complex64/128 with both byte and C/Fortran storage orders.

Cross-review regressions cover XML layout redirection, unsupported DM3 main-image fallback, MRC IMOD signedness, ignored GE compression, malformed Bruker tables, MPA precision/overflow, JPEG envelope/scan provenance, JP2 channel reordering and contradictory embedded TIFF JPEG layouts. Valid JPEG strips, full/edge tiles and shared tables retain their original counts.

## Validation

[Recorded local checks](2026-11-05-native-checks.json): 93 Rust tests pass, including opt-in actual-file container/fixed/Esperanto comparisons; formatting and strict all-target/all-feature Clippy pass; no-default check/tests and strict isolated Clippy for each of 18 optional feature selections pass (20 separate feature checks). All detector fixtures combined remain below 1 MB; external detector corpora stay outside Git. Expanded platform CI and new performance claims are not established.

## Per-codec read events

| Codec | Counts match | Mismatch | Rejected | Decoder missing | No pixels |
|---|---:|---:|---:|---:|---:|
| GEimage | 6 | 0 | 0 | 0 | 0 |
| HiPiCimage | 6 | 0 | 0 | 0 | 0 |
| OXDimage | 10 | 0 | 2 | 0 | 0 |
| binaryimage | 2 | 0 | 0 | 0 | 0 |
| bruker100image | 9 | 0 | 0 | 0 | 0 |
| brukerimage | 15 | 1 | 0 | 0 | 0 |
| cbfimage | 17 | 0 | 0 | 0 | 0 |
| dm3image | 6 | 0 | 0 | 0 | 0 |
| dtrekimage | 16 | 0 | 1 | 0 | 1 |
| edfimage | 133 | 0 | 68 | 0 | 0 |
| eigerimage | 7 | 0 | 0 | 0 | 0 |
| esperantoimage | 9 | 0 | 0 | 0 | 0 |
| fit2dimage | 4 | 0 | 0 | 0 | 0 |
| fit2dmaskimage | 8 | 0 | 0 | 0 | 0 |
| fit2dspreadsheetimage | 3 | 0 | 0 | 0 | 0 |
| hdf5image | 6 | 0 | 0 | 0 | 0 |
| jpeg2kimage | 8 | 0 | 0 | 0 | 0 |
| jpegimage | 3 | 0 | 0 | 0 | 0 |
| kcdimage | 4 | 0 | 0 | 0 | 0 |
| lambdaimage | 10 | 0 | 2 | 0 | 0 |
| limaimage | 6 | 0 | 0 | 0 | 0 |
| mar345image | 0 | 0 | 0 | 30 | 0 |
| marccdimage | 6 | 0 | 0 | 0 | 0 |
| mpaimage | 1 | 0 | 0 | 0 | 0 |
| mrcimage | 8 | 0 | 0 | 0 | 0 |
| numpyimage | 12 | 0 | 0 | 0 | 0 |
| pilatusimage | 13 | 0 | 0 | 0 | 0 |
| pixiimage | 6 | 0 | 0 | 0 | 0 |
| pnmimage | 2 | 0 | 0 | 0 | 0 |
| raxisimage | 3 | 0 | 0 | 0 | 0 |
| sparseimage | 0 | 0 | 0 | 1 | 0 |
| speimage | 42 | 0 | 0 | 0 | 0 |
| tifimage | 27 | 12 | 25 | 0 | 24 |
| xcaliburimage | 0 | 0 | 0 | 0 | 0 |
| xsdimage | 5 | 0 | 0 | 0 | 0 |

ADSC aliases d*TREK. The [original baseline](2026-11-05.json), [typed-HDF5 continuation](2026-11-05-hdf5.json) and [older HDF5-disabled proof](2026-11-05-feature-disabled.json) remain historical evidence. Complete source/metadata/discovery/stream/series contracts and all unsupported variants remain in [the full ledger](../FORMAT_COVERAGE.md) and [reproduction guide](../FABIO_PARITY.md).
