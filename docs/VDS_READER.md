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

The first scope is rank3, contiguous regular, frame-preserving mosaics. Every
source/virtual frame selection starts at 0 and covers the same complete frozen
frame range. Fixed declared frame extents must agree; zero-extent unlimited VDS
frames derive exclusively from the bound concrete modules. Fixed spatial
extents and every rectangle are validated, and overlaps fail. Gapped/overlapping
hyperslabs, points, irregular selections, frame shifts, nested/cyclic VDS,
spatial growth and printf mappings remain unsupported. A declared finite master
with additional fill-only frames is outside this initial full-frame contract.

The reader queries original mapping property lists and their owned dataspaces;
it never reads the virtual dataset or queries its native conditional frame
extent. Source files/datasets are opened through the existing bounded `Sources`
graph. Standard native dtype equality, concrete source shape, filters, chunk
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
rectangles directly into selected regions of a typed full-frame buffer.
No module passes through float conversion or an intermediate detector-sized
array. Every standard integer type, IEEE f32/f64, original endian declaration,
reserved value, NaN payload and signed zero retains its existing typed contract.

`UnmappedRegions::Reject` requires complete spatial coverage. `DeclaredFill`
permits only genuinely unmapped raster regions with an original defined VDS fill
and compatible fill-time declaration. These logical regions remain distinct
from module measurements. The exact typed fill scalar initializes the output,
then exact module samples overlay their original rectangles. Undefined fill or
FillTime::Never fails for uncovered pixels. No source-derived mask, correction,
normalization, flip or geometry transform is applied.

`typed::Metadata.virtual_dataset` records `VdsMetadata`: original mapping shape
and maximum dimensions, frozen shape, unmapped-region policy/count and every
mapping. Each mapping retains original filename/dataset/selection records,
canonical actual source file, resolved dataset, source shape/type/endian/layout,
source fill/time/allocation, filters, origins and mapped shape. `sources()` and
`links()` retain the complete immutable graph. Original selection records preserve
HDF5 UNLIMITED as u64::MAX. Master `fill_value` stays typed; its
`storage_allocated=false` describes the virtual master, while mapped source
allocation is reported separately.

Every source is checked before/after indexing and each frame read. Every concrete
module shape is checked around decoding, including resizable sources and sources
not yet overlaid. Multi-part virtual scans also check every other virtual or
ordinary part, so an unflushed extension elsewhere invalidates global frame
indices too. Changes invalidate the output and require reopening; there is
no live refresh or decoded-count cache. Dataset, mapping, link traversal, source
file, text, metadata, frame and chunk limits remain separate. Coverage uses a
sorted sweep with bounded metadata rather than detector-sized scratch. Payload
limits do not include native HDF5 internal caches/codec bookkeeping or promise
an RSS ceiling; existing variable-length discovery-string allocation is still
managed by upstream HDF5 before its retained length check.

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
