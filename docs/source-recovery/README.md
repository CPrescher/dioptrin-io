# Complete reads, expected failures and available source data

The [current full corpus run](../fabio-parity/2026-11-06-source-recovery.json)
has 542 complete matching read events and 11 independently verified partial
source events, with no native rejection of a successful FabIO read. The
[original 91-event audit](../rejection-audits/2026-11-06-after-source-recovery.json)
retains 80 complete closures and 11 partial cases. This does not claim complete
counts for damaged files or full FabIO API parity. The reference suite has 213
tests, no failures/errors and one existing skip; two known Pillow display
defects remain explicit mismatches.

The pinned FabIO corpus contains both valid acquisitions and deliberately broken
inputs. Passing its suite does not mean producing a complete image from every
file. Its EDF `TestBadFiles` and `TestBadGzFiles` explicitly expect exceptions for
empty files, wrong magic and half a header. Other tests expect an incomplete-file
flag and zero padding for absent data. Those padded pixels are not original
detector counts.

This continuation uses three separate contracts:

| Source condition | Explicit access | Count qualification |
|---|---|---|
| Complete samples with contradictory legacy metadata | Selected bounded layout/window interpretation; original declarations retained | Every original sample must compare independently |
| Lambda virtual master with relocated modules | Exact stored-to-actual filename bindings and a frozen complete source graph | Every mapped module must exist; declared gap fill is identified separately |
| Truncated sample payload, header or compression member | Available typed sample spans, partial element bytes, missing ranges and integrity | Missing data stay unavailable; partial access never counts as full-image parity |

Default complete-image APIs retain their strict behavior. Legacy compatibility
is selected before decoding, without a failed-decoder retry. A recovered output
cannot silently replace an ordinary dense frame. Counts, source dtype, original
orientation and reserved values remain source data; consumers select derived
display views separately.

The original `text.tif` ThunderScan first page declares 359 rows but supplies only
356 complete rows. Its independent second page is a separate complete bilevel
image. Reading that page cannot replace missing four-bit samples on the first.
The available-row API discards an incomplete decoder row, including any error
padding written by the reused codec.

Detailed implementation and independent evidence are in the [VDS contract](../VDS_READER.md),
[TIFF recovery contract](../TIFF_LEGACY_RECOVERY.md),
[EDF recovery contract](../EDF_RECOVERY.md),
[Lambda all-frame proof](../vds/2026-11-06-lambda.json) and
[legacy TIFF source audit](../rejection-audits/2026-11-06-tiff-recovery.json), plus
the [exact EDF source/member proof](../edf-recovery/2026-11-06-proof.json).
All 184 Rust tests, formatting, strict Clippy, no-default check/tests and 20
isolated optional-feature checks pass; see the [recorded checks](../fabio-parity/2026-11-06-source-recovery-native-checks.json).

## Test bridge policies

[`read-policies.json`](read-policies.json) is a test-only manifest of exact source
hashes and narrowly selected pinned test cases. The test-case selectors are
necessary for temporary gzip files whose timestamp changes between runs. A
recreated test case is never claimed to have the original baseline hash.
These recipes do not enter production source detection.

`scripts/fabio_suite_parity.py --read-policies ... --vds-map ...` records the
selected policy, complete source manifests, original/effective windows and VDS
mappings. Both configuration files and every bridge/runtime source are hashed
before and after the run. Partial reads are independently checked against
present source bytes or complete decoded rows, and reported separately as
`partial_source_read`. They are excluded from the complete-count parity gate.

A VDS map is an external JSON file with `filenames` entries containing the exact
`stored_filename` and approved `actual_file`. `unmapped_regions` must explicitly
be `Reject` or `DeclaredFill`. The latter permits only original defined VDS gap
fill; absent modules and unallocated mapped chunks remain errors. No implicit
basename search is used by the native VDS reader.

Expected input errors, partial availability, reference decoder defects, optional
features and unsupported variants remain separate outcomes. None is removed
from the acceptance corpus to make the result look green.
