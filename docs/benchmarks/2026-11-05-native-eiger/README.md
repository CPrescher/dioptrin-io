# Native HDF5 → typed Dioptrin, 2026-11-05

The new raw-chunk snapshot reader preserves all original Eiger counts and feeds
native u32 samples into existing float64 Dioptrin integration kernels. It removes
serialized HDF5 decompression from the measured path. This is a validated first
native implementation, not complete FabIO/Eiger-master parity or a claim of
cold-disk throughput.

## Full-file results

625 frames, 3262 × 3108 u32, eight decode/consumer workers, three alternating
measured whole-file passes after a whole-file warmup for each workflow/error
mode. Final decoder: `lz4_flex` 0.14.0 with safe/checked decode, plus the existing
bundled native Bitshuffle unshuffle; bundled HDF5 2.2.0 for metadata/general reads.

| Read → same typed Dioptrin consumer | Intensity median fps (range) | Poisson median fps (range) |
|---|---:|---:|
| General bundled HDF5 reader under upstream mutex | 30.44 (29.47–30.59) | 29.93 (29.28–30.57) |
| Indexed positional reads + raw chunk decoding | 140.95 (134.75–145.96) | 130.23 (125.15–132.44) |

Within this run, median speedups are **4.63× intensity** and **4.35× Poisson**.
The host was not reserved/isolated and the observed ranges should accompany the
medians. These numbers do not replace the earlier Python/Jon comparisons or
establish superiority over the earlier optimized reusable prototype (193/172
fps in its separately documented run). No new Jon result is claimed here.

The operating-system page cache was not purged. Counts are validated before
timing, and each error mode/workflow gets a complete warmup. These are **warm
whole-file reads**, not physical cold-device bandwidth.

`timings.json` retains every timed pass and raw indexing setup (0.010825 s on
this run). Timing includes file reads, decompression, native integer row-order
copy required by the consumer, integration, normalization, worker/buffer creation
and output materialization. Index construction, file opening, calibration,
geometry/LUT/normalization initialization and warmups are excluded. Neither
workflow reuses decoded counts between frames or passes.

## Independent validation

- The external input SHA256 matches the supplied source provenance.
- Every frame's complete original little-endian u32 count hash matches an
  independent h5py 3.16.0 / hdf5plugin 7.1.0 read: all **625 frames**, including
  masked/reserved pixels. `count-reference.json` retains the 625 SHA256 values.
- Independent h5py → the previously validated existing **float64** Dioptrin
  Python API was run over the complete file with Poisson enabled. Both native
  reader workflows produce **bit-identical intensity and Poisson std** values
  for all 625 × 2000 bins. `output-validation.json` retains full-array hashes
  and maximum absolute differences (all zero).
- The final checked-LZ4 run's materialized output hashes were compared again
  against that independent full-file consumer reference.
- The 16 retained patterns from the earlier full-file comparison also match
  the native intensity output exactly. This is supplemental to the complete
  new 625-frame comparison, not its replacement.

Reference Python is used for independent validation only; the measured Rust
executable does not load Python, FabIO, h5py or NumPy.

## Scientific settings and memory

The isolated adapter calls the existing prepared kernels; no integration
arithmetic or geometry weights were changed. Both measured workflows use:

- full pixel splitting, 2000 uniform two-theta degree bins;
- the same calibration/mask as the existing full-file comparison;
- solid-angle normalization and polarization factor 0.965;
- float64 weights, accumulation, normalization and outputs;
- fixed-mask compacted CSR with cached normalization; fast arithmetic;
- existing Poisson uncertainty semantics when requested.

`dioptrin-io` exposes stored top-to-bottom rows. The **consumer** copies/flips
u32 rows into its geometry convention in reusable thread-local storage. No
full detector image is converted to float64. Each callback has a single-thread
Rayon registry on its current worker, avoiding nested compute pools beyond the
eight decode/consumer workers.

The raw pipeline has one native output and reusable encoded/block scratch per
worker, with no decoded scan queue. Its conservative decoder payload admission
is 130,935,415 bytes/worker (1,047,483,320 bytes for eight); the harness supplies
a 2 GiB decoder budget. The explicit consumer orientation buffer adds 40,553,184
bytes/worker. LUTs, masks, output vectors, thread stacks, allocator/native-library
overhead and index storage are outside that payload cap. This bound is **not a
measured RSS limit**. Output materialization is approximately 10 MB per output
array for the complete 625-frame result.

## External source and host

- File: `/Users/clemens/Programming/Dioptrin/test_data/eiger_0000.h5`
- Dataset: `entry_0000/instrument/eiger/data`
- File bytes: 5,687,072,680; shape: `[625, 3262, 3108]`; dtype: u32
- SHA256: `2b749693d7a942b35d89806d99b9a1025ee119850be0ade27428f1dc11d0a1f3`
- Filter: Bitshuffle/LZ4 32008, parameters `(0,5,4,0,2)`; one chunk/frame;
  stored zero block-size field selects the stable codec default.
- Dataset is resizable but was read under an explicit **immutable snapshot**
  contract after acquisition. No live/SWMR behavior is claimed.
- Calibration:
  `/Users/clemens/Programming/Dioptrin/docs/benchmarks/2026-11-05-bslz4/calibration.poni`
  SHA256 `5999ef2e03bf1ef2db21e6ab042f3170bc86f81e99ce59488d1bd896ddf1b00e`
- Mask: `/Users/clemens/Programming/Dioptrin/test_data/eiger_0000.mask`
  SHA256 `65801632091fd5b0198cc32127056590ee789121b0fe6ff5ef67c034f513bb5d`
- Apple M4 Pro, 14 physical/logical cores, 48 GiB RAM; macOS 26.6.2;
  rustc 1.99.1 (`58cf8f9c3`, LLVM 23.1.1), aarch64-apple-darwin.

These private/external measurement files are not redistributed. Small synthetic
independent fixtures and their licenses/provenance are retained in the repository.

## Source and reproduction

I/O base: `dioptrin-io` initial commit
`21562e680821caa231d79f8e291ee4cccc6eac69`. Implementation is on this branch;
`provenance.json` records source/dependency/benchmark binary hashes.

Consumer base: Dioptrin `e558cf7` (including mask-mode commit `10435d6`).
The isolated consumer worktree is
`/Users/clemens/Programming/Dioptrin-io-adapter`, branch
`codex/shared-io-adapter`, adapter commit `0971f2a`. Its 168 library tests pass,
including comparisons across fixed/dynamic masks, strict/fast arithmetic and
Counting/PixelSplit/Supersampled kernels. The original Dioptrin main checkout
and its unpublished commits were not modified or pushed.

The complete adapter patch is
[`benchmarks/eiger/dioptrin-native-input.patch`](../../../benchmarks/eiger/dioptrin-native-input.patch).
To reproduce elsewhere, create the sibling worktree at the consumer base and
apply that patch. It adds only an opt-in `native-input` feature and typed 1D CSR
adapter; Histogram returns an explicit unsupported error. Numerical kernels are
unchanged. Adjust the benchmark's local dependency path if needed.

```sh
# From the dioptrin-io repository; use an independent reference Python environment.
python scripts/eiger_reference.py /path/to/eiger_0000.h5

cargo build --manifest-path benchmarks/eiger/Cargo.toml --release
benchmarks/eiger/target/release/dioptrin-io-eiger-bench \
  /path/to/eiger_0000.h5 /path/to/calibration.poni \
  /tmp/dioptrin-io-eiger-20261105/mask.u8 \
  docs/benchmarks/2026-11-05-native-eiger/count-reference.json \
  /tmp/dioptrin-io-eiger-20261105 8

# After timing, use the previously validated float64 Dioptrin Python environment:
python scripts/eiger_consumer_reference.py \
  /path/to/eiger_0000.h5 /path/to/calibration.poni \
  /tmp/dioptrin-io-eiger-20261105
```

The benchmark's required data shape/settings are intentional and explicit. It
is not a generic calibration tool. Do not use this benchmark result to claim
support for HDF5 master links, VDS, growing files or other detector formats.
