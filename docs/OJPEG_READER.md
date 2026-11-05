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

Admission currently covers one complete contiguous 8-bit baseline YCbCr strip
or tile, without restart declarations. Two explicit source layouts are admitted:

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

Multi-chunk fragmented legacy streams, restart reconstruction, planar separate,
nonbaseline or higher-precision OJPEG remain explicit gaps. Supporting these two
original files does not claim arbitrary legacy OJPEG coverage. Explicit consumer
RGB/Pillow views have their own conversion and rounding provenance. There is no
performance claim from these component/count proofs.
