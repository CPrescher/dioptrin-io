# Independent container fixtures

All `.spe`, `.mrc` and `.dm3` files in this directory were independently generated
from tiny explicit source arrays by `scripts/generate_container_fixtures.py`.
They are dedicated to the public domain under CC0-1.0. They contain no measured
or upstream image pixels. `provenance.json` records each complete file hash and
canonical little-endian native sample hashes for every frame. Floating hashes
include NaNs, infinities and signed zero; complex hashes include both components.

The generator uses FabIO 2026.6.0 as an independent reference where its reader
admits the layout. `fabio_compared: false` cases check independently constructed
source bytes where the reference does not admit the exact storage contract:
big-endian MRC, correctly stored MRC mode 3, minimal SPE XML/multiple regions,
and DM3 byte arrays/multiple image stacks with richer tags. These cases do not
claim upstream reference parity. All other entries compare complete sample bits
and dtype against FabIO before recording hashes.

`upstream-provenance.json` records actual pinned upstream corpus files and all
33 frames decoded in a separate comparison. Those large upstream files are not
included and retain their upstream terms. The manifest includes the external
path used during this run; regenerate it for another checkout using
`scripts/container_upstream_reference.py`. The optional Rust test uses
`CONTAINER_PARITY_MANIFEST` to read that independently generated reference.
