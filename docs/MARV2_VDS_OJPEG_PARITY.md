# MAR345 V2, irregular VDS and fragmented OJPEG

This continuation adds three native reading contracts. Independent original-word
checks remain separate from unchanged FabIO 2026.6.0 behavior, explicit display
views, malformed files, resource limits and unsupported capabilities. Full FabIO
reading parity remains unestablished.

| Reader | Added contract | Independent evidence |
|---|---|---|
| MAR345 | Original seven-bit PCK V2 by default; explicit eight-bit compatibility framing | 32 original-format fixtures, five untouched original-writer/decoder round trips, 39 explicitly selected compatibility fixtures and both actual V1 corpus files |
| HDF5 VDS | Finite strided/irregular hyperslab unions, differing source/destination block shapes, frame remapping and variable declared-fill coverage | 15 masters and all 46 frames match manually ordered original words, h5py and unchanged FabIO |
| OJPEG | Declared header/table fragments, restart reconstruction and explicit-header stacked tile streams | 20 files, 56 chunks and 168 raw component planes exactly match independent native libtiff |

## MAR345 format evidence and reference bugs

The original J.P. Abrahams writer in the
[official CCP4 source archive](https://ftp.ccp4.ac.uk/opensource/libccp4-8.0.0.tar.gz)
emits the MAR `CCP4 packed image V2` marker. Its header contains three run bits
and four width bits, with widths `0, 3..16, 32`; it uses signed-short predictor
arithmetic. The existing bounded MIT-derived reader now uses those parameters
for the default V2 interpretation. No original CCP4 C code, LGPL runtime or new
format dependency is incorporated into the native reader.

Final provenance review caught a real qualification error before committing:
the pinned FabIO separate legacy decoder consumes four run bits and four width
bits, with a different width alphabet. Synthetic files matching that decoder
alone would not establish original MAR345 V2. The native default and the audit
were corrected against the untouched original writer and decoder.

`mar345::Reader::open_with_options` can select
`PckV2Dialect::FabioLegacyEightBit`; ordinary `open` uses `Original`.
`wrapped::Format::Mar345WithOptions` carries the same explicit policy through
captured/outer-compressed input. Both layouts share a marker, so the reader
never guesses a dialect or retries after a failed decode.
`Metadata::pck_v2_dialect` records the selected V2 interpretation; V1 has `None`.
The eight-bit option preserves the original signed predictor rather than
reproducing the legacy decoder's erroneous unsigned division.

Pinned FabIO's default V2 path still calls its V1 decoder. Safe unchanged calls,
coincidental zero-valued matches, wrong words and existing endian errors are
recorded separately. Known unsafe reinterpretations are not executed and are
not claimed as successful or measured failed reference reads. Two eight-bit
legacy-oracle disagreements are independently reproduced by its unsigned
predictor arithmetic; original-writer/decoder round trips establish the signed
contract independently. See [MAR345 V2 evidence](MAR345_V2.md) and
[the scoped report](mar345/2026-11-06-marv2-vds-ojpeg.json).

No genuinely acquired V2 sample was found in the pinned corpus. The V2 evidence
is original writer/decoder input and independently authored words, not measured
detector-file coverage. Both actual V1 detector sources retain exact counts.

## Irregular VDS source and coverage contracts

Finite mappings use HDF5's existing selection-order projection API and concrete
source reads. The reader does not pair block indices, construct a detector-sized
point map or read through native VDS filename search. Original selections,
canonical inclusive blocks, bounding shapes and actual selected-element counts
remain visible. Declared-fill regions remain distinct from mapped source counts.
Missing, partial or unallocated mapped sources never silently become fill.

Frame coverage can vary. `unmapped_pixels_per_frame` is now `Option<usize>`:
`Some` identifies a constant count, while `None` requires the exact
`unmapped_pixels_by_frame` vector. Existing constant-coverage JSON still emits
the same scalar. `mapped_shape` remains a bounding-box compatibility field;
`selection_layout` states whether it is a contiguous rectangle.

Source checks now reject unflushed source/master unlink/rebind, finite master
growth, recorded soft/external-link retargeting and newly introduced external
path components before reading counts. Hard-path revalidation avoids implicit
lookup of unknown external files. The original unlimited Lambda path retains
its source-derived frozen extent without querying native conditional extent.
Selection block/operation caps and existing source/frame/metadata bounds remain
explicit. See [VDS contracts](VDS_READER.md) and
[the matched proof](vds/2026-11-06-marv2-vds-ojpeg.json).

## OJPEG legacy profiles and display boundaries

The fragment adapter reuses the existing JPEG header parser and native entropy/
IDCT decoder. Original source windows, headers, logical versus encoded-stream
origins, derived restart intervals and inserted markers remain provenance.
Complete interchange streams must agree with their declared entropy partitions;
wrong, overlapping, shifted or swapped contributions fail. Original headers
remain available even when the separately recorded legacy SOS operation applies.

Fourteen new independently encoded legacy profiles complement four existing
CC0 sources and both actual OJPEG corpus files. All original component bytes,
including decoder padding, match native libtiff. The 20 unchanged FabIO calls
also complete, but their luminance/display output is not equated with raw
YCbCr component grids. The TIFF technical note documents ambiguous historical
restart conventions; these are explicitly qualified libtiff profiles, not a
claim of universal legacy-JPEG conformance. See [OJPEG contracts](OJPEG_READER.md)
and [the component proof](ojpeg/2026-11-06-marv2-vds-ojpeg.json).

## Remaining scope and validation

MAR spiral/raw/old IMAGE layouts and degenerate predictor shapes remain
unsupported. VDS nested/cyclic or printf mappings, general unlimited irregular
selections, mapped external-raw sources and other source ranks retain explicit
boundaries. OJPEG planar separation, nonbaseline/high precision, ambiguous
stacked geometry and other unqualified fragment/restart profiles remain gaps.
Malformed input and resource rejection are not relabeled as reader defects.

The [full corpus continuation](fabio-parity/2026-11-06-marv2-vds-ojpeg.json) and
[native validation ledger](fabio-parity/2026-11-06-marv2-vds-ojpeg-native-checks.json)
record exact source, fixture, executable, primary-source and reference hashes.
Historical reports remain unchanged. The existing layout/type, CIF, low-level,
series, companion and table/profile proofs are revalidated separately against
the same final executable.

All required formatting, strict all-target/all-feature Clippy and all-feature
Rust checks pass: **380 Rust tests** and **140 Python controls**. The no-default
build/eight tests, all **23 isolated optional features** and the Rust 1.89 CBF
build also pass. The 29-command native ledger records these checks explicitly.

The full corpus retains **237 unchanged reference tests**, zero failures/errors
and one existing slow skip. All **626 input outcomes** remain qualified with
prior count/reference, metadata, attributes and input/header summaries unchanged.
Fourteen earlier standalone proofs pass against the final executable; three
new scoped proofs bring the supplementary report count to seventeen. These
results do not equate recovered partial sources, failed reference helpers or
explicit display views with complete original-source reading parity.
