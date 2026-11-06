# Source release and Rust desktop consumption

The first release is `v0.1.0`, a GitHub source/tag release of the Rust crate.
The intended consumer is Rust desktop Dioptas. Python bindings and application
migration are separate work; this release does not change Dioptas.

## Dependency contract

```toml
[dependencies]
dioptrin-io = { git = "https://github.com/CPrescher/dioptrin-io.git", tag = "v0.1.0", features = ["wrappers", "hdf5", "jpeg", "jpeg2000", "mar345"] }
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
release does not advertise `dioptrin-io = "0.1.0"` on crates.io.

## Runtime and build requirements

The readers need no installed Python, NumPy, FabIO or h5py. Python is used only
for reference qualification and development checks. Optional HDF5/filter,
JPEG/JPEG2000 and ThunderScan code includes native libraries; this is native
code without Python decoding, not a pure-Rust implementation of every codec.
The relevant features require a C/native compiler and CMake; HDF5 also requires
Git during its bundled build. Original MIT/native codec notices remain included.

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
   remote head, then tag the tested commit `v0.1.0`.
6. Publish the GitHub release with this scope and links to
   [unsupported variants](READING_GAPS.md), then verify a consumer resolves the
   released Git revision. Registry publishing is not part of these steps.

The corpus has 626 qualified outcomes and no unclassified reader failure, not
626 distinct acquired files. Six supplemental HDF5/NPY dtype cases remain
explicit native compatibility gaps. Additional codecs/layouts are file-triggered
optional variants, not an automatic release-blocking queue. The next qualification
work is the [acquired detector-file matrix](READING_GAPS.md#next-acquired-detector-files).
