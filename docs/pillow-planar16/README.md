# Pillow planar16 attribution, 2026-11-06

**Both original display mismatches are Pillow13 decoder defects exposed by
FabIO's fallback.** Both files are complete and their native uint16 samples were
already correct. This investigation also found and fixed two Dioptrin gaps:
an overly broad compressed-planar display rejection and an inherited TIFF
backend edge-tile indexing bug.

| Original file | Stored samples | Independent native source comparison | Wrong Pillow grayscale pixels |
|---|---|---|---:|
| `flower-rgb-planar-16.tif` | Big-endian uint16 RGB, planar2, raw, 43x73 | Every word matches tifffile and native libtiff | 3,138 / 3,139 |
| `flower-separated-planar-16.tif` | Big-endian uint16 CMYK, planar2, raw, 43x73 | Every word matches tifffile and native libtiff | 3,138 / 3,139 |

The [source proof](2026-11-06-proof.json) pins the original source hashes,
Pillow13.0.0/FabIO2026.6.0 code, native reference binaries, source-word hashes,
wrong and corrected displays, and exact checks for every comparison. Large
upstream inputs stay outside Git. Only small independent CC0 fixtures are added.

## Attribution

FabIO's TiffIO cannot read these layouts: its RGB path tries to reshape a single
plane as interleaved RGB; its dtype selection does not admit the CMYK four-element
BitsPerSample tuple. FabIO therefore falls back to Pillow. Its later float32
grayscale conversion matches Pillow's corrupted color image exactly, so that
conversion is not the source of these two errors.

Pillow's default raw TIFF path selects `rawmode[layer]` for each separate plane.
This keeps only a band letter and drops the `;16B`/`;16L` suffix. The resulting
8-bit band decoder consumes bytes instead of uint16 samples, including wrong
row strides. An independent model using the actual raw tile descriptors
reproduces every wrong color value and the complete FabIO grayscale array.
For example, the RGB image begins with source red words `0x58CB, 0x52FC`:
the correct 8-bit display begins `88, 82`, while the broken path begins `88, 203`.

Forcing Pillow to use its libtiff path restores exactly the correct high-byte
color samples and grayscale output. Eight controlled copies change only planar
storage or lossless compression, retaining every original uint16 word; all
produce the same corrected display and match Dioptrin's explicit display view.
Original files and installed packages are never modified by these controls.

Primary code is [Pillow's pinned TIFF path selection and raw plane setup](https://github.com/python-pillow/Pillow/blob/13.0.0/src/PIL/TiffImagePlugin.py),
[its raw unpackers](https://github.com/python-pillow/Pillow/blob/13.0.0/src/libImaging/Unpack.c),
[FabIO's TIFF fallback](https://github.com/silx-kit/fabio/blob/9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece/src/fabio/tifimage.py)
and [grayscale conversion](https://github.com/silx-kit/fabio/blob/9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece/src/fabio/utils/pilutils.py).
Installed source hashes and actual descriptors are retained in the proof rather
than relying only on source inspection or a hardcoded exception list.

## Native fixes

`Policy::Pillow13` now declines the defective **uncompressed** planar16 display
path specifically. Its previous guard also rejected compressed planar16 even
though Pillow uses libtiff and decodes it correctly. Compressed RGB, unassociated
RGBA and CMYK views now match exact pinned Pillow float32 output, with original
planar/precision/compression provenance retained. Source counts stay uint16.

The independent tile fixtures also exposed an inherited `tiff`0.11.3 bug:
`chunk_data_dimensions` passed a global chunk index to spatial edge-padding
calculation. After the first plane, bottom-edge tiles appeared on a different
tile row and lost their padding. Valid small images could panic on the output
buffer assertion. The minimal vendored fix reduces the index modulo the number
of spatial tiles per plane. It reuses the existing decoder, predictor and raster
pipeline; no new reader or fast path is introduced. The complete vendor patch
reconstructs all 21 source files exactly from the pinned registry baseline.

The 84 independent fixtures cover 48 compressed display cases, 12 raw defect
guards, and 24 multi-tile source cases. They include both byte orders,
strips/partial-edge tiles, LZW/PackBits/both Deflate tags, horizontal predictors,
RGB/RGBA/CMYK and u8/u16/u32/i16/f32/f64 source words. Original buffers remain
unchanged after display conversion and source metadata retain stored geometry.

## Classification and validation

The [full 213-test corpus run](../fabio-parity/2026-11-06-pillow-planar16.json)
has 542 complete matching read events / 2,571 frames, 11 independently checked
partial-source events, and **two qualified reference decoder defects**.
There are no unclassified mismatches or native rejections of successful
reference reads. Reference tests have no failures/errors and one existing skip;
17 expected input errors still agree with strict native rejection.

`reference_decoder_defect` preserves the actual differences and passes only if
every native source word, independent decode, exact wrong-byte model, actual
reference output and corrected controlled backend agrees. Deliberately changing
one reference pixel or the native sample hash fails qualification. Unsupported
layouts stay ordinary mismatches. These defects are **not counted as successful
count/view matches**, and the full FabIO API/parity gate remains incomplete.

All 186 Rust tests, formatting, strict all-target/all-feature Clippy,
no-default check/tests and isolated strict checks for all 20 optional features
pass. The [minimal-feature proof](2026-11-06-minimal-proof.json) repeats all 84
fixtures, original files and controls without the production JPEG/HDF5 features.
The [minimal TIFF corpus run](../fabio-parity/2026-11-06-pillow-planar16-feature-disabled.json)
also qualifies both reference defects with no unclassified mismatch. Runtime,
vendor, bridge, policy, executable and independent-reference hashes remain stable.
See [native check evidence](../fabio-parity/2026-11-06-pillow-planar16-native-checks.json).
This is local macOS correctness evidence, not expanded platform or throughput
qualification. No upstream package is patched and no external issue is submitted.

Reproduce fixtures with `scripts/generate_pillow_planar16_fixtures.py`, then build
`parity_probe` with all features and run `scripts/audit_pillow_planar16.py
--tiff-reference-library /path/to/libtiff --output /tmp/planar16-proof.json`.
The normal full bridge enables qualification with its explicit
`--tiff-reference-library` option; schema6 records independent defect evidence
without changing the reference tests or the original files.
