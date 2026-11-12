# dioptrin-io for Python

Native detector-file readers with NumPy image arrays. All decoding uses the
existing Rust library; FabIO and h5py are not runtime dependencies.

Version 0.2.1 bundles Rust reader 0.4.0. Its indexed/fused HDF5 path uses the
[qualified codec defaults](https://dioptrin-io.readthedocs.io/en/latest/PERFORMANCE.html#lz4-defaults-fallback-and-override),
with original counts and float64 integration precision preserved. The basic
Python reading interface is unchanged; benchmarks are manual-only.

Install from PyPI:

```sh
python -m pip install dioptrin-io
```

The release provides standard CPython 3.10–3.15 wheels for macOS Intel/Apple
Silicon, Windows x64, and manylinux 2.28+ x64/ARM64. Windows ARM64 wheels cover
CPython 3.13–3.15. Supported wheels need no Rust compiler, CMake, FabIO or h5py
installation; NumPy is the runtime Python dependency. macOS wheels target 11.0+.

From a source checkout (Rust >=1.89, a C/C++ compiler, CMake and Git required):

```sh
python -m pip install .
```

```python
import dioptrin_io

with dioptrin_io.open("image.edf") as source:
    print(source.nframes)  # Detected automatically, for single images and stacks
    image = source[0]  # Image object; equivalent to source.read(0)
    print(image.data.shape, image.data.dtype)
    header = image.header
    metadata = image.metadata

with dioptrin_io.open("scan.h5", dataset="/entry/data/data") as scan:
    print(scan.nframes)
    first = scan[0]
    if scan.nframes > 1:
        second = scan[1]
    for image in scan:  # Decodes one frame at a time
        print(image.data.shape, image.header, image.metadata)
```

`open()` is the single file-opening entry point. It inspects the file and exposes
`source.nframes` (also `len(source)`) without loading the complete image stack
into memory. Some formats require an indexing scan; gzip/bzip2 wrappers may be
decompressed into a temporary file on disk. `source[index]` or
`source.read(index)` decodes one selected frame as an `Image` containing `.data`
(a NumPy array), `.header` (the parsed source header dictionary) and `.metadata`
(additional type, layout and provenance information).
This works for both single-image files
(`nframes == 1`) and multiframe files, so you do not need to know beforehand
which kind of file you have. `source.read()` selects frame zero. The data,
header and metadata are captured together under the reader's source checks.
Each returned Image owns independent data, header and metadata snapshots; all
remain valid after the source is closed. Editing a returned array or dictionary
affects only that Image, not the source or later reads.

Iteration yields Image objects in stored frame order, loading a frame only when
`next()` requests it. Each iterator starts at frame zero and has its own position;
indexing and `read()` do not change it. Keep the reader open during iteration.
Read failures propagate, including source changes and a reader closed midway
through iteration. Exhausted iterators remain exhausted. Indexing accepts
nonnegative integer frame indices; negative indices and out-of-range indices
raise `IndexError`. Slices are not supported. `list(source)` explicitly collects
all returned Images in memory; ordinary iteration does not retain earlier Images.

Paths accept strings, bytes and `os.PathLike` objects. Content discovery, not the
extension, selects a reader. Eiger, LImA and Lambda HDF5 conventions can be
discovered; other HDF5 files need `dataset=`. This initial Python API rejects
external HDF5 links and does not expose VDS policies, raw layouts, partial-source
recovery, sparse reconstruction, file series or format-specific compatibility
options. Such inputs fail explicitly. Only paths, not Python file-like objects,
are accepted in this initial interface.

Arrays preserve stored axes, original counts and reserved values. Integer
widths stay integer; complex values stay complex. Color arrays use
`(rows, columns, channels)`. Fortran NPY storage is reordered into the same logical
axes. Arrays use native byte order and independent writable NumPy storage: one
copy from the decoded Rust buffer, without conversion through Python lists or
float64. Arrays remain valid after closing, reading another frame or a later
read error. No intensity/geometry corrections, flips or TIFF Orientation
transformations are applied. Existing reader limitations still apply, including
TIFF binary16 widening to float32.

NPY/HDF5 binary16 maps to `float16` without changing its bits, and Boolean arrays
retain original storage bytes (inspect with `.view("uint8")` when needed).
Integer complex MRC samples use a structured dtype with signed16 `real` and
`imag` fields, preserving exact pairs rather than casting to floating complex.

## Header fields and additional metadata

`image.header` contains the fields exposed by the existing native reader. Values
retain their parsed source types: textual numbers remain strings, binary numbers
become Python integers/floats, byte fields stay bytes, and arrays stay lists or
tuples. No units, corrections or orientations are applied. Field spelling and
case follow each native reader (EDF keys, for example, are uppercase). Original
header bytes remain separately available in `image.metadata["raw_header"]` for
formats with retained contiguous header bytes.

| Format | `.header` dictionary |
|---|---|
| NPY | `descr`, `fortran_order`, original `shape` tuple |
| EDF, d*TREK/HiPiC and text detector headers | Native parsed assignment maps; EDF includes inherited general fields |
| TIFF/BigTIFF | Original integer tag IDs, including private tags; rational values are exact `(numerator, denominator)` tuples, including zero denominators |
| Detector TIFF | TIFF tags plus an `acquisition` dictionary from the detector header |
| CBF | Separate `cif`, `binary` and `loops` dictionaries/lists; Pilatus text assignments under `pilatus` when available |
| HDF5 | Attribute dictionaries keyed by absolute object path: `/` and the selected dataset's ancestors and dataset; no other datasets are loaded |
| Bruker, Oxford, R-AXIS, FIT2D, GE, MPA | Native parsed header fields, retaining text, numeric and byte values |
| MAR345 | Native text assignments plus the original `binary_words`; no unit conversion through derived scalar helpers |
| Esperanto | Original grouped text assignments |
| SPE | Native source fields; fabricated defaults and reformatted timestamps are excluded |
| MRC | Typed MRC header fields, with original label, stamp and reserved byte blocks |
| JPEG | Original marker `segments` and the existing parser's `jfif` density records |
| JPEG2000 | Original box type/payload records and `codestream_header` bytes |
| DM3 | Source tag records with paths, names, descriptors, small raw values and explicit offsets/lengths for large values; image arrays are not loaded as header fields |
| Pixi | Validated `record_size`; the remaining opaque bytes stay in metadata |
| XSD | Native parsed dtype, shape, size, coding and optional checksum declarations |

Uninterpreted data is kept explicit rather than guessed: TIFF opaque tags are
retained under `metadata["opaque_tags"]`; CBF source order, data-block names and
parser diagnostics remain under `cif_entries`, `cif_data_blocks` and
`cif_diagnostics`. For header-only inspection, `scan.header(index)` returns the
same dictionary without decoding image pixels.

HDF5 attributes preserve scalar/array shape, integer values, UTF-8 strings and
ASCII bytes. Canonical Boolean codes become Python booleans; other stored codes
stay integers. Empty arrays become empty lists and null dataspaces become `None`.
Attribute dtype/byte-order descriptions and original shapes are retained in
`metadata["header_attribute_types"]` and `metadata["header_attribute_shapes"]`.
Compound, enum, reference and other unsupported attribute types are explicitly
listed in `metadata["header_unsupported"]` and are not substituted with values.
Inspection is bounded to 64 ancestry/rank levels, 4096 attributes and a 16 MiB
metadata budget, including Python list structure. Variable-length string sizes
are checked after the HDF5 library decodes them, as with existing native text
metadata reads. This is scoped attribute access, not a scan of the entire NeXus
hierarchy or an interpretation of neighboring datasets as header fields.

`Image.metadata` also contains `format` and `details`. The latter preserves
the Rust source metadata as diagnostic text and is **not a stable serialization
schema**. Retained original contiguous header bytes are named `raw_header`;
native/EDF/HDF5 provides `source_shape`, native/EDF/TIFF/HDF5 provides
`little_endian`, and HDF5 provides `type_descriptor`. Consult original metadata
for source dtype declarations and format-specific storage interpretations.
`Reader.metadata(frame=0)` remains available for metadata-only inspection
without decoding image pixels; it returns the same dictionary structure.

Readers capture immutable source snapshots and check for changed files on every
read. Close and reopen after acquisition/file changes. Reads release the Python
GIL; calls on one reader are serialized. Negative/out-of-range frame indices
raise `IndexError`, closed readers raise `ValueError`, and decoder/source/bounds
errors raise `OSError`. The Rust readers' default resource limits apply (normally
256 MiB per decoded frame); this initial Python API does not override them.

The default Python build enables HDF5 and the available dense image formats.
Rust consumers retain empty default features and no Python dependency. A smaller
Python build can use `maturin build --no-default-features`, retaining
NPY/d*TREK/HiPiC/PNM, TIFF, CBF, EDF and outer gzip/bzip2 wrappers.

## Fused fixed-mask integration (0.2.0)

Standard wheels also provide `dioptrin_io_fused.FusedHdf5Integrator`, so Qt
Dioptas can use single-decode fixed-mask Eiger display/integration without
building a separate adapter. The bridge preserves uint16/uint32 display counts
and uses float64 weights, sums and Poisson terms. It supports multiple workers
within a frame. Dynamic masks and batch/map keep their existing application
paths. See the [bridge contract](../python-fused/README.md).

The reader API is unchanged. The Rust reader crate remains independent of
Dioptrin; the Python binding's default `fused` feature links the fixed-mask plan.

## Development and packaging

```sh
python -m pip install maturin numpy pytest h5py tifffile
maturin develop
python -m pytest
cargo fmt --manifest-path python/Cargo.toml --check
cargo clippy --manifest-path python/Cargo.toml --all-targets --all-features -- -D warnings
maturin build --release --out dist
maturin sdist --out dist
```

The source distribution includes the core path dependency and vendored native
backends. Wheels include native-reader/codec license notices. Building a wheel
locally does not publish it to PyPI. The `Python release wheels` workflow builds
and tests every release wheel. `Publish Python package` validates the complete
artifact matrix and tested commit before uploading through PyPI Trusted
Publishing. Existing Rust tag `v0.1.0` retains its original source release;
Python releases use separate `python-vVERSION` tags.

h5py is an optional independent test encoder. Where its CPython 3.15 encoder
wheel is unavailable, those encoder-generated tests are explicitly skipped;
the pre-generated licensed HDF5 corpus is still compared bit-for-bit through
the Python/NumPy interface on every release interpreter.
