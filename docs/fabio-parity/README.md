# FabIO suite continuation, 2026-11-05: MAR345, sparse and explicit views

**Full reading parity remains incomplete.** The ledger retains all 36 registry
entries / 35 unique codecs. The full pinned suite runs 213 reference tests with
no failures/errors and one unconditional upstream skip (the slow MAR345
memory-leak test). OpenJPEG 2.5.4 exercises the optional Glymur variants.

| Native outcome | Read events | Compared frames |
|---|---:|---:|
| `counts_match` under recorded format policy | 439 | 1,563 |
| Explicit display/inverse-encoding view matches | 22 | 22 |
| Explicit deterministic sparse reconstruction matches | 1 | 100 |
| Mismatch | 2 | 2 |
| Native input/layout rejection | 91 | — |
| Reference returns no pixels | 25 | — |
| Missing decoder / test adapter / optional feature | 0 | — |

The 462 matching events span 248 distinct source-file hashes. Events include
repeated reads and are not distinct files. The new TIFF/Bruker views record raw samples separately, and sparse modeled
backgrounds have their own status. The 439 `counts_match` events retain the
existing explicit R-AXIS expanded-intensity (three events) and KCD sum (four
events) policies, recorded on each read; they are not all default source-word
reads. Ordinary readers preserve those original words/readouts, independently
tested in the scoped reader fixtures. Compared with the historical
parallel-reader report, matching events increase from 413 to 462, missing-decoder
events fall from 31 to zero, mismatches from 13 to two, and native rejections
from 98 to 91. This comparison crosses schema versions: schema 3 canonicalizes
logical bool bytes to 0/1 and records explicit reference views separately.

The [full report](2026-11-05-mar-sparse.json) records every source, executable,
bridge/inventory and reference-codec hash. Production sources, comparison bridge
and copied executable remained unchanged during the run. Every installed
reference codec matches the pinned checkout. There are no comparison errors,
timeouts, frame-count mismatches or source-component mismatches. Compared
EDF/d*TREK/Pilatus/MAR header fields agree; complete metadata schemas remain a gap.

## What changed

- MAR345/MAR555 PCK V1: all 30 exposed upstream reads match. Independent fixtures
  cover both byte orders, unsigned first rows, 32-bit residual fields, original
  overflow words and whitespace payloads, including documented pinned-oracle
  defects. V2/raw variants remain explicitly unsupported. The bounded adaptation
  retains its MIT attribution without incorporating the LGPL codec.
- Sparse HDF5: all eight original numeric components, named aliases, frame spans
  and event words match; the explicit native `NOISY=False` model matches all 100
  dense f32 frames bit for bit. **The actual file has zero stored events.**
  Independent nonempty/extreme-event fixtures provide that separate evidence.
  Stochastic reconstruction and the distinct NumPy fallback remain unqualified.
- d*TREK rank1..32 source arrays now expose source-ordered 2D planes, retaining
  their complete logical shape.
- Bruker LINEAR float encoding and TIFF Pillow13/TiffIO display conversions have
  explicit views. Ordinary readers retain source words, channels, palette indices,
  ColorMap, packed values, photometric metadata and stored orientation. CFA raw
  samples and narrowly quarantined legacy description bytes are now readable.
- Every TIFF pixel codec is restricted to declared source chunk windows, with
  regression coverage for metadata aliases, padded edge tiles, packed/F16 rows,
  truncation, repeated failures and intermediate-reader buffering.

The [minimal-feature proof](2026-11-05-mar-sparse-feature-disabled.json) executes
42 reference tests with no failures/errors and one upstream skip. It reports
117 feature-disabled events and three direct matches, with zero missing decoders
or adapters. Four reference rejection calls are also correctly feature-disabled.

## Differences that remain

The two mismatches are Pillow13 planar16 RGB/CMYK display defects. Independent
source decodes agree with native stored channels; the helper rejects the defective
view rather than reproducing its alternating-byte layout. They remain visible
mismatches. See [the source/view evidence](../tiff-views-fabio-2026.6.0.json).

| Rejected family | Events | Current boundary |
|---|---:|---|
| EDF | 68 | 21 truncated payloads, one truncated header, three incomplete Deflate streams, 42 invalid/truncated declared windows, one unsupported TY1 stream |
| TIFF | 19 | Nine short declared windows; malformed tags/layouts/LZW and unsupported old JPEG, YCbCr, FillOrder/compression variants |
| Lambda | 2 | VDS/external raw mappings require an explicit source contract |
| Oxford | 2 | TY5 lacks an independently reliable reference |

The nine short-window events are repeated raw/gzip/bzip2 reads of
`Feb09-bright-00.300s_WAXS`: its 1042×1042 u16 pixels require 2,171,528 bytes,
while StripByteCounts declares 1,085,764. The payload exists, but decoding beyond
that declared window is not silently authorized. Source SHA256 is
`89ed2cf5f3f3dd31442bd4eaa2516b0ed860fe88b868406ba7110bbcf72a2733`.
The reused TIFF Deflate decoder can still accept a missing footer after producing
all requested pixels; bounded output is not a full compressed-integrity claim.

Seventeen reference input rejections agree with native rejections. One deliberate
missing-Python-JPEG-backend test is an environment difference. No rejection
comparison fails or substitutes a source. Xcalibur is the sole unexercised codec:
its pinned `.read` fabricates placeholder zeros; its real CCD-characteristics/mask
API remains a separate target. Native autodetection, direct stream APIs, complete
semantic metadata, neighboring-file series and unsupported variants keep the
full-parity gate red.

## Validation

[Recorded local checks](2026-11-05-mar-sparse-native-checks.json): all 124 Rust
tests pass, including opt-in actual-file container/fixed/Esperanto/MAR345 checks.
Formatting and strict all-target/all-feature Clippy pass. No-default check/tests
and isolated strict all-target Clippy for each of 20 optional feature selections
pass (22 feature checks). Fixtures total under 2 MB, including source/reference
arrays and manifests; large detector files remain external. Expanded platform CI,
publication and new throughput claims are not established.

## Per-codec successful and rejected reads

| Codec | Direct match | Additional explicit view | Derived reconstruction | Mismatch | Rejected | No pixels |
|---|---:|---:|---:|---:|---:|---:|
| GEimage | 6 | 0 | 0 | 0 | 0 | 0 |
| HiPiCimage | 6 | 0 | 0 | 0 | 0 | 0 |
| OXDimage | 10 | 0 | 0 | 0 | 2 | 0 |
| binaryimage | 2 | 0 | 0 | 0 | 0 | 0 |
| bruker100image | 9 | 0 | 0 | 0 | 0 | 0 |
| brukerimage | 15 | 1 | 0 | 0 | 0 | 0 |
| cbfimage | 17 | 0 | 0 | 0 | 0 | 0 |
| dm3image | 6 | 0 | 0 | 0 | 0 | 0 |
| dtrekimage | 17 | 0 | 0 | 0 | 0 | 1 |
| edfimage | 133 | 0 | 0 | 0 | 68 | 0 |
| eigerimage | 7 | 0 | 0 | 0 | 0 | 0 |
| esperantoimage | 9 | 0 | 0 | 0 | 0 | 0 |
| fit2dimage | 4 | 0 | 0 | 0 | 0 | 0 |
| fit2dmaskimage | 8 | 0 | 0 | 0 | 0 | 0 |
| fit2dspreadsheetimage | 3 | 0 | 0 | 0 | 0 | 0 |
| hdf5image | 6 | 0 | 0 | 0 | 0 | 0 |
| jpeg2kimage | 8 | 0 | 0 | 0 | 0 | 0 |
| jpegimage | 3 | 0 | 0 | 0 | 0 | 0 |
| kcdimage | 4 | 0 | 0 | 0 | 0 | 0 |
| lambdaimage | 10 | 0 | 0 | 0 | 2 | 0 |
| limaimage | 6 | 0 | 0 | 0 | 0 | 0 |
| mar345image | 30 | 0 | 0 | 0 | 0 | 0 |
| marccdimage | 6 | 0 | 0 | 0 | 0 | 0 |
| mpaimage | 1 | 0 | 0 | 0 | 0 | 0 |
| mrcimage | 8 | 0 | 0 | 0 | 0 | 0 |
| numpyimage | 12 | 0 | 0 | 0 | 0 | 0 |
| pilatusimage | 13 | 0 | 0 | 0 | 0 | 0 |
| pixiimage | 6 | 0 | 0 | 0 | 0 | 0 |
| pnmimage | 2 | 0 | 0 | 0 | 0 | 0 |
| raxisimage | 3 | 0 | 0 | 0 | 0 | 0 |
| sparseimage | 0 | 0 | 1 | 0 | 0 | 0 |
| speimage | 42 | 0 | 0 | 0 | 0 | 0 |
| tifimage | 22 | 21 | 0 | 2 | 19 | 24 |
| xcaliburimage | 0 | 0 | 0 | 0 | 0 | 0 |
| xsdimage | 5 | 0 | 0 | 0 | 0 | 0 |

ADSC aliases d*TREK. Historical reports remain unchanged:
[original baseline](2026-11-05.json), [typed HDF5](2026-11-05-hdf5.json),
[parallel readers](2026-11-05-parallel-readers.json) and its
[validation record](2026-11-05-native-checks.json). Complete targets and
reproduction remain in [the full ledger](../FORMAT_COVERAGE.md) and
[the comparison guide](../FABIO_PARITY.md).
