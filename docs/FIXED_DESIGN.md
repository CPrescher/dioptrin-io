# GE and Pixi fixed-frame input

The optional `fixed` feature depends only on the existing `native` feature.
`ge::Reader` and `pixi::Reader` expose `open(path, Limits)`, `metadata()`,
`frames()` and `read_frame_into(index, &mut FrameBuffer)`. They use the existing
native typed buffer/byte-conversion helper and immutable-source snapshots.
Every index/read checks path and open-handle identity, source length and
modification metadata before and after I/O. Failed reads invalidate public
output. Original stored rows, reserved values and unsigned extrema survive.
These checks detect ordinary changes, not adversarial writes preserving all
source metadata. No decoded count cache or performance claim is introduced.

## GE a-Si Angio

Standard ADEPT version 8001 binary headers admit u8/u16/u32 in little-endian
storage. Declared rows, columns, depth, frame count and standard/user header
lengths must exactly describe the source. The full standard/user header is
retained, including unparsed bytes. All named fields in the existing reference's
3332-byte descriptor table are exposed as exact byte strings or native numeric
values. Swapped, Reordered and detector/image flip flags stay metadata; they
never change the sample order. Unknown standard versions/depths and unexplained
trailing bytes fail explicitly.

Two APS conventions are supported through an explicit metadata `HeaderKind`:
`ApsBlank8192` requires a completely zero 8192-byte header and derives the exact
number of complete 2048x2048-u16 frames from source length. It does not repair
partially blank/nonzero metadata. `ApsEdf8192` retains the original 8192-byte EDF
header and Latin-1 assignments. Num_Images must match the complete source and
header size/dimensions/type/byte-order/per-frame Size must agree with that APS
layout. Other EDF-in-GE layouts require a separately established source contract.
General EDF reading remains the separate EDF reader's responsibility.

The adapter retains provenance of derived blank/EDF layout fields through
`Metadata.kind`. It also exposes original standard/user header lengths, shape,
frames, native dtype, original header bytes, typed binary fields and EDF fields.
Metadata strings/field indexes and source payloads are budgeted; library and
allocator overhead remain additional process memory. GE does not call Python,
apply corrections or inspect neighboring files.

## PiXI / Pixirad

Each record contains a 24-byte header and 512 rows of 476 little-endian u16
samples. Its leading signed-i32 size word must equal 243722 (the number of
16-bit words after the leading four bytes). Every record is checked at indexing,
including later frame headers; every opaque 24-byte header remains in metadata.
The file must contain an integral positive number of complete records. Partial
last frames, trailing bytes, invalid later headers and oversized indexes fail.

Frame indices address this file only. Names containing `$0000` are ordinary
explicit source paths. The reference's next/previous/getframe traversal into
separately named series files is not implicit in this API. Callers may explicitly
open each series member. Automatic series discovery/source-graph snapshots and
interpretation of unknown record-header words remain gaps.

## Reader reuse, licensing and verification

Registry searches for GEimage/Pixirad detector readers found no reusable
permissively licensed native backend. The small fixed layout adapters reuse
existing bounded sample conversion and snapshot validation. GE's header table
is adapted from the pinned MIT FabIO GE_HEADER_INFO definition. Its original
APS/ESRF copyright, authors and MIT notice are retained in
`licenses/native-readers/FabIO-GE-Pixi-MIT.txt`; the full pinned FabIO distribution
notice also remains. There is no GPL/LGPL decoder or Python production runtime.

Independent CC0 GE fixtures cover all three depths, counts above signed ranges,
multiple frames and nonzero orientation flags. Independent Pixi one/three-frame
sources are created at test runtime to avoid committing megabyte-size files.
Complete source/sample hashes were independently generated and compared against
FabIO. Tests exercise header retention, frame ordering, exact raw counts, APS
blank/EDF provenance, invalid later record headers, malformed/truncated layouts,
resource limits and source mutation. These are correctness checks, not throughput
benchmarks.

Actual pinned upstream files were compared independently: GE_aSI_detector_image_1529,
GE_image_1frame_intact_header.ge, GE_image_1frame_blanked_header.ge and
dark_before_000428.edf.ge5 (all ten frames), totaling 13 GE frames. Five files were
copied directly from the pinned TestPixiImage.create_fake_images routine: its
single/three-frame files and three explicitly named series members, totaling
seven Pixi frames. All 20 frames match native dtype, shape and complete canonical
little-endian sample hashes. Source and decode hashes are retained in
`tests/data/fixed/upstream-provenance.json`; original large files remain external.
Outer gzip/bzip2 support is provided by the separate wrapper API at integration.
