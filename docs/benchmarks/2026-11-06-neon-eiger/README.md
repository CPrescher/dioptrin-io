# Upstream NEON kernels: Eiger9M read/decode comparison

The optional HDF5 backend now backports upstream Bitshuffle's bulk ARM NEON
movemask implementation. The indexed reader retains bounded, checked Rust LZ4
decoding. It does not depend on Jon Wright's package. Original native sources,
license notices, baseline hashes, and the two exact upstream patches are retained
in `vendor/hdf5-bitshuffle-1.0.0/`.

## Results

| Reader/decoder | 1 worker fps, median (range) | 8 workers fps, median (range) |
|---|---:|---:|
| dioptrin-io before | 31.48 (31.29–31.54) | 207.27 (199.56–209.76) |
| dioptrin-io upstream NEON | 94.31 (93.96–94.52) | 597.37 (594.01–598.12) |
| Jon public default (KCB) | 80.20 (78.86–81.42) | 543.62 (532.24–543.75) |
| Jon explicit NEON (diagnostic) | 93.21 (91.99–93.69) | 641.68 (619.65–653.36) |

The update gives **3.00×** the previous native throughput at one worker and
**2.88×** at eight. In this run it is 17.6% / 9.9% faster than Jon's
public default at 1 / 8 workers. Against his explicit NEON path the native
median is 1.2% higher at one worker and 6.9% lower at eight workers. The
remaining eight-worker difference has not been isolated to a particular stage.


These are complete **file read + decompression** passes into original uint32
pixels. They exclude masking, normalization, conversion and integration, so they
must not be compared directly with the older read-to-Dioptrin integration rates.
Opening/indexing setup is recorded separately in `results.json`: median native
setup was about 9.1 ms and Jon setup about 2.3 ms, across the timed samples.

## Method and limitations

- All 625 original frames, 3262 × 3108 uint32 pixels each, stored orientation;
  no count conversion, clipping, reserved-pixel removal or decoded-frame cache.
- Each variant is checked against every independent reference frame hash at both
  worker counts before timing. Validation is outside the performance numbers.
- Three timed full-file passes per variant and worker count, with implementation
  order reversed on the middle pass. Median and full observed range are reported.
- Actual positional file reads, decompression, immutable-source checks,
  worker/buffer creation and borrowed-pixel delivery are timed. File opening,
  HDF5 chunk-index construction, subprocess/interpreter startup and reference
  hashing are excluded; setup durations are retained separately.
- The complete validation passes warm the operating-system page cache. The cache
  was not purged. **These are warm-cache file reads, not cold SSD bandwidth.**
- Both pipelines use a fixed number of frame workers and dynamic frame assignment;
  there is no queued decoded scan or additional within-frame thread pool.
  Native uses reusable encoded/block/pixel buffers; the Python API allocates
  compressed bytes through `os.pread` and reuses a pixel buffer per worker.
- The conservative decoder payload admission budget is 2 GiB. This is not a
  measured RSS limit: native bookkeeping, allocators and thread stacks are extra.
- The machine was not reserved or otherwise isolated. Background load averages
  accompany every sample. No unrelated processes were stopped.
- Native baseline is the prior reader at `4fc9071f29d7d54d6388a97dab424c7ec845884c`
  built with the same new benchmark example. Both native binaries use identical
  Rust release profiles, checked `lz4_flex` 0.14.0, HDF5 and callback semantics.
- Jon default is installed `bslz4-to-sparse` 0.0.22a3's public CPU
  `decompress_from_buffer`, which selects KCB. Jon NEON is a separately labelled
  diagnostic using that package's native NEON selector with the same frame API
  semantics; it is not the public default or Jon's fused integration path.
- Jon and native retain their own LZ4 implementations. This compares the usable
  reader/decoder paths; it does not isolate NEON instruction throughput alone.

## Independent validation and build checks

`../2026-11-05-native-eiger/count-reference.json` retains independent h5py/HDF5
pixel hashes for every frame. `results.json` records all eight complete count
validation passes, every timing sample, executable hashes, package versions and
Jon's installed build metadata. Every original pixel must match, including zeros,
high bits and reserved values.

`tests/bitshuffle_neon.rs` additionally checks both encode and decode against
24 independently stock-Bitshuffle-encoded synthetic vectors: 1/2/4/8-byte samples,
vector-sized inputs, multiple blocks and raw tails. These small fixtures are
licensed and retained in `tests/data/bitshuffle-neon/`; the detector file is not
committed.

Checks passed locally on ARM macOS: `cargo fmt --check`, strict all-targets and
all-features Clippy, all 405 tests with all features, the no-default-features
build, release benchmark build, and Ruff for both added Python scripts. This run
does not establish live x86/Windows execution of the backport; the upstream MSVC
guard is included and non-ARM kernels are unchanged.

## Source and provenance

- Input: `/Users/clemens/Programming/Dioptrin/test_data/eiger_0000.h5`
- Dataset: `entry_0000/instrument/eiger/data`
- File bytes: 5,687,072,680
- SHA256: `2b749693d7a942b35d89806d99b9a1025ee119850be0ade27428f1dc11d0a1f3`
- Bitshuffle/LZ4 filter 32008, parameters `(0,5,4,0,2)`, one frame per chunk;
  stored zero block-size field selects the codec default.
- Apple M4 Pro, 14 cores, 48 GiB RAM, macOS 26.6.2, aarch64 Rust 1.99.1.
- [Upstream NEON optimization](https://github.com/kiyo-masui/bitshuffle/commit/45af6537796e15099959529899edbd606002c3f5),
  Sebastian Pop; [MSVC alias guard](https://github.com/kiyo-masui/bitshuffle/commit/aa9bd80f076ccde6320d81833b19b835ecf2acfe),
  Christoph Gohlke. Original upstream MIT attribution remains intact.

`provenance.json` records source/compiler/patch hashes, generated patched-source
hashes, and checks. This is a local implementation and benchmark, with no
application consumer change, publication or release.

An initial diagnostic run is retained in `diagnostic-skipped-patches.json`.
Git had silently skipped patch paths when invoked under Cargo's build directory
inside a parent worktree; consequently both native binaries in that run still
used old kernels. The build now forces standalone patch application and fails
unless each target changes. Generated release/debug sources were inspected for
the new NEON function and retained HDF5 bounds guard. Only the corrected run in
`results.json` supplies the table above. Correct pixel output alone would not
prove that the optimized kernel was selected.

## Reproduce

Use the reference Python environment containing `h5py`, `numpy`, `psutil` and
`bslz4-to-sparse==0.0.22a3`. Keep the original immutable detector file external.
Build a baseline from the prior commit with the same benchmark example:

```sh
git worktree add --detach /tmp/dioptrin-io-before-neon 4fc9071f29d7d54d6388a97dab424c7ec845884c
cp examples/benchmark_eiger_decode.rs /tmp/dioptrin-io-before-neon/examples/
cargo build --manifest-path /tmp/dioptrin-io-before-neon/Cargo.toml --release --features hdf5 --example benchmark_eiger_decode --locked
cargo build --release --features hdf5 --example benchmark_eiger_decode --locked
/Users/clemens/Programming/Dioptas/.venv/bin/python scripts/benchmark_eiger_neon.py \
  --source /Users/clemens/Programming/Dioptrin/test_data/eiger_0000.h5 \
  --reference docs/benchmarks/2026-11-05-native-eiger/count-reference.json \
  --baseline /tmp/dioptrin-io-before-neon/target/release/examples/benchmark_eiger_decode \
  --native target/release/examples/benchmark_eiger_decode \
  --output /tmp/neon-eiger-results.json
```
