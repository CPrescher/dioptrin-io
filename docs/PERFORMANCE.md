# Performance, image density and CPU backends

[Manual index](index.md)

A processing rate is meaningful only with its data, hardware and measured steps.
Reading compressed images, creating masks and integrating pixels can each limit
a batch. A fast decoder rate does not give the complete map-integration rate.

## Reading and integration

A typical workflow has these stages:

| Stage | Work |
|---|---|
| Read and decode | Read file bytes and reconstruct the original pixel values |
| Prepare the image | Apply the application's orientation and corrections, and create any per-frame mask |
| Integrate | Accumulate pixel contributions using the calibrated geometry and lookup table |
| Produce results | Calculate normalized intensities, requested uncertainties and output arrays |

Dioptrin handles integration. Its current Python array interface accepts
C-contiguous two-dimensional `float64` arrays. An application can read those
arrays using h5py; see the [Dioptrin HDF5 guide](https://dioptrin.readthedocs.io/en/latest/HDF5_USAGE.html).

The separate Rust library **dioptrin-io** provides native file reading. Its
indexed HDF5 `RawReader` supports admitted immutable snapshots, including
full-frame uint16/uint32 Bitshuffle/LZ4 chunks. It indexes chunks through HDF5,
then performs positional file reads and checked decoding in frame workers
outside the HDF5 mutex. Other layouts and filters have separate reader contracts;
this fast path does not imply support for every HDF5 file or live acquisition.

The reader preserves native pixel types, reserved values and stored orientation.
The consumer owns orientation changes, masks, corrections and integration. A
consumer must opt into dioptrin-io; installing Dioptrin or choosing an integration
layout does not automatically switch its file reader.

## Codec speed at a glance

For the supported Eiger HDF5 chunks, decoding has two stages: LZ4 decompression,
then **bit-unshuffle**, which restores the original arrangement of the integer
bits. Rust and native LZ4 are alternative implementations of the first stage.
Scalar, SSE2, AVX2, NEON and KCB are implementations of the second stage. KCB is
Kal Conley's alternative Bitshuffle implementation; it preserves the same counts.

### Acquired Eiger9M file on Apple M4 Pro

This comparison read and decoded all 625 uint32 frames of shape 3262 × 3108.
It used eight frame workers and five complete-file passes per configuration,
with alternating/rotating order, a warm OS file cache and no decoded-image cache.
The rates include file reads, decoding and worker/buffer setup, and exclude
source indexing, integration and correctness hashing.

| Bit-unshuffle + LZ4 configuration | Median fps (full range) | Paired speedup | Availability |
| --- | ---: | ---: | --- |
| NEON + checked Rust LZ4 | 746.2 (736.1–756.2) | 1.000× | Explicit Rust override; default at the measured revision |
| NEON + bounded native LZ4 | 804.3 (785.8–807.4) | 1.075× | Current default on macOS ARM64 |
| KCB + checked Rust LZ4 | 580.4 (562.4–597.5) | 0.773× | Preserved experiment |
| KCB + bounded native LZ4 | 615.1 (586.0–628.3) | 0.819× | Preserved experiment |

Paired speedups are medians of the baseline/candidate time ratios within each
pass; they need not equal ratios of the FPS medians. On this acquired file,
native LZ4 improved read/decode throughput by **7.5%**. KCB reduced it by about
**23%** against the current optimized NEON path, so KCB remains experimental.

### Effect on the complete integration workflow

Decoder speed is only one part of producing a diffraction pattern. With the same
source, mask, calibration, float64 weights and 2000-bin integration plan, switching
from Rust LZ4 to native LZ4 produced these paired median speedups:

| Complete read-to-pattern workload | Native LZ4 speedup | Interpretation |
| --- | ---: | --- |
| Complete uint32 frame, intensity only; eight frame workers | 0.999× | Essentially neutral median; individual passes were mixed |
| Complete uint32 frame, intensity + Poisson std; eight frame workers | 1.026× | Modest improvement in all five matched pairs |
| One image at a time; eight block workers within that image | 1.016× intensity / 1.022× with Poisson std | Modest latency improvement; separate from whole-file throughput |

All original counts matched the independent h5py references for every codec
configuration. Timed consumer outputs were bit-identical between configurations
for each workload. Following the dense, uint16 and small-frame qualification
below and the subsequent cross-platform CI comparison, **native LZ4 is the
default on the five qualified targets** listed below. Unqualified targets retain
checked Rust LZ4. Complete-workflow gains still depend on the workload.
Automatic bit-unshuffle selection remains AVX2 when supported on x86-64, and
NEON on the tested ARM64 builds.

The [full comparison](benchmarks/2026-11-11-codecs/README.md) records exact source
revisions, hardware, input provenance, validation and every timing. The Windows
table below compares Scalar, SSE2, AVX2 and NEON on matched synthetic inputs.
Compare configurations within each table: the Apple and Windows measurements
use different hosts and input distributions.

## Sparse and dense images

Here, **sparse** describes an image with many zero-valued pixels. **Dense**
describes an image in which most pixels contain nonzero counts. These words do
not by themselves specify a storage type or a compression ratio.

The Windows benchmark below uses two deterministic synthetic patterns:

| Pattern | Pixel values |
|---|---|
| Sparse | About 99.22% zeros; about 0.78% nonzero counts, from 1 to 2500 |
| Dense | Counts spread across 0 to 65,535, so almost every pixel is nonzero |

Each frame also contains deliberately inserted zero, `2^31` and `2^32 - 1`
values to check preservation of high bits and reserved values. **Both patterns
are stored and decoded as full uint32 images.** The dense pattern's 16-bit value
range does not mean it was stored as uint16 or converted to float32.

Both use 3262 × 3108 pixels, matching the Eiger9M image dimensions used in our
other measurements. The patterns do not reproduce the intensity distribution,
module gaps or other characteristics of a particular acquired scan.

Different pixel distributions produce different Bitshuffle/LZ4 streams.
Compressed size alone does not determine decoding cost. In these measurements,
the dense case decoded faster even though its compressed file was larger.

Image density is also separate from the **CSR/CSC integration lookup layout**.
That lookup table stores geometry weights connecting pixels to output bins.
An image can have nonzero counts in nearly every pixel and still use a sparse
integration matrix. Changing the decoder backend does not change those weights.

## Scalar, SSE2, AVX2 and NEON

The runtime backend-selection API in this section is available on `main`, which
includes revision `9c780a9`. It is newer than the `v0.1.0` source release; pin
that revision or a later descendant when using these APIs.

These are implementations of the same bit-unshuffle operation in dioptrin-io.
They reconstruct the same integer pixels; selecting SIMD does not reduce
numerical precision.

- **Scalar:** portable C operations. The compiler can still optimize or
  automatically vectorize parts of this code.
- **SSE2:** explicit 128-bit SIMD operations for x86 CPUs.
- **AVX2:** explicit 256-bit SIMD operations for supporting x86 CPUs.
- **NEON:** explicit 128-bit SIMD operations for ARM CPUs, including Apple
  Silicon and supported Windows/Linux ARM64 builds.

SIMD applies an operation to several packed values in one instruction. The C
language is used for both the scalar and explicit SIMD kernels; the difference
is the instructions used and how the data is arranged for them.

### Automatic selection and distribution

The indexed raw reader defaults to **Auto**:

| Build | Available decoder backends | Automatic selection |
|---|---|---|
| x86-64, Intel/AMD | Scalar, SSE2, AVX2 | AVX2 when CPU and OS support it; otherwise SSE2 |
| ARM64 | Scalar, NEON | NEON on the tested macOS, Linux and Windows builds |
| Other supported builds | Depends on compiled capabilities | Scalar when no supported SIMD backend is available |

AVX2 code is compiled separately. Baseline code does not require AVX2, and the
reader checks CPU and OS support before selecting it. An explicit unsupported
backend request returns an error. Older x86-64 computers can therefore use the
same architecture-specific distribution through SSE2. Users do not install a
separate package for each kernel.

This selection belongs to **dioptrin-io's indexed raw reader**. Its general
HDF5 reader uses the baseline kernel under the HDF5 mutex; the tested Windows
builds use SSE2 or NEON there. There
is no corresponding `AVX2` option on Dioptrin's Python `Integrator`.

For Rust consumers using the reader revision described below:

```rust
use dioptrin_io::hdf5::{
    Limits,
    raw::{RawReader, UnshuffleBackend},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Acquisition has stopped; the source must remain immutable.
    let reader = RawReader::open_snapshot(
        "scan.h5", "/entry/data/data", Limits::default(),
    )?.with_unshuffle_backend(UnshuffleBackend::Auto)?;
    println!("Selected decoder: {:?}", reader.unshuffle_backend());
    Ok(())
}
```

Explicit `Scalar`, `Sse2`, `Avx2` and `Neon` selections are useful for matched
comparisons. Each frame worker reuses its compressed, pixel and block scratch
buffers. It does not allocate a new unshuffle temporary for every codec block.

### Workers and SIMD are separate

A worker processes a frame; SIMD processes packed values within that worker.
Eight workers do not mean eight physical CPU cores. More workers than allocated
CPUs can oversubscribe a machine, and throughput may stop increasing before all
cores are used because another processing stage or shared resource limits it.

## LZ4 defaults, fallback and override

`RawReader` uses these fixed general defaults, chosen from the completed
[CI comparison](https://github.com/CPrescher/dioptrin-io/actions/runs/43180447512):

| Target | Automatic bit-unshuffle | Default LZ4 |
| --- | --- | --- |
| Linux x86-64 | AVX2 when supported; otherwise SSE2, then scalar | Native |
| Windows x86-64 | AVX2 when supported; otherwise SSE2, then scalar | Native |
| macOS ARM64 (Apple Silicon) | NEON when available; otherwise scalar | Native |
| Linux ARM64 | NEON when available; otherwise scalar | Native |
| Windows ARM64 | NEON when available; otherwise scalar | Native |
| Other targets, including untested macOS x86-64 | Available automatic kernel | Rust |

These are general defaults, not a claim that one combination wins every image.
No runtime benchmark or image-content probe is performed. Worker counts remain
a consumer choice constrained by the processing workload and memory budget.
Native LZ4 won the large sparse and full-width dense cases on all five runners.
SSE2's small uint16 advantages varied by runner, while AVX2 clearly improved
uint32 decoding, so automatic SIMD dispatch is retained. The
[retained CI comparisons](benchmarks/2026-11-12-codec-qualification/ci-platform-comparisons.json)
include every automatically selected kernel's LZ4 ratio and range; all 528
original-count validation passes matched.

Native uses the already bundled
`LZ4_decompress_safe`, with exact input/output bounds and decoded-length checks.
Bit-unshuffle selection remains independent: this change keeps the optimized
NEON kernel on Apple Silicon.

If `Limits::chunk_bytes` exceeds `i32::MAX`, automatic reader selection falls
back to Rust, preserving the existing admitted size range. This is a conservative
per-reader decision based on the configured encoded-chunk/decoded-block budget,
not the total file size. An explicit native override still rejects individual
encoded or decoded blocks outside the signed C API range.

Rust consumers can force either decoder and inspect the selected implementation:

```rust
use dioptrin_io::hdf5::{Limits, raw::{Lz4Backend, RawReader}};

let reader = RawReader::open_snapshot("scan.h5", "entry/data", Limits::default())?
    .with_lz4_backend(Lz4Backend::Rust); // Portable override on Apple Silicon too.
assert_eq!(reader.lz4_backend(), Lz4Backend::Rust);
```

### Dense, uint16 and small-frame qualification

The [November 12 qualification](benchmarks/2026-11-12-codec-qualification/README.md)
used known original uint16/uint32 counts, dense full-width values, sparse images,
8×8 and 64×64 frames, and 65×63 frames with codec tails. On the M4 Pro, nine
alternating matched pairs with automatic NEON and one/four workers gave:

| Synthetic workload | Native/Rust paired median, 1 worker | 4 workers |
| --- | ---: | ---: |
| uint16 sparse, 1024×1024 | 2.692× | 2.788× |
| uint16 dense, 1024×1024 | 1.147× | 1.143× |
| uint32 sparse, 1024×1024 | 3.910× | 4.056× |
| uint32 dense full-width, 1024×1024 | 1.287× | 1.180× |
| uint32 dense 16-bit range, 1024×1024 | 1.042× | 1.033× |
| All six small/tail cases | 0.989–1.077× | 0.981–1.063× |

Every original-count check matched. Large workloads favor native LZ4. Small
frames are largely limited by reading/source checks and show substantial sample
noise; these runs do not establish a small-frame speedup or a repeatable material
regression. The raw samples and ranges are retained in the report. Together with
the acquired Eiger and complete-consumer comparisons, this supports the Apple
Silicon default. It does not establish the best default for all CPUs or files.

### Benchmarks are manual only

**Routine push and pull-request CI does not run performance benchmarks. There is
no scheduled benchmark.** Count/correctness, build, lint and consumer checks stay
in routine CI. Existing measurements are retained and are not recalculated for
each change.

The separate [Codec benchmark (manual) workflow](https://github.com/CPrescher/dioptrin-io/actions/workflows/codec-benchmark.yml)
can be explicitly dispatched if a future codec change needs qualification. It
compares Rust/native LZ4 and every locally available scalar/SIMD kernel on the
five runner types, using five short matched samples per configuration and one
and up to four allocated workers. It publishes per-workload job summaries and
`codec-benchmark-<runner>` raw/summary JSON artifacts. It never changes defaults
automatically. Count mismatches or incomplete results fail that manual job;
timing differences do not, because shared-runner load can distort small gains.

The [first codec CI run](https://github.com/CPrescher/dioptrin-io/actions/runs/43178638558)
also illustrates a workload-specific exception. For dense 16-bit-range values
**stored as uint32**, native/Rust paired medians were 0.902×/0.927× on the virtual
M1 (one/three workers), and 0.937×/0.931× on Linux ARM64 (one/four workers).
Rust was faster in these cases, while native won the large sparse and full-width
dense cases on both hosts. Linux x86-64 favored native for this pattern too
(1.042×/1.067×). The native defaults are therefore general choices based on the
qualified workloads; use the Rust override when your own data favors it.
Reproduce a short run, or a longer Auto-kernel LZ4 comparison, with:

```sh
cargo run --locked --release --features hdf5 --example benchmark_codecs -- codec-benchmark.json
python scripts/report_codec_ci.py codec-benchmark.json codec-benchmark-summary.json
cargo run --locked --release --features hdf5 --example benchmark_codecs -- qualification.json --qualify
```

The older Windows SIMD benchmark below remains a historical comparison with Rust
LZ4 fixed. Both benchmark examples remain manually runnable; routine CI runs
correctness checks without repeating these performance measurements.

## Windows read/decode measurements

Pin the measured development revision when reproducing these results; the
earlier `v0.1.0` tag does not include the decoder changes measured here.


These results measure **file reading plus decoding only**, with original uint32
pixel delivery. They exclude masking, image conversion, normalization and
integration. They are not Dioptrin integration or complete Dioptas map timings.

Source: dioptrin-io commit
[`9c780a9`](https://github.com/CPrescher/dioptrin-io/commit/9c780a9f8131088df55d0a6836a526fa44ee6d01),
[successful CI run](https://github.com/CPrescher/dioptrin-io/actions/runs/42338595936),
7 November 2026. The
[reader benchmark method](https://github.com/CPrescher/dioptrin-io/blob/9c780a9f8131088df55d0a6836a526fa44ee6d01/docs/benchmarks/2026-11-07-windows-simd/README.md)
and [benchmark source](https://github.com/CPrescher/dioptrin-io/blob/9c780a9f8131088df55d0a6836a526fa44ee6d01/examples/benchmark_windows_simd.rs)
identify the exact workload and API. Check that your deployed reader includes
these changes; a package name or version alone does not establish its backend.

Each synthetic file contains 32 different frames. Each timed pass reads and
decodes four scans of that file, without caching decoded pixels. Five passes
alternate backend order; the table reports their median fps. Original counts
are verified separately for every frame, backend and worker count before timing.
All count checks match. The files are in the warm OS cache, and the CI machines
are shared rather than isolated.

| Windows CPU / backend | Sparse: 1 worker | Sparse: 8 workers | Dense: 1 worker | Dense: 8 workers |
|---|---:|---:|---:|---:|
| x86-64 / Scalar | 18.97 | 51.17 | 33.70 | 78.20 |
| x86-64 / SSE2 | 25.45 | 75.50 | 61.36 | 144.70 |
| x86-64 / AVX2 | 28.89 | 90.25 | 87.45 | 198.97 |
| ARM64 / Scalar | 19.49 | 73.63 | 26.82 | 98.44 |
| ARM64 / NEON | 23.67 | 88.72 | 35.21 | 126.62 |

Both Windows runners had **four allocated CPUs**, so the eight-worker passes
oversubscribed that allocation. x86-64 and ARM64 used different machines;
compare backends within an architecture, not CPU families or operating systems
from their absolute rates. These synthetic results do not establish throughput
for the original acquired Eiger file, a cold SSD or a complete integration job.

The archived [x86-64 results](benchmarks/2026-11-07-windows-io/windows-x64-final.json)
and [ARM64 results](benchmarks/2026-11-07-windows-io/windows-arm64-final.json)
include every pass, ranges, expected frame hashes and validation records.
[Provenance](benchmarks/2026-11-07-windows-io/provenance-final.json) retains the
source commit, CI run and verified artifact/file hashes.

## Comparing performance on your data

Keep the same input frames, pixel type, orientation, masks and output settings.
State whether the measurement includes file reads, decoding, copies, dynamic
mask creation, integration, normalization and uncertainty calculation. Report
lookup-table/setup time separately when it is excluded from a steady-state rate.

Also record the CPU, worker count, reader/backend, compression/filter layout,
image size, cache state and the range of repeated measurements. Compare scalar
and SIMD on the same machine. Use acquired data to estimate the gain for an
actual experiment; synthetic patterns qualify a controlled workload.

## Combined loading and fixed-mask integration in Qt

Fused block decoding can integrate one frame while its independent codec blocks
are decoded on several cores. The [native Eiger report](benchmarks/2026-11-07-fused-fixed/README.md)
records 5.38/6.09 ms (intensity/Poisson), versus 16.73/17.31 ms for full-frame
decoding followed by eight-core integration, with float64 arithmetic, warm cache
and a prepared lookup.

The first Qt adapter reread images that were already decoded for display. It was
slower than integrating the resident array. That historical result motivated the
**combined loader**: one native decode now returns original typed display pixels
and the float64 pattern together. Qt consumes the prepared pattern instead of
reading or integrating those pixels a second time.

The [full Qt comparison](benchmarks/2026-11-07-qt-combined/README.md) includes the
real image/pattern controllers, snapshot creation, worker dispatch, pattern
application and the normal 25 ms debounce. On the M4 Pro/Eiger 9M test, combined
loading reduced warm open-to-pattern latency by approximately **20%**. This is a
whole-operation result, distinct from the native pipeline's roughly threefold
speedup. Qt was offscreen at 1200×900; physical-monitor/GPU presentation was not
qualified. Source timings, p95 values, all 625 count/float64 comparisons and exact
conditions are retained in that report.

The combined route requires fixed masks and a supported self-contained full-frame
Bitshuffle/LZ4 uint16/uint32 HDF5 source. Initial files retain the authoritative
LImA dataset/header; selected HDF5 sources keep their selection. Corrections,
transforms, dynamic masks, busy bulk workers and unsupported metadata/layouts use
the existing path. Batch/map and 2D integration algorithms are unchanged. Mask or
calibration edits after loading invalidate a prepared result and integrate the
resident image; they do not trigger a fused reread.
