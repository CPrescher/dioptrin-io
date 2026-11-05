These small synthetic source arrays, TIFF files and oracle outputs were created
independently for dioptrin-io and dedicated to CC0-1.0 by its authors. No upstream
detector counts are redistributed. The largest source is a sparse 16-bit palette
requiring the format's complete ColorMap; its sample array is only 2x3.

The generator records versions and full source/oracle hashes. `.raw` files are
little-endian original samples in HWC order; `.pillow` files are independently
computed display outputs. Bool display bytes are logical0/1, since NumPy bool
backing bytes may otherwise retain0xFF. `.palette16` is a literal ColorMap lookup;
`.tiffio` reproduces the pinned reference's explicit palette representation.

Planar16 color fixtures preserve every independent source sample and identify
the pinned Pillow layout defect. That defect is rejected by the display policy.
JPEG source oracles use libjpeg's raw YCbCr samples and permit the independently
qualified one-count IDCT difference from the stored zune backend; JPEG display
views must match Pillow exactly. Opaque descriptions retain the declared ASCII
count/type and inline0xAF byte without guessing offset175 or text.
