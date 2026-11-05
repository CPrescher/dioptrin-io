# Upstream suite baseline, 2026-11-05

**Full reading parity is incomplete.** This is a failing acceptance baseline.

Pinned FabIO codec modules: 35; reference unittest cases: 213; failures/errors: 0; skips: 5.

| Native comparison outcome | Read events |
|---|---:|
| counts_match | 154 |
| unsupported_reader | 221 |
| native_rejected | 178 |
| reference_no_pixels | 25 |

The matched events cover 422 frames, including repeated reads from the same files. There are no count/dtype/shape mismatches among successful decodes. One EDF parsed-header difference remains visible (terminal NUL padding).

The report retains 281 distinct successful-reference input-file hashes and 16 rejected-reference read events, including sources without a Rust adapter. `unsupported_reader` identifies a missing bridge reader; it is not a measured decoding failure of every existing library API.

The four optional Glymur variants are skipped because OpenJPEG is not discoverable by Glymur on this host. The upstream MAR345 memory-leak case is explicitly marked very slow and skipped by upstream. Pillow JPEG2000 cases still run. Skips are retained as coverage gaps.

Every installed reference codec source matches the pinned upstream file hash. The measured executable copy and all Rust source hashes stayed unchanged throughout this run. The executable is a test-only bridge; it has no production Python dependency.

The complete [JSON report](2026-11-05.json) retains individual tests, source/download hashes, frames, raw configuration, reference exceptions, parsed-header differences and environment versions. [Environment pins](reference-requirements.txt) and [reproduction instructions](../FABIO_PARITY.md) are provided.

## Per-codec read events

| Codec | Counts match | Native rejected | Bridge missing | No pixels |
|---|---:|---:|---:|---:|
| GEimage | 0 | 0 | 6 | 0 |
| HiPiCimage | 6 | 0 | 0 | 0 |
| OXDimage | 0 | 0 | 12 | 0 |
| binaryimage | 2 | 0 | 0 | 0 |
| bruker100image | 0 | 0 | 9 | 0 |
| brukerimage | 0 | 0 | 16 | 0 |
| cbfimage | 17 | 0 | 0 | 0 |
| dm3image | 0 | 0 | 6 | 0 |
| dtrekimage | 16 | 1 | 0 | 1 |
| edfimage | 68 | 133 | 0 | 0 |
| eigerimage | 0 | 0 | 7 | 0 |
| esperantoimage | 0 | 0 | 9 | 0 |
| fit2dimage | 0 | 0 | 4 | 0 |
| fit2dmaskimage | 8 | 0 | 0 | 0 |
| fit2dspreadsheetimage | 3 | 0 | 0 | 0 |
| hdf5image | 0 | 0 | 6 | 0 |
| jpeg2kimage | 0 | 0 | 6 | 0 |
| jpegimage | 0 | 0 | 3 | 0 |
| kcdimage | 0 | 0 | 4 | 0 |
| lambdaimage | 0 | 0 | 12 | 0 |
| limaimage | 0 | 0 | 6 | 0 |
| mar345image | 0 | 0 | 30 | 0 |
| marccdimage | 0 | 0 | 6 | 0 |
| mpaimage | 0 | 0 | 1 | 0 |
| mrcimage | 0 | 0 | 8 | 0 |
| numpyimage | 12 | 0 | 0 | 0 |
| pilatusimage | 0 | 0 | 13 | 0 |
| pixiimage | 0 | 0 | 6 | 0 |
| pnmimage | 2 | 0 | 0 | 0 |
| raxisimage | 0 | 0 | 3 | 0 |
| sparseimage | 0 | 0 | 1 | 0 |
| speimage | 0 | 0 | 42 | 0 |
| tifimage | 20 | 44 | 0 | 24 |
| xsdimage | 0 | 0 | 5 | 0 |

ADSC is the d*TREK alias, not a separate decoder. Some registry entries have no successful `.read` call in the selected upstream suite; those remain listed in the full coverage ledger and the report’s unexercised-codec list. General headers/external sources, detector schemas, other codecs/dtypes/layouts, streams, discovery, rejected-input comparisons and explicit file series remain part of the complete target.
