# Explicit TIFF directories and additional CCITT layouts

The TIFF reader now admits CCITT RLE (Compression=2) and mixed 1D/2D Group3
(Compression=3, T4Options bit0). Output remains unpacked source bits in U8,
with source BitsPerSample=1, PhotometricInterpretation, FillOrder and Orientation
retained. There is no display inversion, rotation, row substitution or decoded
cache. Float16 TIFF continues its existing documented exact F32 widening;
the shared Bool/F16 buffer additions do not alter TIFF admission or output.

The implementation reuses fax 0.2.7's original Huffman maps and its existing
Group4 two-dimensional state machine through a minimal MIT vendor adaptation.
`StrictRowDecoder` shares that state machine, adding complete-row, progress,
run-sum, width and source-bit checks. Original Group4 APIs retain their existing
compatibility behavior. This extension is not a replacement raster pipeline
and does not claim new strict Group4 footer/error-recovery qualification.
The retained original archive/file hashes, MIT LICENSE and complete patch are
in [the fax vendor receipt](../vendor/fax-0.2.7/UPSTREAM.json).

Each strip begins with a fresh reference. Mixed Group3 requires an actual
1D-tagged first row, exact EOL framing, bounded fill bits, and real source bits
for every Huffman/mode codeword. Two-dimensional rows cannot emit a partial row
or use a missing reference/end code as white counts. Last-row byte padding,
one trailing marker, and a complete six-marker RTC are distinguished from
extra coded data. The same strict framing still applies to existing 1D Group3.
RLE rows instead start at byte boundaries without EOL/RTC. Only the unused
bits within the last row byte are ignored; extra complete source bytes fail.

These choices follow the [original TIFF6 specification hosted by ITU](https://www.itu.int/itudoc/itu-t/com16/tiff-fx/docs/tiff6.pdf),
section9, page44 (RLE row bounds and ignored final bits), and section11, page52
(T4 options, per-strip first 1D row and EOL-byte alignment). TIFF6 specifies
alignment of EOL itself. [RFC2306 section3.8.3](https://www.rfc-editor.org/rfc/rfc2306.txt)
also describes a mixed-mode profile aligning EOL plus tag. Both boundaries are
accepted in mixed mode, with the same strict delimiter and tag checks;
unaligned markers fail when alignment is declared. The independent libtiff
4.7.2 writer fixtures use TIFF6's EOL-only boundary. This is explicit bounded
framing support, without retrying a failed codec or inventing padding bits.
Fetched primary-source hashes are in
[primary-sources.json](tiff-layouts/primary-sources.json).

## Catalog and explicit selection

```rust,ignore
let catalog = tiff::directories::Catalog::open(path, limits)?;
let primary = catalog.primary_offsets()?;
let directories = catalog.directories()?;
let offset = directories[0].subifd_offsets[0];
let mut reader = tiff::Reader::open_directory(path, offset, limits, options)?;
assert_eq!(reader.frames(), 1);
reader.read_frame_into(0, &mut output)?;
```

A `Directory` exposes `ifd_offset`, `next_ifd`, ordered `subifd_offsets`
(including null slots and repeated aliases), `primary_index`, typed `tags`,
`opaque_tags`, and `subifd_entry`. The last field preserves the original
SubIFDs field type/count/entry offset, complete classic/BigTIFF descriptor
bytes and any original payload offset. Metadata values keep their existing
typed representation; no scientific number coercion is introduced.
Directories are sorted by source offset; `primary_offsets()` follows the exact
original primary NextIFD chain. Catalog getters check the frozen source.
A catalog reports metadata, not pixel availability or codec support.

Default `Reader::open` still indexes only the primary chain. Explicit selection
traverses only source-declared NextIFD and tag330 SubIFD edges, then requires
the selected offset to be reachable. It exposes exactly the selected directory
as frame0, even when that directory has a NextIFD. A supported child can be
read without interpreting an unsupported primary image's pixels. Arbitrary
file offsets are rejected. Primary pages never silently include children.
Nested SubIFD trees, child NextIFD chains and explicit shared aliases are
retained; cycles, overlapping IFD tables and out-of-source metadata fail.
This follows the [libtiff graph/access distinction](https://libtiff.gitlab.io/libtiff/multi_page.html)
and [explicit TIFFSetSubDirectory offset API](https://libtiff.gitlab.io/libtiff/functions/TIFFSetDirectory.html).

The optional wrappers facade exposes
`Format::TiffDirectory { offset, options }`, including captured byte input and
existing transport/source checks. No filename or extension chooses a child.

`Limits.frames` bounds unique directories across an explicitly traversed graph;
it does not promote their count to primary frame count. The retained metadata
budget charges graph/edge/index storage and tag values. Traversal is iterative,
not recursive. Sample windows remain bounded to original declared chunks,
including when seeking between explicit directories; aliasing an adapted tag
cannot expose synthetic metadata bytes as source counts. Reused output is
invalidated on read/bounds/source failure. Source checks retain the repository's
ordinary-mutation contract; this is not protection against adversarial writes
preserving all identity metadata. Budgeted payloads are not an RSS guarantee.

## Evidence and remaining scope

[The final matched proof](tiff-layouts/2026-11-06-layout-types.json) covers eight
small independent CC0 sources: six native-libtiff encoded fax fixtures (both
fill orders, non-byte width, three strips, 2D options with/without EOL fill),
plus classic and big-endian BigTIFF primary/SubIFD graphs. Fourteen explicitly
selected directories match independent native `TIFFReadScanline` output exactly
in source dtype, shape, all sample bytes and hash. Six fax arrays also match
original pre-encoding integer arrays. Every source/runtime/vendor fingerprint
is unchanged across the proof. Fixture generator/library hashes are retained
in [fixture provenance](../tests/data/tiff-layouts/provenance.json).

Pinned unchanged FabIO2026.6.0 uses its TiffIO reader for the classic primary
chain and Pillow13 for the fax fixtures. The latter applies Orientation=6 and
WhiteIsZero bool display conversion. Those six results equal a separately
identified test-only display consumer; they differ intentionally from the
native stored-count result. No transform is applied by the reader. FabIO's
normal openimage recognition rejects the valid BigTIFF fixture, and it exposes
no public explicit SubIFD-offset selection API. Neither is classified as a
malformed source or a native count mismatch. Native libtiff remains the
independent raw-word authority for all fourteen selected directories.

Ten focused TIFF tests cover source dtypes, orientation, multistrips, both fill
orders, strict EOL/row tags/RTC/tails, RLE ignored byte bits, short declared
windows, scratch admission, nested/chain/aliased children, cycles/overlap/source
bounds, source mutation, adapted-tag aliases, unsupported primary support and
captured-input selection with stale-output invalidation. Five earlier TIFF
rejection tests still pass. Nineteen vendor library tests pass (14 original,
five new strict/compatibility tests, including all vertical offsets, pass,
horizontal, zero-ending sentinels and truncated codewords). Four independent
Python helper controls pass.

Reproduce the native/reference gate with:

```sh
CARGO_TARGET_DIR=/tmp/dioptrin-fax-strict-target cargo test --manifest-path vendor/fax-0.2.7/Cargo.toml --lib --offline
# Standalone Cargo creates vendor/fax-0.2.7/Cargo.lock; remove that generated lock before source-fingerprint checks.
cargo test --no-default-features --features wrappers --test tiff_layouts --test tiff_rejections
/tmp/dioptrin-io-fabio-suite-env/bin/python -m unittest discover -s scripts -p test_tiff_layout_reference.py
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/tiff_layout_reference.py --probe target/release/examples/api_probe --libtiff /opt/homebrew/opt/libtiff/lib/libtiff.6.dylib --output /tmp/tiff-layouts.json
```

Group3 uncompressed extensions, word-aligned RLE32771, Group3/RLE tiles and
arbitrary non-image IFD namespaces remain unsupported. This is decoder/layout
qualification, not full fax transmitter/profile conformance or a performance
claim. Existing unsupported layouts continue to fail explicitly.
