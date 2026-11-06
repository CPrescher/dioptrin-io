# Read-only Pilatus and NeXus helpers

These APIs return bounded owned source metadata and explicit dataset selections.
They do not provide Python/h5py object identity, mutation, writers, statistics or
an implicit detector correction. Native image data access reuses the existing
typed HDF5 reader. No parallel HDF5 sample decoder was introduced.

## Pilatus acquisition text

The existing shared `pilatus::Header::parse` remains the detector parser used by
CBF and TIFF. It now has read-only `get`/`contains` access. The standalone
`pilatus::{Document,Reader}` APIs retain original UTF8 bytes, the original text,
lexical assignments/units, duplicate order, unknown lines and the typed keyword
projection. `Header.entries` retains duplicate assignments; typed/lexical maps
have the final assignment. Unavailable recognized values are explicit `Invalid`
values plus diagnostics, never invented numeric metadata. The retained lexical
source is authoritative when a conversion exceeds the existing signed64/float64
keyword schema.

`Document::from_bytes` and `from_reader` own bounded copies, labelled `Buffer`
and `StreamCapture`. Path `Reader::open` requires a regular file and checks the
original binding, snapshot and open handle on every metadata/path access.
`PILATUS_1.2` is the recorded default convention; other conventions fail
explicitly. Invalid UTF8 is rejected without a lossy conversion.

The explicit `clean_string`/`Document::cleaned_lines` view reuses the detector's
shared delimiter and continuation projection. A borrowed Python-compatible
line iterator recognizes LF, CRLF, CR, vertical tab, form feed, file/group/record
separators, NEL, line separator and paragraph separator; all source lines are
counted for metadata admission before allocation. Original bytes and the raw
description remain unchanged. Terminal continuation and one-token constructor
cases are separately diagnosed. A terminal backslash remains in the original
and safe derived view; a backslash followed by spaces remains literal.

The unchanged pinned `PilatusHeader` constructor, `clean_string`, membership and
lookup are observed independently on fresh objects. Known LF/CRLF, CR-only and Unicode-separated profiles match
constructor fields, typed float bits and lookup values. Dangling continuations
can raise reference `IndexError`; a one-token cleaned line can raise another
reference `IndexError` through fixed keyword positions. Native raw retention
and diagnostics are qualified separately from those failed reference calls.
Missing Rust lookups return `None`; this is an explicit read-only value contract,
not an assertion that Python `__getitem__` stops raising `KeyError`.

## NeXus source-aware navigation

`hdf5::nexus::Reader::open_snapshot(path, typed::Options)` indexes a bounded
read-only object/link graph. It reuses `Sources` for exact filename bindings,
link resolution, regular-file admission and immutable master/intermediate/target
checks. External links reject by default; `FollowSnapshot` follows their exact
stored paths and tracks every traversed file. No basename substitution, missing
source fill or neighboring-file lookup is added.

`get_entry`, `get_entries`, `get_class`, `get_attr`, `get_data`, `find_detector`,
`find_data` and `get_default_nxdata` return owned source-file/path/kind values.
Dataset descriptors retain source shape, datatype descriptor, byte order and
layout; attributes retain type/shape declarations and exact typed numeric values
(including uint64 extrema), endian metadata and logical text values. Shared
attribute handling was extracted unchanged from the sparse reader; public
`sparse::AttributeValue/AttributeStorage/Attributes` remain compatible reexports.
The HDF5-only feature does not require the sparse or image-wrapper features.

A named entry lookup does not sort or interpret unrelated timestamps. Ordered
entries require a valid scalar ISO timestamp prefix, retain the complete source
string and explicitly name their naive first-19-character ordering. Timezone or
other suffixes are retained without a UTC-chronology claim, matching the scope
of the pinned default ordering. Equal timestamps retain source link iteration
order, including HDF5 tracked creation order. Arbitrary unrelated datasets named
`start_time` are not treated as NeXus controls.

Named numeric data filters compare scalar integer values losslessly across
integer widths/signs/endian. Integer/float equality requires an exact integral
value; large uint64 values are never first rounded through float64. Text filters
compare logical scalar text. Arrays with multiple elements are an explicitly
ambiguous filter rather than a Python truth-value imitation. Returned source
attributes keep their original type and shape regardless of the query filter.

All traversed dataset shapes, object tokens, typed attributes, controls, member
order and link bytes are compared against a fresh bounded metadata index before
queries return. This detects unflushed in-process control/shape changes as well
as file growth/replacement. A `DatasetView` borrows that parent graph and delegates
to the existing typed reader; parent/source checks bracket every frame read and
failure invalidates the output. Caller-edited descriptors must still match the
indexed source/kind/shape/type/endian/attribute binding. Scalar, text and other
non-image selections remain descriptors; an incompatible image read fails.
VDS/external raw-storage image admission is not silently added by navigation.

Graph depth is capped before recursion at 128, independently of caller path
limits. File, dataset, member, text, traversal and retained metadata bounds are
explicit. Reorder scratch and owned result clones are admitted before allocation,
including repeated hard-link aliases of large attributes. `metadata_bytes` is
an accounting domain for one index/result, not a total process RSS promise: a
query can concurrently hold its cached index, one fresh verification index and
owned result (and a bounded intermediate result during a combined query).
HDF5 native caches are separate. Variable-length text retains the shared reader's
post-native-read text bound; the wrapper does not claim a new preallocation bound
inside the native HDF5 allocator. Nonstandard/unsupported attribute types remain
explicit admission failures, not converted or omitted values.

## Reference qualifications and pure-helper scope

The unchanged pinned Nexus helper has real failures distinct from valid source
files and native capability:

- `get_data(group)` defaults to `get_attr(dataset, None)`, which raises `TypeError`
  in the pinned h5py environment. `find_data` reaches the same failure. The native
  no-filter query returns intended source dataset selections, with independent
  exact h5py/count proof; those calls are not unchanged-reference parity successes.
- `get_default_NXdata` uses raw fixed-ASCII defaults and then calls
  `bytes.startswith(str)`, raising `TypeError`. A Unicode-default fixture provides
  a successful unchanged positive control; native typed text handles both forms.
- The unchanged Nexus file-object driver fails the nested external-link fixture
  in this pinned environment. Direct path-based h5py and native exact-source
  traversal read all three files. This limitation is recorded with versions,
  module/extension/source hashes and raw link bytes, not called malformed input.

Independent path-driver inspection and reference file-object observations have
separate lifetimes to avoid artificial shared-handle/cache failures. Every
completed unchanged reference call is compared; reference errors are recorded
with their own scopes rather than being counted as parity.

`nexus.is_hdf5` is a separate eight-byte path signature convenience helper; it is
not the native discovery contract (which also admits bounded HDF5 user blocks).
`from_isotime` and `get_isotime` are separate local-time/timezone conversion and
formatting helpers; navigation does not claim to replace them. General h5py
objects, writer/creation APIs, mutation and arbitrary Python mapping behavior
remain outside this read-only value API. The pinned `test_nexus.py` largely tests
creation/live writing; it does not establish immutable read-only traversal parity.

## Reproducible controls

The independent CC0 files under `tests/data/lowlevel/` include acquisition text,
malformed/continuation/separator controls, two entries with ordered timestamps,
big-endian uint64 data/attributes, fixed/Unicode defaults and a three-file exact
external-link graph. `scripts/lowlevel_reference.py --generate-fixtures` records
sizes/hashes in `provenance.json`; no reference writer generates detector data.
The proof compares complete owned descriptors, original text/units/repetitions,
all exact attribute bytes/declarations, full image count hashes and source graphs.
It additionally records unchanged reference outcomes and their qualifications.

The reference checkout is FabIO 2026.6.0 commit
`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`. Relevant MIT source hashes are
`cbfimage.py` = `0161b83dce84114421767969ccfef6ae8d51bf90739ab84cc5cf390da1e844d3`
and `nexus.py` = `8781afe25acd6b987c26882a458c27e8dd21665b037d01f968354f351d8bf960`.
Retained FabIO/PhaseSmith/native-codec notices continue to apply. No new runtime
dependency or installed-system HDF5/Python requirement is introduced.

## Frozen verification result

The [low-level proof](lowlevel/2026-11-06-pilatus-nexus.json) qualifies **27 native
Pilatus document calls across 9 independent sources** (path, owned buffer and
real stdin), with 7 source profiles matching the unchanged constructor/mapping
and 8 matching the unchanged cleaner. CR-only, mixed Python line separators and
CR-only continuations are positively compared. Malformed/dangling cases retain
raw source and diagnostics without being relabelled successful reference reads.

It also qualifies **42 source-aware Nexus actions** across fixed-ASCII,
Unicode-default and exact three-file external-link inputs. All 21 completed
unchanged reference observations match; 18 reference failures remain explicitly
separate, and 3 exact numeric image reads use the independent HDF5 source oracle
plus the existing typed reader. Original endian/type/shape, uint64 extrema,
attribute bytes, raw link values and source manifests are checked. This does
not establish all Python helper/API parity or h5py object identity.

Every source/probe immutability and pinned-reference-source gate is true. Scoped
validation passed 8 Nexus tests, 7 Pilatus tests and 7 independent Python controls;
HDF5-only/TIFF-only builds, strict Clippy and sparse extraction regression checks
are recorded alongside the root repository-wide validation.
