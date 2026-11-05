# Bruker, Oxford and R-AXIS native input

The optional `bruker`, `oxford` and `raxis` features provide independent native
readers. Each currently includes the `native` feature for shared bounded byte
and scalar utilities. No Python or additional codec dependency is added.

All three expose `Reader::open(path, Limits)`, `metadata()`, `frames()` (one), and
`read_frame_into(&mut FrameBuffer)`. There is no implicit file series, directory
search, growing acquisition refresh or decoded cache. Each read checks the
canonical source and open handle before and after I/O. Concurrent reads use
independent file handles and caller buffers. A failed read invalidates the
buffer's shape/pixel view. These metadata identity checks detect ordinary source
replacement/mutation, not adversarial writes preserving all identity metadata.

Header, retained metadata, encoded payload and native decoded frame have separate
admission limits. These are allocation admission limits, not process RSS bounds:
metadata maps, indexing scratch and caller buffers coexist. Bruker86 uses a
bounded position bitset for duplicate overflow checks; Oxford TY1 and R-AXIS scan
the encoded payload during indexing to admit the eventual output dtype. Shape,
integer arithmetic, offsets, table consumption and decoded range are checked.
Stored row order, reserved numeric values and raw header bytes survive. Consumer
code owns geometry, orientation, masks, normalization and corrections.

## Bruker86 and Bruker100

Headers contain 80-byte ASCII records in a declared multiple of five 512-byte
blocks. Original records and bounded trailing padding survive verbatim. Parsed
free-text repeats are LF-joined; duplicate structural fields fail. Supported
storage is little-endian unsigned 8/16/32 for version86 and unsigned8/16 or
signed32 for version100. Unsupported byte orders/compression/version fail.

Version86 expands declared 16-byte ASCII intensity/position overflow entries to
u32. Positions must be unique, inside the image and reference a stored sentinel.
Without an overflow table, the original native unsigned type is retained.

Version100 requires a 1/2/4-byte underflow width whenever an underflow table is
declared; zero-width declared tables are rejected. It decodes padded signed underflow and unsigned16/signed32 overflow
tables and its stored baseline into i32. Both declared table consumption and
baseline addition are checked. Missing tables preserve their numeric sentinels.
The original stored dtype, underflow width, table counts and baseline remain
metadata. Underflow, overflow and baseline are the declared storage encoding;
`LINEAR` is separate calibration metadata and is never applied implicitly.
FabIO's Bruker86 floating writer/read test applies `LINEAR` and returns float32;
that test has an intentional representation difference, demonstrated by a fixture.

## Oxford / OXD

Version3 and4 binary headers, uncompressed little-endian i32 and TY1 are admitted.
The complete raw header (including unused v4 padding), named section bounds,
original acquisition scalars and bounded trailer are retained. Supplementary
layouts with nonzero NSUPPLEMENT fail explicitly. No motor-angle conversion,
standard-deviation calculation, geometry or stored orientation transform is
performed; motor steps, radians per step and variance remain original metadata.

TY1 decodes flat signed differences with unsigned byte code minus127; codes254
and255 consume little-endian signed16 and signed32 exception deltas. A checked
i64 cumulative sum spans the whole frame. Every exception must be consumed once.
The output is the smallest signed native type fitting both extrema. TY1 has no
single stored scalar dtype; metadata distinguishes the encoding and decoded type.
FabIO chooses its output width from maximum alone, so a negative-only range can
wrap; our negative fixture retains those counts and records this difference.

**TY5 remains unsupported.** The pinned reference labels that decoder unreliable.
Its 100nm-film test file is retained as a visible rejection in reference evidence;
matching selected statistics would not independently qualify that decoder.

## R-AXIS

`Reader::open` preserves exact stored u16 words, including the high range flag.
`Reader::open_with_options(path, limits, Options { byte_order, interpretation })`
accepts explicit `ByteOrder::Big` (default) or `Little`, and explicit
`Interpretation::Stored` (default) or `PhotomultiplierExpanded`.

Expansion leaves unflagged words unchanged and computes
`trunc((word & 0x7fff) * source_f32_ratio)` in f64, admitting only finite,
nonnegative values fitting u32. It returns u32 when flags exist, or original u16
when none exist, matching FabIO. Ratio, source u16 dtype, selected policy, range
flag presence, full header/padding and all fixed header fields remain metadata.
Stored and explicitly expanded representations have independent fixture tests.
The default cannot be mistaken for FabIO's implicit photomultiplier expansion.

The admitted single-frame layout has at least1400 header bytes and exactly one
prefix record: its declared Record Length must equal the prefix length inferred
from shape and total file length. Unknown prefix/record layouts, short frames,
trailing data and stacked sources fail. This additional agreement prevents the
reference EOF-relative decoder from silently shifting counts after a truncated
payload. Scan-direction flags, front/back flags, even/odd correction, pixel shift
and fade metadata are retained without changes to counts.

## Reproduction and evidence

`scripts/generate_legacy_fixtures.py` independently packs explicit arrays with the
Python standard library. All 20 small files and their generator are CC0-1.0.
`tests/data/legacy/provenance.json` records source hashes, exact native types,
arrays and canonical little-endian count hashes. Tests cover signed extrema,
all supported underflow widths, overflow chains, signed TY1 widths through i64,
negative TY1 count preservation, raw data, both R-AXIS byte orders/interpretations,
raw metadata, malformed/duplicate/unused tables, resource limits and mutation.

FabIO2026.6.0 at commit `9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`
was used only as an independent test reference. Build and run:

```sh
python3 scripts/generate_legacy_fixtures.py
cargo build --no-default-features --features bruker,oxford,raxis --example legacy_probe
/path/to/reference/python scripts/legacy_reference.py --corpus /path/to/fabio/testimages --output /tmp/legacy-reference.json
```

The external corpus comes from FabIO's test-image service
`http://www.edna-site.org/pub/fabio/testimages` via the pinned upstream test suite;
large files are not redistributed here. Exact source and decoded count hashes
are recorded in [the reference evidence](legacy-readers/2026-11-05-reference.json).
Seven actual upstream files match shape, native dtype and every canonical count
byte: Cr8F8140k103.0026, NaCl_10_01_0009.sfrm, b191_1_9_1.img,
b191_1_9_1_uncompressed.img, d80_60s.img (2048x2048 TY1),
pilatus300k.rod_img and mgzn-20hpt.img (explicit expansion).

This qualifies these encodings/files; it does not claim arbitrary format parity,
TY5, external wrappers/neighbor series, unknown R-AXIS record layouts, arbitrary
supplementary Oxford layouts or performance. Wrapper/probe integration and the
global upstream-test ledger are maintained separately.

Registry searches found no suitable permissively licensed Rust reader for these
three families. The bounded parsers use pinned MIT FabIO format/codec references;
the R-AXIS field-layout table is adapted with Brian R. Pauw's attribution retained
in `licenses/native-readers/legacy-layouts-MIT.txt`. Existing GPL reader candidates
are not incorporated, and no Python runtime is part of this crate.

## Explicit Bruker86 floating-image view

The pinned Bruker writer encodes floating images as rounded unsigned words using
`LINEAR` slope/offset, and its reader decodes them into f32. These coefficients
are file encoding metadata; their presence does not authorize applying a detector
calibration. `bruker::view::read_linear_into(&reader, &mut buffer)` explicitly
selects that file view. It uses separate f64 multiplication/addition followed by
f32 conversion, matching the pinned reader's order. Missing or identity
coefficients preserve the integer representation, and format100 is rejected by
this specific view. `wrapped::Format::BrukerLinear` provides the same explicit
choice through bounded outer compression.

The ordinary reader continues to expose stored counts and original type; its
metadata does not change when a view is requested. The returned buffer's type
identifies the selected representation. The wider f32 output has its own frame
admission check, source changes invalidate the operation, and original words
remain available through another ordinary read. No normalization, geometry or
other correction is performed. The comparison bridge hashes the stored output
and the selected float view separately and reports a view match distinctly.
