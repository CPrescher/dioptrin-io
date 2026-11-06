# Explicit TIFF metadata interpretations and available rows

The scoped [six-source audit](rejection-audits/2026-11-06-tiff-recovery.json)
closes eleven original rejection events with explicit complete-source layouts.
One original event exposes a partial source with exact available counts. Keep
this audit's six changed records alongside the earlier seven TIFF source records;
it does not replace the previous Group3, opaque-description and codec proofs.

`Reader::open` keeps declared chunk admission. The separate constructor
`Reader::open_with_options(path, limits, options)` accepts:

```rust
Options {
    uncompressed_windows: UncompressedWindows::FromShape,
    tile_tables: TileTables::FromStripTags,
}
```

Both options default to `Declared`. `FromShape` replaces missing or too-short
uncompressed bytecounts with exact per-row source byte footprints, including
packing, planes and actual last-strip rows. Every computed window must fit the
physical source and avoid other sample windows and all original header/IFD/tag
payload ranges. Counts are never read from the metadata adapter's virtual bytes.
The unchanged backend decodes source dtype, precision, predictor and stored rows.

This makes all three Feb09 wrapper forms complete: the source count field is
1,085,764 samples while 16-bit storage requires 2,171,528 bytes at offset 186.
The policy also admits `a0009.tif`'s complete 2,097,152-byte U16 payload at offset 174
without inventing a missing original StripByteCounts entry. These original
malformed metadata conditions remain visible; strict opening still rejects them.

`FromStripTags` uses original strip offset/count tables as tiles only when the
original tile dimensions uniquely match the table length and strip geometry
cannot. `quad-tile.tif` declares twelve entries for its twelve 128x128 tiles,
whereas its strip geometry would require three. Original offsets/counts stay
unchanged. Complete decoded source tiles, including padding, must match the
expected count and an explicit LZW end code. The independent mature decoder and
separate tile-placement oracle match all 589,824 RGB source samples; the existing
explicit Pillow view also matches pinned FabIO.

`Metadata.compatibility` retains the source IFD offset and every original entry's
type, count, complete descriptor bytes and external payload offset, plus the
chosen operations and effective windows. `Metadata.tags` stays the original tag
map, including original short values and absence of missing tags. The vendored
TIFF constructor's explicit chunk override changes its layout state, never its
Directory or the source file. No failed-decode retry chooses another layout.

Recovered modern LZW alone recognizes the pinned imagecodecs 2026.10.10 writer's
byte-aligned EOI zero flush. Its maximum eight all-zero tail bits cannot encode
another minimum nine-bit code. EOI and exact decoded count remain mandatory;
longer or nonzero tails fail. Original legacy LSB padding and ordinary modern
streaming behavior stay unchanged. The audit pins the writer source and records
the independent positive/negative fixtures.

ThunderScan reuses unchanged permissive libtiff 4.7.2 row code through a bounded
row-only shim. The default dense read requires every declared row and complete
codec integrity. The explicit API
`read_thunderscan_available_into(index, &mut thunderscan::AvailableFrame)` exposes
only concatenated validated complete rows, their source row spans, missing row
spans, and exact interpreted strip bytes/status. Failed native row buffers,
including native zero fill, never enter this output. All getters become
inaccessible after fatal source/index/resource failures.

The unchanged `text.tif` declares 359 rows of 4-bit grayscale. Independent native
`TIFFReadScanline` and the production adapter each validate exactly 356 rows:
538,272 U8 values retain original 0..15 samples with source precision 4. The encoded
last strip ends exactly at the second IFD, leaving no encoded data for the final
three declared rows. Incorrect declared height versus missing encoded rows cannot
be distinguished from this source. Neither interpretation licenses fabricated
counts or substitution from its different 1-bit image.

The second IFD remains fully readable: all 542,808 unpacked 0/1 source samples over
359x1512 match the independent native oracle. The [import-safe reference helper](../scripts/thunderscan_reference.py)
requires the exact original source hash and an explicitly selected native libtiff
binary, recording its hash/version. It checks original strip bytecounts, native
row results and independent token-only byte boundaries; no display inversion or
partially initialized native row becomes a reference count. Root integration
records `partial_source_read` separately from full parity.

Independent CC0 fixtures cover malformed counts, BigTIFF, big-endian float bits,
packed precision, unique versus ambiguous tile geometry, full source padding,
EOI/tails, every ThunderScan byte class, delta/run/raw behavior, partial-row
discard, discontiguous availability, a complete second IFD, source mutation and
resource bounds. Strict TIFF-only and all-feature tests and Clippy pass; builds
without TIFF do not invoke the new C compiler step. Runtime source and license
provenance are frozen under `native/tiff-thunderscan` and `vendor/tiff-0.11.3`.
