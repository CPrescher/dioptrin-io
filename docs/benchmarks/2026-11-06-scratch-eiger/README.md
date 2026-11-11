# Reusing Bitshuffle scratch: Eiger9M read/decode

[Benchmark index](../index.md) · Historical measurement: 2026-11-06.
Results and implementation descriptions apply to the recorded source revisions.

The indexed native HDF5 reader now owns two block buffers per worker: the
LZ4-expanded shuffled bytes and the intermediate unshuffle scratch. Its small
MIT-licensed C adapter calls the existing upstream Bitshuffle primitive pair
with that scratch. No generic blocking wrapper, malloc or free runs inside the
adapter. This preserves the previous compile-time ISA selection (NEON on this
host; SSE2/AVX2/AVX512/scalar elsewhere) and original upstream kernel arithmetic.

The baseline already includes the new NEON kernels from the previous experiment.
This comparison measures scratch reuse **plus bypassing the generic single-block
wrapper**, not the much larger old-to-new NEON improvement.

## Comparison implementation

The external reader/decoder is
[`jonwright/bslz4_to_sparse`](https://github.com/jonwright/bslz4_to_sparse),
installed as `bslz4-to-sparse==0.0.22a3` (recorded build tag `v0.0.22.a3`).
The [benchmark index](../index.md) defines the measured APIs and maps the
historical JSON identifiers to these labels.
Both external variants return full uint32 images; sparse-output and fused
integration paths are outside this comparison.

## Results

| Reader/decoder | 1 worker fps, median (range) | 8 workers fps, median (range) |
|---|---:|---:|
| NEON before: allocating wrapper | 95.37 (95.35–96.66) | 655.38 (646.98–655.62) |
| NEON after: reusable scratch | 97.62 (97.13–98.04) | 730.53 (725.35–733.85) |
| `bslz4_to_sparse` public default (KCB) | 80.70 (79.10–84.78) | 619.98 (617.99–620.42) |
| `bslz4_to_sparse` explicit NEON (diagnostic) | 93.80 (92.72–93.93) | 714.30 (714.01–715.67) |

Scratch reuse and removing the generic block wrapper increase native median
throughput by **2.4% at one worker** and **11.5% at eight workers**.
The new native median exceeds the `bslz4_to_sparse` explicit NEON median by
4.1% / 2.3% at 1 / 8 workers in this run. The larger eight-worker benefit is
consistent with reducing allocation/wrapper overhead during parallel decoding; this
experiment does not split those two contributions or prove allocator contention.

The allocating native baseline and both `bslz4_to_sparse` variants also ran
faster than in the earlier report. Consequently the scratch gain is the paired
**655.38 to 730.53 fps** comparison, not the difference from the earlier session's 597.37
fps. Absolute throughput is sensitive to run conditions on this unreserved host.


Full-file **read + decode**, original uint32 pixels and stored orientation, no
integration, masking, normalization or float conversion. Three alternating
full-file timed passes per implementation at 1 and 8 workers, after all-frame
validation. The file cache was warm; these are not cold SSD rates. The machine
was not reserved or isolated, and each sample records background load averages.

The input is the same 625-frame Eiger 9M file as before:

- `/Users/clemens/Programming/Dioptrin/test_data/eiger_0000.h5`
- dataset `entry_0000/instrument/eiger/data`, shape `[625, 3262, 3108]`, uint32;
- file bytes 5,687,072,680, one Bitshuffle/LZ4 chunk per frame;
- SHA256 `2b749693d7a942b35d89806d99b9a1025ee119850be0ade27428f1dc11d0a1f3`;
- Apple M4 Pro, 14 cores, 48 GiB RAM, macOS 26.6.2, Rust 1.99.1.

File reads, decoding, immutable-source checks, buffer/worker creation and
borrowed-pixel delivery are timed. Opening, chunk indexing, interpreter startup
and reference hashing are excluded; setup times are retained separately.
Neither reader queues or caches decoded frames. The 2 GiB decoder payload
admission remains unchanged and conservatively covers both retained block
buffers. It is not a process RSS cap.

The `bslz4_to_sparse` public KCB default and explicit NEON diagnostic were rerun
in this same comparison using installed `bslz4-to-sparse` 0.0.22a3. Both use that
package's C LZ4 decoder, while native retains checked Rust `lz4_flex` 0.14.0. The explicit
NEON variant is a private-entry diagnostic, not the default or fused integration.
The full shared protocol and API overhead boundaries are in the
[previous NEON report](../2026-11-06-neon-eiger/README.md).

## Validation and provenance

Each implementation is checked against the independent HDF5 reference's complete
625 frame SHA256 values at both worker counts, outside performance timing.
Counts include zero, high-bit and reserved pixels. The original reference is
[the count-reference manifest](../2026-11-05-native-eiger/count-reference.json).

New regression checks compare reusable-scratch decoding against independent
stock-encoded vectors for 1/2/4/8-byte elements, multiple blocks and raw tails,
with guarded scratch/output buffers and rejected invalid scratch lengths,
element counts and overflow. Another regression confirms that successive frame
reads retain the scratch allocations. Existing malformed input, source-mutation,
uint16/uint32 and parallel reader tests also pass.

Local ARM macOS checks: formatting, strict all-targets/all-features Clippy,
**407 all-feature tests**, no-default-features build, release benchmark build and
Ruff. Other CPU/OS execution was not measured here. No integration mathematics,
application consumer, release or publication is part of this change.

[Raw results](results.json) retain every timing, count-validation result,
executable hash, package version and host-load sample. [Provenance](provenance.json)
records source hashes before/after, compiler and baseline identity. The baseline executable SHA256
matches the **updated NEON** binary in the previous report, not the original
slow decoder. `scratch-reuse.patch` retains the complete native implementation
delta, including the local adapter; original upstream files/patches are intact.

## Reproduce

Restore the measured source state using the [provenance](provenance.json) and
[implementation patch](scratch-reuse.patch), then build the reader. Applying
or reversing this historical patch on a newer checkout is a different experiment.

```sh
cargo build --release --features hdf5 --example benchmark_eiger_decode --locked
```

In a separate copy of this source tree, reverse `scratch-reuse.patch` and build
the same example to obtain the allocating NEON baseline. The benchmark example,
LZ4 implementation, file reader, native kernels and release settings must match.
Run the shared driver using the reference Python environment:

```sh
/Users/clemens/Programming/Dioptas/.venv/bin/python scripts/benchmark_eiger_neon.py \
  --source /Users/clemens/Programming/Dioptrin/test_data/eiger_0000.h5 \
  --reference docs/benchmarks/2026-11-05-native-eiger/count-reference.json \
  --baseline /tmp/dioptrin-io-scratch-20261106/before-reader \
  --native target/release/examples/benchmark_eiger_decode \
  --baseline-label native-neon-allocating --native-label native-neon-scratch \
  --output /tmp/scratch-eiger-results.json
```

Keep the original detector file external and immutable. Copying the existing
baseline binary is sufficient on this host; binaries themselves are not tracked.
