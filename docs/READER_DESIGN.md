# Native reader contracts and implementation choices

`dioptrin-io` owns file interpretation and decompression. Numerical kernels stay
in consumers. A `PixelBuffer` variant owns exactly typed native samples; no
implicit full-frame float64 conversion occurs. Width/height and original storage
type remain explicit. Zeros, large integers, reserved values, signed values,
nonfinite floats and negative zero are retained when admitted by a backend.
Stored row order is never flipped. Header orientation is metadata, not an action.

## HDF5 general reader and raw snapshots

The original `hdf5::Reader` remains available for its validated 3D u16/u32 scope
and bundled HDF5 filter path. `read_frame` retains its exact u32 compatibility
output. The new `hdf5::raw::RawReader::open_snapshot` is opt-in, not an automatic
replacement. It indexes one allocated full-frame chunk per frame once, then
uses Unix/Windows positional reads, safe Rust LZ4 block decompression and the
already bundled native Bitshuffle unshuffle function outside the HDF5 mutex.
Only admitted decompressed slices reach native bit-unshuffle; native unbounded
compressed-input decoding is never called by this path.

Raw snapshots support unfiltered chunks and a sole Bitshuffle/LZ4 filter,
including explicitly skipped chunks. Both zero/default and explicit block-size
headers are validated. Encoded chunks, decoded frames, filter masks, dimensions,
addresses and exact output lengths are checked. Unsupported filters, partial or
multiframe chunks, missing/unallocated chunks, virtual datasets, external
storage/links and nonzero userblocks are rejected. Use the general reader
explicitly when its contracts fit; there is no retry that silently changes
source or decoding interpretation. Other HDF5 integer/float types and broader
layout coverage remain work items in the format inventory.

A raw reader freezes dimensions and addresses. The caller asserts acquisition
has stopped, even when the HDF5 dataset is resizable. Path/open-handle identity,
size and modification metadata are checked before and after indexing and every
read. Reopen after changes. These checks detect ordinary mutation/replacement,
not adversarial writes preserving all metadata. Live/SWMR refresh, VDS source
validation and master-file source manifests are not implemented. No decoded
counts are cached by the raw reader.

`FrameBuffer` is worker-owned and reuses its native, compressed and block scratch
allocations. Failed reads invalidate its public pixels/shape. `for_each_frame`
uses at most 256 explicitly budgeted workers and one frame per worker. Callback
order is unspecified; the frame index travels with each callback. Borrowed
pixels expire when the callback returns. Errors stop unstarted work, join every
worker and return the error; already-running consumers can finish. There is no
unbounded decoded queue. `memory_bytes` bounds conservative decoder payload,
including native unshuffle temporary space; index storage, allocator/library
bookkeeping, thread stacks and caller-retained results are additional memory.
It is not an RSS limit.

## TIFF/BigTIFF

The MIT `tiff` crate is reused instead of maintaining another TIFF parser. The
adapter admits ten native scalar types, single-channel BlackIsZero, strip/tile
storage, primary IFD stacks and lossless none/PackBits/LZW/Deflate compression.
Header tags (including supported private tag values), compression, source byte
order and Orientation are retained. The decoder performs lossless predictor
reversal and endian conversion, never display orientation changes.

WhiteIsZero is rejected because the backend otherwise inverts counts. Packed
sample widths, RGB/palette, JPEG/WebP, float16 and unsupported compressions are
also rejected. MAR/Pilatus-specific semantic header parsing is not established
by a generic TIFF success. SubIFDs are not traversed. The upstream decoder
initially parses the first IFD under its own default per-tag bounds; caller
limits apply immediately afterward. Total admitted metadata, primary page count,
decoded bytes and intermediate decoder scratch have separate bounds. Unknown
TIFF field *types* skipped by the backend are not represented in the tag map.

## CBF and EDF

CBF retains the complete source CIF/Pilatus text header and parsed binary MIME
fields. It admits integer byte-offset images, validates Content-MD5 when present,
checks dimension/element counts and rejects missing/unknown fields or binary
conversions. It does not normalize malformed fields to defaults.

Byte-offset has incompatible escape dialects. The default `Extended64` follows
8/16/32/64-bit escapes and checks counts against their declared type. Callers
must explicitly select `ByteOffsetMode::Legacy32` for FabIO/Pilatus signed32
streams, where INT32_MIN is a terminal 32-bit difference and accumulation uses
32-bit wrapping. Failed reads never try another dialect. The small independent
FabIO fixtures exercise this distinction. Supporting all dialects does not
establish universal CBF/CIF header or multiple-array support.

EDF supports inline 2D frames (including concatenated frames with differing
metadata), all ten scalar types, both byte orders and bounded internal
none/gzip/zlib/bzip2. Original padded header bytes and parsed fields are retained.
External binary sources, global/general headers, volumes, float128, optional
byte-offset EDF and outer compressed-file wrappers remain explicit gaps.
Neither reader pads truncated pixels with zeros or substitutes neighboring files.

## Library evaluation and licensing

- `hdf5-metno` 0.15.1 and `hdf5-bitshuffle` 1.0.0: reuse the extracted reader,
  static HDF5/codecs and original MIT/native notices. HDF5 remains the metadata
  authority and general decoder; new raw code does not reimplement HDF5 indexes.
- `tiff` 0.11.3 (MIT): mature TIFF/BigTIFF, native typed buffers and lossless
  codecs, Rust 1.85 baseline. Explicit admission compensates for display-oriented
  inversions and planar/color behavior inappropriate for raw detector counts.
- `lz4_flex` (MIT): bounded Rust LZ4 blocks; only safe decoder features enabled.
  Bitshuffle is reused from the bundled native codec, rather than a second
  unvalidated bit-plane implementation.
- `flate2` (MIT/Apache-2.0, Rust backend) and `bzip2` 0.6.1 / `libbz2-rs-sys`
  0.2.6 (MIT/Apache-2.0 and retained bzip2 notices): independent native EDF
  decompression, without Python or subprocesses.
- `cryiorust` 0.6.0 was evaluated via its published package metadata. Its license
  is GPL-3.0+, so it is not incorporated into this MIT crate. No source was
  copied from it. CBF/EDF use independently written bounded parsers with
  independent test fixtures instead.

Cargo lockfiles pin exact build dependency resolution. New reader/codec notices
are under `licenses/native-readers`; original attribution remains unchanged.
FabIO, h5py, NumPy, tifffile and imagecodecs are test/reference tools only.
