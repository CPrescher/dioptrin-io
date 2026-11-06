# Planar TIFF fixes and qualified reference defects, 2026-11-06

Both remaining display mismatches are now [independently qualified Pillow decoder defects](../pillow-planar16/README.md).
Related native compressed-planar display and partial-edge tile gaps are fixed.
The [full pinned run](2026-11-06-pillow-planar16.json) retains
**542 matching read events / 2,571 frames across 268 distinct primary hashes**.
There are no unclassified mismatches. No successful reference read is natively
rejected. Eleven deliberately incomplete source events have separately verified
partial access. Events include repeated reads, not distinct files.

| Native outcome | Read events | Compared complete frames |
|---|---:|---:|
| Direct count/dtype/shape match under recorded policy | 510 | 2,440 |
| Explicit reference view | 27 | 27 |
| Independently verified component grids + explicit view | 4 | 4 |
| Deterministic sparse reconstruction | 1 | 100 |
| Independently verified partial source access | 11 | — |
| Independently qualified Pillow decoder defect, excluded from matches | 2 | 2 |
| Unclassified mismatch | 0 | — |
| Native rejection of successful reference read | 0 | — |
| Reference returns no pixels | 25 | — |
| Missing decoder/adapter/optional feature | 0 | — |

All 213 unchanged upstream tests pass, with one existing upstream skip. Seventeen
expected reference input errors agree with strict native rejection; one deliberately
disabled Python JPEG backend remains an environment difference. This answers a
crucial distinction: the suite includes expected failures and incomplete-file
padding tests. FabIO is not expected to return original complete counts from every
input. Its padded or partially loaded arrays are not independent evidence for
missing detector samples.

## Closed and recovered cases

- [Planar TIFF attribution](../pillow-planar16/README.md): all words in both original
  files match tifffile and native libtiff. An exact byte model reproduces Pillow's
  wrong raw unpacking; controlled lossless storage changes restore correct display.
  Compressed planar16 views and spatial tile padding now pass 84 independent fixtures.
- [Lambda VDS](../VDS_READER.md): all five frames, 14,096,075 output pixels, match
  independent direct module assembly and h5py. Exact filename bindings, complete
  allocated module sources, frozen module-derived extents and original gap fill
  are explicit. Missing sources never become fill pixels.
- [Legacy TIFF metadata](../TIFF_LEGACY_RECOVERY.md): all complete Feb09 wrappers,
  missing-byte-count a0009 and mixed strip/tile quad-tile counts match independently.
  Compatibility options preserve original IFD entries and record effective windows.
- [EDF layout and recovery](../source-recovery/README.md): complete split frames and
  the explicitly interpreted stale TY1-labelled raw acquisition match all samples.
  Original declarations and selected interpretations remain available.
- ThunderScan's first page supplies 356 of 359 rows. Its 538,272 original four-bit
  samples and independent complete second page match native libtiff and source
  token boundaries. Three missing rows have no samples. Ten truncated EDF events
  expose only present typed words, partial-element bytes, headers and member
  integrity. Partial availability never passes the complete-count parity gate.

[All 91 original rejection events remain classified and reverified](../rejection-audits/2026-11-06-after-pillow-planar16.json):
80 complete read closures and 11 independently checked partial-source cases.
Original conditions, exact source hashes, recreated timestamp-bearing cases and
baseline event/test multiplicities remain explicit. No input is removed from the
corpus and no count padding hides a gap.

**Full FabIO reading/API parity remains incomplete.** Qualified reference defects
preserve their measured differences and stay outside successful matches.
Complete semantic metadata, discovery, direct
streams, series and unsupported variants remain separate work. The Xcalibur
placeholder `.read` returns no pixels; its substantive mask API remains unexercised.
R-AXIS expansion, KCD sums, views, declared fill and sparse background reconstruction
remain explicit policies or derived output, not uniform default source-word reads.

## Validation and reproduction

[Native check evidence](2026-11-06-pillow-planar16-native-checks.json) records 186 Rust
tests, formatting, strict all-target/all-feature Clippy, no-default check/tests and
strict isolated checks for all 20 optional features. The independent proof also
rejects reference-defect qualification if the native hash or reference pixel is
changed. This is local macOS correctness evidence, not expanded platform or
throughput qualification.

The [minimal wrappers-only TIFF run](2026-11-06-pillow-planar16-feature-disabled.json)
runs 13 reference tests with no failures/errors/skips: 36 direct events, 24 other
views, one lossless component/view, one qualified partial read, three correctly
feature-disabled events, 24 no-pixel events and the same two qualified reference
defects. The [minimal proof](../pillow-planar16/2026-11-06-minimal-proof.json) repeats
all 84 fixtures, both originals and eight storage controls. Optional feature
absence is not reported as an absent reader. All runtime, native C, vendored, Cargo/build, bridge, executable,
policy and independent reference fingerprints stay unchanged during both runs.

Use the commands in [FABIO_PARITY.md](../FABIO_PARITY.md) with
`--read-policies docs/source-recovery/read-policies.json` and an external
`--vds-map` JSON containing the approved exact bindings and explicit gap-fill
policy. Full and minimal reports exit nonzero because their full parity gate
remains incomplete; reference unittest and individual count/recovery outcomes
are reported separately.

## Per-codec events

| Codec | Direct | Other view | Component/view | Reconstruction | Partial | Ref defect | Rejected | No pixels |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| GEimage | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| HiPiCimage | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| OXDimage | 10 | 2 | 0 | 0 | 0 | 0 | 0 | 0 |
| binaryimage | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| bruker100image | 9 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| brukerimage | 15 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |
| cbfimage | 17 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| dm3image | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| dtrekimage | 17 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |
| edfimage | 191 | 0 | 0 | 0 | 10 | 0 | 0 | 0 |
| eigerimage | 7 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| esperantoimage | 9 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| fit2dimage | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| fit2dmaskimage | 8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| fit2dspreadsheetimage | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| hdf5image | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| jpeg2kimage | 8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| jpegimage | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| kcdimage | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| lambdaimage | 12 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| limaimage | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| mar345image | 30 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| marccdimage | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| mpaimage | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| mrcimage | 8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| numpyimage | 12 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| pilatusimage | 13 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| pixiimage | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| pnmimage | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| raxisimage | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| sparseimage | 0 | 0 | 0 | 1 | 0 | 0 | 0 | 0 |
| speimage | 42 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| tifimage | 33 | 24 | 4 | 0 | 1 | 2 | 0 | 24 |
| xsdimage | 5 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
