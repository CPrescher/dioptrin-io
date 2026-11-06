# Current detector-reading gap inventory

Reviewed 2026-11-06. This is the current backlog; dated audits and JSON proof reports retain the scope tested at their revision. Generated from [reading-inventory.json](reading-inventory.json).

## What is actually outstanding

| Measure | Total | Meaning |
|---|---:|---|
| Confirmed acquired-detector incidents in this inventory | 0 | No retained unresolved incident; not a universal support claim |
| Unclassified outcomes in the acceptance snapshot | 0 / 626 | Complete reads, explicit views, partial sources and metadata-only events remain distinct |
| Confirmed supplemental compatibility areas | 2 | HDF5 image dtype representations and nonnumeric/structured NPY arrays |
| Named unsupported, FabIO-readable fixture cases | 6 | Four HDF5 and two NPY cases; not six unread acquired detector files |
| Optional capability families | 12 | Grouped boundaries, not a count of variants or required implementation tickets |
| Real-file validation batches | 6 | Requirements and acquired-source qualification, not known decoder failures |

The [acceptance snapshot](fabio-parity/2026-11-06-vds-advanced-ojpeg.json) ran 237 unchanged reference tests with 1 existing slow-test skip; compared header fields and source attributes have zero unresolved differences. The [native checks](fabio-parity/2026-11-06-vds-advanced-ojpeg-native-checks.json) qualify the reader revision. This inventory cleanup does not rerun or rewrite those historical corpus proofs.

The old `full_reading_parity` boolean is a strict reference-equality/instrumentation gate, not a backlog counter. It also requires no upstream skip, excludes independently qualified decoder defects, and retains exact metadata/presentation differences and broad harness annotations. Its false value does not mean that all those conditions need native decoding work.

## Classification and promotion rules

- A confirmed detector-reading gap needs an acquired-source reproducer, the required reading operation, observed failure and independent expected counts/metadata.
- A confirmed compatibility case is narrower: the retained supplemental fixture completes in unchanged FabIO and the native reader rejects it. Acquired-detector relevance must still be established.
- Optional variants remain file-triggered. A format name or an old `partial` label alone does not make them release blockers.
- Intentional policies, failed/undefined reference branches and APIs outside native input are not implementation tickets.
- Validation needs are tracked separately from code defects. Preserve original counts/dtypes/axes, raw metadata, source manifests and evidence provenance.

## Confirmed supplemental compatibility gaps

<a id="c-hdf5"></a>
### C-HDF5: HDF5 enum/compound image representations

Four authored cases complete in pinned FabIO and are rejected by the native exact-type contract. These are four fixture cases in one capability area; acquired-detector relevance is unestablished.

Evidence: [2026-11-06-vds-advanced-ojpeg-regression](hdf5-layouts/2026-11-06-vds-advanced-ojpeg-regression.json), [HDF5_LAYOUTS](HDF5_LAYOUTS.md).

<a id="c-npy"></a>
### C-NPY: NPY structured and string arrays

Two authored cases complete in pinned FabIO and are rejected by the native image dtype contract. String/record arrays are not ordinary numeric detector-count images.

Evidence: [2026-11-06-vds-advanced-ojpeg-regression](npy-types/2026-11-06-vds-advanced-ojpeg-regression.json), [NPY_LAYOUTS](NPY_LAYOUTS.md).

## Optional variants: implement when an acquired file requires them

<a id="v-hdf5"></a>
### V-HDF5: Additional HDF5/VDS storage and types

Cross-axis non-printf unlimited mappings, point selections, VDS concrete leaves using external raw storage, arbitrary custom types and native HDF5-2 complex types are outside qualified image contracts. Separate raw external windows and finite/nested/printf/frame-axis VDS are already supported.

Evidence: [VDS_READER](VDS_READER.md), [HDF5_LAYOUTS](HDF5_LAYOUTS.md).

<a id="v-ojpeg"></a>
### V-OJPEG: Additional original-JPEG TIFF profiles

Higher-precision/lossless processes, noncomplete subsampled planar streams, planar multiple strips/restarts and remaining ambiguous header/entropy partitions are unqualified. No current corpus blocker is demonstrated.

Evidence: [OJPEG_READER](OJPEG_READER.md).

<a id="v-images"></a>
### V-IMAGES: Additional TIFF/JPEG/JPEG2000 layouts

Mixed TIFF sample representations/packed color and additional bit-order layouts; lossy JPEG above 8-bit/lossless multichannel above 8-bit; JPX/multiple codestreams/palettes/component mapping/subsampled JPEG2000 remain outside current contracts. Not all are supported by the pinned reference.

Evidence: [IMAGE_READERS](IMAGE_READERS.md), [TIFF_LAYOUTS](TIFF_LAYOUTS.md).

<a id="v-mar"></a>
### V-MAR: Legacy MAR layouts

Raw/spiral/old IMAGE layouts and degenerate predictor shapes remain unsupported. PCK V1 and original V2 are implemented. An acquired V2 sample is a validation need, not another decoder implementation task.

Evidence: [MAR345_READER](MAR345_READER.md), [MAR345_V2](MAR345_V2.md).

<a id="v-containers"></a>
### V-CONTAINERS: Additional SPE/MRC/DM3 layouts

Repeated SPE region blocks/additional XML layouts, MRC modes 12/101 and packed IMOD, DM3 packed/RGB/complex images and DM4 are optional extensions. Supported pinned MRC modes and explicit SPE regions/stacks are already qualified.

Evidence: [CONTAINERS_DESIGN](CONTAINERS_DESIGN.md).

<a id="v-edf"></a>
### V-EDF: Extended precision and obsolete EDF compression

EDF 128-bit labels need a verified source ABI; optional OFFSET needs an independent codec authority unavailable in the pinned environment. Arrays/volumes, exact types and available-span recovery are implemented.

Evidence: [EDF_ARRAYS](EDF_ARRAYS.md).

<a id="v-npy"></a>
### V-NPY: Platform-dependent NPY extended precision

16-byte float interpretation is unavailable in the pinned macOS NumPy environment; no portable interpretation is asserted. Boolean, binary16 and empty arrays are implemented.

Evidence: [NPY_LAYOUTS](NPY_LAYOUTS.md).

<a id="v-records"></a>
### V-RECORDS: Other legacy record/type variants

Additional HiPiC/Oxford/GE/Pixi/Bruker/R-AXIS/FIT2D/KCD layouts and binary/external MPA need concrete source examples before new decoding contracts are added. Explicit source records, readouts and derived views are already available.

Evidence: [LEGACY_READERS](LEGACY_READERS.md), [RECORD_READERS](RECORD_READERS.md), [FIXED_DESIGN](FIXED_DESIGN.md).

<a id="v-xsd"></a>
### V-XSD: Additional XSData layouts

Other XML namespaces, ranks, type/coding combinations and nested array layouts are optional; the current explicit 2D numeric record contract remains bounded.

Evidence: [XSD_READER](XSD_READER.md).

<a id="v-cbf"></a>
### V-CBF: Additional CBF decoding/section semantics

Other decoded conversions, multiple binary sections and dictionary semantics remain outside the integer single-section image API. CIF 1.1/2.0 syntax, Pilatus parsing and opaque encoded-byte access are implemented separately.

Evidence: [CBF_ENCODED](CBF_ENCODED.md), [CIF2_SYNTAX](CIF2_SYNTAX.md), [LOWLEVEL_HELPERS](LOWLEVEL_HELPERS.md).

<a id="v-sparse"></a>
### V-SPARSE: Alternative sparse derived reconstructions

Stochastic noise/cutoff synthesis and Python-fallback semantics are separate optional derived models. Original stored events/components and explicit deterministic reconstruction are supported.

Evidence: [SPARSE_READERS](SPARSE_READERS.md).

<a id="v-profiles"></a>
### V-PROFILES: Additional profile declarations

Unspecified CHIPLOT uncertainty/declaration variants remain unsupported; exact ASCII table/profile source tokens and numeric views are qualified.

Evidence: [CHIPLOT](CHIPLOT.md), [COLUMNFILE](COLUMNFILE.md).

## Intentional source and interpretation policies

<a id="i-counts"></a>
### I-COUNTS: Missing counts and malformed input

Complete-image reads reject missing samples; explicit recovery exposes only available spans and integrity. Reference zero padding is not evidence for measured counts.

Evidence: [README](source-recovery/README.md), [EDF_RECOVERY](EDF_RECOVERY.md).

<a id="i-snapshots"></a>
### I-SNAPSHOTS: Immutable sources and explicit manifests

Changed/growing sources require reopening. External links, relocated modules, printf prefixes and file series require explicit source contracts. Live/SWMR refresh and implicit neighboring-file search are separate product scope, not unfinished snapshot reading.

Evidence: [VDS_READER](VDS_READER.md), [SERIES_API](SERIES_API.md), [INPUT_APIS](INPUT_APIS.md).

<a id="i-views"></a>
### I-VIEWS: Stored samples versus consumer views

Orientation, normalization, masks, reference display conversion, KCD sums, Bruker/R-AXIS expansion and sparse backgrounds stay explicit consumer/derived policies. TIFF float16 widening retains source precision metadata.

Evidence: [READER_DESIGN](READER_DESIGN.md), [README](tiff-ycbcr/README.md), [SPARSE_READERS](SPARSE_READERS.md).

<a id="i-objects"></a>
### I-OBJECTS: No object deserialization or guessed declarations

Object NPY input is rejected without unpickling. Missing/unknown source type declarations are not guessed; explicit documented aliases remain accepted.

Evidence: [NPY_LAYOUTS](NPY_LAYOUTS.md), [READER_DESIGN](READER_DESIGN.md).

## Reference limitations, not native implementation tasks

<a id="r-defects"></a>
### R-DEFECTS: Qualified reference defects and presentation

Pillow planar16 errors, HDF5 nested printf cache behavior, MAR dialect/predictor errors, MRC mode3 errors and CIF/header projection differences remain recorded; reproducing wrong source counts is not a native task.

Evidence: [README](pillow-planar16/README.md), [VDS_ADVANCED_OJPEG_PARITY](VDS_ADVANCED_OJPEG_PARITY.md), [MAR345_V2](MAR345_V2.md), [CONTAINERS_DESIGN](CONTAINERS_DESIGN.md), [HEADER_ATTRIBUTES](metadata-parity/HEADER_ATTRIBUTES.md).

<a id="r-producer"></a>
### R-PRODUCER: Rejected or undefined reference capabilities

Tested HDF5 producers reject genuinely infinite unions/unsupported cardinality changes and printf width tokens. Binary column tables are an upstream stub; pinned PNM P3/P4/P6/P7 are unimplemented. These are not demonstrated working FabIO capabilities to port.

Evidence: [HDF5_UNLIMITED_VDS](HDF5_UNLIMITED_VDS.md), [HDF5_PRINTF_VDS](HDF5_PRINTF_VDS.md), [COLUMNFILE](COLUMNFILE.md), [FORMAT_COVERAGE](FORMAT_COVERAGE.md).

## Outside native detector input

<a id="s-api"></a>
### S-API: Python object/writer and integration APIs

Writing, in-place Python/h5py object mutation/identity, statistics and integration mathematics are outside this native input crate. Read-only frame/navigation/document/NeXus helpers have their own qualified contracts.

Evidence: [FABIO_PARITY](FABIO_PARITY.md), [LOWLEVEL_HELPERS](LOWLEVEL_HELPERS.md), [NAVIGATION_API](NAVIGATION_API.md).

## Closed entries removed from the active backlog

| Previously listed as pending | Qualified resolution | Evidence |
|---|---|---|
| MAR345 PCK V2 | Original seven-bit default and explicit incompatible eight-bit policy | [MAR345_V2](MAR345_V2.md) |
| TIFF RLE/Group3 2D/SubIFD | Qualified source codecs and explicit directory selection | [TIFF_LAYOUTS](TIFF_LAYOUTS.md) |
| Fragmented/global/planar OJPEG | Qualified bounded profiles and explicit reset-tile policy | [OJPEG_READER](OJPEG_READER.md) |
| NPY bool/binary16/empty arrays | Exact source words and explicit logical planes | [NPY_LAYOUTS](NPY_LAYOUTS.md) |
| EDF volumes/arrays | Source rank0..32, explicit planes and recovery | [EDF_ARRAYS](EDF_ARRAYS.md) |
| HDF5 half/bool/packed complex/attributes/raw windows | Exact admitted types and explicit source windows | [HDF5_LAYOUTS](HDF5_LAYOUTS.md) |
| Finite irregular, nested and printf VDS | Bounded graph, effective selections and fill provenance | [VDS_READER](VDS_READER.md) |
| Frame-axis unlimited VDS | Complete source-bound offsets/strides/blocks | [HDF5_UNLIMITED_VDS](HDF5_UNLIMITED_VDS.md) |
| Xcalibur CCD/mask/run APIs | Qualified source records and explicit derived masks | [XCALIBUR_MASKS](XCALIBUR_MASKS.md) |
| Content discovery and captured/direct streams | Bounded input and explicit ambiguity/origins | [INPUT_APIS](INPUT_APIS.md) |
| File series and frame navigation | Frozen manifests and checked cursor semantics | [SERIES_COMPANIONS_PARITY](SERIES_COMPANIONS_PARITY.md) |
| CIF 1.1/2.0 and ASCII table/profile companions | Versioned source trees and explicit numeric views | [CIF2_LOWLEVEL_PARITY](CIF2_LOWLEVEL_PARITY.md) |
| Low-level TIFF/Pilatus/NeXus/CBF encoded input | Qualified reader helpers, not Python object cloning | [LOWLEVEL_HELPERS](LOWLEVEL_HELPERS.md) |
| Observed headers/source attributes/header-only reads | No unresolved compared fields; qualified differences retained | [HEADER_ATTRIBUTES](metadata-parity/HEADER_ATTRIBUTES.md) |

These closures cover their documented contracts. They do not promote all possible datatype/layout combinations to universal support.

## Next: acquired detector files

Store large originals outside Git. For each acquisition, record detector/software origin, size/hash, source manifests, required reader options, dtype/shape/frame counts, complete count hashes, reserved values and consumer-required metadata with units. Distinguish acquired originals from converted or writer-generated files; use independent source evidence when FabIO fails.

| Batch | Validation target | Existing evidence |
|---|---|---|
| F-HDF5: Acquired Eiger/LImA/Lambda scans | Complete first/middle/last and all-frame count hashes, source dtype/axes, needed acquisition fields, module/link manifests and frozen source checks. Extend beyond existing acquired Eiger benchmark and relocated Lambda corpus. | [README](benchmarks/2026-11-05-native-eiger/README.md), [VDS_READER](VDS_READER.md) |
| F-RASTER: Acquired TIFF/CBF/EDF detector images and stacks | Original counts, reserved/saturated values, orientation metadata, compression, frame enumeration and required geometry/header fields. Include original files rather than only writer-generated variants. | [FABIO_PARITY](FABIO_PARITY.md), [README](metadata-parity/README.md) |
| F-LEGACY: Acquired legacy detector files | MAR345/MAR555 V1 are covered; seek an acquired V2 if available. Add actual SPE ROI/frame metadata, DM3 main-image selection and legacy detector examples needed by consumers. | [MAR345_V2](MAR345_V2.md), [CONTAINERS_DESIGN](CONTAINERS_DESIGN.md) |
| F-SPARSE: Acquired sparse frames with nonempty events | Existing acquired sparse file has zero events. Validate source event words and metadata on nonempty acquisitions before extending derived models. | [SPARSE_READERS](SPARSE_READERS.md) |
| F-METADATA: Consumer-required acquisition semantics | Name the geometry, wavelength, distance, exposure, detector identity and timestamp fields actually required. Retain units/raw values and compare them on acquired files; unknown vendor metadata is a requirement/validation question, not a counted decoder defect. | [HEADER_ATTRIBUTES](metadata-parity/HEADER_ATTRIBUTES.md), [LOWLEVEL_HELPERS](LOWLEVEL_HELPERS.md) |
| F-PORTABILITY: Deployment and consumer read workflows | Validate required feature combinations on target platforms, stream/series/source-replacement behavior and the agreed reader API. Application migration stays a separate authorized task. | [VALIDATION](VALIDATION.md), [INPUT_APIS](INPUT_APIS.md), [SERIES_API](SERIES_API.md) |

Promote a reproducible failure found during these batches into a confirmed detector incident, then close it against the same source. Existing fixture/reference qualifications remain visible; there is no automatic expansion through every optional family.
