# Native read-to-integration benchmark

This unpublished crate compares original-count HDF5 decoding and the existing
Dioptrin float64 fixed-mask integration plan. Detector files remain outside Git.
The consumer dependency is Dioptrin 0.12.0; use an unmodified pinned checkout:

```sh
cargo build --release --manifest-path benchmarks/fused-eiger/Cargo.toml \
  --config 'patch.crates-io.dioptrin.path="/absolute/path/to/Dioptrin"'
benchmarks/fused-eiger/target/release/fused-eiger-bench \
  FILE PONI MASK_U8 COUNT_REFERENCE_JSON OUTPUT_JSON 5 codecs
python scripts/report_codec_benchmark.py OUTPUT_JSON SUMMARY_JSON
```

`codecs` compares the `dc6e303` decoder algorithms (runtime-selected upstream
unshuffle plus checked Rust LZ4) with bundled native LZ4. The rejected four-way
KCB experiment and exact sources are retained in
`docs/benchmarks/2026-11-11-codecs/` and on `codex/kcb-lz4-evaluation`. The baseline
explicitly preserves the original kernel preference, independently of later
production default changes.

Before timing, every full and streamed frame must match independently generated
h5py SHA256 references. Parallel-block pixels are compared exactly with those
verified frames. Native integer and parallel-block float64/Poisson integrations
must match the full-frame float64 control. Every timed integration output is
checked, outside timing, against the control and against the other codec variants
with exact complete-array hashes. Input orientation is mapped in the consumer.

Whole-file runs use eight frame workers, each with one integration worker.
Single-frame runs use eight block workers within one frame. They are different
workloads. Full-file warmups precede alternating, rotating variant order. The
file cache is warm; no decoded counts are cached. Reading, decoding, orientation
mapping, integration, normalization and output collection are timed. Source
indexing, lookup construction, count hashing and correctness comparisons are not.

The optional final argument `single` selects the older single-frame comparison;
omitting it selects the older complete-frame/serial-block comparison. Their
historical reports identify the exact original sources used for those results.
