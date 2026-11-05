# Small independent detector fixtures

The original `counts-bitshuffle-lz4.h5` and its JSON retain their extracted
PhaseSmith provenance and MIT attribution.

New synthetic TIFF, CBF and EDF arrays and generated fixture files are dedicated
to the public domain under CC0-1.0 by the dioptrin-io authors. They contain no
facility/user measurement data. File SHA256, reference versions and generation
settings are recorded in `tiff/provenance.json` and `binary-provenance.json`.

- `scripts/generate_tiff_fixtures.py`: NumPy arrays written and independently
  read back by tifffile. Covers ten scalar types, native values beyond f64 exact
  integer precision, endian BigTIFF, multiple frames, padded edge tiles,
  PackBits/LZW/Deflate, Orientation/private tags and original WhiteIsZero counts.
- `scripts/generate_binary_fixtures.py`: FabIO 2026.6.0 CBF writer/reader with
  byte-offset escapes and signed/unsigned types. The signed32 fixture uses the
  terminal32 dialect; other fixtures exercise extended64. EDF headers are
  independently assembled with NumPy and Python standard gzip/zlib/bzip2, then
  read/verified with pinned FabIO. Ten scalar types and both byte orders include
  reserved/extreme integers, nonfinite floats and signed zero.

Fixture generation dependencies are separate from the Rust runtime and tests.

`scripts/generate_native_fixtures.py` adds CC0 NPY v1/v2/v3, ten scalar types,
both byte orders, C/Fortran stacks with distinct leading-axis frames, seven
d*TREK scalar types in both byte orders, aligned/unaligned Fit2D masks, HiPiC,
Fit2D spreadsheets and PNM. Original sample hashes and per-file FabIO comparison
status are recorded in `native/provenance.json`. P1/P2 use independent original
values because the pinned reference loses rows. EDF includes small independent
legacy compression aliases, optional Size and binary padding fixtures; gzip
fixture headers now use deterministic mtime zero.

The larger upstream FabIO corpus is external, with exact file/download hashes
in the suite reports under `docs/fabio-parity/`; it is not redistributed here.

`scripts/generate_hdf5_native_fixtures.py` creates CC0 h5py files under
`hdf5-native/`: ten scalar types, both byte orders, ranks 2/3, edge chunks,
lossless filters, NaN payloads, defined fill, fill-disabled allocated/unwritten
storage, an external/soft-link source graph, LImA/Lambda metadata and negative
layout/type/link fixtures. Its manifest records original frame and file hashes.
Rust tests mutate frozen datasets and intermediate links to verify invalidation.
Reference environment used NumPy 2.5.4, FabIO 2026.6.0, tifffile 2026.10.15 and
imagecodecs 2026.10.10. Reproduction versions and the binary files' own hashes are
in the manifests. Full real Eiger data is external; its provenance/count hashes
are in `docs/benchmarks/2026-11-05-native-eiger/`.

The MAR345, sparse HDF5 and TIFF view fixture manifests document small CC0
source arrays, every source/sample hash and independent reference qualifications.
See `mar345/provenance.json`, `sparse/provenance.json` and
`tiff-view/provenance.json`. Sparse fixtures include nonempty events absent from
the actual upstream sparse corpus. MAR345 fixtures retain known pinned-oracle
integer/whitespace defects as explicit differences.
