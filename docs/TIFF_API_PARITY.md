# Low-level TIFF read APIs

The optional `tiff` feature exposes `tiff::api`. It reuses the existing TIFF
decoder, IFD reader, metadata adapter and explicit image views. No new sample
decoder, dependency or Python runtime is introduced.

| Reference read surface | Native API | Contract |
| --- | --- | --- |
| `getNumberOfImages()` | `get_number_of_images()` | Frozen primary IFD count; source changes error |
| `getImageFileDirectories()` and its `_IFD` list | `get_image_file_directories()` | Original ordered offsets; FabIO returns the count and stores offsets internally |
| `getInfo(index)` | `get_info(i64)` | Checked source metadata; no invented reference defaults inserted into source tags |
| `getData(index)` | `get_data_into(index, Rows, buffer)` | Exact native stored logical samples; palette indices stay indices |
| `getImage(index)` | `get_image_into(index, Rows, buffer)` | Alias for stored data |
| Palette / `mono_output` interpretation | `read_view(index, Rows, ViewPolicy)` | Explicit versioned display interpretation, separate from source samples |
| `close()` / context manager | `close(self)` / Rust ownership | Source handles are released; a consumed reader cannot reopen silently |

`api::Reader` wraps the existing sample reader and its admitted codec/layout
contracts. `Info` borrows the original `tiff::Metadata`, including source shape,
dtype/precision, endian, orientation, photometric and planar layout, typed tags,
opaque metadata, lossy-codec provenance and selected compatibility interpretation.
The underlying reader's existing component and specialized recovery APIs remain
available through `into_inner()`. Subsampled Y/Cb/Cr grids are not represented as
fake full-resolution scalar channels by this facade.

`api::MetadataReader` separately wraps `tiff::header::Reader`. Its same count,
directory and signed-info methods do not admit a pixel decoder. `DirectoryInfo`
borrows the original header page's IFD offset, typed tags and opaque tags. It can
index empty IFD chains and unsupported pixel layouts without claiming sample
availability. Its directory iterator borrows existing pages rather than
allocating a second offset list. Original primary IFD order is retained, including
a nonmonotonic `[512, 8]` chain; SubIFDs are not guessed as additional images.
Unknown field types/duplicate tags follow the existing strict header contract.

Reference `getInfo` dictionary names, defaults and presentation are projected
from source tags **only by the test helper**. For example, FabIO's missing
SampleFormat sentinel 4, `Unknown Date`, numbered descriptions, ColorMap
downscaling and parsed PyMca description fields are reference presentations.
They do not become fabricated original tags or a new source dtype. Invalid
descriptive text remains original opaque bytes in the native metadata; the
reference's replacement-character text is a separately identified presentation.

## Rows, views and resource bounds

`Rows::All` reads the full image. `Rows::Inclusive { first, last }` uses explicit
nonnegative inclusive source-row coordinates. Negative indices/rows, reverse
ranges, oversized endpoints and out-of-range image numbers error. They never
wrap or create zero-filled rows. Stored-data errors invalidate every public
frame-buffer getter before returning.

Cropping uses a full bounded decode followed by an in-place contiguous row
selection. The output has only the selected rows. This is not a partial-I/O
optimization or performance claim. Stored orientation is unchanged; tag 274
remains metadata. U16 extrema, signed I32 extrema, F64 negative zero and NaN
words are tested without numeric conversion during cropping.

`ViewPolicy::FabioTiffIo` reuses existing palette lookup and otherwise preserves
the decoded scalar array. `FabioTiffIoMono` explicitly applies the pinned
`.114*R + .587*G + .299*B` rule to color data and returns F32. These are the
literal low-level TiffIO weights, not Pillow's RGB luminance convention. Extra
components are ignored only by this requested mono view and are recorded.
F32 input follows NumPy's F32 scalar-promotion arithmetic; integer/F64 input uses
F64 arithmetic before the final F32 output. Original data and source metadata
are unchanged. Base image-view provenance is retained, with additional policy,
IFD offset, selected source-row range, mono and ignored-component operations.

The source frame plus the initial display output must fit `Limits.frame_bytes`
simultaneously. The source frame is then released; mono input plus its output
must also fit that aggregate bound. Cropping is in place. Decoder scratch has
the existing separate `intermediate_bytes` bound. Source metadata and the
sample reader's additional eight-byte IFD index entries are admitted under the
aggregate metadata budget. These are payload-allocation limits, not RSS limits;
existing upstream initial-IFD and codec qualifications still apply. Both readers
reject nonregular files before a blocking open, including FIFOs. Source identity,
open handle and caller path binding are checked on every public access. There
is no decoded-image cache or implicit growing-file refresh.

`Reader::open` retains strict admission. `open_with_options` exposes the
**existing** explicit source-window policies before reading. In particular,
`UncompressedWindows::FromShape` reads the complete legacy `a0009.tif` payload
without inventing an original StripByteCounts tag. Original descriptors and
effective windows remain in `Info.source.compatibility`. The helper selects this
policy only through explicit caller options captured by source SHA256 before
the first pixel probe; there is no retry after a failed default read. See
[legacy TIFF interpretations](TIFF_LEGACY_RECOVERY.md).

## Pinned reference qualifications

The consulted MIT reference is FabIO 2026.6.0 commit
`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`,
[`TiffIO.py`](https://github.com/silx-kit/fabio/blob/9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece/src/fabio/TiffIO.py),
SHA256 `761e09edb72380be24fb313a60f456b8e2f3d73c26acbe2ccbd02b5c88bdbb50`.
Its source notice attributes V. A. Solé/ESRF and the PyMca X-Ray Fluorescence
Toolkit, copyright ESRF 2004–2022, MIT. The original MIT/copyright notice remains
in the pinned source; the repository also retains the
[FabIO copyright manifest](../licenses/native-readers/FabIO-2026.6.0-copyright.txt)
and [MIT notice](../licenses/native-readers/legacy-layouts-MIT.txt).

The native API does not reproduce these reference behaviors:

- Negative image indices select from Python's list end; native signed indices
  explicitly reject them. This is a policy difference, not missing source data.
- Row selection can return a full image, zero rows outside selected strips or
  fail broadcasting instead of returning the requested contiguous rows. Native
  selected samples are checked against independent source-array spans.
- The single-strip path skips PackBits decompression. The independent valid
  literal-run fixture fails in the reference and decodes exactly natively.
- The single-strip palette path substitutes the derived RGB image byte count
  for the source index-byte count. With a valid ColorMap located after the pixel
  payload, it reads metadata as extra indices and fails broadcasting six rows
  into two. Native preserves four indices and produces twelve RGB values only
  under the explicit palette policy; both are independently verified.
- `getImageFileDirectories()` can seek a closed owned file after
  `getNumberOfImages()` closed it. Native metadata access remains checked and
  usable while its owned reader lives.
- Classic-only TiffIO rejects BigTIFF version 43. Native metadata and source
  samples are independently verified rather than altering the source version.

Source-window inconsistencies, malformed/truncated inputs, unsupported pixel
layouts and reference defects remain separate conditions. A metadata-only read
never proves pixels exist. No writer, object identity, cache mutation, implicit
refresh, arbitrary file-object adapter or missing-pixel padding is qualified.
The existing TIFF component/recovery and codec-integrity qualifications remain
in [image reader contracts](IMAGE_READERS.md).

## Same-file proof and controls

The [read-API report](tiff-api/2026-11-06-read-apis.json) runs the pinned unchanged
`test_tiffio.py`: **three tests, zero failures/errors/skips**. Its original writer
calls only create the upstream test fixtures; native writing is not claimed.
Five actual outer public read calls are observed without changing reference
objects or caches. Nested/fixture-only calls are counted separately. A capture
failure cannot change a reference return value or exception.

The report adds 166 independent calls across nine small CC0 fixtures, existing
CC0 F64/BigTIFF fixtures and four actual corpus files (`a0009.tif`, `fit2d.tif`,
`frame_00017.tif`, `indexed_color.tif`). Its 171 records comprise 83 matching
reads/queries, 28 matched rejections and 60 explicit reference/policy
qualifications. All 530 native metadata queries verify original tags and IFD
offsets against an independent TIFF metadata reader. Fifty-eight stored-array
reads and 58 explicit views verify dtype, shape and every canonical sample byte
against separate source/view calculations, including cases where TiffIO fails.
Source-window provenance descriptors and effective byte counts are checked
independently; both missing-bytecount files use preselected FromShape policies.

Ten Rust tests cover metadata-only layouts/empty chains, exact words and crop
controls, explicit palette/mono views, aggregate allocations, strict/default
source windows, identity changes and bounded FIFO rejection. Twelve Python
controls cover metadata omissions, dtype/bit fidelity, palette/index separation,
literal mono weights, stale errors, command correspondence, observer restoration
and noninterference, unexpected reference errors, source-window provenance and
the narrow palette defect classification. Reports retain source/import/helper,
build/probe and per-input hashes and require their before/after stability.

Reproduce with the pinned reference environment and the recorded native probe:

```sh
python -m unittest discover -s scripts -p test_tiff_api_reference.py
python scripts/tiff_api_reference.py --probe target/release/examples/api_probe \
  --fabio-source /tmp/dioptrin-io-fabio-2026.6.0 \
  --corpus /tmp/dioptrin-io-fabio-corpus \
  --from-shape /tmp/dioptrin-io-fabio-corpus/a0009.tif \
  --from-shape tests/data/tiff-api/missing-bytecounts.tif \
  --report /tmp/tiff-read-apis.json
cargo test --no-default-features --features tiff --test tiff_api
```
