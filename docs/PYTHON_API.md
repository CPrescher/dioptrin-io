# Python package and API

```{include} ../python/README.md
:start-line: 2
```

## Optional fused integration adapter

Starting with 0.2.0, the standard `dioptrin-io` wheels include the
`dioptrin_io_fused` compatibility module used by Qt Dioptas. Install it with
`python -m pip install --only-binary=:all: dioptrin-io`; no separate adapter build
or locally patched Cargo dependency is needed. The Python wheel links Dioptrin's
fixed-mask float64 plan; the core Rust reader crate remains independent of
Dioptrin and integration mathematics. The bridge contract is described in
[`python-fused/README.md`](../python-fused/README.md).

The [native benchmark](benchmarks/2026-11-07-fused-fixed/README.md) compares
compressed-input pipelines: 16.73 to 5.38 ms for intensity and 17.31 to 6.09 ms
with Poisson errors, using eight cores, float64 arithmetic and a fixed mask on
Eiger 9M data. File cache and the prepared lookup were warm; lookup setup was
reported separately. This is approximately a threefold latency improvement.

Qt's first adapter reread an already decoded display image and was slower. The
combined API now provides `load_integrate1d(frame, errors=False)`, returning a
read-only stored-order NumPy count array and a float64 result from one decode.
`set_source(path, dataset)` retains the fixed lookup for compatible shapes while
replacing the immutable source. The GUI maps rows for display and passes its
prepared pattern to the integration worker without another file read.

The [combined Qt report](benchmarks/2026-11-07-qt-combined/README.md) measures the
real image/pattern controllers, coordinator, default 25 ms debounce and pattern
application. It reports approximately 20% lower latency on the tested M4 Pro/Eiger
9M setup, while preserving all 625 independent count hashes and float64 outputs.
The API is bundled in 0.2.0 wheels. Older wheels retain their existing reader
API and do not gain fused integration automatically. Dynamic masks and batch/map
processing retain their existing application paths.
