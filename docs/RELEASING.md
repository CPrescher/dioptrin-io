# Source release and Rust desktop consumption

The current Rust source release is `v0.3.0`. Its HDF5 source-contract APIs are
described in [the native contract guide](HDF5_SOURCE_CONTRACTS.md). The Python
distribution remains independently versioned at 0.2.0 for this Rust-only release.

The first Rust release is `v0.1.0`, a GitHub source/tag release of the Rust crate.
The intended consumer of that release is Rust desktop Dioptas; it does not
change Dioptas. The working tree also provides an optional Python binding crate
and `pyproject.toml`; see [Python packaging](../python/README.md). These additions
do not retroactively change the original `v0.1.0` archives.

## Dependency contract

```toml
[dependencies]
dioptrin-io = { git = "https://github.com/CPrescher/dioptrin-io.git", tag = "v0.3.0", features = ["wrappers", "hdf5", "jpeg", "jpeg2000", "mar345"] }
```

Select any additional formats required by the application: `containers`,
`bruker`, `oxford`, `raxis`, `fixed`, `fit2d`, `kcd`, `mpa`, `esperanto`,
`sparse`, `xcalibur`, `xsd`, `columnfile` and `chiplot`. `wrappers` includes
`native`, `tiff`, `cbf` and `edf`; `sparse` includes `hdf5`. Default features
are empty. Use explicit typed/source policies rather than automatic fallback.

The complete source includes `vendor/tiff-0.11.3` and `vendor/fax-0.2.7`.
Both direct and transitive fax references use that sibling source. Cargo ignores
a dependency crate's root patch table; relying on it previously caused a real
consumer compile failure (`StrictRowDecoder` missing). The consumer check now
verifies the resolved backend source and six independent fax image counts and
metadata from a separate temporary Cargo project, without a patch section.

Registry publication is disabled (`publish = false`). Cargo would strip path
references and select unpatched registry TIFF/fax versions. Publishing patched
backends or incorporating them differently is a separate packaging task; this
release does not advertise `dioptrin-io = "0.3.0"` on crates.io.

## Runtime and build requirements

The core Rust readers need no installed Python, NumPy, FabIO or h5py. The optional
Python package requires Python >=3.10 and NumPy >=2; FabIO/h5py remain unnecessary
at runtime. Optional HDF5/filter,
JPEG/JPEG2000 and ThunderScan code includes native libraries; this is native
code without Python decoding, not a pure-Rust implementation of every codec.
The relevant features require a C/native compiler and CMake; HDF5 also requires
Git during its bundled build. Original MIT/native codec notices remain included.

## Python release validation

The Python distribution is named `dioptrin-io` and imports as `dioptrin_io`.
Its separate manifest is `python/Cargo.toml`; the core manifest stays independent
of PyO3/NumPy and integration mathematics. The Python default `fused` feature
links a pinned Dioptrin implementation and supplies `dioptrin_io_fused` in the
same wheel. Every release wheel checks counts and float64 patterns against
recorded independent reference results. The Python default features enable supported dense readers;
specialized source/recovery policies remain outside this initial Python API.

The `Python release wheels` workflow (`python-release.yml`) builds and tests 33
portable wheels: CPython 3.10–3.15 for macOS Intel/Apple Silicon, Windows x64 and
manylinux 2.28+ x64/ARM64, plus CPython 3.13–3.15 for Windows ARM64. The sdist is
rebuilt and tested outside the source checkout. Wheel notices and strict Twine
metadata validation are required. The normal `Python package` CI additionally
checks the binding crate and representative Python versions on all three OSes.

For a Python release:

1. Set matching versions in `pyproject.toml` and `python/Cargo.toml`, updating
   `python/Cargo.lock`. Do not move or replace the original Rust `v0.1.0` tag.
2. Push the candidate branch `codex/python-pypi-release` (or dispatch the wheel
   workflow) and require its complete wheel matrix, source archive, Rust CI and
   Python CI to pass. Keep the successful wheel workflow run ID.
3. Fast-forward `main` to that tested commit, then tag it `python-vVERSION`.
4. Dispatch `pypi-publish.yml` at that tag with `artifact_run_id` and `release_tag`.
   It verifies that the successful build run and release tag point to its exact
   source commit, checks all 33 portable wheels plus the sdist, and publishes only
   those tested artifacts. It does not rebuild artifacts in the publishing job.
5. Verify PyPI file names/hashes and wheel-only installation, then attach the same
   archives to a GitHub release for the Python tag.

PyPI Trusted Publishing is scoped to owner `CPrescher`, repository `dioptrin-io`,
workflow `pypi-publish.yml`, and environment `pypi`. The upload job alone has
OIDC permission; build and validation jobs have no publishing credentials.
For the first upload, configure this as a pending publisher for `dioptrin-io`.
No long-lived PyPI token is stored in the repository.

## Release acceptance

1. Regenerate/check the classified inventory and document unsupported variants:
   `python scripts/inventory_fabio.py --check`.
2. Run repository formatting, strict all-target/all-feature Clippy, all-feature
   tests and separate no-default checks/tests. Keep actual large detector sources
   outside Git; recorded corpus proofs remain scoped to their tested revision.
3. Check the crate as an external path consumer:
   `python scripts/check_consumer.py --all-features`.
4. Commit a candidate and check that exact Git revision without root patches:
   `python scripts/check_consumer.py --all-features --git file:///path/to/dioptrin-io --rev COMMIT`.
5. Push the candidate branch and require the Linux/macOS/Windows CI and inventory
   jobs to pass. Advance the default branch only by fast-forward from the verified
   remote head, then tag the tested commit `vVERSION`.
6. Publish the GitHub release with this scope and links to
   [unsupported variants](READING_GAPS.md), then verify a consumer resolves the
   released Git revision. Registry publishing is not part of these steps.

The corpus has 626 qualified outcomes and no unclassified reader failure, not
626 distinct acquired files. Six supplemental HDF5/NPY dtype cases remain
explicit native compatibility gaps. Additional codecs/layouts are file-triggered
optional variants, not an automatic release-blocking queue. The next qualification
work is the [acquired detector-file matrix](READING_GAPS.md#next-acquired-detector-files).
