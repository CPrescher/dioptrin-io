# SPE, MRC and DM3 native container input

The optional `containers` feature exposes `spe::Reader`, `mrc::Reader` and
`dm3::Reader`. Each reader has `open(path, Limits)`, `metadata()`, `frames()` and
`read_frame_into(index, &mut FrameBuffer)`. Outputs use the existing reusable
native typed buffer. Readers preserve stored row order and sample bits, including
signed extrema, floating NaNs/infinities/negative zero and complex components.
There are no Python runtime dependencies, orientation changes, intensity
corrections or neighbor-file substitutions.

Every source is an immutable snapshot. Path/open-file identity, source length
and modification metadata are checked before and after indexing and frame reads.
The contracts detect ordinary replacement/mutation, not adversarial writes that
preserve all metadata. Failed frame reads invalidate the public output. Reopen
changed files; no decoded-pixel cache is reused. Limits bound frame payloads,
encoded payloads, admitted headers and retained metadata. These are conservative
payload/admission limits rather than process RSS limits; allocator and library
bookkeeping add memory.

## SPE

The 4100-byte binary header and complete original SPE 3 XML footer are retained.
Binary dtype codes 0/1/2/3 map to f32/i32/i16/u16. Binary dimensions, frame count,
XML offset, DataBlock count/type/shape/size/stride and every region's range are
checked. SPE 2 requires an exact declared image payload. SPE 3 supports explicit
frame strides, multiple regions, region padding and per-frame metadata bytes;
`read_region_into(frame, region, buffer)` selects any declared region without
folding padding or metadata into counts. `read_frame_into` selects region zero.
A bounded gap between frame payloads and the footer may contain original
non-pixel data; it is never interpreted as counts. Multiple repeated Region
blocks (`count != 1`) remain unsupported. The XML source must have one
`SpeFormat/DataFormat/DataBlock type="Frame"` layout in the known SPE2009
namespace (or an unnamespaced legacy document). Duplicate/misplaced frame blocks,
foreign structural namespaces and unknown Frame/Region children are rejected;
metadata with similarly named blocks cannot redirect frame offsets. Explicit
per-region pixel formats must agree with the frame and binary header.

SPE metadata includes the original file version, dtype code, region shapes and
byte offsets, primary ROI, selected common exposure/device/time/grating fields,
and wavelength calibration. Original XML remains authoritative for unparsed
fields. Legacy polynomial coefficients remain available independently of the
calculated wavelength axis. Exposure and axis interpretation affect metadata
only. Frame metadata/padding is indexed through stride/region offsets; semantic
parsing of timestamps, tracking and arbitrary device-specific XML is a gap.

The MIT/Apache `roxmltree` 0.21.2 parser is reused. DTDs/external entities are
disabled and XML size/node count bounded. No permissively licensed reusable
native SPE reader matching these source contracts was located. The bounded
binary adapter uses the pinned FabIO source and its format tests as an
independent format reference; its copyright notice is retained.

## MRC/CCP4/MAP/FEI

The MIT `mrc` 0.8.0 library decodes the 1024-byte header. Its full Reader is not
used because it can map/load a whole file, repair invalid headers, pad truncated
payloads and convert types before caller admission. The adapter checks the
machine stamp and rejects its fallback byte-order interpretation. An unstamped
legacy file requires explicit `open_with_byte_order(..., ByteOrder::Little/Big)`;
the default is `MachineStamp`. Standard little-endian 44 44 / CCP4 44 41 and
big-endian 11 11 are admitted. The original stamp remains in both raw and typed
metadata even under explicit override.

All pinned FabIO MRC modes are supported: 0=i8, 1=i16, 2=f32, 3=CI16 pairs,
4=C32 pairs, 6=u16. For mode0 an explicit numeric IMOD stamp1146047817
and flags are decoded in the already selected byte order: flag1 selects signed
bytes, and a clear flag1 selects u8. Original mode/header/flags remain metadata.
This follows the [IMOD header contract](https://bio3d.colorado.edu/imod/doc/mrc_format.txt)
without guessing from sample values. Packed-byte flag16 is unsupported and fails;
a recognizable stamp in the opposite order fails rather than changing byte order.
Without an explicit IMOD stamp, mode0 retains the standard i8 interpretation.
The original raw main header and NSYMBT extended header remain
available, along with the existing library's typed header. MAPC/MAPR/MAPS,
origin and cell metadata do not reorient sections. Exact stack length is checked;
negative dimensions/NSYMBT, unknown modes, truncation and unexplained trailing
bytes fail explicitly. Legacy/FEI files do not require a modern MAP signature.

[CCP-EM's MRC2014 specification](https://www.ccpem.ac.uk/mrc-format/mrc2014/)
defines mode 3 as two signed 16-bit integer components, totaling four bytes per
sample. Native `PixelType::CI16` preserves that storage. The optional explicit
`read_frame_as_complex32_into` converts each i16 component exactly to f32 under
a separate output-byte check; metadata stays CI16. The pinned FabIO reader
incorrectly treats mode 3 as eight-byte complex64, so matching that misread is
not a count-preserving policy. Mode 12 float16 and mode 101 packed data remain
explicitly unsupported. FEI extended headers are retained raw, without a claim
of semantic decoding of every vendor-specific structure.

## DigitalMicrograph DM3

DM3's big-endian directory structure is parsed incrementally. Value byte order
comes from its original header. Supported tag descriptors include scalar
short/long/ushort/ulong/float/double/bool/char/octet, UTF16 strings, structs,
arrays, nested arrays and arrays of structs. Raw descriptors and small payloads
remain in hierarchical tags; large arrays retain checked source ranges.
`read_tag_raw` explicitly reads a chosen tag under the metadata budget. Image-associated
Data must have supported scalar-array storage and complete Dimensions; an
unsupported nested/structured main image fails instead of selecting an earlier
thumbnail. Non-image metadata arrays remain original tag payloads. Original
name bytes survive, including unnamed entries and non-ASCII names. Directory
entry count, depth, descriptor words/nesting, shape products and all payload
ranges have limits. Dimensions are indexed once rather than scanning the entire
directory for each candidate image. The retained-index metadata limit includes
image path/dimension storage.

Numeric image arrays with sibling Dimensions are indexed separately. The last
image is selected by default, matching the pinned file's main Data selection;
its thumbnail remains independently available through `select_image(index)`.
Source dimensions are fastest-axis first. Dimensions 0/1 become frame columns/
rows, with later dimensions flattened into consecutive stored planes. Native
Data dimensions are authoritative: Active Size and Binning metadata do not
resample or alter source pixels. This intentionally avoids the reference's
flattened-key metadata collisions. Bool values preserve raw i8 bits and their
source descriptor; character/octet arrays preserve u8 bytes.

Only DM3 version 3 is admitted. The header's historical declared-length
conventions must agree with actual source length (difference 0/12/16/20 bytes),
and a parsed directory can end in at most eight zero padding bytes. Unknown
encoded types, malformed nesting and arbitrary trailing bytes fail explicitly.
DM4, packed/RGB/complex image semantics and unsupported numeric extensions remain
gaps. Compound metadata is retained raw, not coerced to a numeric pixel type.

Existing readers were evaluated first. `dmio` 0.1.5 is LGPL-3.0 and was not
incorporated. MIT/Apache `openreadout-em` 0.4/0.3 requires Rust 1.91, newer than
this crate's Rust 1.89 baseline, and pulls a larger HDF5 stack; it was not added.
The independent parser follows the pinned FabIO tests and the original
[DM3 format description by EMAN contributors](https://wsr.imagej.net/ij/plugins/DM3Format.gj.html),
including two-byte Unicode strings and explicit struct/field-name byte lengths.
No GPL/LGPL implementation source is copied into production.

## Verification and reference limitations

The tiny independent CC0 fixtures cover every admitted SPE dtype, both MRC byte
orders/all six modes, integer extrema, float/complex reserved bits, all DM3
numeric descriptor types, separate thumbnails/main arrays, stacks, SPE regions,
padding/frame metadata and compound DM3 tags. Tests compare every sample's bits,
source hashes, shapes/order, metadata and explicit complex conversion. They
also reject truncation, malformed/unsupported layouts, bad descriptors/strides,
DTD XML, resource limits and changed sources. Optional dependencies remain absent
from the no-default-features build.

The external pinned FabIO 2026.6.0 corpus comparison includes all eight files:
`v2.SPE`, `v2_converted.SPE`, `v3.spe`, `v3_custom_roi.spe`, `v3_2frames.spe`,
`EMD-3001.map`, `0p67_5s_0000.mrc`, `ref_d20x_310mm.dm3`. All 33 declared frames
match native dtype, shape and canonical native sample hashes. The DM3 main image
is 2048x2048 f32, with a separate 384x384 i32 thumbnail. The MRC map has 25
sections. Large source data remains outside Git, with file/decode hashes in
`tests/data/containers/upstream-provenance.json`.

Two pinned reference API defects need deliberate test access: SPE's inherited
`nframes` stays one despite header `num_frames=2`, and MRC `getframe` raises an
AttributeError while setting deprecated dimensions on an empty image. The
reference generator calls fresh `Reader().read(path, frame=i)` for each declared
SPE/MRC frame. Production has explicit frame-index validation. These adaptations
do not change expected samples. Outer gzip/bzip2 staging is handled by the
separate wrapper API during integration, not by these readers. No performance
claim or universal variant support follows from the corpus comparison.
