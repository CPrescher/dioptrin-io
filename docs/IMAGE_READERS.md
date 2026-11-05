# TIFF detector headers and image codecs

These optional readers preserve decoded source samples and stored row order.
They do not apply TIFF/EXIF orientation, masks, correction flags, normalization,
ICC profiles or detector geometry. Color output is HWC: `shape()` retains the
spatial `[height,width]` API, while metadata and buffers expose `sample_shape()`
with a trailing channel axis. Scalar `PixelBuffer` variants carry every channel.
Dependencies remain optional; there is no Python runtime bridge.

## TIFF and detector acquisition headers

The `tiff` feature reuses `tiff` 0.11.3 for TIFF/BigTIFF primary IFD pages,
strips/tiles, uniform integer/floating samples, contiguous/planar color, packed
1/2/4-bit MSB-first grayscale, Group4, PackBits, LZW and Deflate. Planar buffers
are interleaved; rows and channel values are retained. WhiteIsZero samples
bypass the backend display inversion, including floating and signed samples. Packed values are unpacked to their
native integer values, without full-range scaling. `bits_per_sample`,
`photometric_interpretation`, `planar_configuration`, typed tags, source endian,
orientation and compression remain explicit.

Float16 source samples widen exactly to F32 buffers. Source precision remains
`bits_per_sample=16`; output `pixel_type=F32` is not source dtype parity. Bit
expansion preserves signed zero, subnormals and NaN payload/signaling bits
without floating arithmetic. Consumers performing arithmetic may quiet NaNs.
Decoded byte limits apply to the wider output too.

Modern TIFF JPEG is admitted with `lossy=true`. Every strip/tile JPEG header is
checked against its TIFF dimensions, component count, photometric interpretation
and 8-bit precision before native pixel decoding. This reuses the same mature
zune-jpeg parser as the TIFF backend, including shared JPEGTables reconstruction;
source ranges and combined encoded/table bytes obey intermediate limits. Strips
use their actual last-strip dimensions; tiles retain their full encoded tile
shape, including edge padding. Contradictory or ambiguous layouts fail rather
than letting the backend discard extra components/rows. Its zune backend retains
JPEG's input colorspace rather than forcing RGB; TIFF photometric metadata
identifies these channels. Native JPEG RGB/grayscale fixture values differ by at most one
count from the independent libjpeg-turbo reference because IDCT rounding differs.
That test is explicitly a bounded lossy comparison, not exact FabIO parity.
Subsampled TIFF YCbCr currently fails in the upstream backend and remains a gap.
Old-style JPEG, CCITT RLE/Group3, mixed scalar types/bit widths, packed
color and non-MSB packed FillOrder remain unsupported.

Palette TIFF now exposes original integer indices, source precision and exact
ColorMap tag values. CFA photometric 32803 exposes its single unsigned 8/16 plane,
retaining CFA tags without demosaicing. A bounded read-only IFD adapter presents
only the photometric interpretation as gray to the mature backend; sample bytes
are decoded by the same codecs and original source tags are restored in metadata.
Primary classic/BigTIFF IFD chains, metadata sizes, cycles, overlaps and aliases
are checked before decoding. Pixel windows or out-of-line metadata referring to
adapted bytes are rejected, preventing the adapter from substituting source data.

A narrowly recognized malformed ImageDescription270 (ASCII/count1 with a
non-ASCII inline byte) is exposed through `Metadata::opaque_tags`, retaining its
original type, count, entire IFD record, literal value byte and reason. Typed
`tags` omits that entry. The adapter presents undefined bytes only internally;
it never treats0xAF as offset 175 or invents text. Structural tags remain strict.

Only primary IFDs are frames. SubIFD pointer tags remain visible, but SubIFD
pixels are not enumerated or decoded. The mature backend has no public API for
selecting an arbitrary image IFD. A separate topology/API extension is required;
this implementation does not claim SubIFD parity.

`detector_tiff::Reader::open(path, Format::{Pilatus,MarCcd}, tiff::Limits)` layers
semantic acquisition headers on the same pixel reader. Pilatus retains original
ImageDescription, all interpreted entries (including duplicates), a last-value
field map with textual units, and unparsed lines. MAR retains all 3072 bytes and
all 125 published fields as typed native integers, arrays or bytes, including
reserved bytes. Header endianness is honored independently from TIFF sample
endianness; goniostat values and correction flags remain unapplied. Source/header
snapshots and joint TIFF/semantic metadata admission are checked.

## Explicit TIFF consumer views

`tiff::view::convert(metadata, frame, Policy, max_output_bytes)` consumes an
already-read source frame without modifying it. The separate `Image` exposes
`Pixels::Scalar(PixelBuffer)` or view-only `Pixels::Bool(Vec<bool>)`, with a
`Provenance` record for source shape/type/bits/photometric/planar/orientation/
compression/lossy status, output shape/type, policy and ordered operations.
These are transformed/display results, never relabeled native detector counts.
Orientation, geometry, calibration and masks are not applied.

- `Policy::Pillow13` reproduces the pinned Pillow fallback's validated unsigned
  8/16-bit and float32 scalar representations, packed 2/4 full-range gray,
  bilevel logical bool, palette RGB8 and RGB/RGBA/CMYK float32 luminance. Source
  16-bit contiguous color first loses its low byte; CMYK uses an explicit RGB8
  conversion, and associated-alpha 8-bit uses explicit unpremultiplication before
  ignoring alpha. Pillow's dtype-dependent WhiteIsZero behavior is retained:
  byte/packed values invert for display, whereas I;16/F values stay unchanged.
  Unsupported scalar representations and associated-alpha 16-bit fail explicitly.
- `Policy::FabioTiffIo` retains source scalar/color arrays and reproduces
  TiffIO's palette behavior: maps containing values above 255 downscale to 8 bits,
  but output retains the index container dtype, including uint16.
- `Policy::PaletteRgb16` performs literal ColorMap lookup with 16-bit channels.

With the optional `jpeg` feature, `tiff::view::read_pillow13(&mut reader, index,
max_output_bytes)` reads a consumer view. Contiguous JPEG gray/RGB/YCbCr pages
render original bounded JPEG chunks through the existing libjpeg-turbo backend
before luminance conversion. This preserves Pillow's JPEG colorspace and IDCT
rounding, including edge tiles, while the raw reader continues exposing native
photometric samples through zune. Provenance records `JpegRgbDecode`. There is
no retry after a decoding failure. Encoded and rendered chunk scratch each obey
the reader intermediate limit; view output has its separate explicit limit.

Pillow13 has a proven planar 16-bit color layout defect: it alternates high and low
bytes rather than reducing each 16-bit sample correctly. The Pillow policy rejects
these cases; independent source counts match tifffile and contig/planar native
samples agree. No compatibility mode reproduces that defect. Mode1 NumPy bool
arrays can physically store true as 0xFF; view comparisons canonicalize logical
bool values to 0/1 while preserving raw source signatures separately.

The scoped `scripts/compare_tiff_views.py` / `examples/tiff_view_probe.rs` report
in `docs/tiff-views-fabio-2026.6.0.json` records raw/source and explicit view
comparisons separately. Of 55 unique files selected from the previous TIFF
report, 43 explicit views match, 2 are proven reference layout defects, 9 retain
explicit unsupported/corrupt-layout errors, and 1 generated temporary source is
unavailable. The 10 nondefect display differences are closed. Palette, CFA and
opaque-description cases additionally gain source-preserving decoding. TIFF
JPEG raw IDCT differences against an independent decoder remain qualified;
its explicit libjpeg view matches exactly. No performance claim is made.

## JPEG

Feature/module `jpeg` uses vendored libjpeg-turbo 3.1.0 through `turbojpeg` 1.5.1
for 8-bit JPEG (including predictive lossless), with `jpeg-decoder` 0.3.2 for native lossless 12/16-bit
grayscale. Original precision, SOF marker, lossy provenance, decoded colorspace
and raw pre-first-scan marker segments (Exif/ICC/JFIF/Adobe/comments/tables) are
retained. Every predictive lossless SOS is scanned through entropy marker
boundaries, retaining `lossless_point_transforms`; a nonzero transform marks
`lossy=true` because source low bits were discarded. Component IDs must occur
exactly once across lossless scans. Precision below 8 bits is rejected at open;
the lossless fallback's sub-byte buffer convention is not admitted. Intrinsic JPEG YCbCr-to-RGB decoding produces RGB channels; EXIF
orientation and ICC profiles are not applied. Native CMYK output retains the
backend's Adobe inversion convention; CMYK display conversion is not provided.
Inter-scan/trailing metadata is not indexed. A terminal EOI is required; files
with bytes after EOI are explicitly unsupported. Lossy precision above 8 bits
and lossless multichannel precision above 8 bits are unsupported.

## JPEG 2000

Feature/module `jpeg2000` uses `jpeg2k` 0.10.1 with the mature vendored BSD
OpenJPEG C backend (`openjpeg-sys` 1.0.12). The alternative Rust OpenJP2 port is
not enabled. Direct native component `i32` samples are checked and copied into
the smallest uniform U8/U16/U32 or I8/I16/I32 container; the wrapper's intensity
scaling and display conversion helpers are not used. Per-component signedness
and 1–31-bit precision remain metadata. Mixed signedness promotes the output
container when necessary; original component metadata still distinguishes it.

Admission parses bounded JP2 boxes and the main codestream SIZ/COD header before
any pixel decoder allocation. Image/tile geometry, component count, output size
and native component payload bounds are checked. Full-resolution equally sampled
components and single-image JP2/raw J2K are supported. JPX brands, multiple
codestreams, palettes/component mapping, channel-definition (`cdef`) boxes,
subsampling and precision above 31 bits are rejected. OpenJPEG applies `cdef`
associations by reordering decoded components, so these boxes fail at admission
to preserve original codestream channel order. Original non-codestream top-level boxes and main codestream header
are retained. Tile headers remain encoded source data, not semantic metadata.

A default irreversible wavelet records `lossy=Some(true)`. A default reversible
wavelet records `lossy=None`, because rate-limited quality layers can still lose
information and tile overrides are not fully interpreted. It never claims a
reversible wavelet alone proves losslessness. Strict native decoding and terminal
EOC admission reject truncation; corrupt streams are not padded to valid images.

## Resource and source contracts

Raster `Limits` bounds encoded bytes, output frame bytes, retained metadata and
admitted native component payload. These are not process RSS limits: codec
bookkeeping, entropy/progressive working storage, allocator overhead and
simultaneous encoded/output storage are additional. JPEG progressive scan count
is limited to 1000. JPEG2000 limits components to 256 and tiles to 65536.

Readers reject ordinary source mutation/replacement before/after reads using the
shared snapshot contract. They do not cache decoded counts. On every failed read,
`FrameBuffer::pixels/shape/sample_shape` become unavailable. TIFF may reuse scalar
allocation; color interleaving, packed expansion and float16 widening allocate
an output buffer. No performance claim or parallel fast path is introduced.

## Validation and provenance

`scripts/generate_image_fixtures.py` independently generates the small CC0
fixtures and source/sample hashes in `tests/data/images/provenance.json` using
NumPy, tifffile, imagecodecs and Pillow. Checked fixture classes include all cases
above plus Exif orientation, duplicate semantic fields, both MAR header endian
orders, resource limits, malformed box/geometry/precision/fill-order, truncation
and source mutation. Lossy TIFF JPEG oracle differences remain qualified above.

`examples/image_probe.rs` emits sample hashes and acquisition fields for external
oracle comparison. `scripts/compare_image_readers.py` compares the seven actual
upstream files named in `docs/image-readers-fabio-2026.6.0.json`. With pinned FabIO
2026.6.0 (`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`) all seven matched shape,
native output dtype and complete little-endian sample hashes. Both MAR files
matched all 125 fields against direct `MarccdImage._readheader` (normalizing
FabIO's lists of single-byte characters to bytes); both Pilatus semantic maps
matched exactly. Generic TIFF pixel decoding alone is not detector-header parity.
Large upstream sources remain external; only hashes/provenance are committed.

Runtime codec licenses/notices are retained in `licenses/native-readers`.
This software is based in part on the work of the Independent JPEG Group.

TIFF chunk integrity qualification: all codec reads are confined to their original
nonempty declared strip/tile source windows, with metadata prefetch discarded on
entry/exit. Uncompressed windows must cover each complete packed source row,
including horizontal tile padding and actual last-strip/bottom-tile row counts;
float16 bounds use original 16-bit storage. The native Deflate decoder requests
only the bytes needed for samples and does not drain the zlib stream to verify
its final checksum/footer. Missing or invalid Deflate footer bytes after all
samples have been produced may therefore be accepted; this reader does not claim
full Deflate integrity validation. Truncation before required samples fails,
and bytes beyond the declared chunk cannot supply samples or adapted metadata.
`tests/tiff_alias_bounds.rs` independently exercises these distinctions for both
Deflate IDs, packed rows, float16, padded edge tiles and repeated failed reads.
