# Independent GE/Pixi fixtures

The tiny `ge-uint*.ge` fixtures are independently generated CC0-1.0 files. Their
explicit source arrays include native unsigned extrema and different stored
rows/frames, with every GE orientation/reorder flag set. Original binary fields,
user header bytes and pixels must survive unchanged. `provenance.json` records
source and sample hashes independently compared against pinned FabIO2026.6.0.
Run `scripts/generate_fixed_fixtures.py` in the external reference environment to
regenerate them.

Pixi's smallest legal record is about 0.5 MB. Its independent one/three-record
sources are generated directly into temporary test files rather than committed
to Git. The same manifest retains independently computed Python/FabIO source and
sample hashes, including 0, 65535, 32768 and nonuniform first/last rows. Rust tests
construct these source bytes from the explicit integer formula and verify the
entire source hash before decoding; no Python is invoked by Rust tests.

`upstream-provenance.json` records the four actual external GE corpus files and
the five exact files produced by pinned FabIO's TestPixiImage.create_fake_images.
All 20 declared frames were independently compared. Large source files remain
outside Git with their original terms. Regenerate the external manifest with
`scripts/fixed_upstream_reference.py CORPUS OUTPUT PIXI_CAPTURE_DIRECTORY`, then
run `FIXED_PARITY_MANIFEST=OUTPUT cargo test --features fixed --test fixed`.
