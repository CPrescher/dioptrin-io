# Reading performance and CPU backends

[Manual index](index.md)

A processing rate is meaningful only with its data, hardware and measured steps.
Reading compressed images, creating masks and integrating pixels can each limit
a batch. A fast decoder rate does not give the complete map-integration rate.

## Measurement scope

The [benchmark index](benchmarks/index.md) groups the retained reports by
measured operation and identifies their inputs, source revisions and comparison
implementations. All published numbers here are historical measurements; they
do not benchmark the current checkout or automatically qualify later releases.

A typical workflow has these stages:

| Stage | Work |
|---|---|
| Read and decode | Read file bytes and reconstruct the original pixel values |
| Prepare the image | Apply the application's orientation and corrections, and create any per-frame mask |
| Integrate | Accumulate pixel contributions using the calibrated geometry and lookup table |
| Produce results | Calculate normalized intensities, requested uncertainties and output arrays |

**dioptrin-io** provides native file reading; Dioptrin handles integration. The
indexed HDF5 `RawReader` supports admitted immutable snapshots, including
full-frame uint16/uint32 Bitshuffle/LZ4 chunks. It indexes chunks through HDF5,
then performs positional file reads and checked decoding in frame workers
outside the HDF5 mutex. Other layouts and filters have separate reader contracts;
this fast path does not imply support for every HDF5 file or live acquisition.

The reader preserves native pixel types, reserved values and stored orientation.
The consumer owns orientation changes, masks, corrections and integration. A
consumer must opt into dioptrin-io; installing Dioptrin or choosing an integration
layout does not automatically switch its file reader.

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

The Scalar/SSE2/AVX2/NEON runtime-selection API was introduced in revision
[`9c780a9`](https://github.com/CPrescher/dioptrin-io/commit/9c780a9f8131088df55d0a6836a526fa44ee6d01)
and is included in the `v0.3.0` Rust source release. The measurements below
remain tied to the measured revision. See [release scope](RELEASING.md) for
installation and packaging.

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

For Rust consumers:

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

## Acquired Eiger read/decode measurements

The [November 6 reusable-buffer report](benchmarks/2026-11-06-scratch-eiger/README.md)
compares complete read/decode passes over 625 acquired Eiger 9M frames
(3262 × 3108 uint32) on an Apple M4 Pro, with warm OS caches. Three alternating
passes per implementation follow independent all-frame count validation at
both worker counts. Opening/indexing, hashing, masking and integration are
excluded; file reads, decoding, source checks and worker/buffer creation are
included. The machine was not isolated.

| Reader/decoder | 1 worker fps, median (range) | 8 workers fps, median (range) |
|---|---:|---:|
| dioptrin-io, allocating NEON wrapper | 95.37 (95.35–96.66) | 655.38 (646.98–655.62) |
| dioptrin-io, reusable NEON buffers | 97.62 (97.13–98.04) | 730.53 (725.35–733.85) |
| `bslz4_to_sparse` 0.0.22a3, public default (KCB) | 80.70 (79.10–84.78) | 619.98 (617.99–620.42) |
| `bslz4_to_sparse` 0.0.22a3, explicit NEON (diagnostic) | 93.80 (92.72–93.93) | 714.30 (714.01–715.67) |

The external implementation is
[`jonwright/bslz4_to_sparse`](https://github.com/jonwright/bslz4_to_sparse).
Its public `decompress_from_buffer` API and private NEON diagnostic both return
full original images in this test. The [comparison definition](benchmarks/index.md)
identifies the recorded build and entry points. These rates compare complete
reader/decoder paths with different LZ4 implementations and API overheads;
they do not measure the repository's sparse-output or fused integration paths.

The earlier [NEON-kernel report](benchmarks/2026-11-06-neon-eiger/README.md)
measures a different implementation step in a separate session. Use paired
results within each report when assessing a change.

## Windows read/decode measurements

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

## Consumer and application measurements

The [native typed-consumer report](benchmarks/2026-11-05-native-eiger/README.md)
includes decoding, orientation mapping and Dioptrin integration. The later
[fixed-mask block-fusion report](benchmarks/2026-11-07-fused-fixed/README.md)
separately measures scan throughput and single-image latency, with a prepared
lookup and float64 arithmetic. Those numbers include more work than the reader
measurements above.

The [combined Qt loader report](benchmarks/2026-11-07-qt-combined/README.md)
measures open-to-pattern latency through real image/pattern controllers,
snapshots, dispatch and the normal 25 ms debounce. On its M4 Pro/Eiger 9M test,
combined loading reduced median latency by about 20%. Qt ran offscreen at
1200 × 900; physical-monitor/GPU presentation was not qualified. This result
applies to that fixed-mask configuration and recorded application revision.
The report retains timings, p95 values, all-frame validation, eligibility rules
and reproduction commands.
