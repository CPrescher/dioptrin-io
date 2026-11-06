# CIF 2.0 continuation

`cbf::syntax` now selects CIF 2.0 for the required `#\#CIF_2.0` version header,
with an optional initial UTF-8 BOM. Unmarked documents retain CIF 1.1 rules;
unknown version declarations fail. The detector header projection remains on
its explicit legacy scanner policy. No source bytes, prior fixtures or prior
proof artifacts are rewritten by this continuation.

The grammar follows the [normative IUCr specification](https://journals.iucr.org/j/issues/2016/01/00/aj5269/)
and [annotated EBNF](https://www.iucr.org/__data/assets/text_file/0009/112131/CIF2-ENBF.txt).
CIF2 adds UTF-8 strings, triple quotes, lists and tables. It retains a
2048-character physical-line limit, counted as Unicode scalar values rather
than bytes, while removing the 75-character identifier limit. Only ASCII space,
tab, CR and LF serve as whitespace. Forbidden controls, noncharacters and
noninitial BOMs fail. Names use canonical caseless matching. The versioned
grammar permits empty save frames, requires separators between values, and
requires a table key's colon to touch its quoted closing delimiter.

The existing ordered block/save-frame/scalar/loop AST is preserved. `Value`
remains `Copy` and gains `composite: Option<usize>`, referencing
`Document::composites`. `Composite::List(Vec<Value>)` retains element order;
`Composite::Table(Vec<TableEntry>)` retains every key/value entry in source
order. `TableEntry` contains a key token and value. The arena uses stable
indices, so deeply nested data do not create recursive ownership or recursive
destruction. `doc.composite(value)` checks the index before returning a borrow.

Composite tokens cover their complete brackets, with content spans for the
interior. New styles are `List`, `Table`, `TripleSingleQuoted`, and
`TripleDoubleQuoted`; the internal `Delimiter` style cannot become a parsed
data value. Existing raw token access remains unchanged: `doc.raw(token)`
returns the original lexeme, and `doc.text(token)` returns original UTF-8
content, including original CR/LF sequences. Null markers retain their separate
meaning only as bare `?` and `.`. Numerical-looking strings, uncertainties,
commas and large integers never become scientific numbers. For example,
`[1,2]` contains one bare string `1,2`, not two inferred numeric values.

`doc.logical_text(token)` is an explicit source-derived string view with
`text`, `normalized_eol`, `prefix`, and `folded` fields. CIF2 EOL interpretation
normalizes CR and CRLF to LF in that view. Only semicolon text fields apply the
prefix and folding protocols, in that order. A recognized prefix must occur
on every subsequent physical content line; otherwise the field remains
ordinary text. A single-backslash prefix header is removed. A double-backslash
header leaves a folding header after de-prefixing. Folding removes each fold
separator only when the resulting content begins with a folding header. Triple
quotes never perform these elisions. Raw tokens and source hashes remain
unchanged throughout.

The implementation follows the procedures in normative sections 5.2 and 5.3.
The article's rendered combined-protocol example appears to show one opening
backslash where its prose requires two. The explicit procedures and independent
double-backslash fixtures define this implementation's behavior; a
single-backslash prefix does not automatically enable folding of later lines.

Name keys use the mature `caseless` 0.2.2 and `unicode-normalization` 0.1.24
libraries under their MIT / MIT-or-Apache licenses. Their Unicode 16.0 tables
implement NFD → full default case folding → NFD. Values and original name
spelling are not normalized. These pinned Unicode tables define the matching
profile; later Unicode revisions are not silently claimed. Canonical matching
does not apply compatibility equivalence: identifiers `①` and `1` stay
distinct. Lookup queries have a generous 64 KiB bound, separate from physical
file-name limits, to prevent unbounded normalization of caller-supplied or
mutated strings.

The normative well-formedness criteria require unique data names, block codes
and frame codes but do not explicitly impose table-key uniqueness. Ordered
duplicate table entries therefore remain available rather than being
overwritten. `doc.table_get(value, key)` performs case-sensitive logical-key
lookup and errors if several entries match. PyCifRW accepts duplicate keys but
retains only the final value in its dictionary; comparison with that lossy view
is separate from complete native AST verification.

Existing byte, metadata, token, item, block, column and total-value limits apply
to all nested elements. `Limits::nesting` defaults to 64 with a maximum supported
configuration of 128; this is a resource policy, not a grammar depth limit.
Recursive parsing is bounded before descent, and arena destruction is flat.
Unicode-key working space is admitted before normalization. Retained record
accounting is conservative; temporary normalizer buffers are bounded by
admitted source/name sizes, not a promise about total process RSS.
`Limits::logical_text_bytes` separately admits each logical view's worst-case
working/output buffers before allocation. Invalid mutated ranges and arena
indices return errors or `None` without indexing panics.

`tests/data/cif2/provenance.json` records the CC0 sources, handwritten expected
trees, exact hashes, dependency versions and primary-source receipt. The receipt
hash covers the retained web-tool-rendered article/protocol excerpts, including
retrieval framing, at `/tmp/dioptrin-cif2-iucr-web-capture.txt`; it is not an
unobserved HTTP-body hash. The formerly unsupported
`tests/data/cif-syntax/cif2.cif` is included at its unchanged original path/hash.

`scripts/cif2_reference.py` checks path, byte-buffer and nonseekable-stream APIs
against the complete independent source/AST/span/lexeme/logical-view tree.
Malformed syntax, configured resource rejection and unknown versions remain
different input conditions. Unchanged pinned FabIO `CIF.loadCIF` / `readCIF`
calls are observed through paths and `BytesIO`. Isolated PyCifRW 5.0.1 supplies
an additional CIF2 semantic comparison, with source-module fingerprints.
Its lowercased-name/map/string projection cannot establish raw AST parity,
quote/null distinctions or duplicate-entry retention. Oracle acceptance alone
does not establish standards conformance.

Rust controls cover composites and loop cardinality, separator and colon rules,
canonical Unicode names, line/character/version rules, prefix/folding behavior,
all EOL forms, bounds, UTF-8 split across stream reads, source mutation,
truncation and byte mutation. Python comparison controls alter composite order,
null flags, source spans, integer text and logical-view flags to ensure that
representation losses cannot be ignored.
