# Original CBF encoded payloads

`cbf::Reader::open_encoded(path, limits)` opens a bounded single binary section
for original-byte access without selecting its conversion codec. The source
integer type, nonzero dimensions, element count, MIME transfer encoding, payload
extent, header/trailer and metadata budgets remain validated. Source fields and
header bytes are retained unchanged. Unknown conversion labels are opaque
metadata; accepting their bytes does not establish a valid decoded image.

`read_encoded_into(&mut EncodedBuffer)` captures exactly `X-Binary-Size` bytes,
excluding the marker, padding and closing trailer. It reuses the same source
capture routine as decoded CBF input. Every read checks the original handle,
path binding and current source before/after I/O. Replaced/grown/retargeted or
truncated sources fail, and failed reads clear/hide reusable output.

`EncodedBuffer::integrity()` returns the declared and computed MD5 digests.
`matches()` returns `Some(true)`, `Some(false)` or `None` for an absent source
checksum. Encoded bytes with a mismatching checksum remain accessible with that
explicit status; no count array or checksum success is invented. The existing
complete image API still rejects mismatching checksums, malformed byte-offset
escapes and unsupported layouts. It never decodes an opaque conversion, unknown
byte order or frame admitted only for encoded access.

Encoded access does not allocate a decoded frame or require its byte budget.
If the declared decoded frame exceeds `frame_bytes`, encoded reads remain
available but decoded reads fail before allocation. Dimension/product and encoded
resource bounds still apply. Explicit byte-offset dialect selection remains in
`open_with_mode`; encoded bytes do not depend on a predictor interpretation.

The direct path API requires a regular immutable file. Outer transport decoding
remains a separate explicit operation; it does not infer gzip/bzip2 or search for
neighboring files. Floating/unknown element metadata, volumes, multiple binary
sections and partial-source recovery remain outside this integer single-section
contract. No second decoder or new production dependency is introduced.

## Reference distinction and evidence

The test-only `cbf_encoded_reference.py` calls unchanged pinned FabIO
`CbfImage.read(..., only_raw=True)` and `read_raw_data` after its normal header
setup. A separate byte-slice/MIME/MD5 oracle checks every native payload and
metadata projection. Reference raw success is not interpreted as successful
count decoding: its `only_raw` branch bypasses the checksum/count decoder and
can return fewer bytes than declared when a source is truncated.

Seven independent CC0 fixtures cover a complete two-delta source, absent/wrong
checksums, an incomplete escape, a byte-order label outside decoded admission,
an opaque conversion label with an unvalidated body, and an actually truncated
payload. Five original pinned-corpus CBF sources supplement them. Fixture hashes
and the independent generator remain in `tests/data/cbf-encoded/provenance.json`.
Source bytes, declared metadata, compressed-payload hashes and integrity are
compared separately from scientific counts.

The [proof](cbf-encoded/2026-11-06-raw.json) and
[combined audit](CIF2_LOWLEVEL_PARITY.md) record the final executable/source
fingerprints, complete captures, strict incomplete-source rejection and actual
reference outcomes. Three Rust tests cover integrity, decode separation,
resource admission, output invalidation, source replacement and symlink changes;
Python controls check that source/integrity changes cannot be ignored and that
named pipes are rejected under a bounded timeout.
