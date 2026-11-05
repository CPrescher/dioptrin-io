# Native Eiger read-to-Dioptrin benchmark

This separate unpublished benchmark crate keeps integration code and its
application dependency outside `dioptrin-io`'s runtime/dependency tree. It uses a
minimal optional `native-input` adapter on a sibling Dioptrin worktree. No Python
is used by the measured binary.

The adapter's patch, base/commit provenance, external data, commands, count
hashes, scientific settings, timings and output validation are documented in
`../../docs/benchmarks/2026-11-05-native-eiger/README.md`.

Default dependency paths expect these sibling directories:

```
Programming/
  dioptrin-io/                 # this repository
  Dioptrin-io-adapter/          # isolated consumer, NOT the user's main checkout
```

Do not place detector files in this repository. After supplying the documented
external source and calibration, use `scripts/eiger_reference.py` to produce the
independent count manifest and geometry-order mask. Then build/run the Rust
binary with the paths shown in the report. The reference-generation Python tools
are not required by production readers or by the measured executable.
