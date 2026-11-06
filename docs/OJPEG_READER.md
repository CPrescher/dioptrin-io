# Original TIFF JPEG component reader

With `tiff` and `jpeg`, original TIFF Compression6 pages use the existing bundled
libjpeg-turbo codec and expose native Y, Cb and Cr grids through
`tiff::Reader::read_ycbcr_into`. Scalar reads reject these nonuniform grids. The
component buffer identifies `Storage::JpegDecodedComponents`; decoded JPEG is
lossy, with its original precision and TIFF orientation/YCbCr tags retained.
Neither RGB conversion nor chroma expansion is part of the source read.

The bounded container adaptation follows LibTIFF4.7.2 `tif_ojpeg.c`, specifically
`OJPEGReadHeaderInfoSecTables*`, `OJPEGWriteStreamSof` and `OJPEGWriteStreamSos`.
The source file SHA256 is
`e0dcb8bf12d3b403407155ac84402158c4efb0f19d2c6e10f16329b2bf311bc4`.
Its permission notice is retained in `licenses/native-readers/LibTIFF-OJPEG.txt`.
There is no production LibTIFF/system-library dependency or Python bridge.

The initial single-chunk admission covers one complete contiguous8-bit baseline
YCbCr strip or tile, without restart declarations. These two source layouts
remain unchanged and are now complemented by the qualified fragmented profiles
below:

- A complete declared JPEGInterchangeFormat/Length stream containing the source
  chunk. Its original JPEG header through SOS remains available. The obsolete
  baseline SOS `Ss=0, Se=0, AhAl=0` is interpreted as `Se=63` by the same header
  operation as LibTIFF's OJPEG codec; original bytes and the normalization flag
  remain visible. Other unexpected scan parameters fail.
- A single entropy chunk plus original JPEGProc1, subsampling and quantization/
  Huffman table offset declarations. Quantization windows contain64 bytes;
  Huffman windows contain16 code counts and their exact declared symbol count.
  SOF/SOS/terminal EOI are assembled according to that TIFF6 container contract,
  using the full encoded tile extent. Source pixels and orientation are untouched.

`Reader::ojpeg_metadata(index)` retains named original source windows, encoded
shape, sampling, original interchange header and reconstruction policy. Existing
TIFF metadata retains all original tags, including table selectors/offsets,
reference black/white and YCbCr coefficients. Every extra table/interchange
window obeys source bounds and the TIFF metadata-overlay alias guard. Snapshots
are checked around indexing and every read; no decoded pixel cache is retained.

Nonuniform component planes include native decoder padding and separate valid
extents. Zackthecat's logical213x234 image retains its full224x240 encoded luma
plane and112x120 chroma planes. Smallliz retains160x160 luma and80x80 chroma.
All six native plane hashes match a separate LibTIFF4.7.2 decoding of the original
files exactly. Four CC0 fixtures independently exercise complete interchange,
legacy SOS, table-defined padded tiles and asymmetric4:2:2 sampling. Their
complete planes match the system LibTIFF oracle, including padding. They are
produced from explicit component arrays by a test-only system encoder, then
manually wrapped without calling the production reader or a FabIO writer.

Output planes obey the frame budget; reconstructed encoded bytes plus temporary
YUV output obey the intermediate budget. Retained headers/windows obey metadata
admission. Huffman parsing, entropy decoding and IDCT reuse existing native
codecs. Complete frames/MCUs and structural SOI/first-EOI/exact source windows are
required, and native codec warnings are errors. Libjpeg can nevertheless absorb
a small number of extra entropy bytes immediately before EOI without warning
because of bit lookahead. This mature-codec integrity limitation is not a claim
of complete semantic entropy validation. Partial frames, unsupported fragments,
contradictory geometry/sampling, unknown larger tails and bytes after EOI fail.

Qualified multi-chunk fragmented profiles and restart reconstruction are now
implemented as described below. Planar separate, nonbaseline/higher precision,
ambiguous table-only multi-across tile stacks, arbitrary intra-fragment restart
sequences and header splitting beyond the first fragment remain explicit gaps.
These matched component proofs do not claim arbitrary legacy OJPEG coverage.
Explicit consumer RGB/Pillow views retain separate conversion/rounding provenance.
No performance claim follows from this work.


## Fragmented source profiles

The continuation reuses the same table builder, JPEG header parser and native
component decoder. It assembles only declared source windows, with no source
patch writes and no alternate entropy/IDCT implementation. The supported
libtiff4.7.2 profiles are table-defined entropy strips/vertical tiles; complete
headers in the first chunk; a declared header-only interchange prefix; a header
split between that prefix and the first chunk; and complete interchange streams
whose exact entropy partition agrees with the declared fragments. Header source
bytes remain available, including obsolete baseline SOS values before the
explicit existing Se0-to63 interpretation.

Fragment rows must be MCU-aligned. The restart interval is the number of MCUs
in a full fragment. Nonzero tag515/header DRI declarations must agree with this
geometry. An absent DRI may be assembled from that bounded interpretation;
cycling RST0..RST7 markers are inserted only at declared fragment boundaries,
following libtiff's `OJPEGReadHeaderInfo`, `OJPEGWriteStreamDri` and
`OJPEGWriteStreamRst`. Internal source restarts/headers are rejected rather than
retried under a second interpretation. A terminal source EOI is retained in the
original window but excluded from its entropy contribution; absent EOI is the
same explicit old-TIFF container assembly as the initial table-defined reader.
No compressed sample bytes or unavailable rows are fabricated.

This is a qualified legacy interpretation. [TIFF Technical Note2](https://libtiff.gitlab.io/libtiff/specification/technote2.html)
documents the old design's incompatible interpretations of restart presence,
intervals and numbering. Consequently, independent libtiff agreement is evidence
for these profiles, not a declaration that every legacy TIFF6 dialect is
wellformed or supported. The [pinned upstream decoder](https://gitlab.com/libtiff/libtiff/-/blob/v4.7.2/libtiff/tif_ojpeg.c)
and retained permission notice identify the adaptation authority.

For a complete interchange stream, the reader checks more than the JPEG alone:
declared fragments must be ordered exact entropy extents, with only the expected
restart marker gaps and a final source EOI. Shifted, reordered, gapped or short
fragment windows fail even if ignoring that metadata would decode a plausible
complete JPEG. Separate entropy fragments may occur in any physical file order;
their declared TIFF order determines reconstruction. Their source windows must
be nonempty, disjoint and in the original file. Gaps and unrelated bytes outside
those windows never enter the codec.

Multi-across tiles are admitted when an explicit source SOF proves the entire
vertically stacked encoded tile frame. The source `encoded_shape` can therefore
differ from the logical TIFF raster. Table-only tile grids without that evidence
are rejected. `Fragment.origin` is the logical tile origin, while
`Fragment.stream_origin` is its position in the encoded JPEG stack; these are
separate when a tile row has multiple columns. Native decoding uses the proved
stream extent, then splits complete original component grids into logical
chunks without color conversion. Full tile padding and valid extents remain
separate, including the right/bottom edges. Strip component padding follows the
native subsampling grid, including odd widths/heights.

`ojpeg::SourceMetadata` adds `restart_interval`, `restart_markers_inserted`, and
ordered `fragments`. Each fragment retains its index, complete original source
offset/bytecount, entropy offset/bytecount, logical/stream origins, valid image
shape and encoded shape. The enum adds `TableDefinedFragments` and
`FragmentedInterchange`; `CompleteInterchange` also describes verified complete
streams with multiple declared fragments. Existing tags, source windows,
original header and SOS-normalization flag remain intact. The reader still
returns `Storage::JpegDecodedComponents`: equality with independent decoded
samples does not undo JPEG's original lossy compression.

Before source reads, fragmented source bytes are conservatively charged against
one-fifth of the intermediate budget, covering bounded header/assembly staging.
Before native output/splitting, encoded bytes plus twice the full native component
size must fit the intermediate budget; the component size itself must fit the
frame budget. This charges simultaneous global/split component buffers without
claiming an RSS limit for native codec tables, allocator bookkeeping or
caller-retained results. Fragment/window/header retention obeys the metadata
budget. Source checks and the existing adapted-metadata alias guard apply around
indexing and reads; failures invalidate every component-buffer accessor.

The fragmented continuation requires at least two declared chunks. A single
strip whose header-only interchange window lies outside that strip remains an
explicit unsupported source layout.

Fourteen small independent CC0 fixtures, their encoder/decoder binary hashes,
source-word goldens and generator receipt are retained in
[fixture provenance](../tests/data/ojpeg-fragments/provenance.json). They exercise
50 chunks/150 planes: table/header fragments, physically reversed windows,
partial and odd-width strips,4:2:2, vertical and2×2 tile grids, legacy SOS,
geometry-inferred DRI, terminal source EOI and RST7-toRST0 wraparound. All raw
planes match separate native libtiff source-word arrays exactly.

The final continuation proof is
[2026-11-06-marv2-vds-ojpeg.json](ojpeg/2026-11-06-marv2-vds-ojpeg.json).
It also revalidates the four initial independent fixtures and both unchanged
upstream OJPEG files, without replacing the historical report. Unchanged pinned
FabIO2026.6.0 reads the new profiles through Pillow; its oriented float32 display
luminance is recorded separately from native raw component grids. It is not a
source dtype/shape mismatch or proof of raw-component equality.

Five focused new Rust tests cover all source-word goldens, exact contribution
bounds, wrong restart declarations/markers, missing/truncated rows, ambiguous
geometry, scratch budgets, source changes and stale-output invalidation. The
four initial OJPEG tests and six Python comparator/authority controls also pass.
Reproduce the component proof with:

```sh
cargo test --no-default-features --features tiff,jpeg --test tiff_ojpeg --test tiff_ojpeg_fragments
/tmp/dioptrin-io-fabio-suite-env/bin/python -m unittest discover -s scripts -p test_ojpeg_fragment_reference.py
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/compare_ojpeg_fragments.py --probe target/release/examples/ojpeg_probe --reference-library /opt/homebrew/opt/libtiff/lib/libtiff.6.dylib --output /tmp/ojpeg-fragments.json
```
