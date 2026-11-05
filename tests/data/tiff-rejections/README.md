# Independent TIFF admission fixtures

All input arrays, byte-level metadata and expected sample files in this directory
are dedicated to the public domain under CC0-1.0. No upstream detector images are
included. Recreate them with `scripts/generate_tiff_rejection_fixtures.py` in the
versioned test environment recorded in `provenance.json`; Python is not required
by the Rust reader or tests.

Group3 payloads are encoded independently by Pillow/libtiff, then checked against
`imagecodecs.ccittfax3_decode` before inclusion. Small classic TIFF IFDs assemble
those exact payloads into independent multi-strip, photometric0/1, FillOrder1/2,
and EOL-byte-aligned layouts. The long-run fixture exercises makeup codewords and
the final complete codeword at source EOF. FillOrder reversal changes only stored
bit packing; `.raw` files always retain unscaled source values 0/1.

Packed 1/2/4-bit FillOrder2 rows are assembled directly from explicit integer
arrays. Malformed ImageDescription fixtures retain every original literal byte;
those bytes never become an offset or repaired text. `.pillow` files contain
separate Pillow13 display values, with bool canonicalized per element to 0/1.
