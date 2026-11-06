# CIF grammar and legacy table/profile qualification

Native input now includes a source-preserving CIF 1.1 syntax tree, labeled
ASCII column tables and FIT2D/CHIPLOT profiles. All three expose path snapshots,
owned bytes and nonseekable streams with explicit resource/source contracts.
Tables and profiles are separate APIs; no detector image, dtype or neighboring
file is invented from them. There are no new production dependencies.

## Closed native gaps

| API | Preserved representation | Independent proof |
|---|---|---|
| [`cbf::syntax`](CIF_SYNTAX.md), existing `cbf` feature | Ordered blocks/save frames/items/loops, exact token/style/null distinctions, source spans and bytes; case-insensitive scoped lookup | [13 sources / 39 native calls](cif-syntax/2026-11-06-cif11.json): 15 exact successful comparisons, 21 specific malformed-CIF rejections and 3 explicit CIF 2.0 rejections |
| [`columnfile`](COLUMNFILE.md), optional feature | Original label/comment/numeric bytes and an explicitly decoded row-major float64 view, with plain-integer exactness flags | [8 sources / 24 ASCII calls](columnfile/2026-11-06-ascii.json): 18 successful comparisons and 6 malformed-row/token rejections; 3 separate binary-capability rejections |
| [`chiplot`](CHIPLOT.md), optional feature | Three opaque headers, declared point/curve counts, every stored X/Y token, comments and row order; opt-in float/grid views | [9 sources / 27 calls](chiplot/2026-11-06-profiles.json), checked against independent exact Python `int`/`Decimal` identities and source spans |

The fixtures are small independently constructed CC0 sources, with generators
or handwritten expected trees and recorded hashes. The actual upstream CIF
fixture is extracted unchanged and identified separately. Text above binary64
precision remains exact in its authoritative lexical representation. Stored X
values are never replaced by a reconstructed uniform axis. Shape/resource errors
return no partial successful document/table/profile.

The strict CIF API shares the existing scanner through an explicit legacy mode.
Detector CBF and the older `cbf::document::HeaderProjection` retain their prior
permissive projection semantics and binary stopping point. Strict standalone CIF
handles `_array_data.data` as an ordinary text item.

## Reference defects, malformed input and unsupported capabilities

These classifications are separate from native source-reading qualification.
The reference is unchanged FabIO 2026.6.0 at commit
`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`.

| Observation | Classification and native decision |
|---|---|
| CIF scopes flatten into FabIO's dictionary; lexical values can lose embedded comment text or empty-literal distinctions | Reference projection limitations/defects. Preserve the strict source tree; record all 52 original load/read observations separately. A flat dictionary is not a complete syntax oracle. |
| The unchanged upstream `test_cif` LaB6 literal repeats `_chemical_formula_sum` | Malformed CIF 1.1 under the [IUCr grammar](https://www.iucr.org/what-we-do/digital-standards/cif/cif1/file-syntax). Strict syntax rejects it; its permissive FabIO acceptance does not establish source validity. The existing detector projection keeps its compatibility policy. |
| `cf_io.read` raises `EBADF` before decoding; `datIO.columnfile.read` wraps that failure | FabIO wrapper defect. No failed original read is counted as successful parity. |
| Untouched C ASCII decoder discards a complete EOF-terminated final row | C decoder defect. Native retains the complete row. Three safe shared-grammar fixtures match the untouched C decoder exactly; a fourth isolates this defect. |
| C fallback/label/row handling permits unsafe memory access or substitutes missing/invalid values | Unsafe/malformed-input behavior, not source counts. Production Rust uses a bounded independent parser. The test-only C oracle admits only small prevalidated inputs, with its actual process-limit results recorded. |
| `CIF.loadCHIPLOT` fails on the profile fixtures: seven `TypeError`, one `ZeroDivisionError`, one comma-input `ValueError` | Broken or limited reference helper. Native acceptance follows the [FIT2D format manual](https://www.esrf.fr/computing/scientific/FIT2D/FIT2D_REF/node115.html) and independent source/numeric checks. No patched reference or successful unchanged-reference parity is claimed. |
| Declared CIF 2.0 | Real remaining native version gap: explicit unsupported-version error; Unicode/composite/triple-quote/text-processing grammar is not implemented. |
| Binary column tables | Unsupported capability in both implementations. FabIO's C decoder is only `return NULL;`; no binary layout/endian/ABI schema is inferred. This is not a malformed-file finding. |

Other profile uncertainty/declaration variants remain explicitly unsupported
where the consulted format description does not define their layout. CIF syntax
does not validate scientific/dictionary semantics. Optional Gemmi observations
are an additional parser comparison, not a substitute for the versioned grammar;
its permissive acceptance is recorded where it differs from that grammar.

## Regression and validation

The [native checks](fabio-parity/2026-11-06-cif-legacy-native-checks.json) record
293 passing Rust tests and 72 passing Python controls, formatting, all-target
all-feature Clippy with warnings denied, all-feature tests, no-default build/eight
tests and 23 isolated optional-feature Clippy checks. External corpus manifests
are supplied explicitly and fingerprinted. Timeout-protected FIFO/directory
controls require the new path APIs to reject nonregular sources before opening;
stream APIs remain explicit caller-supplied captures.

The [full corpus continuation](fabio-parity/2026-11-06-cif-legacy.json) reruns the
237 unchanged codec/opening/header tests with zero failures/errors and one
existing slow-test skip. All 626 native stdin comparisons remain qualified;
prior pixel/reference outcomes and metadata/attribute summaries are unchanged. The
separate [series regression](series/2026-11-06-cif-legacy-regression.json),
[run regression](companions/2026-11-06-cif-legacy-run-regression.json) and
[CIF projection regression](companions/2026-11-06-cif-legacy-projection-regression.json)
retain their original contracts against the new executable: 37 additional
unchanged reference tests, 515 cursor actions, 10,010 filenames and nine calls
each for run records and the earlier CIF projection pass. All six additional
proof gates and source/probe immutability gates pass. All proof artifacts
record exact native/probe/reference/source hashes; historical reports are retained.

Full FabIO reading parity remains unestablished. This continuation closes the
documented native CIF 1.1/ASCII table/profile gaps; it does not turn reference
failures into matches or qualify arbitrary outside-corpus layouts, CIF 2.0,
dictionary semantics or remaining low-level/NeXus helper APIs.
