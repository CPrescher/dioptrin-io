# Repository guidelines

`dioptrin-io` provides native detector-file input independently of Dioptrin,
PhaseSmith, Python and integration mathematics.

- Preserve original counts, source dtype metadata, reserved values and stored
  orientation. Consumers own flips, masks, corrections and normalization.
- Keep format dependencies optional. The initial `hdf5` feature is adapted from
  `phasesmith-hdf5`; retain its MIT attribution and native-codec license notices.
- Validate shape/resource bounds and unsupported filters/layouts explicitly.
- Treat source changes, external links, growing datasets and decoded caches as
  explicit contracts; do not silently substitute data or reuse stale counts.
- Reuse existing readers before adding implementations. New parallel fast paths
  need independent count comparisons and complete read-to-consumer benchmarks.
- Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  and `cargo test --all-features`. Verify no-default-features builds separately.
- Keep large detector files outside Git; retain small licensed independent
  fixtures and record external dataset provenance for performance claims.
- Use commit prefixes such as `feat:`, `fix:`, `test:`, `bench:` and `doc:`.
