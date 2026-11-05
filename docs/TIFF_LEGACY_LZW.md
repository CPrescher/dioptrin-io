# Legacy LSB TIFF LZW

The optional `tiff` reader now admits the legacy LSB-first, late-change LZW
streams in the unchanged upstream `quad-lzw.tif`. Its 77 declared strips decode
to exactly 589,824 U8 channel samples with shape `[384,512,3]`, matching both
independent tifffile/imagecodecs and Pillow RGB source arrays. The existing
explicit Pillow13 view separately matches pinned FabIO's F32 luminance output.
The complete source/file, chunk, raw sample and view hashes are recorded in
[`tiff-legacy-lzw-2026-11-06.json`](tiff-legacy-lzw-2026-11-06.json).

No public API changes. `compression=5`, source bits, channels, dtype, photometric
interpretation, predictor, byte order and orientation metadata stay unchanged.
Color samples remain interleaved HWC and stored orientation remains unapplied.
There is no grayscale conversion, inversion, normalization, source repair or
fallback on a failed modern decoder. This is a legacy compatibility dialect;
acceptance does not assert normative TIFF6 conformity.

TIFF 0.11.3 has no external codec hook. A narrowly vendored copy therefore keeps
its mature raster, predictor, endianness and sample conversion implementation,
with one private codec change. Each original chunk's initial nine-bit clear code
selects legacy LSB/late-change versus modern TIFF MSB/early-change before decoding.
This is the same explicit legacy signature used by libtiff's `LZWPreDecode`.
The existing permissive weezl 0.1.12 decoder handles every dictionary entry.
No new dictionary decoder or second raster implementation is introduced.

The legacy path requires a complete end code, exact full source chunk byte count
and only zero padding to the final byte. It rejects trailing bytes, partial end
codes, extra decoded bytes and incomplete source rows. Both horizontal and
vertical tile padding are validated before the unchanged TIFF raster code crops
the visible image. A small bounded code-boundary walk locates the final bit;
it does not reconstruct dictionary values or pixels. This avoids treating bytes
consumed by weezl's private lookahead as verified end padding. These stricter
integrity checks apply to explicitly recognized legacy chunks; modern TIFF LZW
keeps its upstream streaming interpretation.

Encoded bytes, decoded bytes, one overflow detection byte and the existing input
buffer together obey the intermediate payload limit. Fixed codec dictionaries,
allocator bookkeeping and caller-owned buffers remain additional memory. Source
windows and identity checks are unchanged; bytes past a declared bytecount never
supply a missing end code. There is no decoded cache or new performance claim.

The CC0 independent fixtures exercise 9/10/11/12-bit dictionary transitions,
clears after growth, KwKwK, predictors 2/3, planar RGB, big-endian U16, float IEEE
bits, packed 2 counts, strip last rows and nonzero tile padding. A separate
imagecodecs decoder checks every generated stream and tifffile checks complete
source arrays. Regressions cover truncation, end/tail integrity, output counts,
source-window aliases, source replacement/mutation and intermediate limits.
Orientation 6 fixtures explicitly retain stored sample order even though Pillow
applies an orientation transform. A mixed modern/legacy source fixture matches
independent source arrays while Pillow/libtiff fails to switch its codec between
strips; that oracle limitation remains visible in fixture provenance.

The exact MIT upstream archive/commit, baseline file hashes, local patch and
license are retained under `vendor/tiff-0.11.3`. Existing weezl MIT/Apache notices
are retained under `licenses/native-readers`. The root optional path dependency
uses this backend in source/git checkouts. Future crates.io publication requires
a separately published patched backend or another packaging design: Cargo strips
the dependency path and registry TIFF 0.11.3 lacks this patch. No publication was
performed.

Reproduce the external count proof without modifying its source:

```sh
cargo build --no-default-features --features tiff,jpeg --example tiff_view_probe
python scripts/audit_tiff_legacy_lzw.py CORPUS target/debug/examples/tiff_view_probe
```
