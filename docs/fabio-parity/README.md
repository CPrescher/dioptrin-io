# Advanced VDS and OJPEG, 2026-11-06

The [current continuation](2026-11-06-vds-advanced-ojpeg.json) retains all 237
unchanged FabIO reference tests (zero failures/errors and one existing slow skip)
and 626 qualified input outcomes. Previous pixel/reference, metadata, attribute,
stream and header summaries remain unchanged.

Four new proofs qualify nested VDS (10 views / 24 frames), explicit printf VDS
(14 cases / 60 frames), frame-axis unlimited mappings (six cases / 27 frames)
and OJPEG (17 profiles / 81 component planes). Native source-word correctness,
a separately controlled HDF5 cache defect, stock decoder failures, explicit
reset-tile policy and missing mapped counts retain separate gates. See the
[combined audit](../VDS_ADVANCED_OJPEG_PARITY.md).

The [native checks](2026-11-06-vds-advanced-ojpeg-native-checks.json) record
**404 Rust tests**, **164 Python controls**, required/no-default checks,
**23 isolated optional features** and the Rust 1.89 CBF build. All seventeen
prior standalone proofs and four new scoped proofs use the same final probes.
Full reading parity remains unestablished for the documented remaining
layouts/processes and source/reference differences. Historical reports remain
unchanged.

## Previous MAR345 V2, irregular VDS and fragmented OJPEG, retained

The [previous continuation](2026-11-06-marv2-vds-ojpeg.json) retains 237 unchanged
reference tests (zero failures/errors, one existing slow skip) and all 626
qualified input outcomes. Prior pixel/reference outcomes, metadata, attributes
and input/header summaries remain unchanged.

New proofs establish original seven-bit MAR345 V2 against the untouched CCP4
writer/decoder, explicit incompatible eight-bit compatibility, finite irregular
VDS selection projection and source-preserving OJPEG fragments. The original
writer check corrected a provisional eight-bit V2 assumption before committing;
reference bugs, unsupported profiles and deliberate source policies remain
separate. No genuinely acquired V2 sample was available in the pinned corpus.

See [combined evidence](../MARV2_VDS_OJPEG_PARITY.md) and the
[native checks](2026-11-06-marv2-vds-ojpeg-native-checks.json): **380 Rust tests,
140 Python controls**, all required/no-default checks, **23 isolated features**
and the Rust 1.89 CBF build pass. Fourteen prior standalone proofs and three new
proofs use the final executables. Full reading parity remains unestablished for
the documented remaining variants and qualified source/reference differences.

## Previous layout and type readers, retained

The [current continuation](2026-11-06-layout-types.json) preserves all 626
qualified input outcomes and 237 unchanged reference tests (zero failures/errors,
one existing slow skip), including their pixel, metadata and attribute results.
New independent proofs add EDF arrays/planes, NPY Boolean/binary16/empty arrays,
exact HDF5 types/raw external windows and TIFF SubIFD/fax layouts. Six valid
NumPy empty-array failures remain explicitly qualified FabIO reshape bugs.
Other valid unsupported types/layouts, malformed input and resource rejection
remain separate.

See [combined evidence](../LAYOUT_TYPE_PARITY.md) and the
[native checks](2026-11-06-layout-types-native-checks.json): 362 Rust tests,
19 fax library tests, 120 Python controls, required/no-default checks,
23 isolated optional features and Rust 1.89 CBF all pass. The ten previous
standalone proofs also pass against the same final executable. Full FabIO
reading parity remains unestablished for the documented unsupported variants
and qualified source/reference differences.

## Previous CIF 2.0 and low-level readers, retained

The [reader continuation](2026-11-06-cif2-lowlevel.json) preserves all 626 input
outcomes and 237 unchanged reference tests (zero failures/errors, one existing
skip). Separate proofs qualify CIF 2.0, TIFF controls, Pilatus, NeXus and original
encoded CBF bytes, retaining independent source/count checks and original FabIO
failures. The real Pilatus CR-only/separator gap is closed. The earlier series,
companion, table/profile and CIF 1.1 proofs also pass against the final executable.
See [combined evidence](../CIF2_LOWLEVEL_PARITY.md) and
[native checks](2026-11-06-cif2-lowlevel-native-checks.json): 331 Rust tests,
100 Python controls, all required/no-default checks, 23 isolated features and
the Rust 1.89 CBF build pass. Full FabIO reading parity remains unestablished
for the documented layout/type/helper boundaries and qualified source differences.

## Previous CIF grammar and legacy tables/profiles, retained

The [reader continuation](2026-11-06-cif-legacy.json) preserves all 626 input
outcomes and 237 unchanged reference tests (zero failures/errors, one existing
slow-test skip). Separate proofs add strict CIF 1.1 syntax, labeled ASCII tables
and source-preserving CHIPLOT profiles, while distinguishing FabIO defects,
malformed input and unsupported CIF 2.0/binary-table capabilities. The previous
37-test series audit and both companion proofs also pass against the final
executable. See [combined evidence](../CIF_LEGACY_PARITY.md) and
[native checks](2026-11-06-cif-legacy-native-checks.json): 293 Rust tests,
72 Python controls, no-default checks and 23 isolated features pass. Full reading
parity remains false for the qualified differences and remaining version/layout
and helper scope.

## Previous series, navigation and companions, retained

The [reader continuation](2026-11-06-series-companions.json) preserves all 626
input outcomes and 237 reference tests. A separate 37-test series audit verifies
515 cursor actions, 10,010 filenames and 3,429 metadata-field occurrences. Run and
CIF companion proofs each qualify 9 path/buffer/stdin comparisons under explicit
source/projection contracts. No failed legacy helper is counted as successful
read parity. See [combined evidence](../SERIES_COMPANIONS_PARITY.md) and
[native checks](2026-11-06-series-companions-native-checks.json): 272 Rust tests,
56 Python controls and 21 isolated feature checks pass. Exact/full FabIO parity
remains false for the qualified differences and remaining grammar/variant scope.

## Previous content discovery and streams, retained

The [schema 10 report](2026-11-06-input-apis.json) qualifies **626 native stdin
comparisons**, every content-discovery decision and three separate EDF header
calls. It runs all 237 unchanged reference tests (213 codecs +24 input/header),
with no failures/errors and one existing skip. The original 580 codec outcomes
are unchanged. Validation passes 240 Rust tests, 36 Python controls, all required
checks and 21 isolated features. See [input contracts and evidence](../INPUT_APIS.md),
[native checks](2026-11-06-input-apis-native-checks.json) and
[disabled-feature checks](2026-11-06-input-apis-feature-disabled.json). Full
reading parity remains false; series, companions and outside-corpus variants
remain separate.

## Previous Xcalibur API coverage, retained

The [current schema 9 report](2026-11-06-xcalibur.json) adds **11 matching Xcalibur
CCD/mask API events**, retaining every prior image-reading outcome. The
[Xcalibur audit](../XCALIBUR_MASKS.md) records partial machine-function tails in
all five original CCD files, 430 accounted field occurrences, exact native mask
decomposition and the remaining API contracts. Validation passes 210 Rust tests,
22 Python comparison controls and all 21 isolated feature checks. The
[check record](2026-11-06-xcalibur-native-checks.json) retains commands and hashes.
Full reading/API parity remains false.

## Previous header-only and attribute coverage, retained

The [current audit](../metadata-parity/HEADER_ATTRIBUTES.md) closes all 25
previously unqualified header-only events and accounts for all observed source
attributes outside `.header`: 2,591 matching comparisons, 133 qualified reference/presentation
differences, 43 unspecified properties and zero unresolved attributes. The
[schema 8 full report](2026-11-06-headers-attributes.json) preserves every pixel
outcome below. All 213 upstream tests run without failures/errors (one existing
skip); 202 Rust tests and all 20 isolated feature checks pass. Exact metadata and
full reading/API parity remain false for the measured differences and separate
API qualification boundaries documented in the audit.

## Previous metadata pass, retained


The [format-by-format metadata audit](../metadata-parity/README.md) accounts for
all compared `.header` fields across 34 codecs: 218,722 matching occurrences,
3,289 qualified reference differences and zero unresolved compared fields.
It closes native CBF, DM3, Esperanto, MAR345, JPEG, PNM, Oxford and wrapped
FIT2D metadata gaps. The [current full report](2026-11-06-metadata.json) preserves
the pixel outcomes below. Metadata equality remains false for documented FabIO
behavior and unqualified header-only reads; empty headers prove no coverage.
Validation now includes 196 Rust tests and all 20 isolated feature checks.

## Previous pixel qualification, retained

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

## Xcalibur API continuation

`2026-11-06-xcalibur.json` is the schema 9 continuation, retaining the prior
pixel/header/attribute outcomes and adding 11 CCD mask API events. All 430
top-level CCD field occurrences are accounted for; five original files have
explicitly retained truncated machine-function tails. Native full decomposition
reconstructs the source Pilatus mask exactly, independently of the upstream
pyFAI-dependent assertion. See [XCALIBUR_MASKS.md](../XCALIBUR_MASKS.md) for the
source and projection contracts, reference defects and remaining API work.
