# Layout and type qualification, 2026-11-06

This continuation closes measured native reader gaps while retaining original
source samples, types, dimensions and metadata. Comparisons use unchanged
FabIO 2026.6.0 at commit `9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`.
Independent source-word checks and reference reading outcomes are separate.

| Area | Added contract | Independent checks |
|---|---|---|
| EDF | Full-rank source records, owned array output and explicitly indexed trailing 2D planes; original record identity retained | Manually encoded integer/IEEE words, four compression modes, missing axes, external sources, complete and partial recovery |
| NumPy | Versions 1–3 Boolean/binary16 arrays and empty dimensions, both byte orders and C/Fortran storage | Handwritten NPY headers and payload words; NumPy `load` and unchanged FabIO reads |
| HDF5 | Exact packed complex64/128, h5py Boolean enum and IEEE binary16; matching attributes/fill; explicitly bound external raw windows | Manual IEEE/Boolean words, h5py, original raw file windows and unchanged FabIO reads |
| TIFF | Declared primary/SubIFD graph catalog, explicit reachable-directory selection, CCITT RLE and mixed 2D Group3 | Independent libtiff decoding, handwritten directory/codec cases and original reference observations |

`PixelBuffer::Bool(Vec<u8>)` retains every source byte, including noncanonical
true values. Its optional logical view treats nonzero bytes as true without
changing storage. `PixelBuffer::F16(Vec<u16>)` retains binary16 words, including
NaN payloads, subnormals and signed zero; these words are not unsigned detector
counts. Consumers choose any numerical widening. Existing TIFF binary16 widening
remains a separately documented contract.

EDF `frames()` continues to count source image records. It does not flatten
volumes into invented records. `read_array_into` exposes the complete source
shape; `read_plane_into` requires explicit leading coordinates. Genuine 2D and
singleton-leading arrays retain their existing frame compatibility. Recovery
retains full source shape and original partial-element bytes; missing counts
never become complete images. Complete-array decoding remains bounded and does
not claim partial I/O or a new performance path.

NumPy frame numbering uses the product of leading source axes. Empty leading
axes expose zero frames; empty trailing axes expose the declared empty planes.
The original source shape remains available for scalar and 1D inputs too.

TIFF default page enumeration remains the original primary IFD chain. Explicit
SubIFD selection traverses only declared edges and rejects cycles, overlapping
tables, out-of-graph offsets and resource violations; shared aliases retain
original slots. A supported child can be selected independently of an unsupported
primary image. Codec changes reuse the MIT fax decoder's Huffman tables and
existing 2D state machine through a separately strict row API. The original
Group4 behavior and its tests remain intact. Vendored source/archive hashes,
licenses and complete local patches are retained.

HDF5 external raw windows require exact caller-provided filename bindings.
Every source and alias is frozen, even if a reservation contributes no current
samples. Physical/logical offsets, finite/unlimited declarations and effective
byte windows are retained. There is no implicit filename search, missing-file
fill or HDF5 fallback. See [the HDF5 contract](HDF5_LAYOUTS.md).

## Reference failures and remaining scope

The NumPy audit identifies six valid rank4 empty-spatial inputs for which NumPy
loads the original array but FabIO's `reshape(-1, 0, 3)` fails. Native reads retain
the explicit leading shape and succeed independently. These are reference
reshape defects, not malformed files or successful FabIO parity matches.
Other unexpected reference exceptions must fail qualification.

Structured/string/object/128-bit floating NumPy arrays remain explicit type
gaps. Object input is rejected without unpickling. EDF float128 labels remain
ABI dependent, and its obsolete byte-offset branch needs an external module
absent from the pinned runtime. No lossy conversion is inferred. MAR345 V2
remains unsupported: the pinned default reference path's V1 behavior does not
supply an independent V2 decoder contract.

General HDF5 enums, padded/renamed/mixed-endian compounds, custom precision,
other image ranks, irregular/nested/printf VDS and raw-external VDS modules remain
unsupported. TIFF fragmented OJPEG and other sampled layouts remain outside the
qualified profile. Reference backend inability to decode a particular valid
layout is recorded as a capability boundary, rather than called corruption or
silently retried. These remaining variants keep full reading parity false.

## Evidence

The [full corpus continuation](fabio-parity/2026-11-06-layout-types.json) and
[native validation ledger](fabio-parity/2026-11-06-layout-types-native-checks.json)
retain exact source, executable, reference, fixture and command provenance.
Supplementary reports qualify [EDF arrays](edf-arrays/2026-11-06-layout-types.json),
[NumPy types](npy-types/2026-11-06-layout-types.json),
[HDF5 storage/types](hdf5-layouts/2026-11-06-types-external.json) and
[TIFF layouts](tiff-layouts/2026-11-06-layout-types.json).
Historical reports remain unchanged. Full reference equality, independent source
correctness, malformed input, resource policy and valid unsupported capabilities
remain distinct outcomes.

The required formatting, strict all-target/all-feature Clippy and all-feature
Rust checks pass: **362 Rust tests**, plus **19 fax library tests**. The
no-default build/eight tests, all **23 isolated optional features** and the
Rust 1.89 CBF build also pass. All **120 Python controls** pass.

The main corpus retains **237 unchanged reference tests**, zero failures/errors
and one existing slow skip. Every one of its **626 input outcomes** remains
qualified, with prior pixels, metadata, attributes and header/discovery results
unchanged. Separate existing audits retain 37 series tests and three TIFF tests;
the EDF proof repeats an existing corpus regression. Earlier CIF2, low-level,
series, companion, table/profile and CIF1 proofs all pass against the same final
executable.

Supplementary exact-source checks cover **19 EDF records/30 planes** and four
partial recovery sources, **138 NumPy arrays/228 planes**, **9 HDF5 datasets/12
frames/7 attributes**, and **14 selected TIFF directories**. The six NumPy
reference exceptions remain separate from successful unchanged reference reads.
The [EDF](EDF_ARRAYS.md), [NPY](NPY_LAYOUTS.md), [HDF5](HDF5_LAYOUTS.md) and
[TIFF](TIFF_LAYOUTS.md) contracts give reproduction commands and exact limits.
