# FabIO metadata comparison, 2026-11-06

This page retains the first metadata pass. The [header-only and attribute
continuation](HEADER_ATTRIBUTES.md) closes its 25 unqualified no-pixel cases and
accounts for source metadata outside `.header`, with 202 passing Rust tests.
The original schema 7 numbers below describe that earlier snapshot.

The same pinned FabIO corpus now has an explicit field-by-field `.header`
comparison, independently of pixel results. Across 34 intercepted codecs and
2,589 native frames or available-source headers, **218,722 field occurrences
match**, and **3,289 have qualified reference differences**. There are **zero
unresolved compared fields**. These are weighted occurrences from repeated reads
and stacks, not that many distinct fields or files. The format-specific unions
contain 1,996 header names.

The [full schema 7 report](../fabio-parity/2026-11-06-metadata.json) executes all
35 unchanged codec test modules: 213 tests, no failures/errors, one existing
upstream skip. Its pixel outcomes remain identical to the preceding planar16
report: 510 direct events, 27 explicit views, four component/views, one sparse
reconstruction, 11 qualified partial-source events, two qualified Pillow defects
and 25 reference no-pixel reads. No successful reference read is rejected by the
native bridge. The [original 91-case rejection audit](../rejection-audits/2026-11-06-after-metadata.json)
retains 80 complete closures and 11 independently verified partial-source cases. The executable, Rust, vendor, native C, build, reference-library,
bridge and explicit-policy fingerprints remain unchanged throughout the run.

**This does not establish identical metadata or full reading/API parity.**
`metadata_fields_accounted_for_pixel_reads` is true; `metadata_parity` and
`full_reading_parity` remain false. In that snapshot, twenty-five no-pixel reads were explicitly
unqualified for header-only access: 24 have empty TIFF headers, while the empty
d*TREK writer test has six fields and zero declared dimensions. Xcalibur's
separate mask API, metadata attributes outside `.header`, unsupported variants,
discovery, streams and neighboring-file APIs remain separate qualification work.
An empty reference header supplies no evidence about native metadata coverage.

## Native gaps closed

- **CBF:** bounded scalar and loop CIF parsing before the binary data item,
  including quoted/multiline strings, comments, source-order duplicates and data
  blocks. Original header bytes survive. Optional malformed/unsupported text is
  diagnostic; resource-budget violations remain errors. MIME storage metadata
  stays separate. No Pilatus correction is applied.
- **DM3:** `Tag::decode_small_value` exposes typed scalars/arrays, UTF-16 code
  units and named structures under an output-allocation budget. Hierarchy and
  raw tag names survive. Large tags remain explicit source references; complex
  arrays remain opaque bytes rather than being discarded. Existing image and
  raw-tag access remain available.
- **Esperanto:** typed named acquisition scalars accompany the original grouped
  assignments. Missing tokens are not invented; malformed optional tokens have
  an explicit `Invalid` value. All retained fields are budgeted.
- **MAR345:** documented binary scalar units and ordered text subfields expose
  repeated COUNTS groups. Empty assignments such as `REMARK` are now retained.
  Original binary words, complete Latin-1 text and all repeated entries survive.
- **JPEG:** JFIF APP0 version, density units, original densities and declared
  thumbnail geometry are accessible without unit conversion or resampling.
  All original marker segments remain available.
- **PNM:** WIDTH/HEIGHT and MAXVAL retain their original token spelling, including
  leading zeros, alongside parsed shape/type.
- **Oxford:** both signed overflow flags, source Time text and the exact padded
  three-byte compression selector are exposed. Motor steps/radians and variance
  remain stored values; reference degrees and std are explicit comparison views.
- **Wrapped FIT2D:** masks and axes can be read through the existing native array
  reader, including gzip/bzip2 staging, with original-source checks before and
  after access. Errors invalidate the exposed buffer.

## Names, representations and reference behavior

The bridge exports existing metadata that its earlier four-format header check
omitted. It records exact aliases and conversions rather than changing the
native APIs to copy Python presentation choices: EDF case normalization, Bruker
HDRBLKS text/int, HiPiC geometry, PIXI frame-size arrays, SPE calibration arrays,
MAR CCD byte arrays, Lambda `str(bytes)`, TIFF numeric tag aliases and Esperanto
subfields. TIFF defaults and palette/description display views are explicitly
labelled; no fabricated acquisition field is inserted into native metadata.
MPA's reference namespace overwrites repeated report titles, while native ADC
scopes retain both reports. MAR345's corrupt DATE presentation is an explicit
pinned truncation view; its complete original value is still retained.

Three cases deliberately retain differences:

| Case | Affected native frames/headers | Different field occurrences | Evidence and native contract |
|---|---:|---:|---|
| FIT2D float decoder | 4 | 28 | Pinned `hex_to(..., float)` ignores its input and always uses bits `38d1b717`. Every native value must equal an independent decode of its original record bits. Masks/axes match separately. |
| MRC float and word layout | 104 | 1,556 | Reference zips semantic names to consecutive int32 words, treats stored floats as integer bits, and fails to skip the 100-byte EXTRA region. Native values must match independent source-correct offsets/types. |
| GE APS blank/EDF compatibility | 11 | 1,705 | Reference interprets blank or EDF bytes using the standard binary GE schema, then substitutes geometry. Native reports the actual source kind, original bytes and actual EDF fields, without asserting fake acquisition values. This is a legacy presentation difference, not a native reader failure. |

The [controlled reference audit](2026-11-06-reference-controls.json) changes only
headers in five small independent CC0 FIT2D/MRC variants. Real pinned FabIO and
native reads demonstrate the reference behavior while every native count hash
stays unchanged. Twenty-six per-field negative controls reject wrong native or
reference values. Another eight comparator tests cover exact types, array
shape/type/all-byte hashes, aliases, TIFF defaults/palettes, GE fallback guards
and compact report preservation. Qualified differences remain outside metadata
equality; a filename or known codec alone never qualifies a wrong value.

The report retains reference/native hashes and complete differences. Repeated
mapping schemas are deduplicated by SHA256; each schema contains the reference
field names and the source of every nontrivial projection. Small array previews
are optional diagnostics; dtype, complete shape and canonical sample-byte hashes
are mandatory array identities. The [initial inventory](2026-11-06-baseline.json)
separates missing bridge projections from actual reader gaps; it is not a claim
that all initially missing fields were absent from the native library.

## Per-format result

“Matching” below means the named source/projection comparison passes, not that
native types or dictionary names duplicate Python's API. Empty references are
reported separately. Partial-source headers are compared only where a complete
header is present; unavailable detector samples remain unavailable.

| Codec | Distinct header names | Compared frames/headers | Matching | Qualified differences | Empty reference | Header-only unqualified |
|---|---:|---:|---:|---:|---:|---:|
| GEimage | 155 | 15 | 4 | 11 | 0 | 0 |
| HiPiCimage | 121 | 6 | 6 | 0 | 0 | 0 |
| OXDimage | 74 | 12 | 12 | 0 | 0 | 0 |
| binaryimage | 0 | 2 | 0 | 0 | 2 | 0 |
| bruker100image | 84 | 9 | 9 | 0 | 0 | 0 |
| brukerimage | 82 | 16 | 16 | 0 | 0 | 0 |
| cbfimage | 24 | 17 | 17 | 0 | 0 | 0 |
| dm3image | 202 | 6 | 6 | 0 | 0 | 0 |
| dtrekimage | 147 | 21 | 21 | 0 | 0 | 1 |
| edfimage | 466 | 1247 | 1247 | 0 | 0 | 0 |
| eigerimage | 0 | 261 | 0 | 0 | 261 | 0 |
| esperantoimage | 96 | 9 | 9 | 0 | 0 | 0 |
| fit2dimage | 19 | 4 | 0 | 4 | 0 | 0 |
| fit2dmaskimage | 0 | 8 | 0 | 0 | 8 | 0 |
| fit2dspreadsheetimage | 3 | 3 | 3 | 0 | 0 | 0 |
| hdf5image | 0 | 202 | 0 | 0 | 202 | 0 |
| jpeg2kimage | 0 | 8 | 0 | 0 | 8 | 0 |
| jpegimage | 4 | 3 | 3 | 0 | 0 | 0 |
| kcdimage | 36 | 4 | 4 | 0 | 0 | 0 |
| lambdaimage | 1 | 60 | 60 | 0 | 0 | 0 |
| limaimage | 1 | 95 | 95 | 0 | 0 | 0 |
| mar345image | 61 | 30 | 30 | 0 | 0 | 0 |
| marccdimage | 125 | 6 | 6 | 0 | 0 | 0 |
| mpaimage | 71 | 1 | 1 | 0 | 0 | 0 |
| mrcimage | 41 | 104 | 0 | 104 | 0 | 0 |
| numpyimage | 0 | 194 | 0 | 0 | 194 | 0 |
| pilatusimage | 30 | 13 | 13 | 0 | 0 | 0 |
| pixiimage | 4 | 14 | 14 | 0 | 0 | 0 |
| pnmimage | 4 | 2 | 2 | 0 | 0 | 0 |
| raxisimage | 112 | 3 | 3 | 0 | 0 | 0 |
| sparseimage | 0 | 100 | 0 | 0 | 100 | 0 |
| speimage | 13 | 44 | 44 | 0 | 0 | 0 |
| tifimage | 20 | 65 | 65 | 0 | 0 | 24 |
| xsdimage | 0 | 5 | 0 | 0 | 5 | 0 |

## Verification and reproduction

[Native check evidence](2026-11-06-native-checks.json) records 196 passing Rust
tests, formatting, strict all-target/all-feature Clippy, no-default build/tests,
and strict isolated checks for all 20 optional features. Independent fixtures
cover CIF syntax/bounds without count changes, both DM3 byte orders and float
bits, repeated/empty MAR345 groups, original PNM tokens, signed Oxford flags,
and auxiliary FIT2D arrays through compressed/mutated sources. This is local
macOS evidence, not additional platform or throughput qualification.

Use the environment and pinned sources in [FABIO_PARITY.md](../FABIO_PARITY.md).
Keep detector data and the explicit Lambda source binding map outside Git:

```sh
cargo build --release --locked --all-features --example parity_probe
DYLD_LIBRARY_PATH=/opt/homebrew/lib \
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/fabio_suite_parity.py \
  --fabio-source /tmp/dioptrin-io-fabio-2026.6.0 \
  --data /tmp/dioptrin-io-fabio-corpus \
  --probe target/release/examples/parity_probe --probe-timeout 300 \
  --tiff-reference-library /opt/homebrew/lib/libtiff.6.dylib \
  --read-policies docs/source-recovery/read-policies.json \
  --vds-map /tmp/dioptrin-lambda-vds-map.json --metadata \
  --report /tmp/dioptrin-metadata.json
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/test_metadata_parity.py
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/audit_metadata_reference_defects.py \
  --probe target/release/examples/parity_probe --report /tmp/metadata-controls.json
```

The full report exits nonzero while its full parity gate is false. Omitting
`--metadata` cannot produce a full metadata/reading parity claim. Historical
reports remain unchanged.
