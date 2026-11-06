# CHIPLOT/FIT2D profile input

The optional `chiplot` feature exposes a native profile reader through
`chiplot::{Profile, Reader}`. It is independent of Python, CBF, image wrappers,
plotting and integration. The authority is the
[FIT2D input manual](https://www.esrf.fr/computing/scientific/FIT2D/FIT2D_REF/node115.html):
its minimum format has three title/axis-label records, a point count and optional
curve count, then one X and every curve's Y on each row. Spaces and commas are
supported. The retrieved primary HTML has SHA256
`e3b012c52ad4134f32cbd0fb72373931c5c1e1f4c53a73701260db9ac7ba59b7`
(37,018 bytes); fixture provenance records the audit copy. FIT2D's
[output manual](https://www.esrf.fr/computing/scientific/FIT2D/FIT2D_REF/node128.html)
also shows the common single-curve ASCII profile.

`Profile::from_bytes` owns a copy (`Provenance::Buffer`);
`Profile::from_reader` captures bounded nonseekable input (`StreamCapture`).
`Reader::open` retains the path snapshot and original file handle; every
`metadata()` and `source_path()` access checks growth, replacement and source
identity again. Buffer/stream provenance does not claim to monitor a file behind
an owned capture.
Path snapshots admit only regular files before opening; caller-owned streams
are supplied explicitly instead of implicitly opening named pipes.

The primary representation is **exact source lexemes**. Original bytes, header
and declaration spans, declared point/curve counts, physical row numbers, every
X/Y token and offset, comments and row order remain available. Labels are opaque
source bytes with a fallible UTF8 view. LF and CRLF records are supported. There
is no automatic sorting, uniform-grid reconstruction, correction or intensity
normalization. Multiple Y columns are retained as multiple curves; no column is
silently reinterpreted as an uncertainty.

`Sample.number` distinguishes exact `I64`/`U64`, larger `IntegerText`, decimal
text and explicit nonfinite tokens. Decimal precision/exponents and integers
beyond binary64 or uint64 remain in their original strings. This ASCII source
has no invented fixed binary storage dtype. Nonfinite spellings are an explicitly
diagnosed lexical extension. `#` comments are retained, including a count-line
comment extension. Parenthesized uncertainties, D exponents, extra declaration
flags, missing comma values, wrong row widths, additional datasets/trailers and
count mismatches fail explicitly; unsupported uncertainty layouts are not guessed.

`Sample::f64_view` is an opt-in conversion. Its returned `FloatView` labels exact
integers, rounded integers, unverified large-integer approximations, decimal
approximations or nonfinite values. `RequireExactInteger` rejects integer rounding;
`AllowRoundedInteger` must be selected explicitly. Comparisons use wider integer
ranges, avoiding the saturating-cast trap at `i64::MAX` and `u64::MAX`. Finite
scalar overflow/underflow fails, signed zero is preserved where represented by
the token, and caller-edited lexeme/kind inconsistencies fail.

`Profile::grid_view(tolerance, policy)` is a separate derived float64 assessment,
with explicit absolute tolerance and integer conversion policy. It reports actual
first/last/minimum/maximum values and an endpoint-average step. A uniform step
is present only when every converted X agrees within the caller's tolerance.
Descending steps remain signed; nonuniform axes and zero/one-point profiles have
explicit diagnostics. Source X/Y values are never replaced. Decimal/rounded
conversion flags accompany the assessment. These are not automatically emitted
pdCIF fields or a claim that an arbitrary profile has a uniform physical grid.

Encoded bytes, four-line header bytes, point/curve counts, sample storage,
retained lexemes/comments and transient token spans are bounded with shared
`Limits`. All count arithmetic is checked before allocation; shape validation
rejects missing/extra rows without padding them. The source and parsed metadata
are charged together, with conservative scratch/comment-vector allowance.

## Independent proof and pinned reference defect

The nine CC0 profiles in `tests/data/chiplot/` are generated independently by
`scripts/generate_chiplot_fixtures.py`; no reference writer produces them. They
cover ascending, descending and irregular axes, multiple curves, empty and
one-point profiles, exact large integers, long decimal strings, tiny magnitudes,
nonfinite extensions, comments and CRLF. Malformed declaration, row width,
uncertainty/D-exponent, limit and source-mutation controls are separate Rust tests.
Public mutation controls ensure grid queries return errors for malformed rows
and float queries reject inconsistent scalar kinds rather than panic or claim
false exactness.

`scripts/chiplot_reference.py` uses a separate Python `int`/`Decimal` oracle.
It checks all native source fields and every exact scalar identity without a
binary64 roundtrip, using path, owned-buffer and real stdin calls. The unchanged
pinned `CIF.loadCHIPLOT` API is observed separately. Its Python3 implementation
formats the derived step as text and then compares it with integer zero, raising
`TypeError`; one-point input can raise `ZeroDivisionError` earlier, and its
single-Y whitespace-only path cannot represent all documented multi-curve/comma
inputs. Those observations are failures or reference limitations, not successful
unchanged read parity. No patched reference is used, no original reference object
is repaired, and no reference-derived uniform grid replaces native X values.

The pinned FabIO checkout is commit
`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`; `cbfimage.py` SHA256 is
`0161b83dce84114421767969ccfef6ae8d51bf90739ab84cc5cf390da1e844d3`.
Its MIT notice remains in `licenses/native-readers`; this native reader follows
the documented format and uses shared resource/source utilities and Rust scalar
parsing, without a production Python or new numerical/parser dependency.

## Frozen verification result

The [profile proof](chiplot/2026-11-06-profiles.json) qualifies **27 native API
comparisons across 9 independent CC0 sources** (path, owned buffer and real
stdin), including every raw source field and exact integer/Decimal scalar
identity. Native/reference source hashes and the probe remain unchanged during
the run. The unchanged pinned reference records 7 TypeError failures,
1 ZeroDivisionError and 1 delimiter-related ValueError. It establishes no
unchanged reference reading parity; no patched reference is used.

Scoped checks passed: 6 native Rust tests, 4 independent Python controls,
CHIPLOT-only strict Clippy and scoped formatting. Global repository checks and
the original detector corpus are recorded separately.
