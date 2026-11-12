# dioptrin-io user manual

dioptrin-io reads detector files for Rust applications, including Dioptrin and
diffraction-analysis tools. It preserves native samples and source metadata;
applications own orientation changes, masking, corrections and integration.
Optional codecs include bundled C libraries, without a Python decoding runtime.

Start with [installation and release scope](RELEASING.md), the
[overview and examples](overview.md), and the [reader contracts](READER_DESIGN.md).
The [coverage catalog](FORMAT_COVERAGE.md) and [current gaps](READING_GAPS.md)
describe qualified formats and explicit limitations. Coverage of a pinned test
corpus does not establish support for every acquired detector file.

```{toctree}
:caption: Getting started
:maxdepth: 1

Installation and release scope <RELEASING>
Python package and API <PYTHON_API>
Overview and examples <overview>
INPUT_APIS
READER_DESIGN
```

```{toctree}
:caption: Reader guides
:maxdepth: 1

HDF5_LAYOUTS
HDF5_SOURCE_CONTRACTS
TIFF_LAYOUTS
IMAGE_READERS
SPARSE_READERS
SERIES_API
NAVIGATION_API
reader-reference
```

```{toctree}
:caption: Performance and validation
:maxdepth: 1

PERFORMANCE
FORMAT_COVERAGE
READING_GAPS
VALIDATION
FABIO_PARITY
validation-reports
```

```{toctree}
:caption: Project
:maxdepth: 1

changelog
BUILDING
```

[Dioptrin's manual](https://dioptrin.readthedocs.io/en/latest/) covers diffraction
integration. File-reading comparisons and SIMD decoder measurements are maintained
here in the [performance guide](PERFORMANCE.md), with their input, revision and
hardware conditions.
