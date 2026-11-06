# Header-only reads and source attributes, 2026-11-06

All **25 previously unqualified no-pixel read events now have verified metadata
access**. All populated source attributes observed outside FabIO's `.header` are
accounted for: **2,591 matching comparisons**, **133 qualified reference/presentation
differences**, **43 unspecified properties/placeholders**, and **zero unresolved
attributes**. These are repeated read occurrences, not distinct fields/files;
145 codec-specific attribute names (including common shape/type/frame properties) were observed. The [schema 8 full report](../fabio-parity/2026-11-06-headers-attributes.json)
retains every reference value, classification, source mapping, comparison and
independent header check.

All 35 unchanged upstream codec modules execute: 213 tests, no failures/errors,
one existing skip. Every previous pixel outcome is unchanged. Header inspection
never promotes a no-pixel read into a complete count match. The original source,
Rust, vendor, native C, build, reference libraries, executable, bridge and explicit
policies have unchanged fingerprints throughout the run.

## Header-only APIs and proof

`native::DtrekHeader` reuses the existing d*TREK parser and admits empty declared
dimensions. It exposes the six fields from the upstream empty-image writer test.
It also indexes complete bounded headers whose image payload is truncated, while
the complete-image reader continues to reject absent/empty counts.

`tiff::header::Reader` reuses source-range/cycle admission and the existing vendored
TIFF IFD/value decoder. A small factored header bootstrap supplies both the image
constructor and the new directory reader. Header inspection constructs no image,
selects no sample codec and allocates no pixel buffer. Typed tags, original
photometric declarations and explicitly opaque text remain available even for
unsupported packed-bit, LogL/LogLuv and compression layouts. Unknown field types,
duplicate tag IDs, directory cycles, invalid metadata ranges, excessive directory
entry counts and exhausted metadata budgets fail explicitly.

All **24 original TIFF no-pixel events** open successfully for metadata. An
independent tifffile-based directory inventory and source-byte decoder compare
**25 primary IFDs / 408 tag occurrences**, including the two-page `dscf0013.tif`.
The oracle checks source-endian values, rational pairs, original ASCII bytes,
opaque declarations, original IFD offsets and the first source bytes. It does
not call an image/pixel decoder. EXIF/SubIFD pointers remain original tag values;
this API indexes the primary IFD chain rather than claiming arbitrary recursive
container traversal. Every emitted header explicitly has `pixels_available=false`.

`wrapped::HeaderReader` exposes both header formats through the existing bounded
outer gzip/bzip2 transport. `metadata()` and the bounded unwrapped source prefix
validate original/staged source identities. The backend is dropped before its
staging file. Tests cover both wrappers, empty d*TREK geometry, source replacement,
all classic/BigTIFF byte-order combinations, unsupported sample layouts, absent
sample windows, unknown metadata types and cycles. Neither header-only API offers
a frame/pixel read method or treats metadata validity as sample availability.

## Source metadata outside .header

CBF now exposes typed Pilatus acquisition metadata: detector identity, pixel size,
sensor thickness, exposure, thresholds, beam position, distance, wavelength,
angles and other named fields. A shared bounded parser serves CBF and the existing
TIFF acquisition layer. Original text, units, raw assignments, unknown lines and
invalid optional tokens survive; no correction is applied to counts. Combined
CIF/Pilatus retained metadata stays inside the configured budget. CBF's parsed
CIF dictionary is also compared independently of its `.header` projection; the
reference's binary-block placeholder is explicitly a presentation value.

EDF general headers are accessible through the wrapper as original separate
records, alongside local overrides and inherited per-frame fields. The reference
`incomplete_data` property describes the selected frame, so it is compared to
that frame's available sample span. Whole-source truncation, trailing incomplete
headers and codec integrity remain separate recovery diagnostics. A complete
first frame never makes the following missing counts available.

Common `shape`, logical `dtype` and `nframes` properties are checked too. Dtype
comparison uses the logical scalar name; native storage byte order remains
separate metadata. The selected explicit display view supplies its own geometry
and dtype, while the source layout stays unchanged. Header-only shapes remain
declarations rather than evidence for available samples.

Other attributes use existing native metadata and source arrays:

- HDF5 dataset path, canonical source identity, full shape and dtype, including
  every Eiger dataset in order. The explicitly bound Lambda VDS reference exposes
  a module-derived frozen extent; that view is compared while the original
  declared extent remains in native VDS provenance.
- NumPy's complete public dataset has an independently accumulated native logical
  byte hash, dtype and full shape. Its reference-only leading-axis flattening or
  rank-one padding is explicit; original source axes remain native metadata.
- Sparse masks, profiles, frame pointers and stored event arrays use the existing
  bounded native component reader. Optional normalization, original configuration,
  dummy value/default, peaks inventory and deterministic-noise policy are separate
  from measured detector counts.
- SPE XML offsets/frame counts, Esperanto header length/encoding, FIT2D record
  width, MAR345 high-count/pixel totals/byte order, DM3 declared length/byte order,
  Bruker format version, R-AXIS byte order, MRC frame byte size and XSData coding,
  checksum declaration and element count are compared by name and type.

Every populated public instance attribute is classified. An unknown attribute or
unsupported value becomes an unresolved result. File handles, decoder functions,
backend names, encoded/working count buffers, filename-series context and
statistics/ROI caches are recorded as reader state; `.header` is checked by its
own comparison. Raw HDF5 file handles do not turn arbitrary file traversal into
a promised FabIO metadata API. Private decoder caches and Python mutation/writing
interfaces are likewise outside this source-metadata comparison.

## Qualified TIFF reference behavior

FabIO's preliminary TIFF parser sets `nbits` from native-endian **word 21 of the
first 64 file bytes**. This is unrelated to TIFF tag 258 (BitsPerSample), and the
source itself marks the preliminary dimensions as suspect. In all 101 populated
occurrences the exact source-word model reproduces the reference value, while an
independent TIFF tag read verifies the native stored bit-depth declaration.
The native value is never replaced with the wrong probe result. Changed native
bit depths or changed probe values lose qualification. Twenty-three no-pixel TIFF shapes similarly reproduce wrong preliminary
source words 15 and 9; native stored rows/columns are independently verified.
Two SPE objects retain inherited `nframes=1` despite declaring more frames; all
native frames are independently checked. One multidimensional d*TREK array is
one reference image but six source-ordered native planes, with the complete
original axes retained. Six EDF boundary reads flag bounded zero storage padding
as `incomplete_file` even though every declared frame matches. The exact decoded
suffix is recorded and must be nonempty, shorter than 512 bytes and entirely
zero/whitespace before qualification. Wrong frame totals, types or suffixes fail
qualification. The remaining 43 undefined/unspecified property occurrences
assert no value and supply no coverage evidence.

`header_only_parity`, `metadata_attributes_accounted_for` and
`metadata_fields_accounted_for_pixel_reads` are **true**. Exact `metadata_parity`
and `full_reading_parity` remain **false**: the measured FIT2D/MRC metadata defects,
GE legacy presentation differences and these TIFF probe differences stay explicit,
as do the qualified shape/frame/padding conventions and earlier pixel/reference differences. Discovery, direct streams,
neighboring-file operations, variants outside the corpus and Xcalibur's separate
mask API remain distinct qualification work. This closes the tested metadata gaps;
it does not claim a Python API shim or readable complete counts from missing data.

## Per-format attributes

| Codec | Source attribute names observed | Matched occurrences | Qualified differences | Unspecified properties |
|---|---|---:|---:|---:|
| GEimage | dtype, nframes, shape | 18 | 0 | 0 |
| HiPiCimage | dtype, nframes, shape | 18 | 0 | 0 |
| OXDimage | dtype, nframes, shape | 36 | 0 | 0 |
| binaryimage | dtype, nframes, shape | 6 | 0 | 0 |
| bruker100image | dtype, nframes, shape, version | 36 | 0 | 0 |
| brukerimage | dtype, nframes, shape, version | 64 | 0 | 0 |
| cbfimage | cif, dtype, nframes, pilatus_headers, shape | 85 | 0 | 0 |
| dm3image | byte_order, bytes_in_file, dtype, nframes, shape | 30 | 0 | 0 |
| dtrekimage | dtype, nframes, shape | 51 | 1 | 2 |
| edfimage | dtype, generalframe, incomplete_data, incomplete_file, nframes, shape | 1200 | 6 | 0 |
| eigerimage | dataset, dtype, nframes, shape | 28 | 0 | 0 |
| esperantoimage | HEADER_LINES, dtype, format, nframes, shape | 45 | 0 | 0 |
| fit2dimage | BUFFER_SIZE, dtype, nframes, shape | 16 | 0 | 0 |
| fit2dmaskimage | dtype, nframes, shape | 24 | 0 | 0 |
| fit2dspreadsheetimage | dtype, nframes, shape | 9 | 0 | 0 |
| hdf5image | dataset, dtype, nframes, shape | 24 | 0 | 0 |
| jpeg2kimage | dtype, nbits, nframes, shape | 24 | 0 | 8 |
| jpegimage | dtype, nbits, nframes, shape | 9 | 0 | 3 |
| kcdimage | dtype, nframes, shape | 12 | 0 | 0 |
| lambdaimage | dataset, dtype, nframes, shape | 48 | 0 | 0 |
| limaimage | dataset, dtype, nframes, shape | 24 | 0 | 0 |
| mar345image | byteorder, dtype, nframes, numhigh, numpixels, shape | 180 | 0 | 0 |
| marccdimage | dtype, nbits, nframes, shape | 18 | 0 | 6 |
| mpaimage | dtype, nframes, shape | 3 | 0 | 0 |
| mrcimage | dtype, imagesize, nframes, shape | 32 | 0 | 0 |
| numpyimage | dataset, dtype, nframes, shape | 48 | 0 | 0 |
| pilatusimage | dtype, nbits, nframes, shape | 39 | 13 | 0 |
| pixiimage | dtype, nframes, shape | 18 | 0 | 0 |
| pnmimage | dtype, nframes, shape | 6 | 0 | 0 |
| raxisimage | dtype, endianness, nframes, shape | 12 | 0 | 0 |
| sparseimage | background_avg, background_std, cutoff, dtype, dummy, frame_ptr, index, intensity, mask, nframes, noisy, normalization, peaks, radius, shape | 15 | 0 | 0 |
| speimage | dtype, nframes, shape, xml_offset | 166 | 2 | 0 |
| tifimage | dtype, nbits, nframes, shape | 217 | 111 | 24 |
| xsdimage | coding, dtype, md5, nframes, shape, size | 30 | 0 | 0 |

## Validation and reproduction

[Native check evidence](2026-11-06-headers-attributes-checks.json) records **202
passing Rust tests**, formatting, strict all-target/all-feature Clippy,
no-default build/tests and isolated strict checks for all 20 optional features.
Seventeen Python comparison tests cover types, complete array hashes, unknown
attributes, dataset identities, source-scoped EDF metadata, TIFF qualification
and the independent header oracle. Five controlled FIT2D/MRC variants retain
unchanged counts and reject 26 wrong native/reference values in the
[refreshed control audit](2026-11-06-headers-attributes-controls.json). This is
local macOS correctness evidence, not broader platform/throughput qualification.

Use the full command in [FABIO_PARITY.md](../FABIO_PARITY.md), including
`--metadata`, the explicit source read policies and the external Lambda binding
map. Additional controls:

```sh
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/test_metadata_attributes.py
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/test_metadata_parity.py
cargo test --locked --all-features --test header_only --test wrapped
```

The schema 8 report exits nonzero while full reading parity is false. Historical
schema 7 evidence and the original detector inputs remain unchanged.
