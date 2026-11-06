# Legacy labeled column tables

The optional `columnfile = ["native"]` feature provides a separate table API.
It does not pretend a labeled table is a detector image or select neighboring
files. No new production dependency or Python/C runtime is required.

`columnfile::Table::from_bytes(bytes, Format::Ascii, limits)` copies immutable
source bytes. `Table::from_reader(reader, Format::Ascii, limits)` captures the
current position through EOF without seeking or retaining the caller's reader.
`columnfile::Reader::open(path, Format::Ascii, limits)` retains the original
handle and a checked path binding. Its `table()` and `source_path()` methods
reject source replacement, growth and symlink retargeting. Reopen after changes;
the path reader holds an explicitly bounded decoded table snapshot, not a live
view. Stream/buffer captures remain independent of subsequent caller changes.

## Source representation

`original()` retains every byte. `labels()`, `comments()` and `lexemes()` borrow
exact source spans; labels are opaque bytes in their original order, including
duplicates. Comments retain complete lines and line endings. Numeric lexemes
follow row-major source order. `shape()` is `[rows, columns]` and `values()` is
a **decoded float64 view**, matching the legacy C ASCII reader's numerical
representation. ASCII files contain text, not stored IEEE binary64 samples.
No transpose, normalization, mask, label-key grouping or implicit integer dtype
conversion occurs.

For example, the source integer `9007199254740993` survives exactly in its
lexeme; its float64 view is `9007199254740992`. `integer_exactness()` returns
`Some(true/false)` for plain signed/unsigned decimal integer tokens and `None`
for other spellings, which makes no exactness assertion for decimal fractions,
exponent notation or nonfinite values. Exactness compares the full integer
spelling with the decoded binary value's integer representation, avoiding the
saturating float-to-integer cast trap at i64/u64 maxima. Negative zero survives
in both its text and decoded float64 sign bit. Finite/nonfinite interpretation
does not replace the authoritative original tokens.

The explicit native ASCII grammar requires a first `#` label line, at least one
label, and exactly that many full numeric tokens on every data row. Decimal
float/exponent notation and Rust's IEEE inf/nan spellings are admitted; C99 hex
floats and partial numeric prefixes are unsupported. NUL bytes, blank data rows,
missing/extra values and malformed tokens fail. Header-only tables have zero
rows. A complete final data row may end at EOF without a newline. Later lines
whose first non-whitespace byte is `#` are retained as comments, rather than
inventing numeric rows from them. This comment contract and support for more
than 31 columns are checked with independent known arrays; the unsafe legacy C
decoder is not used as their oracle.

`Limits` separately bound source bytes, line bytes, rows, columns, decoded value
bytes and aggregate retained metadata. Original bytes, table structure and each
label/comment/lexeme span are admitted before their allocations; decoded values
are separately admitted before allocation. Bounded source capture reads at most
the source/metadata limit plus one byte to reject oversized streams. Invalid
format/limits are rejected before consuming input. Allocator bookkeeping and
small parsing/formatting stack or temporary space are additional; these limits
are not RSS limits. Integer-exactness iteration formats at most a binary64-sized
integer spelling temporarily and does not retain a second table.

`Format::Binary` is an explicit **unsupported capability**, not a malformed-input
classification. The pinned upstream `cf_read_bin` consists solely of
`return NULL;`, while the binary writer is only declared/commented out. There
is no binary schema, endian convention or ABI serialization to implement from
that source. The native reader does not guess one or label ASCII interpretation
as binary bit preservation. Compression is likewise not inferred by this table
API; callers can explicitly supply an already decoded stream.

## Existing reader review and independent oracle

The consulted FabIO 2026.6.0 source is pinned at
`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`:

- [C decoder](https://github.com/silx-kit/fabio/blob/9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece/src/fabio/ext/src/columnfile.c), SHA256 `611d2caf76266720bed9c42b4ef66d5da1f22cceee450a316a13f74b5b7cf7c0`.
- [Declarations](https://github.com/silx-kit/fabio/blob/9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece/src/fabio/ext/include/columnfile.h), SHA256 `1a83de5847afc53230daeaf8328ce101dda69b0300adac160e4e2127f1ff24ab`.
- [Original wrapper](https://github.com/silx-kit/fabio/blob/9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece/src/fabio/ext/cf_io.pyx), SHA256 `2cc6f074376422420fe72d66ae4a2b6df792dea329b5ce84d7dcfcf3a930a2fa`.

The existing MIT C implementation was evaluated before adding the Rust parser.
Runtime reuse is unsafe: `%s` label reads have no width limit; the 32-column/zero
fallback has an unbounded OR loop and a wrongly sized realloc; missing row
cells are uninitialized; extra cells are ignored; invalid numeric text becomes
zero through `atof`. The C ASCII writer also indexes `data[c][r]` despite the
reader's row-major `data[r][c]`. None of these behaviors define substitute source
counts for the strict reader.

The original `cf_io.read` wrapper raises `OSError(EBADF)` in both ASCII and binary
mode before calling either C decoder: it passes a descriptor to `os.fdopen`,
then closes that same descriptor again. `datIO.columnfile.read` wraps this in a
generic read error. The reference audit runs those original functions unchanged
in a disposable child TMPDIR; it neither repairs the descriptor handling nor
claims that the wrapper passed.

`scripts/columnfile_reference.py` separately compiles the exact pinned C decoder
unchanged in a temporary directory. It verifies source bytes against their Git
objects and records source, declaration, copyright, compiler, wrapper and binary
hashes. The oracle is restricted to 1–31 labels of at most 127 bytes, at most 64
complete rows, 2,047 bytes per line and 64 KiB overall. Blank/comment data rows,
control bytes and malformed rows are rejected **before** invoking unsafe C.
These inputs cannot enter the faulty label fallback or row-reallocation branch;
the pinned decoder's admitted allocation requests total less than 3 MiB.

The C child has a two-second CPU limit, five-second wall timeout, zero core
limit and one-MiB regular-file output limit. Its stdout is bounded by the admitted
31 × 64 result schema. A 256-MiB address-space limit is attempted after exec and
the actual OS result is recorded: this macOS host rejects it with EINVAL (22),
so no successful address-space/RSS limit is claimed. The source-admission bound
remains mandatory. ABI evidence records int=4, double=8, pointer=8, little endian
and IEEE754 binary64 on this host; it does not establish any binary file schema.

The Rust parser is an independent bounded implementation. No C source is copied
or linked into the production crate. The consulted/oracle FabIO implementation
is MIT, copyright Risoe authors (2007–2009) and Jonathan P. Wright, Jérôme Kieffer,
Gaël Goret/ESRF (2009–2026); its original copyright manifest and license notices
remain in [FabIO attribution](../licenses/native-readers/FabIO-2026.6.0-copyright.txt)
and the [MIT notice](../licenses/native-readers/legacy-layouts-MIT.txt).

## Independent fixtures and checks

`scripts/generate_columnfile_fixtures.py` and `tests/data/columnfile/*` are
CC0-1.0. Eight small independent sources cover extrema/rounding, duplicate and
opaque labels, raw CRLF/comments, 33 columns, a header-only table, a complete
EOF-terminated row, and malformed row/token failures. The C oracle verifies
safe shared grammar exactly. The EOF case independently demonstrates that the
legacy C reader discards a complete final row when its `feof` check fires after
`fgets`; native retains both complete tokens without adding bytes.

Eight Rust tests cover source representations, extrema/exactness, unsupported
binary selection, row/label/token failures, resource bounds, nonseekable stream
capture, source replacement/growth and symlink changes. Six Python controls
exercise unsafe-C admission, complete source/view projections, high integer
rounding, fixture provenance and the original C/wrapper distinction. No runtime
performance or general arbitrary-input safety claim is made for the C oracle.

The [2026-11-06 same-source proof](columnfile/2026-11-06-ascii.json) passes every
gate: eight fixtures × three native APIs produce 18 complete/empty table reads
and six expected strict malformed-input rejections. Three additional explicit
binary-mode requests report the unsupported capability. Three safe C cases match
their independent shapes, labels and float64 bit patterns; the fourth records
the qualified EOF-row discard above. The original ASCII/binary Python wrapper
calls both retain EBADF, and the original `datIO` call retains its generic error.

The proof compares every source byte, label/comment/token span, decoded value
bit, canonical value hash, integer-exactness indicator and capture provenance.
Its 91 tracked source/fixture/helper/import/build inputs and native probe remain
unchanged before/after the run. Exact final report/probe hashes are recorded in
the [combined native checks](fabio-parity/2026-11-06-cif-legacy-native-checks.json).

Reproduce with the pinned test Python environment (only the audit needs Python):

```sh
python -m unittest discover -s scripts -p test_columnfile_reference.py
python scripts/columnfile_reference.py --probe target/release/examples/api_probe \
  --fabio-source /tmp/dioptrin-io-fabio-2026.6.0 \
  --report /tmp/columnfile-reference.json
cargo test --no-default-features --features columnfile --test columnfile
```
