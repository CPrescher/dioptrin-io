All synthetic arrays, detector headers, compressed files and expected decoded
samples in this directory were independently generated for dioptrin-io and are
dedicated to the public domain under CC0-1.0 by the dioptrin-io authors.
No upstream detector counts are embedded in these fixtures.

`scripts/generate_image_fixtures.py` records generator versions and complete
source/sample hashes in `provenance.json`. `.samples` files are little-endian,
channel-last expected native output, independently decoded with Pillow/libjpeg
or imagecodecs where compression is lossy. The float16 fixture has explicit F32
output bits, retaining source half-precision NaN payload/signaling bits.

TIFF JPEG uses a different IDCT from the independent oracle; the Rust test
explicitly permits one-count differences only for those two lossy fixtures.
Exact parity claims exclude that comparison. Palette indices and floating WhiteIsZero now decode as source samples; SubIFDs are retained as pointer tags
but their pixels are not enumerated.
