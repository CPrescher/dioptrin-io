# Independent decoder fixtures

`counts.h5` contains two small generated uint16 and uint32 frames in each byte
order, compressed with the stock hdf5plugin Bitshuffle/LZ4 compressor. Each frame
crosses several codec blocks and has a raw tail. Original values include zero,
the high bit, and the maximum unsigned value. The independent manifest retains
canonical little-endian frame hashes, encoder versions and source-file hash.

Recreate with `python scripts/generate_codec_fixtures.py`. The script uses the
stock standalone compressor and HDF5 direct chunk writes, avoiding plugin
`set_local` callbacks across incompatible HDF5 installations. Its plugin-loading
changes affect only that generator process. Tests preserve the existing legacy
raw reader's admission of native-endian types and rejection of opposite-endian
types; this codec comparison does not broaden that contract.

Generated data and generation code are MIT licensed. No experimental detector
files are included.
