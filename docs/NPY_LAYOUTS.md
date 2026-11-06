# Exact NPY bool, binary16 and empty arrays

The native NPY reader retains source dtype, original header bytes, complete
source shape and C/Fortran storage order. It returns original Bool storage bytes
and binary16 words through `PixelBuffer::Bool` and `PixelBuffer::F16`. Consumers
choose logical truth or floating conversion explicitly. Source NaN payloads,
signed zero, subnormals and noncanonical true bytes are not normalized.

The existing frame API keeps its explicit logical plane mapping: rank0 is one
1×1 plane, rank1 one 1×N plane, and rank2 or greater uses the trailing two axes.
Leading axes flatten in logical C order, independently of physical Fortran
storage. `source_shape` remains authoritative and is never replaced by this
consumer frame shape. Empty spatial axes produce valid empty planes; empty
leading axes produce zero frames. No zeros or other samples are invented for an
empty source.

The independent CC0 fixtures under `tests/data/npy-types/` use manually packed
NPY v1/v2/v3 headers, explicit dictionary text, little-endian header lengths and
alignment padding. NumPy's NPY writer and FabIO writers are not used. Payloads
come from hand-chosen binary16 words and Bool bytes 0/1/2/127/128/255, stored in
explicit C or Fortran order. The provenance manifest retains complete original
header/payload hex, dtype/endian, source/plane shapes and every expected logical
frame word. Unsupported dtype and malformed files have separate ledgers.

The [frozen proof](npy-types/2026-11-06-layout-types.json) passes all source,
executable and pinned-reference gates for **138 arrays and 228 logical planes**.
All arrays load independently with `numpy.load(..., allow_pickle=False)` and
match the original fixture words. Unchanged FabIO2026.6.0 `NumpyImage.read` and
frame access complete on **132 arrays**, with exact dtype, shape and all sample
bits matching. This includes noncanonical Bool bytes and Fortran layouts.

The remaining **6 reference reads fail** on valid shape `(2,2,0,3)` arrays, for
both types and all three versions. Pinned `slice_dataset` reshapes rank4 to
`(-1,0,3)`, which NumPy cannot infer from zero elements. The native reader uses
the explicitly stored leading dimensions and returns four empty planes. The
proof records the exact unchanged reference `ValueError`; these calls are not
counted as successful reference API parity. Other reference failures cannot use
that narrow qualification.

Seven rejection controls remain separate from admitted reading:

- Structured and string arrays are valid source dtype gaps; unchanged NumPy and
  FabIO read them, while the native scalar image API rejects them explicitly.
- A 16-byte floating descriptor is a valid platform-dependent NPY dtype variant.
  This macOS arm64 NumPy environment has no such floating dtype; native support
  and reference interpretation are unqualified. No numerical interpretation of
  its raw payload is claimed.
- The independently authored object file contains a benign None-array pickle.
  Native and reference `allow_pickle=False` reject it without unpickling. Object
  deserialization is not part of this native input API.
- Truncated payload, negative shape and bad magic are malformed source controls.

Four independent Python controls check every manual fixture with NumPy and
ensure changes to source shape, dtype, order, header, IEEE words or Bool bytes
fail comparison. Root Rust controls separately exercise the entire 138-case
manifest, resource limits, source changes and captured nonseekable input.
This work adds no production Python dependency or performance claim.
