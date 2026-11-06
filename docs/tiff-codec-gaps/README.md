# Four TIFF codec and layout gaps, 2026-11-06

All four targeted source files are now readable with independently verified
native samples and separate exact reference display views. The [full FabIO run](../fabio-parity/2026-11-06-tiff-codecs.json)
reduces native rejections 44→40 and increases matching read events 509→513.
The prior two Pillow planar16 defects remain explicit mismatches. Full reading
parity is still incomplete.

| Original source | Native contract | Independent verification |
|---|---|---|
| `quad-lzw.tif` | Original U8 RGB through explicitly recognized old LSB/late-change LZW | All 589,824 samples / 77 strips and the F32 display match |
| `ycbcr-cat.tif` | Original lossless units, native Y/Cb/Cr planes, full stored strip padding | All 33 chunks / 99 planes plus exact RGB and F32 views |
| `zackthecat.tif` | Original JPEG tables + one declared entropy tile, native decoded component grids | All 3 full encoded-tile planes plus exact RGB/F32 views |
| `smallliz.tif` | Complete declared interchange stream, original SOS/header/window provenance | All 3 planes plus exact RGB/F32 views |

YCbCr source grids are not RGB/HWC arrays. `read_ycbcr_into` retains units or
explicitly labelled decoded JPEG components, physical shapes, valid extents,
sampling and padding. `uniform_sample_shape()` is None; scalar reads reject
implicit expansion and failed outputs are inaccessible. Logical `sample_shape()`
is not a component allocation size. Explicit versioned views carry every original
component geometry and record color/chroma and legacy reference-level operations.
Original tag types, source dtype/precision, orientation and reserved values stay
unchanged. Both gzip/bzip2 wrappers preserve the same contract and validate their
original sources around each read.

The ordinary modern JPEG source path is corrected to the same truthful component
contract. Its already-matching `quad-jpeg.tif` display stays exact, and all 72
native component planes are independently verified. No default chroma expansion
is presented as original samples.

## Reused implementations and bounds

Legacy LZW reuses [vendored TIFF/weezl](../TIFF_LEGACY_LZW.md), with a minimal
private codec patch and complete baseline archive/file hashes. The unchanged
mature raster, predictor, sample conversion and placement code handles the data.
Dialect selection uses the original first clear-code signature before decoding;
there is no failed-decoder retry. Exact full chunk counts, complete EOI, bit tails,
physical tile padding and encoded/decoded scratch are checked.

[Original JPEG](../OJPEG_READER.md) reconstructs only documented table/interchange
layouts and feeds the existing bundled libjpeg-turbo codec. Original source
windows/header/SOS interpretation remain available; no production system libtiff
or Python dependency is added. Full frames and structural EOI/windows are required;
the mature codec's small pre-EOI entropy-lookahead tolerance is explicitly qualified.
Fragmented/multichunk OJPEG and other unsupported variants remain gaps.

[Lossless YCbCr](../tiff-ycbcr/README.md) reads all encoded source units rather than
truncating them to logical image rows. In the actual last strip, 3750 decoded bytes
are stored, while an ordinary libtiff strip read returns only 2250. All source
padding remains in the native component buffer. Multi-chunk JPEG plane totals
are admitted under the frame budget before allocating the next decode.

## Independent reference qualification

The test-only component oracle records libtiff 4.7.2 plus imagecodecs 2026.10.10
codec versions, library/extension hashes and exact source signatures. Lossless LZW
uses the complete original compressed payload to avoid libtiff's padding clip.
OJPEG uses native libtiff RAW sampling units. Modern aligned JPEG uses explicit
YCbCr/no-fancy output and inverse nearest-grid decimation: an independently
reproduced libtiff RAW bug returns partially zero-filled modern chroma despite
reporting a complete byte count. Those invented zeros are not copied. Odd grids
outside this independent reference contract are marked unqualified, not inferred.

The full bridge compares every unit/plane before checking the selected view;
`component_view_match` is separate from uniform source/count and other view
statuses. Vendored files, Cargo inputs, Rust sources, bridge, executable and
reference libraries remain unchanged during the full and minimal-feature runs.

## Validation and evidence

All 156 Rust tests pass, plus formatting, strict all-target/all-feature Clippy,
no-default check/tests and strict isolated checks for all 20 optional features
(25 required/feature commands). Independent fixtures exercise dictionary growth,
clears/KwKwK, predictors/tiles/endianness/float bits, odd grids, full nonzero source
padding, multiple chunks, total output budgets, missing codec features, bad
windows/streams, mutation and stale-output invalidation.

The full pinned 213 reference tests have no failures/errors and one upstream skip.
The separate [minimal-feature run](../fabio-parity/2026-11-06-tiff-codecs-feature-disabled.json)
runs 22 tests without failures/errors: six JPEG-dependent events are correctly
feature-disabled, lossless native components still compare, and known rejected
sources/Pillow defects remain explicit. These are local correctness checks;
expanded platform CI, publication and new throughput claims are not established.

Source-level evidence remains in:

- [Legacy LZW count/view proof](../tiff-legacy-lzw-2026-11-06.json).
- [OJPEG component proof](../ojpeg/2026-11-06-components.json).
- [YCbCr full-unit/view proof](../tiff-ycbcr/2026-11-06-proof.json).
- [Modern JPEG component/view proof](../tiff-ycbcr/2026-11-06-modern-jpeg-proof.json).
- [Zack view proof](../tiff-ycbcr/2026-11-06-zack-view-proof.json) and [Liz view proof](../tiff-ycbcr/2026-11-06-liz-view-proof.json).
- [All 91 original outcomes reverified](../rejection-audits/2026-11-06-after-tiff-codecs.json), with original source conditions and exact/recreated identity distinctions retained.
- [Native/check evidence](../fabio-parity/2026-11-06-tiff-codecs-native-checks.json).

Of the 40 current rejections, 36 are inconsistent/truncated/malformed inputs;
two are Lambda VDS/source contracts, one is the mixed strip/tile legacy dialect,
and one is ThunderScan with unqualified complete-source integrity. No case is
removed from the original ledger.
