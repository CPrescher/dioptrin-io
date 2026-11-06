# Xcalibur run descriptions

The optional `xcalibur` feature exposes `xcalibur::run::{RunDescription, Sweep,
Reader, Provenance}`. These are acquisition metadata, with no image or pixel
representation. The schema comes from the MIT-licensed FabIO 2026.6.0
`xcaliburimage.py`; its retained permission notice is
`licenses/native-readers/FabIO-Xcalibur-MIT.txt`.

`RunDescription::from_bytes` copies its input into owned storage;
`RunDescription::from_reader` captures a bounded `Read` without requiring seek.
Their provenance is respectively `Buffer` and `StreamCapture`; neither claims to
monitor a file behind that buffer/stream. `Reader::open` captures a checked file
snapshot and retains its open handle. Every `metadata()` and `source_path()` call
checks the path and handle for replacement, growth or other identity changes.

Two raw 256-byte text slots precede an unsigned little-endian 64-bit sweep count
and one float64 value (528 bytes total). Every declared sweep has 88 bytes,
containing two uint16 fields, one uint32, eight float64 fields, four uint16 fields,
and one float64 exposure. Field names describe the pinned API rather than an
inferred scientific interpretation. Unknown enum values and unknown/reserved
fields remain readable. `Text.0`, `Sweep.original`, `RunDescription.original`
and `trailing_bytes()` retain source octets, including embedded NULs and invalid
UTF-8. Float values retain all source bits, including signed zero, NaN payloads,
infinities and subnormal values. Decoding text is an explicit fallible view.

The reader checks the complete counted extent and multiplication/addition bounds
before allocating sweep records. `Limits.frames` bounds the number of sweeps;
`encoded_bytes`, `header_bytes` and aggregate `metadata_bytes` bound input and
parsed storage. The aggregate charge includes both original bytes and parsed
records. A short header or any short counted sweep fails; no record is padded or
fabricated. Uncounted trailing bytes are preserved without an invented meaning.

The three independent fixtures in `tests/data/xcalibur/*.run` are CC0-1.0,
created by `scripts/generate_xcalibur_run_fixtures.py` using direct fixed-layout
packing. `run-provenance.json` records their sizes and hashes. No upstream
serializer generated them. Rust controls cover every field, exact float bits,
all counted truncations, overflow/limits, nonseekable input, owned-buffer
isolation, file growth and same-size replacement.

`scripts/companion_reference.py` qualifies the separate native path, byte-buffer
and stdin APIs against the unchanged pinned `RunDescription.read/loads` and
`Sweep.loads` APIs, with an independent raw-slot oracle. Every field, raw text
slot, float bit pattern, sweep/source/trailer hash and provenance is checked.
Fresh reference objects provide meaningful-field roundtrip diagnostics;
reference serialization is not a source writer or a production dependency.
The reference strips surrounding text NULs and omits trailers on serialization,
so semantic roundtrips do not imply byte preservation. The invalid-UTF8 fixture
has a recorded pinned `RunDescription` text rejection: native raw preservation
and standalone `Sweep` parity are qualified separately from a successful
reference run read.

## Remaining companion API scope

A read-only search of top-level pinned FabIO classes and their test references
found these distinct non-image APIs. No additional `.run` file was found in the
external FabIO acceptance corpus; its codec tests do not exercise
`RunDescription` or `Sweep`. The application `app/eiger2crysalis.py` produces run
descriptions, confirming the acquisition metadata role, but is a writer rather
than another source reader.

| API | Current boundary |
| --- | --- |
| `xcaliburimage.CcdCharacteristiscs` and its point/row/column/polygon/machine subrecords | Existing native CCD reader and separately qualified geometry APIs; not new run counts. |
| `datIO.columnfile.read`, `ext.cf_io.read` | Separate labeled numerical-table reader, including ASCII/binary extension modes. No references were found in pinned tests. Not implemented or claimed by this task. Existing MIT C/Cython reader must be evaluated before a native implementation. |
| `cbfimage.CIF.loadCIF`, `CIF.loadCHIPLOT` | Standalone CIF scalar/loop projection is now available through the existing detector parser, with explicit grammar/binary/scope limits; positive profiles qualify the path/BytesIO reference entry points. CHIPLOT remains a separately observed broken helper, described below. No pinned test references were found. |
| `TiffIO.TiffIO.getData/getInfo/getImageFileDirectories` | Existing image/header backend; its standalone low-level API is distinct from a new companion file format and needs its own API qualification. |
| `cbfimage.PilatusHeader` | A text-metadata parser used by existing detector CBF/Pilatus readers; direct constructor/mapping API remains a separate compatibility question. |
| `nexus.Nexus` | General HDF5/NeXus access and writer helper (`get_entry/get_entries/find_detector/find_data/get_default_NXdata`). Native typed dataset/source-graph reads do not claim an h5py-object API replacement. |
| Filename/series utilities and file/compression wrappers | Transport/iteration utilities rather than additional stored companion formats. Their entry points belong in the separate API inventory. |

The audited checkout is commit
`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`. Relevant source SHA256 values:

| Source | SHA256 |
| --- | --- |
| `xcaliburimage.py` | `f8bf057585d3ae26bde5139ea0bb5ae4b594e8b3d729e35fff0e4eec87dfd611` |
| `datIO.py` | `c9bee7f0ebeb263c67aeaee4e7183dbae45aab7a280f6e5f8ea354944d32292a` |
| `ext/cf_io.pyx` | `2cc6f074376422420fe72d66ae4a2b6df792dea329b5ce84d7dcfcf3a930a2fa` |
| `cbfimage.py` | `0161b83dce84114421767969ccfef6ae8d51bf90739ab84cc5cf390da1e844d3` |
| `TiffIO.py` | `761e09edb72380be24fb313a60f456b8e2f3d73c26acbe2ccbd02b5c88bdbb50` |
| `nexus.py` | `8781afe25acd6b987c26882a458c27e8dd21665b037d01f968354f351d8bf960` |
| `app/eiger2crysalis.py` | `57e7074e2c89573f3db56f5345fdb601e67557b9cb88456f0e517947e40dfb45` |

## Standalone CIF document projection

The optional `cbf` feature now exposes `cbf::document::{Document, Reader,
Coverage, Provenance}` independently of the wrapped image APIs. This wrapper
calls the existing `cbf::header::parse`; it introduces no second CIF parser.
`from_bytes` and `from_reader` retain an owned original source, labelled
`Buffer` or `StreamCapture`. The path reader retains a checked source identity
and file handle, and checks them on every metadata/path access. Encoded, header
and aggregate retained-metadata bounds remain explicit.

Every result is labelled `HeaderProjection`, with a coverage warning. It retains
original bytes, data-block names, scalar entries in source order (including
repeated assignments), final scalar values, loop columns/rows and every existing
parser diagnostic. This projection is not complete CIF grammar validation. The
shared parser stops before reserved `_array_data.data` bodies; save frames and
other unsupported syntax remain diagnostic, and multiple data-block field
scopes are explicitly reported as flat. A quoted occurrence of the reserved
name does not trigger an extra detector or false-positive syntax diagnostic.
No numerical value, dtype, image shape or image counts are inferred from text.

The independent CC0 companion fixtures cover quoted and multiline values,
unknown/numeric text, complete loops, duplicate assignments, orphaned values,
incomplete rows, unsupported save tokens and a binary-item boundary. Native
controls additionally cover non-UTF8/unterminated values, resource limits,
nonseekable capture, owned-buffer isolation, growth and same-size replacement.
`scripts/cif_document_reference.py` compares unchanged `CIF.loadCIF` and its
`readCIF` alias with both path and `BytesIO` inputs. Pinned loop byte strings have
an explicitly labelled strict-ASCII reference representation; their original
byte hex values are also recorded. Positive field/loop projections are compared
in full; unsupported-source results qualify raw retention/diagnostics rather
than pretending to establish full standalone CIF parity.

## Observed legacy helper defects

The same helper runs the remaining table/profile APIs in isolated Python
subprocesses with a disposable working directory and `TMPDIR`. Native temporary
files left by the extension are inventoried and removed with that directory.
Inputs, imported modules/native extensions, errors and source immutability are
fingerprinted in the separate proof artifact. The observed pinned environment
is FabIO 2026.6.0 on Python 3.13; these failures are not successful reader/API
parity events and do not prove arbitrary format files are malformed.

- `ext.cf_io.read` on the independent two-row ASCII column-table input raises
  `OSError: [Errno 9] Bad file descriptor`. The pinned Cython source writes with a
  temporary `os.fdopen(...)` object and then calls `os.close(fd)` again, before
  choosing its ASCII/binary C decoder. Its imported extension SHA256 is
  `3d873b44a9b953aa7d8a7eb7a0556b11d38d151ad86ce4b6c891f0f2cc96768d`.
- `datIO.columnfile.read` on the same input catches the failure and raises its
  generic column-file read error. The direct extension observation identifies
  the failure before format decoding; no new column-file parser is substituted.
- `CIF.loadCHIPLOT` on the independent three-point profile raises `TypeError:
  '<' not supported between instances of 'str' and 'int'`. Its source formats
  `_pd_meas_2theta_range_inc` as a string and then compares it with integer zero.
  The profile read does not complete; no corrupted/reference-derived native
  profile is fabricated.

The column input SHA256 is
`e6d81d980fb927c1061fa95e237837f8548d2414081cf361df0702c12662a0fb`;
the profile input SHA256 is
`ec34173cd0794f41b5825fcacad0c931917f4dc9e636131a1537d6c18448a5d5`.
These are minimal helper-probe inputs inferred from the pinned source, not a
claim of independently qualified column/profile format acceptance.

## Frozen verification results

The [run proof](companions/2026-11-06-xcalibur-run.json) qualifies all **9 native
API comparisons** (3 independent sources × path/buffer/stdin). Two sources match
all pinned RunDescription fields; the invalid-UTF8 source retains native raw
bytes with the reference text rejection recorded separately. Standalone Sweep
fields and bits are compared for all three stored sweep records.

The [CIF proof](companions/2026-11-06-cif-document.json) qualifies all **9 native
HeaderProjection comparisons** (3 independent sources × path/buffer/stdin). It
records 12 unchanged reference loadCIF/readCIF observations: 8 positive-profile
field/loop comparisons and 4 observations on the explicit unsupported-syntax
source, whose native raw/diagnostic projection has a separate qualification.
All 3 isolated legacy helper probes remain `read_not_completed`; none is counted
as reading/API parity. Both proofs use the same frozen native probe and verify
that their source files, Rust sources and probe remain unchanged during the run.

Scoped validation passed: 6 run-reader Rust tests, 5 CIF-document Rust tests,
5 companion Python controls, 4 CIF Python controls, isolated-feature strict
Clippy and formatting checks. Repository-wide checks are recorded separately.
