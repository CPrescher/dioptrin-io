# Benchmark reports

[Performance guide](../PERFORMANCE.md)

These reports preserve measurements from specific source revisions. They are
historical evidence, not a benchmark of the current checkout or every released
package. Use each report's source hashes, inputs, validation and commands when
reproducing a result. Large detector files remain outside Git.

## Choose the measured operation

| Scope | Report | Input and hardware | Comparison |
|---|---|---|---|
| File read + decode | [ARM NEON kernels, November 6](2026-11-06-neon-eiger/README.md) | Acquired Eiger 9M, 625 uint32 frames; Apple M4 Pro | Prior native reader, updated NEON reader, and `bslz4_to_sparse` CPU decompression |
| File read + decode | [Reusable codec buffers, November 6](2026-11-06-scratch-eiger/README.md) | Same acquired file and host, separate session | Allocating/reusable native buffers and `bslz4_to_sparse` CPU decompression |
| File read + decode | [Windows SIMD, November 7](2026-11-07-windows-simd/README.md) | Synthetic sparse/dense uint32 Eiger-sized frames; separate x86-64 and ARM64 CI machines | Scalar/SSE2/AVX2 or scalar/NEON on each machine |
| File read → integrated pattern | [Native typed consumer, November 5](2026-11-05-native-eiger/README.md) | Acquired Eiger 9M, 625 frames; Apple M4 Pro | General HDF5 reader versus indexed raw reader, same Dioptrin consumer |
| File read → integrated pattern | [Fixed-mask block fusion, November 7](2026-11-07-fused-fixed/README.md) | Same acquired file and host | Full-frame versus fused decoding/integration; single-frame latency and scan throughput reported separately |
| Qt open → applied pattern | [Combined Qt loader, November 7](2026-11-07-qt-combined/README.md) | Same acquired file and host; offscreen Qt | Existing versus combined image/pattern loading, including controllers and debounce |

All dates are in 2026. The acquired-file reports use warm OS caches on an
unreserved host. The Windows report uses warm caches on shared CI machines.
None measures cold-device bandwidth. Decoder fps, integrated-pattern fps and
Qt latency have different timing boundaries and cannot form a single ranking.
Worker counts, repeated-sample ranges and excluded setup costs remain in the
individual reports. Results from separate sessions are not paired comparisons.

## External comparison: `bslz4_to_sparse`

The November 6 comparisons use
[`jonwright/bslz4_to_sparse`](https://github.com/jonwright/bslz4_to_sparse),
installed as **`bslz4-to-sparse==0.0.22a3`**. The
[recorded build metadata](2026-11-06-scratch-eiger/results.json) reports
`git: v0.0.22.a3`, no modified files, and source SHA256
`fe6b3c32d2a22d0e0a06fa000a3c14df7ee26459db697fa603a65369a7db9ced`.
The repository's
[`v0.0.22.a3` tag](https://github.com/jonwright/bslz4_to_sparse/tree/v0.0.22.a3)
resolves to commit `936ce98166fefff75ac0630069ac8d8370756478`.
The installed-build metadata, rather than a current default branch, identifies
the measured package.

| Report label | Measured entry point | Archived identifier |
|---|---|---|
| `bslz4_to_sparse` public default (KCB) | Public `decompress_from_buffer`, default KCB unshuffle in the measured build | `jon-kcb` |
| `bslz4_to_sparse` explicit NEON (diagnostic) | Private `_ext.decompress` with `lz4-stock` and `neon` selectors | `jon-neon` |

Both variants reconstruct complete original uint32 images. These are comparisons
of usable reader/decoder paths, including their respective LZ4 implementations
and API overheads. They do not measure the repository's sparse-output or fused
integration paths, or isolate the unshuffle kernel alone. The upstream
[versioned README](https://github.com/jonwright/bslz4_to_sparse/blob/v0.0.22.a3/README.md)
describes its broader processing pipeline and credits the contributing codecs.

Archived JSON and the original reproduction driver use the legacy identifiers
`jon-kcb`, `jon-neon`, `jon_build` and `--jon-child`. The table above maps them
to repository/API names. These files remain unchanged for reproducibility.

## Recording another measurement

Record the source revision and local changes, executable/package hashes, CPU/OS,
allocated CPUs, worker counts, dtype/shape, filter/chunk layout and external input
provenance. Validate every original count independently before timing a decoder;
validate consumer outputs separately when integration is included. Preserve
reserved values and stored orientation unless the measured consumer explicitly
requires a documented transformation.

State the exact start/end of timing, cache state, retained buffers, payload
budget, setup costs and excluded operations. Alternate implementation order,
retain individual samples and report their median and observed spread. Keep
synthetic and acquired inputs distinct, and compare implementations within the
same session and machine. A payload admission budget is not a measured RSS cap.

Add the report to this index with links to raw timings, validation, provenance
and reproduction commands. Performance claims should cite that report and its
measured revision, without implying qualification of later releases.

```{toctree}
:hidden:
:maxdepth: 1

2026-11-05-native-eiger/README
2026-11-06-neon-eiger/README
2026-11-06-scratch-eiger/README
2026-11-07-windows-simd/README
2026-11-07-fused-fixed/README
2026-11-07-qt-combined/README
```
