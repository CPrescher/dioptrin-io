# Dense, uint16 and small-frame codec qualification

## Decision

Use the existing bounded native LZ4 decoder by default for `RawReader` on
Linux/Windows x86-64 and macOS/Linux/Windows ARM64. Retain checked Rust LZ4 on
other targets, an explicit Rust/native override, and automatic Rust selection
if the configured chunk budget exceeds
`i32::MAX`. This conservative per-reader fallback preserves the accepted size
range without attempting a native call with out-of-range encoded/decoded sizes.
The total file size is unrelated to this individual-block API limit.

Keep automatic unshuffle dispatch unchanged. KCB remains a preserved experiment.
This decision combines the synthetic qualification here with the
[acquired Eiger and complete-consumer comparison](../2026-11-11-codecs/README.md).
Native improved large synthetic cases; small-frame comparisons are near parity
with wide timing ranges. These results establish neither a universal speedup nor
an improved complete integration rate for every workload.

## Source and method

The measured production reader is main `c5b1849c5ef159c8f888d47d79c4ebd85913d1a8`,
before changing the default. Both alternatives are explicitly selected in every
timed comparison, so changing the automatic choice does not change those paths.
The [measured benchmark source](benchmark_codecs_measured.rs.txt) and shared
fixture generator are identified by SHA256 in the [raw record](apple-m4-pro-qualification.json).
The final example removes one redundant borrow for Clippy and corrects order
alternation when testing multiple kernels. This Auto-kernel qualification uses
only one kernel, so its pair order, timed operations and fixture generator are
unchanged. The raw record also retains the production reader/native
shim hashes, Rust compiler, CPU, OS, host byte order, start time and system load.

- Apple M4 Pro, macOS ARM64; 14 available CPUs; one and four frame workers.
- Eleven synthetic cases: uint16/uint32 sparse and dense full-width 1024×1024;
  dense 8×8, 64×64 and 65×63; and uint32 with a dense 16-bit value range.
- Four large frames or 32 small/tail frames or 64 tiny frames per file. Explicit
  zero, source-width maximum and high-bit values in every frame.
- Stock upstream Bitshuffle/LZ4 encoder generates each file from deterministic
  original plaintext. SHA256 hashes are calculated before compression. Every
  frame is checked against this oracle for every backend and worker count before
  timing, including the automatic reader once per case. The decoder under test
  does not create the expected counts. No detector files are committed.
- Timed scope: warm file reads, source/snapshot checks, decoding, thread and
  reusable-buffer setup, and original typed pixel delivery. No decoded cache.
  Fixture generation, file indexing and correctness hashing are excluded.
  There is no masking, numerical integration or output conversion in this test.
- Nine matched repetitions, alternating Rust/native order. Automatic unshuffle
  resolves to NEON. A common number of frame scans per pair is calibrated toward
  250 ms on the Rust baseline. This is a target, not a guaranteed native sample
  duration. Rates retain thread setup and are particularly noisy for tiny frames.
- The preliminary five-pair factorial run also checked scalar/NEON and both LZ4
  choices; the longer run below was used for the default decision. The known
  acquired-data comparison provides a separate complete-consumer validation.

Reproduce the longer comparison:

```sh
cargo run --locked --release --features hdf5 --example benchmark_codecs -- qualification.json --qualify
python scripts/report_codec_ci.py qualification.json qualification-summary.json
```

The following tables are generated from the raw samples with
`scripts/report_codec_ci.py`. Paired speedups are medians of matched time ratios,
so they need not equal the ratio of independently calculated median frame rates.
All slow/noisy cases and full ranges are retained.

## Small codec qualification

CPU: Apple M4 Pro; macos/aarch64; allocated CPUs: 14.

Known generated original counts; uint16/uint32, extrema, dense/sparse and short frames/tails. Warm file reads + source checks + decode + typed pixel delivery; no decoded cache. Alternating matched pairs; sample counts and duration are recorded in the raw JSON. Shared-runner timings are diagnostic, with no speed threshold gating CI.

### Native versus Rust LZ4 with the automatically selected unshuffle kernel

| Case | Workers | Unshuffle | Rust fps | Native fps | Paired native/Rust (range) |
| --- | ---: | --- | ---: | ---: | ---: |
| u16-sparse-1m | 1 | Neon | 527.9 | 1420.5 | 2.692× (2.642–2.748) |
| u16-sparse-1m | 4 | Neon | 2087.5 | 5825.4 | 2.788× (2.751–2.828) |
| u16-dense-1m | 1 | Neon | 1020.5 | 1165.9 | 1.147× (1.120–1.713) |
| u16-dense-1m | 4 | Neon | 4096.4 | 4688.7 | 1.143× (0.847–1.219) |
| u16-small-64 | 1 | Neon | 50664.2 | 48530.8 | 0.989× (0.863–1.039) |
| u16-small-64 | 4 | Neon | 68716.2 | 69454.5 | 0.981× (0.879–1.174) |
| u16-tiny-8 | 1 | Neon | 61094.0 | 61078.7 | 1.017× (0.980–1.071) |
| u16-tiny-8 | 4 | Neon | 76909.4 | 79303.4 | 1.063× (0.886–1.273) |
| u16-tail-65x63 | 1 | Neon | 41382.1 | 40816.3 | 1.068× (0.784–1.908) |
| u16-tail-65x63 | 4 | Neon | 56214.2 | 56453.1 | 1.050× (0.957–1.110) |
| u32-sparse-1m | 1 | Neon | 162.9 | 686.3 | 3.910× (3.336–6.981) |
| u32-sparse-1m | 4 | Neon | 570.1 | 2363.2 | 4.056× (3.466–9.788) |
| u32-dense-full-1m | 1 | Neon | 309.0 | 369.3 | 1.287× (1.022–1.612) |
| u32-dense-full-1m | 4 | Neon | 1555.8 | 1864.6 | 1.180× (1.063–1.201) |
| u32-small-64 | 1 | Neon | 41437.7 | 41659.1 | 1.007× (0.927–1.049) |
| u32-small-64 | 4 | Neon | 74316.2 | 77528.3 | 1.044× (0.902–1.181) |
| u32-tiny-8 | 1 | Neon | 55226.7 | 60864.2 | 1.072× (1.006–1.184) |
| u32-tiny-8 | 4 | Neon | 72196.0 | 71986.8 | 0.994× (0.950–1.016) |
| u32-tail-65x63 | 1 | Neon | 39742.0 | 43719.4 | 1.077× (1.030–1.168) |
| u32-tail-65x63 | 4 | Neon | 77417.9 | 81144.7 | 1.038× (0.911–1.096) |
| u32-dense-u16-range-1m | 1 | Neon | 707.2 | 734.6 | 1.042× (0.856–1.058) |
| u32-dense-u16-range-1m | 4 | Neon | 2852.7 | 2947.3 | 1.033× (0.978–1.041) |

### Fastest observed configuration per workload

Rankings use median paired speedups against Auto + Rust among the tested kernels. Small differences can be scheduling noise; this table does not automatically change platform defaults.

| Case | Workers | Observed configuration | Median MiB/s | Paired speedup vs Auto + Rust |
| --- | ---: | --- | ---: | ---: |
| u16-sparse-1m | 1 | Neon + Native | 2841.1 | 2.692× |
| u16-sparse-1m | 4 | Neon + Native | 11650.8 | 2.788× |
| u16-dense-1m | 1 | Neon + Native | 2331.8 | 1.147× |
| u16-dense-1m | 4 | Neon + Native | 9377.5 | 1.143× |
| u16-small-64 | 1 | Neon + Rust | 395.8 | 1.000× |
| u16-small-64 | 4 | Neon + Rust | 536.8 | 1.000× |
| u16-tiny-8 | 1 | Neon + Native | 7.5 | 1.017× |
| u16-tiny-8 | 4 | Neon + Native | 9.7 | 1.063× |
| u16-tail-65x63 | 1 | Neon + Native | 318.8 | 1.068× |
| u16-tail-65x63 | 4 | Neon + Native | 440.9 | 1.050× |
| u32-sparse-1m | 1 | Neon + Native | 2745.3 | 3.910× |
| u32-sparse-1m | 4 | Neon + Native | 9452.8 | 4.056× |
| u32-dense-full-1m | 1 | Neon + Native | 1477.3 | 1.287× |
| u32-dense-full-1m | 4 | Neon + Native | 7458.4 | 1.180× |
| u32-small-64 | 1 | Neon + Native | 650.9 | 1.007× |
| u32-small-64 | 4 | Neon + Native | 1211.4 | 1.044× |
| u32-tiny-8 | 1 | Neon + Native | 14.9 | 1.072× |
| u32-tiny-8 | 4 | Neon + Rust | 17.6 | 1.000× |
| u32-tail-65x63 | 1 | Neon + Native | 682.9 | 1.077× |
| u32-tail-65x63 | 4 | Neon + Native | 1267.6 | 1.038× |
| u32-dense-u16-range-1m | 1 | Neon + Native | 2938.3 | 1.042× |
| u32-dense-u16-range-1m | 4 | Neon + Native | 11789.1 | 1.033× |

All 44 original-count validation passes matched. Raw per-pass timings, exact shapes, generated-count hashes and source/compiler metadata are retained in the artifacts.


After changing the default, all eleven automatic readers selected Native and
all 88 scalar/NEON × Rust/native × worker-count validation passes still matched
the original counts. The signed-budget fallback and explicit overrides are also
covered by an independent fixture regression test.

## Cross-platform CI comparison

The qualification runs originally used every Rust CI matrix job. Routine CI
now omits benchmarks; there is no schedule. The separate
[manual benchmark workflow](https://github.com/CPrescher/dioptrin-io/actions/workflows/codec-benchmark.yml)
can be explicitly dispatched for a future codec change. It runs the same small
factorial benchmark: both explicit
LZ4 choices with every available scalar/SSE2/AVX2/NEON kernel on that host, five
matched samples targeting 80 ms on the Auto+Rust baseline, one and up to four
workers (bounded by allocated CPUs). Unsupported kernels are skipped explicitly.
All eleven data cases are checked. The workflow compares Linux x86-64/ARM64,
macOS ARM64 and Windows x86-64/ARM64 without transferring detector data.

Each job publishes a readable summary and `codec-benchmark-<runner>` artifacts
with raw and summary JSON. The summary ranks configurations by median paired
speedup against Auto+Rust for each case and worker count. Source and compiler
metadata travel with the timings. Compare alternatives within the same host;
absolute rates from different machines do not compare operating systems.

Count mismatches and incomplete paired records fail CI. Performance does not
have a threshold: shared-runner load, scheduling and compiler changes can alter
small differences. CI rankings identify candidates for longer qualification;
they do not automatically change defaults. The manual links
this method so later runs remain inspectable when runner hardware changes.

The previous long Windows SIMD example remains manually available. Historical
Windows reports keep their pinned source, workload and Rust-LZ4 baseline.

### Initial CI observations at the measured source

[CI run 43178638558](https://github.com/CPrescher/dioptrin-io/actions/runs/43178638558)
tests production revision `48dd774b730864744f0b27ad26865e447f224e6b`, including
the new default. The Mac/Linux artifact source, generator, reader and native-shim
hashes were checked against that revision; re-summarizing raw samples reproduced
the uploaded summaries exactly. All 88/88/132 count validations matched on
macOS ARM64/Linux ARM64/Linux x86-64 respectively. Every Mac automatic reader
selected Native; both Linux variants retained Rust.

| Runner | Automatic unshuffle; allocated CPUs | Sparse 1M pixels, native/Rust paired medians | Dense full-width 1M | Dense 16-bit-range uint32 1M |
| --- | --- | ---: | ---: | ---: |
| macOS 15, Apple M1 (Virtual) | NEON; 3 | 2.35–2.86× | 1.19–1.27× | 0.902× / 0.927× |
| Linux ARM64; CPU model unavailable in `/proc/cpuinfo` | NEON; 4 | 1.57–1.58× | 1.11–1.15× | 0.937× / 0.931× |
| Linux x86-64, AMD EPYC 9V74 | AVX2; 4 | 2.09–3.17× | 1.17–1.25× | 1.042× / 1.067× |

Sparse/full-width ranges span uint16/uint32 and one/up-to-four workers. The last
column gives one/multiple-worker paired medians. These are ratios within each
host, not relative machine speeds. Full per-case scalar/SIMD/LZ4 rankings,
short-frame results, raw timing ranges and Windows results are published in the
same run's job summaries/artifacts.

The M1 and Linux ARM results show that native LZ4 can be roughly 6–10% slower
for the dense-16-bit-range uint32 pattern, despite winning the other large
patterns. This exception is retained explicitly; it is not hidden by an aggregate
score. It reinforces the Rust override for these workloads, even though Native is
the selected general default on the qualified targets. The Mac result supports native for
large sparse/full-width workloads on another Apple CPU, without establishing
universal superiority for every Apple Silicon workload.

## Fixed defaults after cross-platform qualification

[The completed second run](https://github.com/CPrescher/dioptrin-io/actions/runs/43180447512)
at `7374c8d3583e33e5a01ab7fb95314325e98e6ad9` passed all five Rust platform jobs,
all six Python wheel jobs, inventory and strict documentation checks. The five
artifact archive digests, source hashes (Windows CRLF accounted for), exact
original frame hashes and regenerated summaries were verified. All **528**
original-count validation passes matched. The [retained per-platform comparisons](ci-platform-comparisons.json)
record paired native/Rust medians and full ranges with the automatic kernel.

| Target | General choice | Large sparse native/Rust medians | Large full-width dense medians |
| --- | --- | ---: | ---: |
| macOS ARM64, virtual M1 | NEON + Native | 2.42–2.84× | 1.09–1.22× |
| Linux ARM64 | NEON + Native | 1.55–1.59× | 1.11–1.16× |
| Windows ARM64 | NEON + Native | 1.33–1.50× | 1.05–1.10× |
| Linux x86-64, EPYC 7763 | AVX2 + Native | 2.23–3.28× | 1.19–1.26× |
| Windows x86-64, AMD | AVX2 + Native | 1.73–3.93× | 1.12–1.21× |

Ranges here span uint16/uint32 and the tested worker counts, not uncertainty
intervals. Per-pass timing ranges are retained in the JSON. Native is the
consistent general choice for large sparse/full-width dense patterns on all five
hosts. Dense 16-bit-range uint32 remains an ARM exception: Linux/Windows ARM
medians favored Rust; virtual M1 results varied with worker count/run. Tiny-frame
rankings are noisy. Explicit Rust/native and scalar/SIMD overrides remain.

Keep automatic AVX2 dispatch on supporting x86-64 CPUs and NEON on supporting
ARM64 builds, with existing SSE2/scalar fallbacks. SSE2 versus AVX2 was within
roughly 0–3% for large uint16 cases on the measured x86 runners, sometimes mixed
within the matched samples, while AVX2 had clear uint32 gains. Do not encode
individual runner CPU names or tiny-frame timing winners as new dispatch rules.
Worker counts remain the consumer's choice; these comparisons did not establish
a universal optimal number of workers.

These are fixed defaults; there is no runtime auto-tuning. Automatic selection
still falls back to Rust for configured chunk budgets above `i32::MAX` and on
unqualified targets. No benchmark is run on push, pull request or a schedule.
The manual workflow is retained for explicitly requested future qualification;
this default-selection change reuses the existing measurements without rerunning
them.
