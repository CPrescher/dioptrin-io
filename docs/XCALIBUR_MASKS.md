# Xcalibur mask and CCD characteristics APIs

The optional `xcalibur` feature provides native CCD characteristics parsing,
caller-shaped mask reconstruction and binary-mask decomposition. These are
separate from source detector counts. FabIO's `XcaliburImage.read` fabricates a
50×60 zero array; this crate does not expose that placeholder as measured data.
The binary schema's MIT attribution is retained under
`licenses/native-readers/FabIO-Xcalibur-MIT.txt`.

```rust,ignore
use dioptrin_io::xcalibur::{Reader, Limits, Projection, ReadPolicy};
let ccd = Reader::open_with_policy(
    "scan0001.ccd", Limits::default(),
    ReadPolicy::RetainTruncatedMachineFunction,
)?;
let source_fields = ccd.metadata()?;
// Shape comes from the consumer: the CCD file stores no detector dimensions.
let mask = ccd.build_mask([3265, 3265], Projection::Strict, Limits::default())?;
```

`Reader::open` and `Characteristics::from_bytes` default to strict parsing.
`open_with_policy` and `from_bytes_with_policy` expose the narrow optional
`RetainTruncatedMachineFunction` policy. Complete fixed headers, section counts,
and counted records are always required. An incomplete terminal machine function
may instead remain original raw bytes with its offset, expected byte count and
available byte count. Missing machine parameters and subsequent row sections
remain `None`; no default correction or fabricated source values are supplied.
Source changes invalidate both path-backed metadata and mask operations.

Every record family is retained: original 256-byte strings (including invalid
UTF-8), dark current/read noise, FIP origin, corner/glowing coordinates, polygons,
points, general/binned columns and rows, electronics and machine function. All
reserved polygon coordinates, replacement coordinates and treatment flags remain
available. Original bytes and trailing bytes remain lossless. Parsing uses explicit
little endian, u16 coordinates and admitted CCD versions 1.1–1.12; unknown
versions fail. Limits bound source bytes, retained metadata, aggregate records
and output bytes before the corresponding allocation. `Limits.frames` caps the
aggregate source/decomposition record count for this metadata API.

`Projection::Strict` accepts bounded rectangles and points and rejects reversed
or out-of-shape rectangles and records without an implemented treatment model.
`Projection::Fabio2026` explicitly selects FabIO's rectangle clipping and omission
of nonrectangle polygons, rows, columns and corners. It reports the number of
ignored records. Points outside the requested shape always fail. Neither policy
applies flags or replacement values as corrections to image counts. Derived
bitmaps contain u8 values 0/1 in the source orientation.

`decompose(&binary_mask, shape, full, limits)` requires a binary u8 bitmap.
Coarse mode retains only full row/column gaps. Full mode adds a native lossless
row-run cover, with isolated pixels represented as points. Rectangle partitions
are not unique: equality means the reconstructed bitmap, not identical pyFAI
rectangle lists. `Decomposition::build_mask` reconstructs explicitly derived
geometry. This API needs no Python or pyFAI.

## Same-file qualification

[Schema 9 full-suite evidence](fabio-parity/2026-11-06-xcalibur.json) runs both
unchanged Xcalibur tests alongside the full upstream codec suite. The bridge
intercepts CCD `read`, serialized `loads` and mask `decompose`, without changing
reference objects or assertions. All **11 API events match**:

- Five original CCD files and their five serialized byte buffers account for
  all **430 top-level field occurrences**. Thirty-five occurrences are explicitly
  absent native sections with matching reference defaults; the remaining 395
  compare native source fields. Named one-element-tuple scalar mistakes in the
  reference are mapped explicitly, rather than coercing arbitrary lists.
- All five original files terminate with only **8 of 18 machine-function bytes**.
  Native strict reads reject them. The explicit partial-metadata policy preserves
  every available field and tail byte with a diagnostic. FabIO silently retains
  earlier fields and stops. Its subsequent serialized buffers are complete.
  The native parser does not treat a reference serialization as original data.
- Each original and serialized buffer yields the same derived mask under both
  explicit projections, with a caller-selected enclosing shape and no ignored
  records. The file does not supply that shape.
- The original Pilatus negative-pixel bitmap is **1043×981**. Native full
  decomposition gives 12 rectangles and 27 points; native reconstruction and
  independent FabIO rendering of that native geometry both match its original
  byte hash. Native coarse reconstruction matches FabIO's coarse mask exactly.
  pyFAI is absent in this reference environment: FabIO's `full=True` falls back
  to five coarse rectangles and its original exact-equality assertion is disabled.
  The native exact-reconstruction check is enforced independently of that branch.

Eight Rust regression tests include all 4,096 possible 3×4 masks, malformed
truncations, changed sources, limits, projection policies and complete u16 edge
coordinates. An independent CC0 binary fixture covers every record family and
unequal binned row/column counts. Five Python comparison controls guard the
bridge and reproduce two pinned reference defects: point treatment is parsed as
a tuple that its serializer rejects, and binned row loops use column counts.
The native parser preserves scalar source flags and uses each row section's
own count. No experimental CCD files are copied into Git.

## Remaining reading/API qualification

This closes the tested CCD characteristics and mask APIs. Full FabIO reading
parity remains false. The remaining contracts at that snapshot were (content discovery and stream input
are subsequently addressed in [INPUT_APIS.md](INPUT_APIS.md)):

1. Content-based format discovery, including ambiguity and feature availability.
2. Direct stream/buffer input for the general reader, with bounded staging and
   explicit origin/relative-link handling; the existing image bridge materializes
   Python streams into files and does not establish that native API.
3. File-series and frame-navigation behavior, including end-of-series errors and
   source changes. Current source-local frame access does not establish implicit
   neighboring-file traversal.
4. Remaining non-image companion readers, including Xcalibur `.run` sweep records,
   and variants/layouts outside the current corpus.

Qualified reference bugs, incomplete source spans and metadata presentation
conventions remain explicit in the prior reports. Writer/mutation/statistics
interfaces are outside this native input crate's contract.
