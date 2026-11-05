# Independent legacy LZW fixtures

All arrays, the small independent test encoder, structural TIFF bytes and their
expected samples are dedicated to the public domain under CC0-1.0. No upstream
image is redistributed here. Recreate the files with
`scripts/generate_tiff_legacy_lzw_fixtures.py`; versions and file/sample hashes are
recorded in `provenance.json`. Python is not needed by the Rust library or tests.

The test-only Python encoder emits dictionary codewords independently of weezl.
A separate packer writes LSB-first codes with the legacy late width switch.
Every valid legacy stream is independently decoded by imagecodecs before being
included. Complete arrays are then independently checked with tifffile/imagecodecs
for strip/tile placement, planar channels, byte order and predictors.

Fixtures exercise all four code widths 9..12, dictionary clears after growth,
KwKwK codewords, packed 2 source precision, integer horizontal and floating byte
predictors, big-endian 16, planar RGB, multi-strip last rows, and complete edge
tiles with nonzero stored padding. `invalid-lsb-early.tif` intentionally uses the
wrong early width switch under an LSB initial-clear signature and must fail.

`.raw` files contain all original logical samples in little-endian representation,
with color channels interleaved. Source orientation 6 on the planar/contiguous RGB
strip fixtures remains unapplied. Their Pillow `.pillow` views have Pillow's
orientation transform and are therefore not stored-orientation raw oracles.
The mixed-dialect fixture has explicit per-chunk recognition and exact independent
source counts; Pillow/libtiff cannot switch its decoder between these strips, so
its failure is recorded rather than used as a fallback or fabricated reference.
