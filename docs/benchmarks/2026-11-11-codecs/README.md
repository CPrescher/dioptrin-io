# KCB and native LZ4 against current main

The default decision below is historical. The subsequent
[dense, uint16 and small-frame qualification](../2026-11-12-codec-qualification/README.md)
and cross-platform CI comparison select native LZ4 on the five
qualified targets, retaining Rust on other targets,
automatic fallback for larger configured block budgets and explicit overrides.
The measurements in this report remain unchanged.

## Decision at the measured revision

Retain the current automatic unshuffle policy and checked Rust LZ4 default.
Add the already bundled, bounded native LZ4 implementation as an explicit
`RawReader` choice. It improves reader throughput and the measured Poisson
workflows, while intensity-only complete-frame timings are mixed. Do not merge
the KCB implementation into the production decoder: it regresses every measured
workflow against the newer NEON implementation on this host.

```rust
use dioptrin_io::hdf5::{Limits, raw::{Lz4Backend, RawReader}};

let reader = RawReader::open_snapshot("scan.h5", "entry/data", Limits::default())?
    .with_lz4_backend(Lz4Backend::Native);
```

Native LZ4 checks both slice lengths before conversion to the C API's signed
32-bit sizes, uses `LZ4_decompress_safe`, and requires the exact decoded length.
It does not call the unbounded Bitshuffle compressed decoder. Oversized blocks
return an explicit error when this alternative is requested. The Rust default
retains its existing resource contracts on every platform. Original source,
dtype, reserved values, orientation, snapshot checks and buffer invalidation are
unchanged. No Dioptrin adapter or integration kernel changes are needed.

## Whole-file results

All values below use eight frame workers and the same current Dioptrin 0.12.0
fixed-mask float64 plan. Each cell includes five complete 625-frame passes.
Speedups are the median of paired baseline/candidate time ratios within each
pass, not a ratio of independently selected fastest runs.

| Workload | Main median fps | Native LZ4 median fps | Native LZ4 paired speedup (range) |
| --- | ---: | ---: | ---: |
| File reads and original uint32 decoding | 746.21 | 804.33 | 1.075× (1.067–1.078) |
| Full uint32 frame → intensity | 255.85 | 244.88 | 0.999× (0.924–1.034) |
| Full uint32 frame → intensity + Poisson std | 215.96 | 219.66 | 1.026× (1.010–1.030) |
| Serial fused blocks → intensity | 247.81 | 251.85 | 1.015× (0.988–1.041) |
| Serial fused blocks → intensity + Poisson std | 209.88 | 215.47 | 1.015× (1.005–1.034) |

The complete-frame intensity row illustrates the importance of retaining raw
runs: the ratio of marginal medians suggests a decrease, while matched within-pass
ratios have an essentially neutral median and a broad range. This does not
support changing the default for every consumer. Poisson gains are modest but
positive in all five matched pairs for both complete-frame and streamed paths.

| Workload | KCB paired speedup | KCB + native LZ4 paired speedup |
| --- | ---: | ---: |
| File reads and decoding | 0.773× | 0.819× |
| Full uint32 frame → intensity | 0.909× | 0.932× |
| Full uint32 frame → intensity + Poisson std | 0.948× | 0.960× |
| Serial fused blocks → intensity | 0.959× | 0.977× |
| Serial fused blocks → intensity + Poisson std | 0.975× | 0.989× |

KCB's older favorable comparison therefore does not carry over to the current
decoder. Production main retains the optimized upstream NEON kernel on this Mac.

## One image at a time

Eight workers decode and integrate contiguous block ranges within one image.
There are 120 samples per variant/error mode: five passes over the same 24 evenly
spaced acquired frames, with variant order rotated for each frame/pass.

| Output | Main median / p95 (ms) | Native LZ4 median / p95 (ms) | Paired median speedup |
| --- | ---: | ---: | ---: |
| Intensity | 4.868 / 5.178 | 4.782 / 5.070 | 1.016× |
| Intensity + Poisson std | 5.733 / 6.097 | 5.607 / 5.964 | 1.022× |

KCB paired speedups were 0.955× and 0.979×; combined KCB/native-LZ4 speedups
were 0.972× and 0.984×. These are within-frame latencies, not map throughput or
whole-application Qt timings. Each variant/frame is warmed once using the same
worker before its timed sample. Thread launch/join and histogram setup within
the frame operation remain timed; initial reusable-buffer allocation is excluded.

## Independent counts and consumer results

Fresh h5py/stock-hdf5plugin decoding reproduced all 625 retained original-frame
SHA256 values and the complete source-file SHA256. Each of the four variants then
matched every frame via complete-frame, serial-block and eight-core parallel-block
reads, including reserved pixels. Parallel block pixels were compared exactly
with the independently verified full frames. No decoded frame cache is used.

Each candidate's complete-frame and parallel float64/Poisson integrations matched
the current full-frame float64 control. Every timed integration result was also
checked outside timing, and complete output hashes were identical between codec
variants for each workload/error mode, including all within-frame samples.
Range-partition reductions differ from the complete-frame control only within
the existing tolerance of 1e-10 absolute plus 2e-12 relative; the maximum observed
scaled error was below 7e-15. The codec alternatives do not change reduction order.

Additional independent small vectors cover uint16/uint32 widths, integer extrema,
several codec blocks, short blocks, raw tails, worker reuse, and the existing
byte-order admission/rejection contract. Malformed/truncated LZ4 blocks are
checked against Rust decoding with output guards. AddressSanitizer/UBSan smoke
checks passed on native ARM64 and x86-64 under Rosetta. Rosetta provides count
and memory-safety evidence, not native Intel performance qualification.

## Conditions, provenance and reproduction

The acquired Eiger9M source has 625 uint32 frames of shape 3262 × 3108 and
5,687,072,680 stored bytes. Original decoded counts total approximately 25.3 GB
per whole-file pass. The source remains outside Git at
`/Users/clemens/Programming/Dioptrin/test_data/eiger_0000.h5`; its hash is retained
in the raw results and independent reference. The calibration and mask match the
earlier [native benchmark](../2026-11-05-native-eiger/README.md).

All variants use 2000 two-theta degree bins, pixel splitting, polarization 0.965,
the same static mask, float64 weights/accumulation/normalization, and a consumer
vertical-row mapping. The consumer is unmodified Dioptrin main `c6ef978`.
The reader baseline is main `dc6e303`'s algorithms: Auto resolved to NEON plus
checked Rust LZ4. All four choices ran in the same optimized binary, with one
shared unchanged consumer plan; this isolates codec selection from binary-build
and consumer changes.

The machine is an Apple M4 Pro with 14 CPU cores and 48 GiB RAM. Exact OS, compiler,
source, binary, input and load metadata are in
[experiment-provenance.json](experiment-provenance.json). Other applications
remained active. The file cache was warm; no cache purge, cold-storage claim,
unrelated process changes, or Linux/Windows performance claim is made.

Whole-file warmups preceded each workload. Five passes rotated and alternated
variant order. Setup/indexing, lookup construction, independent hashing, numerical
comparison and output hashing were excluded from timing. Positional reads,
decompression, orientation mapping, integration, normalization, worker/buffer
setup and collection/sorting of complete output patterns were included.

The four-way evaluation is preserved on `codex/kcb-lz4-evaluation` at
`9a767d967bffffe81cf1dd9cd424e0dcd538d6c1`. The
[exact experimental source overlay](experiment-source-overlay.tar.gz), applied
to `dc6e303`, contains all changed compiled files and the pinned, unmodified KCB
source/header and both original license alternatives. Its file hashes match the
measured binary's source provenance. The later benchmark harness pins the old
Auto policy explicitly to keep the baseline stable after future default changes.
Use the build/run commands in the [benchmark README](../../../benchmarks/fused-eiger/README.md)
with the retained independent reference and the external mask/calibration.
Production's `codecs` mode compares only the retained Rust and native LZ4 choices.

[results.json](results.json) retains every measurement and count check.
[summary.json](summary.json) retains all median/range and paired statistics.
Recompute it with `python scripts/report_codec_benchmark.py RESULTS_JSON SUMMARY_JSON`.
