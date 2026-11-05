# Remaining MAR345 and Xcalibur scope audit

This audit refers to pinned FabIO 2026.6.0, commit
`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`. It records viable reuse and unsupported
scope rather than adding another unqualified decoder.

## MAR345

`mar345image.py:89` reads compressed images through `compression.decPCK`, returning
uint32 with the image plate's overflow table applied. Other Format values return
None with a message about missing documentation. File support requires binary
and textual header interpretation, byte-order handling, dimensions, PCK payload
and the high-count overflow table; header parsing alone is insufficient.

The older `ext/src/ccp4_pack.c` is LGPL and remains excluded from the MIT crate.
However, `ext/mar345_IO.pyx` carries an explicit MIT notice and states that the
CCP4 algorithm is reimplemented in Cython. Its `uncompress_pck` defaults to
`use_CCP4=False`, dispatching its own `unpack_pck` plus `postdec`, then applies the
high-count table. The original LGPL implementation is used only with explicit
`use_CCP4=True`. Thus MAR345 is not universally blocked by the old codec's
license. A carefully bounded native adaptation of the existing MIT V1 decoder
is a viable reuse path, provided attribution and scientific integer semantics
are retained.

That source is not already a safe standalone native backend: it imports NumPy,
uses Python/Cython containers and disables bounds/wrap checks. Its `unpack_pck`
docstring at line 586 explicitly says V1 only. The higher layer recognizes V2
but default dispatch still invokes that V1 function. A valid adaptation must
establish complete output count/input bounds, exact 16-bit predictor wrapping,
high-value records, endian variants and corrupt/truncated behavior against the
same upstream files. V2 remains a separate contract rather than being guessed
from a recognized marker. The later bounded MIT V1 adaptation and its
source-correct oracle differences are now documented in `MAR345_READER.md`;
V2 remains explicitly unsupported.

Registry evaluation found cryiorust 0.6.0 (GPL-3.0+) and bioformats 0.1.8
(GPL-2.0-or-later), neither admitted as an MIT production dependency. No already
bounded permissively licensed standalone Rust MAR345 backend was located in
these searches. This is the search result, not a claim that none can exist.

## Xcalibur

`xcaliburimage.py:1029` describes a CrysAlisPro mask image helper. Its
`XcaliburImage.read` at lines 1049–1065 opens a file and initializes a header,
then constructs a fixed 50x60 all-zero array rather than interpreting source
pixels. This placeholder must not be reproduced as detector-file reading.

The substantive APIs are `CcdCharacteristiscs.read/loads/dumps`, `.build_mask`,
mask decomposition and related binary structs/run descriptions. The pinned
`test_xcaliburimage.py` roundtrips five scan*.ccd mask-characteristic structures
and decomposes an already-loaded Pilatus CBF mask. It does not establish an
Xcalibur detector-image read path. File-reader parity for the dummy method would
fabricate counts, violating this crate's contract.

A future CCD-characteristics API should explicitly read these versioned mask
structures under independent bounds, retain their original records and expose
mask reconstruction as a separate operation. It must not silently label `.ccd`
files as detector counts. Actual detector files under CrysAlisPro Esperanto and
Oxford diffraction formats have their own source contracts and readers.
