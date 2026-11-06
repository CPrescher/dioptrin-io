# Bounded ThunderScan row codec

`tif_thunder.c` is the exact permissive libtiff 4.7.2 source pinned in
`UPSTREAM.json`, with its original copyright and license preserved. The local
`tiffiop.h` supplies only the row state required by this unchanged codec. The
wrapper validates signed sizes and the exact packed output row extent before
calling the native row function. It cannot parse headers, open files, repair
metadata or allocate image buffers.

The optional TIFF feature compiles this small C translation unit through the
optional `cc` build dependency. Builds without TIFF do not invoke a C compiler.
Rust admits original source strip windows and scratch budgets, tracks consumed
bytes, and exposes only complete native rows. libtiff's failed-row zero fill is
always discarded. Remaining raw/run/delta bytecodes that could produce pixels
prevent a dense complete frame; only the three defined zero-output codes are
accepted as a verified tail. Missing rows remain explicit, without sample slots.
