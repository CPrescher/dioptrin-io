# Independent Esperanto/AGI fixtures

All `.esperanto` files here are independently generated CC0-1.0 bytes. The tiny
source arrays are intentionally rectangular/smaller than 256 to exercise exact
raw stored dimensions and avoid committing large detector files. Native reading
retains declared shapes. The reference EsperantoImage property setter pads such
shapes, so the independent reference for these tiny AGI cases is its underlying
AGI decoder followed by its explicit signed32 cast. This distinction is recorded
in the generator and the manifest's intermediate hashes/dtypes. No padded pixels
are substituted for source counts.

`scripts/generate_esperanto_fixtures.py` contains a separate simple Python bit
packer from explicit source arrays. It verifies every source image against the
pinned FabIO AGI primitive, then records original source and native signed32
sample hashes. Fixtures cover all packed widths 1..8, both escape sizes, extreme
signed32 counters, modulo32 differences/sums, first pixels, packed groups, tail
pixels, single-column rows and unknown header metadata. Uncompressed Long32
fixtures use the same source arrays. Counts, row order and shapes stay exact.

`upstream-provenance.json` separately records full pinned EsperantoImage reader
parity on both real upstream files and the four files written from those images
by the pinned writer in both encodings. All six files have standard square
256x256 or 2048x2048 shapes and match complete int32 samples, dtype and shape.
Large original/derived upstream files remain external with upstream terms.
Regenerate using `scripts/esperanto_upstream_reference.py CORPUS OUTPUT GENERATED`
and run `ESPERANTO_PARITY_MANIFEST=OUTPUT cargo test --features esperanto --test esperanto`.
