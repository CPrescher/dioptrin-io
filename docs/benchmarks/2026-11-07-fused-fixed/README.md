# Fused fixed-mask Eiger integration

[Benchmark index](../index.md) · Historical measurement: 2026-11-07.
Results and implementation descriptions apply to the recorded source revisions.

This local implementation connects dioptrin-io's raw HDF5 block reader to
Dioptrin's fixed-mask CSC accumulator. It decodes all original integer counts
and uses float64 geometry weights, signal sums, normalization and Poisson terms.
The comparison is between native full-frame and fused block paths. It includes
no timing measurement of
[`jonwright/bslz4_to_sparse`](https://github.com/jonwright/bslz4_to_sparse).

## Single-image parallel decoding and integration

**Splitting one frame across eight cores substantially reduces median
read-to-pattern latency on this host.** Block fusion alone on one core is slightly
slower than the complete-frame path; parallel work within the frame provides the
large latency gain here.

The new run uses the same Eiger file, calibration, mask, bins, corrections and
float64 arithmetic as the initial comparison below. The compressed frame is read
once. Contiguous groups of codec blocks are decoded and integrated by private
workers. Their raw signal and variance histograms are merged in range order, and
normalization is applied once. No complete decoded image or float64 image is
assembled in the timed fused path.

| Single-image method | Intensity median / p95 (ms) | With Poisson errors median / p95 (ms) |
| --- | ---: | ---: |
| Complete image, 1-core decoding + 1-core integration | 28.49 / 28.96 | 33.67 / 34.31 |
| Complete image, 1-core decoding + 8-core integration | 16.73 / 17.37 | 17.31 / 17.77 |
| Fused blocks, one core | 30.71 / 31.92 | 35.47 / 36.03 |
| Parallel block API, one core | 30.85 / 31.61 | 35.60 / 36.46 |
| Fused blocks, eight cores within one frame | 5.38 / 6.18 | 6.09 / 6.88 |

Each cell summarizes 72 timed samples: three passes over the same 24 evenly spaced
frames, with method order rotated. Reading, decompression, thread launch/join,
private histogram initialization, orientation mapping, integration, merging and
normalization are included. Lookup construction and correctness comparisons are
outside the interval. This measures one image at a time, not eight images in
flight; reciprocal latency should not be confused with measured map throughput.

The complete-image eight-core control invokes the existing parallel CSC kernel
from a non-Rayon coordinator thread, using a global pool configured to exactly
eight compute threads before plan setup. This matters because the kernel skips
nested parallel execution inside a Rayon worker. Coordinator launch/join is
included; the compute pool is persistent. Decompression and image conversion
still run on one core before integration starts.

The parallel one-core row controls for the parallel API's framing and thread
launch overhead. The eight-core row uses exactly eight block workers, with no
outer frame-worker pool. A persistent compute pool could change startup costs;
these results include the current per-frame thread launch cost.

Before timing, every block from both the 1- and 8-core paths was compared exactly
with the full-frame decoder for all 625 images. That decoder had just passed all
625 independent h5py count hashes in the initial run. Both parallel integrations,
including Poisson errors, were compared with the full-frame float64 result. The
maximum scaled difference was 6.26e-15. All timed results were compared again
outside timing.

The API addition followed the initial throughput run; the two source snapshots
are recorded separately. [Single-frame raw timings](single-frame.json) and
[single-frame provenance](single-frame-provenance.json) identify this final code.
Reproduce with the build command below and append `single` to the run command,
using `single-frame.json` as the output path. The new run's lookup preparation was
0.778 s and file indexing 0.013 s, excluded from the per-image results. Other apps
were active, including indexing and browser/windowing work; p95 captures
some scheduling variation. These are local macOS measurements.

The eight-core fused path is **3.11 times faster** than full-frame decoding
followed by the existing eight-core integrator for intensity, and **2.84
times faster** with Poisson errors. Against the one-core complete-frame path,
intensity latency improves by **5.30 times**. The same cores now handle
both decoding and integration within the frame.

The earlier parallel-block run is retained as `single-frame-first.json` with its
own provenance and log. It gave similar results before the genuine eight-core
full-frame control was added. The full-frame control above runs outside a Rayon
worker so its existing parallel kernel actually uses the configured eight cores.

## Initial frame-throughput comparison

These measurements used the serial fused prototype before the single-frame
parallel API was added. Eight frame workers means eight images in flight; the
serial block accumulator does not split an individual image across those workers.
The medians and full three-pass ranges are:

| Output | Frame workers | Complete float64 image (fps) | Complete uint32 image (fps) | Serial fused blocks (fps) |
| --- | ---: | ---: | ---: | ---: |
| Intensity | 1 | 34.84 (34.83–34.85) | 34.03 (33.85–34.08) | 32.13 (32.08–32.14) |
| Intensity | 8 | 214.60 (190.18–220.40) | 243.69 (216.39–251.37) | 232.57 (227.98–237.13) |
| Intensity + Poisson | 1 | 29.49 (29.41–29.51) | 29.02 (28.88–29.03) | 27.65 (27.62–27.89) |
| Intensity + Poisson | 8 | 180.89 (175.86–203.47) | 210.36 (197.50–218.91) | 200.39 (196.51–208.19) |

At one frame worker, fusion alone regressed throughput. With eight frame workers,
it improved over the float64-image baseline in this run, while the complete
uint32 path was faster still. This does not support replacing all paths with
serial fusion. The main new result is the separate single-image latency gain
from parallel blocks.

The serial prototype retained 124.9 MiB of decoder/image payload at one worker,
versus 47.5 MiB with a complete uint32 image and 8.9 MiB fused. Eight serial-fused
frame workers retained approximately 70.8 MiB combined versus 999 MiB for the
float64-image baseline. These capacities exclude shared tables, consumer
histograms and output storage, and are not RSS or peak-memory measurements.

## Matched comparison

One Eiger 9M HDF5 file contains 625 uint32 frames of shape 3262 × 3108,
with full-frame Bitshuffle/LZ4 chunks. All paths read the same chunks, use the
same native NEON unshuffle and LZ4 decoder, and share one prepared lookup plan:
2000 bins, pixel splitting, two-theta degrees, polarization 0.965, the same
static mask and a vertical row flip into the calibration's image order.

| Path | Per-frame operations |
| --- | --- |
| `current-f64` | Read/decode complete uint32 image, flip/cast into reusable complete float64 image, existing full-frame CSC integration |
| `full-u32` | Read/decode complete uint32 image, new accumulator maps rows and integrates native integer samples |
| `fused-u32` | Read compressed frame, repeatedly decode one codec block and immediately accumulate, normalize once at completion |

`full-u32` separates avoiding the float64 image copy and changing the accumulator
from the benefit of codec-block fusion. All three preserve float64 integration
arithmetic; native integer input does not imply float32 weights. The baseline is
the native full-frame kernel using the same prepared tables, not the complete Qt
Dioptas or h5py application pipeline.

In the initial throughput run, one worker has one compute thread. Eight workers process different frames from
this same file using a shared immutable plan; these are not eight files, and they
do not split a single image over eight cores. Single-image latency is measured
separately with one worker and 24 evenly spaced acquired frames, rotating mode
order. Setup is excluded from latency/throughput and reported separately.

The throughput test makes three complete passes per mode, worker count and error
setting, rotating mode order. Timed work includes positional reads, decompression,
orientation/conversion where needed, accumulation, normalization, returned result
allocation, thread/buffer setup and collecting/sorting output patterns. Reference
comparisons are outside timing. Fixed mask creation and lookup construction are
excluded. The OS file cache is warm from validation; there is no decoded-image
cache. These are not cold-storage measurements.

## Reproduction and evidence

The standalone `benchmarks/fused-eiger` crate contains the complete callback bridge
and comparison harness. It requires Dioptrin commit
`72e9b72fa3ccaed19aa4ee09e3cb8bf002590a78` on `codex/fused-fixed-blocks`; the
published 0.9.0 crate alone does not contain these APIs. Build against that checkout:

```sh
cargo build --manifest-path benchmarks/fused-eiger/Cargo.toml --release \
  --config 'patch.crates-io.dioptrin.path="/absolute/path/to/modified/Dioptrin"'
benchmarks/fused-eiger/target/release/fused-eiger-bench \
  FILE PONI MASK_U8 COUNT_REFERENCE_JSON results.json 3
```

`MASK_U8` is the extracted calibrated mask, not the packed Fit2D mask file.
The acquired image data remain outside Git. [Provenance](provenance.json) records
input hashes, independent h5py count references, source hashes, base commits,
compiler, hardware and benchmark binary. [Raw results](results.json) contain every
pass and sampled latency, not only selected best results.

The host is an Apple M4 Pro with 14 CPU cores and 48 GiB RAM, running macOS 26.6.2.
Unrelated applications were active (a Node process around one core plus browser
and windowing activity). Repeated-pass spreads are part of the result; this run
does not establish an uncontended peak or Windows/Linux performance.

## Reader and consumer contract

Every full-frame and streamed decoded frame is checked against its independent
h5py SHA256 reference, including original reserved counts. Every integration
validation and timed output is compared with the existing full-frame float64
kernel, including Poisson errors when requested, at absolute tolerance 1e-10 plus
relative tolerance 2e-12. Reduction order can change float64 rounding.

The decoder's callbacks are provisional until the complete read, including the
final source-snapshot check, succeeds. The bridge aborts the accumulator on any
read or consumer error. It never publishes an incomplete pattern. Unit tests
also exercise corrupt later blocks, source changes, callback cancellation,
invalid block sequences, row orientation, high integer counts, static masks,
worker reuse and shared immutable plans.

Retained decoder/image payload is the sum of compressed buffer, typed decoded
storage, codec scratch and full float64 image capacity across workers. It excludes
the shared lookup, histograms, output vectors, allocator overhead and application
state; it is not a process RSS measurement. Fused decoding still retains one
compressed frame per worker.

This is an opt-in Rust API implementation on local branches. The calling Rust
code chooses it explicitly using `prepare_fixed1d` and `fold_frame_blocks`;
standard integration calls do not automatically select it. There is no new
Dioptas/Python user setting or automatic batch/map routing. The fused integration
plan enforces fixed-mask mode and must be recreated when geometry, corrections
or the mask change. Dynamic masks keep their existing complete-frame path.

Each fused call integrates one frame. It can be reused for successive frames in
one HDF5 file, or for compatible frames from other files. The measured multicore
speedup is parallelism within one frame, rather than a restriction to single-file
jobs. No application integration or published package changes are included.
