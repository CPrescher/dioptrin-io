# Local TIFF 0.11.3 codec patch

This is the MIT image-rs `tiff` 0.11.3 crate from the exact registry archive and
upstream commit recorded in `UPSTREAM.json`. That file records hashes of every
copied baseline file; `dioptrin-legacy-lzw.patch` records the complete code delta.
The upstream LICENSE is retained without changes. No LZW dictionary decoder,
predictor, endian conversion, sample conversion or raster pipeline is replaced.

The private LZW reader checks each original chunk's first nine-bit clear code.
The disjoint byte prefix `00` followed by a byte with low bit 1 selects the legacy
LSB-first/late-change dialect also recognized by libtiff's LZWPreDecode. This is
source recognition before decoding, never a retry after modern decode failure.
Modern TIFF LZW retains its upstream MSB-first/early-change streaming decoder.

Recognized legacy chunks are decoded with weezl 0.1.12's mature LSB decoder. A
bounded code-boundary walk locates EOI and validates only zero padding through the
last byte; it does not decode dictionary entries or pixels. This is necessary
because weezl reports byte consumption including private bit lookahead. The exact
source byte count must match full TIFF chunk rows, including all tile padding;
only complete source bytes then enter upstream raster/predictor handling.

The legacy path charges encoded bytes + decoded bytes + one overflow-detection
byte + the existing bounded input buffer against `intermediate_buffer_size`.
Fixed codec dictionaries, allocator bookkeeping and caller-retained frame buffers
are additional memory, as with upstream codecs. All compressed source reads stay
inside the original declared bytecount, including when a missing end code is
physically followed by more bytes elsewhere in the source. This patch introduces
no decoded cache and no alternate source path.

The root crate's optional path dependency enables this patch in source/git
checkouts. A future crates.io publication needs a separately published patched
backend or another packaging design: Cargo removes a dependency's local path
when publishing and upstream 0.11.3 does not contain this patch. No publication is
performed as part of this change.

Explicit caller-supplied chunk layouts now support bounded legacy metadata
interpretations while retaining the original Directory and tag APIs. The
default constructor supplies no override. Recovered modern LZW chunks use the
same exact-count/end-code preflight; the pinned imagecodecs 2026.10.10 writer
adds a zero flush byte when EOI is already byte aligned. This explicit path
accepts at most eight validated zero tail bits, insufficient for another
nine-bit code. Legacy LSB padding and ordinary modern streaming stay unchanged.
