# Independent TIFF metadata and availability fixtures

All arrays, byte-level TIFF metadata and independently specified ThunderScan
codes are dedicated to the public domain under CC0-1.0. No upstream detector
image is included. Recreate with `scripts/generate_tiff_recovery_fixtures.py`;
versions and hashes are recorded in `provenance.json`. Python is not a runtime
reader dependency.

Uncompressed fixtures start with independent arrays, then deliberately shorten
or remove bytecount metadata. BigTIFF, big-endian float IEEE bits, packed source
precision, and multiple strips retain their original descriptors. The mixed-tag
fixture uses independently encoded modern LZW tiles; source strip tags are not
rewritten by the Rust reader. Strict admission still rejects these fixtures.

The LZW tile writer is imagecodecs 2026.10.10 via tifffile 2026.10.15. Its aligned
EOI flush writes one zero byte, covered by a positive test; additional or nonzero
tails fail under the explicit recovered layout. Source samples are independently
specified and exact full source tile counts are required, including padding.

ThunderScan uses independently specified raw/run/delta/skip codewords, including
nibble wrapping. Partial-row samples are discarded, with discontiguous complete
rows mapped to the original declared shape. A second complete IFD remains usable.
Run overflow, coded tails, no-op tails and source mutation exercise native bounds
and failed-buffer visibility. `.raw` files contain available samples only for
partial cases; their provenance separately records the declared source shape.
