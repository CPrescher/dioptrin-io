# Strict row adapter for TIFF

Baseline is fax 0.2.7, MIT, registry archive and original file hashes in
`UPSTREAM.json`; the original LICENSE is unchanged. Only the library, original
manifests/README/license/VCS receipt are copied. Unused explicit example/test
manifest targets and the tiff dev dependency are removed. No upstream sample
program or large test corpus is included.

`dioptrin-strict-rows.patch` is the complete delta from those baseline files.
The upstream Group4 row state machine is factored into one shared function.
Its compatibility API retains its historical bounds/EOF recovery. The new
`StrictRowDecoder` API calls that same state machine with progress, width,
run-sum and complete-row checks, using the original public Huffman tables.
It consumes neither EOL/RTC nor virtual/padded bits. A staged lookahead commits
only a successful table codeword; public codeword lookup handles a complete
short last codeword without inventing lookahead bytes. Failed strict rows
clear their current transitions and retain the last complete reference row.

The TIFF adapter owns strip limits, 1D/2D tags, EOL/fill/RTC framing, first-row
1D requirements and complete source-row output. The strict API reserves two
bounded transition vectors from the declared u16 width. Fixed table storage,
allocator bookkeeping and the caller's output are additional memory.

Five focused new unit tests cover every vertical offset, pass and horizontal,
exact last codewords, zero-ending sentinels, overrun, premature end, extension,
reference validation and compatibility behavior. The original 14 library tests
remain. Unused baseline imports/lifetime spelling and a test-only format warning
are cleaned to keep the path dependency warning-free; no code-table changes.
