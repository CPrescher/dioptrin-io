# Native TIFF YCbCr components

`tiff::Reader::read_ycbcr_into(index, &mut tiff::ycbcr::Buffer)` exposes original
component grids. `Metadata.sample_layout` identifies `YCbCrComponents` and its
horizontal/vertical sampling factors. `uniform_sample_shape()` returns `None`;
the existing `sample_shape()` describes the logical raster/channel shape only.
It is not a native HWC array or an allocation size for this layout. Ordinary
`read_frame_into` fails and invalidates its buffer instead of expanding chroma.

The component buffer exposes source raster shape, precision, sampling factors,
storage interpretation and source chunks. Every chunk retains its original
image origin, valid image extent and Y/Cb/Cr planes with separate stored shape,
valid shape and row/column sampling step. U8 source values and stored orientation
are preserved. Color coefficients, ReferenceBlackWhite, positioning, source bit
width, sample format and all original typed TIFF tags remain in `Metadata`.

Lossless contiguous unsigned8 TIFF YCbCr with none/LZW compression exposes
`Storage::TiffSamplingUnits`. Each chunk additionally retains the complete
losslessly decoded source unit order: `h*v` Y words in row order, then Cb and Cr.
Unit/plane edge padding is retained, including nonzero reserved words. Sampling
factors are 1/2/4 with vertical no greater than horizontal; predictor1, FillOrder1,
no extra samples, aligned strip/tile boundaries and exact chunk tables are
required. Other filters, sample widths and planar layouts fail explicitly.

The final strip may encode either the minimum complete unit rows covering the
remaining image, or the full effective nominal RowsPerStrip. The effective
nominal row count is clipped to image height, so the UINT32_MAX one-strip default
does not request enormous padding. The complete codec output must have exactly
one of those two physical shapes; arbitrary intermediate lengths are rejected.
The actual stored grids and every padded value are exposed. The reader never
fills missing samples or silently truncates complete extra unit rows.

Modern/legacy JPEG use `Storage::JpegDecodedComponents`, native decoded grids and
no lossless `sampling_units` claim. JPEG component decoding requires the optional
`jpeg` feature. TIFF-only builds can index the sampled source and explicitly
reject unavailable JPEG component decoding. Native JPEG decoding, bounded
container reconstruction and its source-lossy provenance are distinct from RGB
rendering; modern source assembly is covered by the same component contract.

## Explicit display views

`tiff::view::render_ycbcr_rgb472(metadata, &buffer, max_output_bytes)` explicitly
derives RGB8 from original TIFF sampling units (or legacy-JPEG components).
`convert_ycbcr(..., Policy::Pillow13, ...)` additionally derives pinned consumer
F32 luminance. No raw read calls either conversion. The existing optional
`read_pillow13` convenience API dispatches sampled lossless/legacy TIFF through
these conversions; modern JPEG keeps its separate libjpeg-turbo RGB view.

The TIFF sampling-unit view follows libtiff 4.7.2's
[fixed-point YCbCr color recipe](https://github.com/libsdl-org/libtiff/blob/v4.7.2/libtiff/tif_color.c),
using source coefficients and reference ranges. It replicates each sampling
unit's chroma rather than interpolating centered/cosited grids. Positioning stays
visible as metadata, and orientation is never applied. This is an explicitly
versioned reference display interpretation, not a measured full-resolution
three-channel array. Provenance carries source layout/sampling, raster shape,
original dtype/bit width/photometric/compression/orientation/lossiness, exact
per-chunk component origins, stored/valid shapes and sampling, and the
chroma expansion, RGB conversion and optional luminance operations. Source
component/padding grids remain available unchanged in the buffer.

## Source, bounds and codec contracts

The existing TIFF structural admission, chunk bounds, original source snapshot
and open-handle checks are reused. Original/staged file identity, size and
modification metadata are checked around reads; source changes invalidate every
component accessor. There is no growing-source refresh, decoded cache, layout
guess or alternate-source retry.

The modern TIFF MSB/early-change LZW codec reuses permissive weezl 0.1.12. The
adapter caps output before decoding, requires a real end marker and exact
physical output shape, and consumes input one byte at a time to prevent codec
prefetch from hiding bytes after EOI. Every declared encoded window is bounded.
Old LSB/late-change LZW has separate TIFF codec admission and is not silently
selected for this sampling-unit path.

Worst-case retained source units plus all component grids are admitted together
under `frame_bytes`. Combined encoded chunk plus bounded LZW output scratch is
admitted under `intermediate_bytes`. TIFF metadata/index and source ranges keep
their existing separate budgets. These are payload bounds, not RSS limits;
codec dictionary/allocation bookkeeping is additional. There is no parallel fast
path or throughput claim.

Legacy OJPEG sources can declare ReferenceBlackWhite as LONG[6] rather than
RATIONAL[6]. The explicit view interprets those exact integer levels, retains
the original LONG words, exposes `Colorimetry.legacy_integer_reference`, and
records `Operation::LegacyIntegerReferenceBlackWhite`. Native components are
never rescaled. An independent nondefault-level fixture proves this distinction.

The libtiff color adaptation's permissive copyright/permission notice is retained
in `licenses/native-readers/libtiff-4.7.2-LICENSE.txt`. weezl retains MIT/Apache-2.0
licensing; TIFF's existing codec notices remain applicable. Python, Pillow,
imagecodecs and independent libtiff are test-only reference tools.

## Exact actual-source proof and fixtures

[2026-11-06-proof.json](2026-11-06-proof.json) records every native/reference
chunk and plane signature, source/probe/helper/native-library hashes and source
stability. `ycbcr-cat.tif` is the unchanged 72,766-byte actual upstream file,
SHA-256 `264d4955898a35ae70af0ec6eed0dbc49d7929f6c18c92a4ed6198f6db396f4b`,
from the external `libtiffpic.tar.gz` corpus. It contains 33 LZW strips for a
250×325 logical image with 2×2 sampling and RowsPerStrip10. All 33 complete
source-unit arrays and 99 component planes match independent full-stream
imagecodecs output. There are 82,500 stored Y words and 20,625 words in each
chroma grid, including padding. The last strip's Y grid is `[10,250]` with valid
extent `[5,250]`; its chroma grids are `[5,125]` with valid `[3,125]`.

The full final LZW stream contains 3,750 decoded bytes, including 1,500 extra
physical padding bytes beyond the minimum 2,250. `TIFFReadEncodedStrip` clips
these to logical rows, so the independent lossless proof decodes raw LZW bytes
through imagecodecs rather than treating that clipped output as complete source
storage. The actual extra words are zero; an independent fixture also verifies
nonzero full-strip padding. All RGB8 and F32 luminance bytes match Pillow 13.0.0
with libtiff 4.7.2 exactly. This display equality is separate from raw source
component equality.

[Modern JPEG proof](2026-11-06-modern-jpeg-proof.json) verifies all 24 original
`quad-jpeg.tif` chunks and 72 native planes, plus its full explicit F32 Pillow
view. JPEG-derived components do not claim lossless source sampling units.
The independent modern oracle uses imagecodecs with explicit YCbCr input/output,
no fancy upsampling and an aligned physical-width/height gate; exact nearest
replication is decimated to native chroma grids. This is necessary because, on
this actual source, the current libtiff RAW-strip path supplies zero tails absent
from complete independent JPEG decoding (also reproduced in a separate C
program). Those zeros are not emulated. Other unproven odd sampled reference
layouts remain explicit oracle gaps; the native reader still retains its own
physical component grids under bounds.

[Zack](2026-11-06-zack-view-proof.json) and
[Liz](2026-11-06-liz-view-proof.json) each additionally verify all three actual
OJPEG planes against independent libtiff, and exact RGB8/F32 views against
Pillow. Their legacy integer ReferenceBlackWhite interpretation is visible in
provenance. Small independent modern JPEG strip/tile fixtures retain full
edge-tile planes with imagecodecs signatures. A tight cumulative budget test
rejects four padded tiles whose individual allocations fit but whose total does
not; admission occurs before decoding the next chunk.

Reproduce without writing the external corpus:

```sh
cargo build --no-default-features --features tiff --example ycbcr_probe
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/audit_tiff_ycbcr.py \
  --reference-helper scripts/tiff_component_reference.py \
  --reference-library /opt/homebrew/lib/libtiff.6.dylib
```

Large upstream files remain outside Git. `tests/tiff_ycbcr.rs` generates small
independent CC0-1.0 fixture bytes without an upstream writer. A literal 9-bit
clear-per-symbol LZW generator is independent of the mature decoder. Tests cover
odd shapes, strip and tile edges, reserved/extreme values, each plane and source
unit, nonzero final-strip padding, known neutral-chroma RGB counts, unchanged
orientation, explicit view limits, malformed layouts, codec truncation/extra
output/hidden tails, source changes and reused-buffer invalidation.
