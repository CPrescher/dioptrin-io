# Metadata-derived HDF5 source contracts

`hdf5::contracts::SourceContracts::resolve_snapshot` prepares bindings for an
explicitly selected typed image dataset or Eiger/LImA/Lambda convention. It uses
the same native selection and link resolver as the typed reader. The caller
opts into resolving exact declared paths: relative filenames use their physical
owner's directory, and absolute filenames retain their absolute meaning. An
explicit captured-root origin replaces only the root's directory. There is no
basename search, availability-based series inference, or ambient HDF5 lookup.

```rust,ignore
use dioptrin_io::{wrapped::Input, hdf5::typed};
let input = Input::open(master, Default::default())?;
let selection = typed::Selection::Eiger;
let options = typed::Options {
    external_links: typed::ExternalLinks::FollowSnapshot,
    ..Default::default()
};
let contracts = input.hdf5_source_contracts(selection, options, &choices, &printf)?;
// If requirements() is nonempty, present those exact declarations to the user.
contracts.require_complete()?;
let reader = input.read_hdf5_with_contracts(
    selection, options, &contracts, typed::UnmappedRegions::DeclaredFill,
)?;
```

The ordinary reader defaults are unchanged. `FileChoice` identifies an exact
owner/original-filename pair and a selected absolute local file. Canonical owner
aliases are equivalent; duplicate choices fail. Missing files become structured
`Requirement::File` records with original names, owners and expected paths.
Errors other than absence (permissions, nonfiles, corrupt HDF5, missing datasets,
cycles, limits) remain errors. Native storage admission independently checks all
shapes, types, selections, coverage, filters and allocation before sample reads.
A complete contract does not promise support for a layout outside that reader.

Finite/nested and source-bound non-printf frame-axis unlimited mappings derive
their same explicit source bindings from declarations. The existing `%b`/`%%`
grammar is reused. An unbounded printf declaration becomes `Requirement::Printf`;
the caller must provide the recorded finite `PrintfMapping::blocks`. Discovery
never scans for matching files or chooses a block count by testing availability.
Every chosen block must have a complete bound source graph. Nested declarations
retain their actual physical owner, including `.`. The native global filename
map cannot represent identical stored filenames resolving to different files in
different owners; this case fails explicitly rather than rebinding a source.

External raw filenames use a separate contract. The wrapped input now exposes
`read_hdf5_with_storage`, so consumers do not need a second reader/ownership
adapter to pass `ExternalRawOptions`. `read_hdf5_with_contracts` also checks that
the plan belongs to that same input. Captured input remains owned by the wrapped
reader; declared-path resolution of streams requires an explicit origin.
Original raw offsets and byte windows remain validated by the existing native
raw reader. VDS leaves using external raw storage remain unsupported.

A source plan retains all admitted file/path snapshots plus object, DCPL and
concrete-extent guards. `check_sources` diagnoses mutation before/after planning
and before/after opening through the wrapped helper. Subsequent frame reads use
the typed reader's complete immutable graph checks. The existing metadata
snapshot contract does not promise protection against adversarial writes that
preserve file identity and timestamps. No growing-file/SWMR refresh is added.

`hdf5::catalog::image_paths` centralizes bounded image candidate enumeration.
It retains master aliases, follows only the explicit link policy, skips group
cycles and does not query a virtual dataset's conditional shape. It returns
candidate paths; scalar/type/storage admission still belongs to the typed reader.

## Qualification

`tests/hdf5_contracts.rs` covers missing acquisition-system absolute names,
explicit relocated files, exact original finite I16 words, nested original
U64/F16/bool/complex words from independent CC0 manifests, cycles, finite printf
scope, split raw windows, changed companions, preserved master aliases, group
cycles, captured origins and wrong-input contracts. Native VDS/raw suites remain
separate and cover storage admission and original selection semantics.

This is source-contract plumbing over existing qualified decoders, with no new
codec, numerical conversion or throughput claim. The acquired Lambda evidence
in `VDS_READER.md` is prior decoder evidence, not a new acquired-file run of this
source-discovery API. Large acquisition files remain outside Git.
