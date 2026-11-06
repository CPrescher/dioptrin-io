# Explicit immutable HDF5 virtual mosaics

`hdf5::typed::Reader::open_snapshot_with_vds` adds caller-selected VDS reading
to the existing typed/native HDF5 decoder. The `hdf5` feature remains optional;
there is no Python runtime, new codec, parallel fast path or performance claim.
`open_snapshot` continues rejecting VDS, and the existing `Options`/external-link
defaults remain unchanged.

```rust
use dioptrin_io::hdf5::typed::{
    FilenameBinding, Options, Reader, Selection, UnmappedRegions, VdsOptions,
};
let files = vec![FilenameBinding {
    stored_filename: "/acquisition/original-module.h5".into(),
    actual_file: "/chosen/immutable/module.h5".into(),
}];
let reader = Reader::open_snapshot_with_vds(
    "/chosen/immutable/master.h5",
    Selection::Lambda, // Selection::Dataset("/path") also works
    Options::default(),
    VdsOptions {
        filenames: &files,
        unmapped_regions: UnmappedRegions::DeclaredFill,
    },
)?;
```

Each binding names the exact original UTF8 filename string and one caller-approved
actual file. There is no basename inference, environment-prefix lookup, printf
expansion, neighboring-file substitution or retry. The mapping states which
actual file the caller intends; validation cannot infer that a different file
with identical shape/type contains the intended experiment. Canonical bound
paths and original names remain visible together. Duplicate/unused VDS bindings
fail. Regular datasets ignore irrelevant global VDS bindings entirely, including
their source-file budget, so one explicit manifest can accompany Lambda module
and master reads without changing regular module interpretation.

Rank 3 finite mappings admit contiguous, strided and irregular unions of
hyperslabs, including different source/destination block shapes, frame offsets,
frame reshaping and frame-varying coverage. Native HDF5 selection iteration order
pairs the original source and virtual selections; block index or bounding shape
is never used as a substitute for that pairing. Fixed declared dimensions and
every selected source/virtual coordinate are validated. Overlapping mappings
fail explicitly rather than selecting an implicit precedence.

The original unlimited Lambda contract remains separate: contiguous,
frame-preserving selections start at frame0 and derive their frozen extent only
from complete explicitly bound modules. General unlimited irregular/strided
selections, point selections, nested/cyclic VDS, spatial growth and printf
mappings remain unsupported. Finite masters may contain additional fill-only
frames only under the explicit defined-fill policy.

The reader queries original mapping property lists and their owned dataspaces;
it never reads the virtual dataset or queries an unlimited mapping's native
conditional frame extent. Finite mappings retain/check their fixed master
extent, which does not depend on source availability. Source files/datasets are
opened through the existing bounded `Sources` graph. Standard native dtype equality, concrete source shape, filters, chunk
bounds, raw-external rejection and storage allocation are admitted before output.
Mapped sources must have fully allocated storage. Missing files, missing datasets,
partial module chunks or unallocated module regions cannot turn into fill,
even when that concrete dataset declares zero fill. Source soft/external links
require the existing explicit `ExternalLinks::FollowSnapshot` policy; intermediate
files are retained and checked too.

This differs deliberately from native VDS availability behavior: HDF5 may return
fill for missing mapped data and may change the view extent according to which
sources are available. The [HDF5 dataset-access documentation](https://portal.hdfgroup.org/documentation/hdf5/latest/group___d_a_p_l.html)
specifies those native behaviors. Here, native HDF5 decodes concrete module
selections directly into a typed full-frame buffer. Finite mapping reads use
`H5Sselect_project_intersection` (available since HDF5 1.10.6) to project one
virtual frame into its original concrete source selection. No module passes
through float conversion, a per-pixel mapping array or an intermediate detector
image. Standard integers, f32/f64, admitted complex pairs, original bool bytes and
binary16 words preserve their existing type/endian/reserved-bit contracts.

`UnmappedRegions::Reject` requires complete spatial coverage. `DeclaredFill`
permits only genuinely unmapped raster regions with an original defined VDS fill
and compatible fill-time declaration. These logical regions remain distinct
from module measurements. The exact typed fill scalar initializes the output,
then exact module samples overlay their original selected coordinates. Undefined
fill or FillTime::Never fails for uncovered pixels. No source-derived mask, correction,
normalization, flip or geometry transform is applied.

`typed::Metadata.virtual_dataset` records `VdsMetadata`: original mapping shape
and maximum dimensions, frozen shape, unmapped-region policy/count and every
mapping. Each mapping retains original filename/dataset/selection records,
canonical actual source file, resolved dataset, source shape/type/endian/layout,
source fill/time/allocation, filters and original selection records. Irregular
records retain native canonical inclusive block bounds. `selection_layout`
distinguishes `ContiguousRectangle` and `FiniteSelection`. Origins and the legacy
`mapped_shape` describe bounding boxes for finite selections; they do not imply
full coverage. Explicit `source_bounding_shape`, `virtual_bounding_shape` and
`selected_elements` remove that ambiguity, including unequal block shapes.

Coverage is recorded in `unmapped_pixels_by_frame`. The Rust field
`unmapped_pixels_per_frame` is now `Option<usize>`: `Some(n)` means constant
coverage, `None` means variable coverage. Existing constant JSON remains a
number; variable JSON is null alongside the complete per-frame vector.
`sources()` and `links()` retain the complete immutable graph. Original selection records preserve
HDF5 UNLIMITED as u64::MAX. Master `fill_value` stays typed; its
`storage_allocated=false` describes the virtual master, while mapped source
allocation is reported separately.

Every source and original path alias is checked before/after indexing and each
frame read. Every concrete module shape and file/object identity is checked,
including sources not used by the requested frame. Concrete paths are walked
component by component and must remain hard links before native lookup; an
introduced external link cannot cause implicit file search. Recorded original
soft/external link kinds and exact raw values are revalidated even before file
timestamps change. Master object/mapping-property identities and finite master
extents are also checked, detecting unflushed replacement, retargeting and extent
changes. Returned creation property lists are copies; editing a copy does not
mutate a created dataset. Multi-part scans retain their complete-graph checks.
Changes invalidate output and require reopening; no live refresh or decoded
sample cache is introduced.

Dataset, mapping, link traversal, source, text, metadata, frame and chunk limits
remain separate. Irregular block arrays and per-frame coverage vectors are
charged before Rust allocation. Finite selections additionally cap block
complexity at 16,384 and aggregate frame/selection operations at 1,000,000.
Contiguous repeated blocks count as one compact interval for that admission.
Finite coverage uses native hyperslab set operations, with no detector-sized
point scratch; the unlimited rectangle path retains its sorted sweep. These
bounds qualify admitted operations and retained Rust metadata, not total RSS.
Native HDF5 dataspace/property deserialization, selection bookkeeping, caches and
codec allocations are separate, including allocations before wrapper admission.
Variable-length discovery strings retain their existing post-native-read bound.

The actual Lambda master has three missing acquisition-system absolute names.
Explicit bindings select the three independently fingerprinted corpus modules.
All 5 frames, each 1813x1555 in I16, match both independent direct-module
assembly and h5py's VDS output: 14,096,075 pixels checked. The original mapping
shape is `[0,1813,1555]`; frozen source-backed shape is `[5,1813,1555]`.
There are 413,623 declared-fill pixels per frame. Module data remains original
I16, and all four actual files are tracked. The master itself stays unchanged.

`docs/vds/2026-11-06-lambda.json` updates the original two baseline rejection
events to `explicit_source_layout_closed`, preserves the original
`incomplete_exact_source_graph` input condition, records exact read policies,
source fingerprints and all-frame signatures, and retains the prior independent
TY5 record separately. This supersedes the implementation-gap conclusion of the
historical Lambda section in `rejection-audits/LAMBDA_TY5.md`; it does not rewrite
that original-source evidence.

Independent CC0 fixtures cover finite/unlimited mosaics, renamed explicit sources
while the native VDS extent is 0, exact U64/IEEE fill, all ten scalar types and a
big-endian case, complete external graphs, source growth/replacement, partial
storage, wrong bindings, overlaps, unsupported selections and independent limits.
The fixture generator uses h5py only as test tooling. Reproduce the external
source proof with the existing `vds_probe` example and:

```sh
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/compare_lambda_vds.py \
  --probe target/debug/examples/vds_probe \
  --bindings /tmp/dioptrin-lambda-explicit-bindings.json
```

The bindings JSON is an explicit array of `stored_filename`/`actual_file` records;
the proof script never infers those paths. It reads actual modules independently,
assembles rectangles/fill, compares every native and h5py pixel signature, and
checks all input fingerprints again. Large detector files remain outside Git.


## Finite selection proof

The [finite irregular proof](vds/2026-11-06-marv2-vds-ojpeg.json) compares 15
independent CC0 masters and all 46 frames against manually sorted original
source words, direct h5py VDS reads and unchanged pinned FabIO `Hdf5Image` calls.
The cases include differing block shapes, irregular selections on both sides,
strided coordinates, source-to-different-virtual frames, variable coverage and a
whole fill-only frame. U64 extrema, signed integers, float/complex NaN payloads,
negative zero, binary16 and noncanonical bool bytes remain exact in both byte
orders where tested. A bound source with a nested concrete path and a complete
three-file external-link graph are included. Seven separately classified
rejections cover unavailable/partial/out-of-bounds bound sources, mapping
overlap, source-policy/fill-policy admission and the selection-complexity cap.

Five focused Rust tests additionally exercise unflushed source/master
unlink/rebind, finite master growth, new external leaf/ancestor injection and
recorded external-link retargeting, output invalidation and resource limits.
Three Python controls ensure source words, source dtype and variable-coverage
provenance cannot pass after mutation. Generators and proof helpers are test-only;
all reference and executable/source fingerprints are retained in the report.
