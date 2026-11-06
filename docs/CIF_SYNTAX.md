# Structured CIF syntax

`cbf::syntax` exposes versioned strict CIF 1.1 and CIF 2.0 documents under `cbf`.
This page describes the CIF 1.1 profile; see [CIF 2.0 syntax](CIF2_SYNTAX.md)
for composite values, Unicode names and explicit logical text views.

The CIF 1.1 syntax document uses the existing `cbf`
feature. `Document::from_bytes`, `Document::from_reader`, and the immutable
`Reader::open` path API preserve original bytes and produce ordered data blocks,
save frames, scalar items and loops. This API parses syntax, not dictionary
semantics, chemical structures, experimental corrections or scientific numbers.

The supported grammar follows the [IUCr CIF 1.1 specification](https://www.iucr.org/what-we-do/digital-standards/cif/cif1/file-syntax).
Names are case insensitive within their scopes; duplicate block, frame or item
names fail. Save frames cannot nest. Loops require columns and complete nonempty
rows. Reserved STAR constructs fail. Quoted literals, column-one semicolon
fields, comments and null markers retain their syntactic distinction. The fixed
ASCII character set, 2048-character line limit and 75-character name/code limits
are grammar rules, separate from configurable resource limits. An empty file or
empty data block is valid; an empty save frame violates the formal production's
requirement for at least one data item.

`Document` contains `original`, `version`, `provenance`, and `blocks`. Each block
has `name`, `items`, and `span`. `Item` is `Scalar`, `Loop`, or `SaveFrame`:

- Scalars expose a name token, value, and complete item span.
- Loops expose column tokens, flat row-major values, and their span. `rows()`
  provides slices without changing values or allocating a table copy.
- Save frames expose a name token, ordered items, and heading-through-terminator
  span. Their scope remains separate from the enclosing block.

Every token contains its original `span`, `content` span, and `style` (`Bare`,
`SingleQuoted`, `DoubleQuoted`, `TextField`). Spans are zero-based half-open byte
ranges. Block/frame content excludes the `data_`/`save_` prefix, while their raw
span retains it. `doc.raw(token)` returns the original lexeme and
`doc.text(token)` returns unmodified content. A text field's content excludes its
delimiters and the closing delimiter's preceding EOL; internal CR/LF/CRLF bytes,
leading spaces and trailing spaces remain intact. These are raw lexical
contents, not an EOL-normalized semantic string. Consumers may explicitly
normalize a separate view if required for comparison.

`Value::null` distinguishes bare `?` (`Unknown`) and `.` (`Inapplicable`) from
quoted literal question marks and periods. All numerical-looking text stays
text, including uncertainties, leading zeros and integers beyond f64 precision.
`block`, `scalar`, `loop_column`, and `save_frame` perform ASCII case-insensitive
lookup while preserving original spelling and order.

`Limits` bounds source bytes, aggregate retained metadata, tokens, blocks, items,
columns per loop and total values. Limits must be positive and finite. The
scanner advances over one bounded source and parsing has only the grammar's
single permitted save-frame level. File access uses the shared snapshot's
original alias, resolved path and open-handle identity checks. A failed parser
returns no partial AST. Nonseekable stream capture is an owned snapshot from the
current position through EOF; it does not monitor the original producer later.
Path snapshots require regular files before opening; named pipes belong in an
explicit caller-supplied stream, rather than an implicitly blocking path open.

The detector header parser and the existing `cbf::document` API continue using
their permissive projection policy. Their scanner is shared with strict syntax
through an explicit legacy mode: duplicate handling, diagnostics, costs, newline
quirks and the detector `_array_data.data` stopping point remain intact. Under
standalone strict syntax, `_array_data.data` is an ordinary data name. Binary CBF
payloads are outside this text-only API; image decoding stays in the existing
detector reader.

Declared CIF 2.0 now selects a [separate versioned grammar](CIF2_SYNTAX.md).
Unknown version markers return `unsupported CIF syntax version`. Unmarked input
is interpreted as CIF 1.1. Some one-line strings are valid in both versions with
different meanings, so syntax cannot infer an omitted version declaration.

The CC0 fixtures and handwritten expected scope trees are recorded in
`tests/data/cif-syntax/provenance.json`, including exact file hashes. Its primary
specification receipt pins the URL, version, paragraphs and hash of the retained
web-tool capture at `/tmp/dioptrin-cif-iucr-web-capture.txt`. That hash covers
rendered primary-source excerpts plus retrieval framing; it is explicitly not an
HTTP-body hash. Direct HTTP access returned 403, so no unobserved body hash is
claimed.

`scripts/cif_syntax_reference.py` compares all three native input APIs against
the independent expected scope/order/style/null/value/source/span records. It
also runs the unchanged pinned FabIO `CIF.loadCIF` and `readCIF`, each with a path
and `BytesIO`, in isolated subprocesses. An explicit flat consumer projection
records reference differences separately: scope flattening, case-sensitive
keys, comment stripping inside values, empty literal handling and raw EOL
differences do not redefine valid source syntax. Optional Gemmi 0.7.3 observations
pin both Python and native-module hashes; its acceptance alone does not establish
conformance (for example, it accepts empty save frames and `global_` input).

The actual pinned FabIO `test_cif` LaB6 string is extracted unchanged from its
test module into a temporary file and hashed. It repeats
`_chemical_formula_sum`; strict syntax correctly rejects this malformed source.
Its permissive FabIO acceptance is recorded separately from genuine reference
projection defects. No upstream source, test or input is repaired.

Targeted Rust checks cover scopes and case, loops, raw token boundaries, all
three EOL forms, malformed syntax, version rejection, resource limits, source
replacement/growth/symlink retargeting, and truncation/mutation controls. Existing
standalone projection and CBF decoded-count tests also pass. The reference
controls deliberately change scope, null flags, styles, large integer text,
spans and source hashes to verify they cannot be ignored by comparisons.

The [original CIF 1.1 proof](cif-syntax/2026-11-06-cif11.json) qualifies 39 native calls:
15 exact valid-source trees, 21 specific malformed-source rejections and three
then-unsupported CIF 2.0 rejections. All 52 unchanged FabIO observations complete;
their projection/acceptance differences remain separately classified. Six Python
controls include timeout-protected FIFO/directory admission checks for both the
CIF and profile path APIs. Reproduce the syntax proof with the pinned test
Python environment to reproduce the current CIF 1.1-only regression (the
historical CIF 2.0 source is now separately accepted and qualified):

```sh
python scripts/cif_syntax_reference.py --probe target/release/examples/api_probe \
  --upstream /tmp/dioptrin-io-fabio-2026.6.0 --cif1-only --output /tmp/cif11-reference.json
```

`--independent-module-directory` optionally supplies an isolated Gemmi module
directory; the final report records its exact module hashes and observations.
