# Optional image codec notices

This software is based in part on the work of the Independent JPEG Group.

The `jpeg` feature statically builds libjpeg-turbo 3.1.0 through
`turbojpeg-sys` 1.2.0 (CMake; no runtime Python). Its IJG and BSD notices are
retained in the adjacent README.ijg and LICENSE.md files. The source library
also includes zlib-licensed SIMD code covered by the license roll-up.
The Rust wrapper `turbojpeg` 1.5.1 is used under MIT. `jpeg-decoder` 0.3.2 is
MIT/Apache-2.0 and handles source lossless 12/16-bit JPEG.

The `jpeg2000` feature uses MIT `jpeg2k` 0.10.1 and BSD-2-Clause OpenJPEG
vendored by `openjpeg-sys` 1.0.12, built with the C compiler. It does not enable
the alternative OpenJP2 Rust port. OpenJPEG's license notice is retained.

The `tiff` feature adds MIT `fax` 0.2.7 and `zune-jpeg` 0.5.15 / `zune-core`
0.5.3 (MIT/Apache-2.0/Zlib). Their notices are retained alongside TIFF's
existing MIT notice. Group3 TIFF directly reuses the same optional `fax` 0.2.7
public Huffman tables/codewords and the strict row entry point adapted from its
existing Group4 state machine; the minimal MIT source copy, baseline hashes and
complete local patch are under `vendor/fax-0.2.7`; its exact MIT notice is retained as
`fax-0.2.7-LICENSE`. None of these optional runtime codecs requires GPL/LGPL.
These notices must accompany redistributed binaries as required by their
respective licenses. Codec build tools are not runtime file readers.

The optional TIFF consumer view follows Pillow 13.0.0's display conversion
semantics (8-bit CMYK, luminance and associated-alpha handling). Its HPND
license/copyright notice is retained as Pillow-13.0.0-LICENSE.txt. These
explicit consumer operations never run as part of stored-count decoding.

The optional TIFF backend is a narrowly patched MIT `tiff` 0.11.3 source copy
under `vendor/tiff-0.11.3`, with its original LICENSE, baseline file hashes and
complete local codec patch retained. The separate source-dependency manifest
patch binds the sibling MIT fax backend for source/Git consumers; all upstream
baseline hashes and notices are unchanged. Legacy LSB/late-change TIFF LZW reuses the
existing MIT/Apache-2.0 `weezl` 0.1.12 decoder; both exact notices are retained as
`weezl-0.1.12-LICENSE-MIT` and `weezl-0.1.12-LICENSE-APACHE`.

The optional TIFF feature compiles the unchanged permissive libtiff 4.7.2
`tif_thunder.c` row codec with a bounded local shim. Its original copyright,
license and exact source hash are retained under `native/tiff-thunderscan`.
The TIFF parser/layout remains the Rust backend; no full native TIFF parser or
automatic metadata repair is linked by this shim.

Fragmented original-JPEG TIFF profiles additionally follow libtiff 4.7.2
`OJPEGReadHeaderInfo`, `OJPEGWriteStreamDri` and `OJPEGWriteStreamRst` container
operations. The existing full `LibTIFF-OJPEG.txt` permission notice applies.
Entropy decoding and IDCT still use the existing optional native JPEG codec;
no production libtiff dependency is introduced.
