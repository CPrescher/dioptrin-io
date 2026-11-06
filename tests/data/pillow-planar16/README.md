These independently generated arrays, encoded TIFFs and oracle bytes are
dedicated to CC0-1.0 by the dioptrin-io authors. No upstream detector images or
samples are redistributed. The generator is scripts/generate_pillow_planar16_fixtures.py;
versions and complete hashes are in the two provenance manifests.

The 60 display fixtures cover RGB, unassociated RGBA and CMYK, both byte orders,
strips and partial-edge tiles, raw, LZW, PackBits and both Deflate tag values.
Predictor2 is exercised with LZW/Deflate. All source words are independently
known uint16 values with different high and low bytes. `.raw` is canonical
little-endian HWC source data; `.pillow` is Pillow13's actual float32 grayscale
output. Twelve raw planar fixtures deliberately demonstrate its bad 8-bit band
unpacking; those outputs are defects to refuse, not successful display oracles.
The other 48 use the correct default Pillow/libtiff path.

The 24 additional multi-tile fixtures use a 19x21 raster, 16x16 tiles, RGB/RGBA,
both byte orders, and u8/u16/u32/i16/f32/f64 samples. They independently check
that each plane repeats the same spatial grid and partial-edge padding. Their
`.raw` data contain original words only; no display conversion is exercised.
