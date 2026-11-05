# Independent MAR345 fixtures

All small `.mar345` files here are independently generated CC0-1.0 source bytes.
`scripts/generate_mar345_fixtures.py` contains explicit uint32 arrays, a separate
Python predictor/bit packer and header/overflow framing. `provenance.json`
records every complete source hash, native uint32 sample hash, dimensions,
endianness, high-value count and independent FabIO comparison outcome.

Cases cover every PCK V1 field width (0/4/5/6/7/8/16/32), block powers 1..128,
negative predictor sums and division, signed16 wrapping, unsigned first-row
values, 32-bit delta fields, leading binary whitespace, both endian headers and
overflow words, padded/compact tables, documented header FORMAT aliases and
original reserved header bytes. Stored rows differ and remain untransformed.

The reference is pinned FabIO 2026.6.0, commit
`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`. Full file-reader samples match for
all cases marked `fabio_file_equal: true`. The manifest separately records
source-correct differences: first-row sign extension, observed 32-bit decoder
mask overflow and trimming of valid binary whitespace. The whitespace case is
not passed to the reference's unsafe native decoder. Native expected counts
come from the independent original arrays, not a repaired reference output.
Big-endian oracle reads explicitly set the reference's byteorder because its
header parser assigns a differently spelled attribute. This adjustment only
selects the original source order; it never changes expected samples.

`upstream-provenance.json` records both real downloaded corpus images and four
files rewritten by the pinned writer in little/big endian. All six native U32
arrays match complete reference samples and shapes. The real MAR555 image has
four compact overflow records and shape 3072x2560; this is distinct from the
manufacturer's padded MAR345 table convention. Large original/derived files
remain outside Git and retain upstream terms. Regenerate the manifest with
`scripts/mar345_upstream_reference.py CORPUS OUTPUT GENERATED_DIRECTORY`, then
run `MAR345_PARITY_MANIFEST=OUTPUT cargo test --features mar345 --test mar345`.
