# CIF 2.0 and low-level reader qualification

This continuation adds versioned CIF 2.0 syntax, low-level TIFF metadata and
sample controls, standalone Pilatus input, read-only NeXus navigation and original
CBF encoded-payload access. Production code reuses the existing sample readers,
TIFF directory reader, Pilatus parser, HDF5 source graph and typed attribute
reader. There is no Python runtime or new parallel decoding implementation.
The optional CBF feature adds pinned Unicode libraries for name matching; their
[original notices](../licenses/native-readers/CIF2-UNICODE-NOTICES.md) are retained.

## Native contracts and evidence

| API | New native scope | Evidence |
|---|---|---|
| [`cbf::syntax`](CIF2_SYNTAX.md) | CIF 2.0 UTF8 tokens, nested lists/tables, triple quotes, canonical caseless names and explicit logical text views; original bytes/scopes/order preserved | [CIF 2.0 proof](cif2/2026-11-06-cif2.json) and [CIF 1.1 regression](cif-syntax/2026-11-06-cif2-cif1-regression.json) |
| [`tiff::api`](TIFF_API_PARITY.md) | Source-checked image counts, original IFD offsets/tags, stored-word row selection, image alias and explicit reference palette/mono views; separate metadata-only catalog independent of pixel layout | [TIFF API proof](tiff-api/2026-11-06-read-apis.json) |
| [`pilatus`](LOWLEVEL_HELPERS.md) | Bounded path/buffer/stream document, existing typed/raw fields and read-only queries, explicit cleaned-line view | [Helper proof](lowlevel/2026-11-06-pilatus-nexus.json) |
| [`hdf5::nexus`](LOWLEVEL_HELPERS.md) | Entry/class/detector/data/default/attribute queries returning owned source descriptors; existing typed image reading, explicit external-link snapshots and source creation order | [Helper proof](lowlevel/2026-11-06-pilatus-nexus.json) |
| [`cbf::Reader::open_encoded`](CBF_ENCODED.md) | Exact encoded bytes and declared/computed integrity, including opaque conversions; decoded image admission remains separate | [Encoded proof](cbf-encoded/2026-11-06-raw.json) |

The CIF 2.0 proof qualifies 87 native calls: 30 complete valid-source trees,
51 malformed-source rejections, three explicit default nesting-bound rejections
and three unknown-version rejections. The TIFF proof qualifies 171 records,
including five actual upstream outer reads, 530 metadata queries and 58 each
of independently checked stored arrays and explicit display views. Pilatus adds
27 document calls across nine profiles; NeXus adds 42 actions, with 21 completed
unchanged reference observations matching, 18 original failures kept separate
and three exact image reads checked independently. Encoded CBF adds 11 complete
captures and one strict incomplete-source rejection across 12 inputs.

CIF 2.0 interpretation follows the [IUCr specification and grammar](https://www.iucr.org/what-we-do/digital-standards/cif/cif2).
Original lexical bytes and logical string views are separate. Numeric-looking
values remain text; duplicate table entries are retained, with ambiguous lookup
reported instead of overwriting values. CIF 1.1 and the detector's permissive
legacy projection retain their own policies. The formerly unsupported CIF 2.0
fixture is read from its unchanged historical path and hash.

The TIFF adapter preserves native words, dtype, channels and orientation.
Reference palette lookup and its BGR-weighted mono output are explicitly derived
views. Inclusive row selection does not promise partial decode performance.
Missing legacy strip counts can use the existing explicit `FromShape` policy;
the proof selects this by source hash before reading and retains the absent
original tag, actual IFD descriptors and effective windows. Default strict reads
never retry through a different interpretation.

NeXus navigation preserves exact supported attribute types, shapes, endian and
values, including integers beyond binary64 precision. Dataset descriptors do not
claim to be h5py objects. Image selection reuses the typed HDF5 reader; scalar,
1D, text or unsupported datasets remain readable as metadata without fabricated
image arrays. Source-checked metadata queries compare the bounded graph again,
including typed attribute bits, object identities, shapes and selected entry
control text. Group depth, query output and metadata admission are explicit;
native variable-length text allocation retains the shared HDF5 reader's
documented post-read bound, without a total RSS guarantee.

## Classification boundary

The proof artifacts distinguish independent source correctness from unchanged
FabIO results at pinned commit `9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`.

- FabIO's CIF reader does not implement the complete CIF 2.0 value grammar.
  Loss of composite/string/scope information is a reference capability boundary;
  native standards-based success is not relabeled as unchanged FabIO equality.
  PyCifRW is a separate independent comparison, with its own disagreements
  retained rather than treated as the specification.
- TIFF row-selection, single-strip palette-window and PackBits defects require independent
  stored-word and display calculations before qualification. Negative indices
  are a separate native checked-index policy, not evidence of a corrupt source.
- Pilatus constructor/cleaner exceptions are observed on the original methods.
  Native raw text and diagnostics remain available, including malformed fields.
  The comparison isolated a real native CR-only/separator gap; the shared parser
  and cleaned view now handle Python-compatible line boundaries and continuation
  while retaining original bytes. Valid source variants are not relabeled as
  malformed or blamed on the reference.
- Pinned NeXus default attribute filtering can raise `TypeError` with the pinned
  h5py stack; byte-valued default paths and file-object external-link access have
  separately observed reference limitations. Successful named queries and
  independent exact-source navigation are measured separately from those failures.
- A complete encoded CBF payload can contain a wrong checksum or an incomplete
  count escape. Byte access reports integrity and does not certify count decoding.
  An actually truncated declared payload remains a strict native rejection;
  the reference's shorter raw return does not supply the missing bytes.

Pure time/string/signature helpers, writer/mutation methods and Python object
identity are separately inventoried in the helper contracts. Unsupported HDF5
attribute/dataset kinds, other TIFF pixel layouts/codecs, multiple CBF binary
sections and source variants outside the measured profiles remain explicit.
CIF dictionary/scientific semantics and Unicode tables beyond the documented
implementation profile are not established by a syntax proof.

## Regression and validation

The [native checks](fabio-parity/2026-11-06-cif2-lowlevel-native-checks.json) record
331 Rust tests, formatting, all-target/all-feature strict Clippy, no-default
build/eight tests, all 23 isolated optional features and the Rust 1.89 CBF build.
All 100 Python controls pass. Exact source/probe/report hashes are recorded there.

The [full corpus continuation](fabio-parity/2026-11-06-cif2-lowlevel.json) runs
the existing 237 unchanged codec/opening/header tests with zero failures/errors
and one existing slow-test skip. All 626 native stdin comparisons remain
qualified; prior pixel/reference outcomes and metadata/attribute summaries are
unchanged. The 37 series tests and three TIFF tests also pass, giving 277 unchanged
reference tests across the separate audits. TIFF writer tests provide reference
input files/read observations; native writer parity is not claimed.
Separate revalidations retain [series/navigation](series/2026-11-06-cif2-lowlevel-regression.json),
[run records](companions/2026-11-06-cif2-lowlevel-run-regression.json),
[CIF projections](companions/2026-11-06-cif2-lowlevel-projection-regression.json),
[column tables](columnfile/2026-11-06-cif2-lowlevel-regression.json) and
[CHIPLOT profiles](chiplot/2026-11-06-cif2-lowlevel-regression.json).
Historical reports are retained unchanged. Full FabIO reading parity remains
unestablished; qualified source differences, unsupported variants and failed
reference APIs are not counted as exact successful matches.
