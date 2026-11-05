# XSDataImage input

The optional `xsd` feature uses roxmltree (MIT) and data-encoding (MIT).
No dedicated XSData detector reader was found in the crate registry; XML
schema tools are not numerical image readers. The MIT FabIO reference notice
is retained under `licenses/native-readers/` for the format conventions.

The reader admits one 2D array with explicit shape, size, dtype and
base64/base32/base16 coding. Ten integer/IEEE scalar types plus complex64/128
retain little-endian source components, precision, nonfinite bits and stored
axes. Metadata retains the original dtype and XML bytes before/after the data
element. Optional MD5 verifies the decoded bytes. Unknown coding/types,
duplicate descriptors, DTDs, missing/extra samples, malformed coding and source
changes fail; output is invalidated before every read. No decoded counts are
cached. Higher ranks, compound/string/half types and non-UTF8 XML are explicit
gaps. It does not reproduce the reference's unknown-coding base64 guess.

`scripts/generate_xsd_fixtures.py` writes 36 small CC0 records independently
with NumPy and Python's base encoders, verifies every source byte against the
pinned FabIO reader, and retains file/sample SHA256 in the fixture provenance.
Rust tests compare all sample bits and exercise descriptor, checksum, resource
and mutation errors. The pinned upstream XSD tests provide the larger actual
XSDataImage/XSDataImageInv matrix files through the complete-suite harness.
