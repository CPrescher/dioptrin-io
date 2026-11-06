# Nested and printf VDS, unlimited mappings and OJPEG profiles

This continuation extends native source reading and keeps independent source
correctness separate from unchanged FabIO/HDF5/libtiff behavior. Full FabIO
reading parity remains unestablished. Historical reports and their fixtures
remain unchanged.

| Added contract | Retained independent evidence |
|---|---|
| Nested VDS | [10 typed views / 24 exact frames](vds/2026-11-06-vds-advanced-ojpeg-nested.json), both cold h5py and FabIO references, six rejection controls |
| Printf VDS | [14 cases / 60 exact frames](vds/2026-11-06-vds-advanced-ojpeg-printf.json), thirteen direct reference matches, one separately qualified cache defect, two combined-domain controls |
| Unlimited VDS | [Six cases / 27 exact frames](vds/2026-11-06-vds-advanced-ojpeg-unlimited.json), original/effective selections and a separately rejected exhausted-source case |
| OJPEG | [17 profiles / 27 chunks / 81 exact component planes](ojpeg/2026-11-06-vds-advanced-ojpeg.json), fourteen mandatory libtiff matches plus explicit policy and stock-decoder qualifications |

## Explicit VDS source graph

Nested virtual datasets compose native HDF5 selection ordering through each
level to concrete leaves. The reader does not read a virtual dataset through
`H5Dread`, pair hyperslab blocks by index or allocate a detector-sized point map.
Original and frozen selections remain visible. Flat node IDs retain shared-node
provenance without recursively duplicating a DAG; cycles and bounded-resource
violations fail explicitly.

Defined fill in an intermediate VDS retains its originating node, scalar and
selection projected into the requested output. The fill policy applies to fill
visible in that output. Per-node unmapped counts describe that node's own
mapping coverage; effective fill regions also include projected fill from
its descendants. Missing or unallocated mapped sources remain errors.
The complete referenced source graph is admitted and tracked, including source
path aliases and the current owning file for the special `.` filename. Dynamic
nodes retain the source/plan-frozen domain without querying or refreshing HDF5's
native conditional view extent. An unflushed change to that native dynamic view
extent does not redefine this domain. Finite-node and concrete-source extent
changes, mapping/link retargeting and physical file changes retain their guards.

Printf expansion requires an explicit `PrintfMapping` for the exact owning
file/dataset, original mapping index and original filename/dataset patterns.
Only a caller-selected contiguous prefix of `%b` block indices is expanded;
`%%` denotes a literal percent. There is no filename search, missing-file scan,
implicit stop condition or fallback to fill. Original declarations, selected
plans and expanded block indices remain metadata. A bounded prefix can be
shorter than a predeclared extent; both original and frozen shapes are retained.
Anonymous captured input identifies its root owner with `source_file: None`.

Non-printf unlimited mappings freeze frame-axis selections from complete bound
source extents while retaining finite spatial strides. Every mapping is checked
against the same final domain, so an exhausted mapping cannot silently turn
into fill. Cross-axis unlimited layouts remain outside this qualified contract.
Genuinely irregular infinite unions must also be distinguished from regular
unlimited hyperslabs: the reference HDF5 API rejects combining two unlimited
selections, before a VDS reader receives a file.

The pinned HDF5 1.14.6 reference exposes a cache-dependent nested printf result:
a cold outer read returns zero fill despite complete nonzero sources. A separate
fresh-process control that keeps the child reader open after reading its shape
and counts makes the outer read match every authored word. The cold primary
reference is retained unchanged. This qualifies a pinned HDF5 extent-cache defect
also exposed through unchanged FabIO, rather than a native source-count gap.
The exhausted unlimited mapping is a different category: native HDF5 legally
supplies availability fill, while this reader requires complete mapped counts.

See [VDS source contracts](VDS_READER.md), [printf contracts](HDF5_PRINTF_VDS.md)
and [unlimited contracts](HDF5_UNLIMITED_VDS.md) for supported layouts,
source-change rules and scoped independent proofs.

## Additional original-JPEG profiles

Recognized header fragments and global entropy streams reuse the existing
bounded native JPEG decoder. Source byte windows retain their actual header
and entropy contributions separately from logical raster chunks. Sequential
planar scans retain component identity, original grids and source scan windows.
No color conversion, component upsampling or detector orientation is applied.

Table-only tiles can have historically ambiguous DC-reset conventions.
`OjpegTiles::IndependentResetTiles` explicitly asserts independently reset
chunks and decodes each tile with its declared geometry. Ordinary opens retain
strict admission. The selected policy and reconstruction are metadata; the
reader never chooses this interpretation after a failed default decode.

Independent original standalone JPEG component words, native libtiff results
and stock FabIO display/reading behavior are recorded separately. A successful
FabIO luminance/display read does not prove equality of native component grids.
A libtiff layout rejection does not by itself establish malformed input.
See [OJPEG source contracts](OJPEG_READER.md) for qualified profiles and limits.

## Evidence and remaining scope

The [full corpus continuation](fabio-parity/2026-11-06-vds-advanced-ojpeg.json)
and [native validation ledger](fabio-parity/2026-11-06-vds-advanced-ojpeg-native-checks.json)
record commands, outcomes, executable hashes and immutable source/reference
fingerprints. Prior standalone proofs are rerun against the final executables.
The new VDS and OJPEG proofs retain source correctness and reference outcomes
as separate gates.

Cyclic graphs, cross-axis unlimited mappings, mapped external-raw VDS sources,
other ranks and unqualified printf syntax remain explicit boundaries. OJPEG
high-precision, lossless and other unqualified fragment/scan/reset profiles
remain unsupported. These limits are not equated with malformed files or
reference-reader bugs.

The independent stress review also checks 32 three-level irregular mappings and
128 repeated/out-of-order frame reads against manually ordered source words and
h5py. It caught a typed-fill element/byte-unit error before qualification. A
separate two-module counterexample caught and now rejects an exhausted printf
prefix inside a larger combined output. The validation ledger retains the
review scripts and results alongside the retained regression fixtures.

All **404 Rust tests** and **164 Python controls** pass. The 29-command native
ledger records required formatting, strict all-target/all-feature Clippy,
all-feature tests, the separate no-default build/eight tests, all **23 isolated
optional features** and the **Rust 1.89 CBF** build. No new dependency or
performance claim is introduced.

The full corpus retains **237 unchanged FabIO tests**, zero failures/errors and
one existing slow skip. All **626 input outcomes** retain prior pixel/reference,
metadata, attribute, stream and header summaries. **Twenty-one supplemental
reports** (seventeen previous proofs and four new ones) bind the final probes;
all recorded runtime/reference immutability gates pass. The ledger verifies
current repository fingerprints and exact committed fixture bytes.
