# Upstream suite baseline, 2026-11-05: typed HDF5 continuation

**Full reading parity is incomplete.** All 35 codec modules remain in the acceptance target.

The current full run executes 213 reference tests: no failures/errors and five explicit skips. Native count/dtype/shape comparisons match **202 read events / 1,049 frames**, up from 154 events / 422 frames. Repeated reads are counted as events, not distinct files.

| Native outcome | Events |
|---|---:|
| counts_match | 202 |
| decoder_missing | 171 |
| test_adapter_missing | 0 |
| feature_disabled | 0 |
| native_rejected | 180 |
| reference_no_pixels | 25 |

The previous term "bridge missing" described test plumbing and was too broad. The new report distinguishes:

- `decoder_missing`: no format-specific Rust read entry point is implemented. A generic container reader may still read some pixels.
- `test_adapter_missing`: a compiled reader exists but the comparison adapter is missing. There are none in this run.
- `feature_disabled`: an existing optional reader was left out of the test executable. There are none in this full-feature run.
- `native_rejected`: a reader was called and explicitly rejected this variant/source. This includes unsupported layouts and intentional malformed-input differences, not just absent decoders.

The full results and source hashes are in [the current JSON](2026-11-05-hdf5.json). The [original baseline](2026-11-05.json) is retained with its historical schema. A separate [HDF5-disabled run](2026-11-05-feature-disabled.json) correctly classifies six events as feature-disabled.

## What became readable

- Typed explicit HDF5: all six generic upstream events match, including 2D and f32 stacks.
- Eiger: all seven upstream events match, including typed writer-generated stacks and dataset discovery.
- LImA: all six upstream events match through default-entry/measurement discovery.
- Lambda: ten events match. The two VDS-master events remain rejected pending explicit mapping/source-relocation contracts.
- MARCCD/Pilatus: all 19 pixel read events match through the existing TIFF decoder. Detector-specific acquisition/image-description headers remain pending; this is not complete format parity.

Independent CC0 h5py fixtures additionally verify all ten scalar types in both byte orders, partial chunks, NaN payloads, declared fill, source graphs, cycles, missing links, limits and mutation. HDF5 test sources include linked intermediate files, not only endpoint datasets.

The complete source/executable remained unchanged during this run, and every reference codec source matched the pinned upstream hash. No count/dtype/shape mismatch occurred among successful native reads. One EDF parsed-header difference (terminal NUL padding) remains. Four Glymur variants lack a discoverable OpenJPEG library; upstream explicitly skips one slow MAR345 memory-leak test.

## Per-codec read events

| Codec | Counts match | Native rejected | Format reader missing | No pixels |
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
| eigerimage | 7 | 0 | 0 | 0 |
| esperantoimage | 0 | 0 | 9 | 0 |
| fit2dimage | 0 | 0 | 4 | 0 |
| fit2dmaskimage | 8 | 0 | 0 | 0 |
| fit2dspreadsheetimage | 3 | 0 | 0 | 0 |
| hdf5image | 6 | 0 | 0 | 0 |
| jpeg2kimage | 0 | 0 | 6 | 0 |
| jpegimage | 0 | 0 | 3 | 0 |
| kcdimage | 0 | 0 | 4 | 0 |
| lambdaimage | 10 | 2 | 0 | 0 |
| limaimage | 6 | 0 | 0 | 0 |
| mar345image | 0 | 0 | 30 | 0 |
| marccdimage | 6 | 0 | 0 | 0 |
| mpaimage | 0 | 0 | 1 | 0 |
| mrcimage | 0 | 0 | 8 | 0 |
| numpyimage | 12 | 0 | 0 | 0 |
| pilatusimage | 13 | 0 | 0 | 0 |
| pixiimage | 0 | 0 | 6 | 0 |
| pnmimage | 2 | 0 | 0 | 0 |
| raxisimage | 0 | 0 | 3 | 0 |
| sparseimage | 0 | 0 | 1 | 0 |
| speimage | 0 | 0 | 42 | 0 |
| tifimage | 20 | 44 | 0 | 24 |
| xsdimage | 0 | 0 | 5 | 0 |

ADSC aliases d*TREK. Complete detector headers, VDS/source relocation, remaining codecs/dtypes/layouts, direct streams, content autodetection, rejected-input comparisons and explicit neighboring-file series remain targets. See [reproduction and contracts](../FABIO_PARITY.md) and the complete [format ledger](../FORMAT_COVERAGE.md).
