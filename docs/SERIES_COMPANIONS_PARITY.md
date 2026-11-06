# File series, navigation and companion qualification

Native file-series and navigation APIs now reuse the existing readers, with
explicit frozen source manifests and source-checked frame access. Acquisition
companions now include Xcalibur run descriptions and a standalone CIF header
projection. These remain separate from detector counts and Python object APIs.

## Source and navigation contracts

`series::Series::open(paths, opener, limits)` indexes every ordered file and its
actual frame count. It maps global indices to file ordinal and file-local frame,
without homogenizing shape, dtype, channels or orientation. The deterministic
opener supplies explicit existing format, interpretation, source and resource
policies. Only the most recent successful reader is retained; indexed graphs
hold bounded metadata rather than one decoded cache or open handle per file.
The callback must keep its interpretation configuration unchanged: source and
frame-count checks cannot prove an arbitrary caller callback's determinism.

`Numbered::from_seed`, `.path`, `.through` and `.until_missing` preserve padding,
format/compression suffixes and checked numeric bounds. First-missing traversal
is explicit and frozen, not a live acquisition view. Broken symlinks, missing
interior explicit entries and bounds exhaustion fail without skipping data.
Native series never trust fixed-frame hints instead of actual file contents.
See [series API](SERIES_API.md) for usage and limits.

`navigation::Cursor` works with a `FrameSource`, including wrapped readers and
series. It owns one reusable output and has explicit before-first/frame/after-last
anchors. Absolute, signed and relative moves, first/last and next/previous never
wrap. Every failed move hides output; failed I/O does not substitute previous
counts. A cursor on one reader stays file-local. Use a series source to cross
file boundaries. [Navigation contracts](NAVIGATION_API.md) describe boundary,
retry and frozen-count behavior, including pinned negative-index differences.

`SourceBinding` retains a requested alias and its resolved target. Snapshot
checks now verify both, so retargeting a symlink fails even if its old target
still exists. EDF and HDF5 retain every selected external alias, including
multiple aliases deduplicated to one physical target. Series compare reopened
source graphs and counts with the indexed manifest and check inactive sources
as well as the selected frame. Private decoded staging aliases are excluded
from persistent series contracts; genuine external aliases to the same root
remain visible. Independent tests cover these distinctions and output invalidation.

## Unchanged upstream series tests

The [series/navigation report](series/2026-11-06-series-navigation.json) executes
**37 unchanged pinned tests** from `test_file_series.py`, `test_filename_steps.py`
and `test_filenames.py`, with zero failures/errors/skips. Imported helper and test
sources match the pinned checkout. Reference objects, iterator consumption and
assertions retain their behavior; additional independent reference objects provide
stronger full-frame checks where upstream assertions inspect only truthiness or
one sample.

- **19 series instances** supply **49 actual frame requests**, including 20 iterator
  yields. Native and reference full sample hashes, dtype/shape, global/local/file
  mapping, source paths and headers match.
- **515 native cursor actions** check those actual calls and additional explicit
  boundary/reverse/seek controls: 290 successes and 225 checked errors. All
  **3,429 compared metadata field occurrences** match.
- **25 actual FilenameObject/path opens** receive independent native read checks.
  Filename object parser types and Python identity are not claimed as native APIs.
- **36 filename helper calls** have 34 exact names and two explicitly qualified
  nonnumeric seeds. Native code requires a numeric seed rather than inventing one.
- Two unique numbered batches compare **10,010 complete paths**, including every
  path in the upstream 10,006-element rollover series.

The observer does not prefetch iterator yields or consume an unexhausted filename
generator. Missing expected test/event coverage, unrelated native failures,
timeouts, malformed output or changed source hashes fail qualification. Python
controls enforce these failure paths. Arbitrary string-list object controls remain
reference-only; native frame navigation and explicit manifests have their own
source contracts.

## Companion reads and reference failures

[Companion contracts](COMPANION_READERS.md) and the separate proofs cover:

| Native API | Measured proof |
|---|---|
| Xcalibur `.run` and 88-byte sweeps | [Nine path/buffer/stdin comparisons](companions/2026-11-06-xcalibur-run.json) across three independent CC0 fixtures; all fields, float bits, text octets, reserved values and trailers |
| Standalone CIF document | [Nine path/buffer/stdin projections](companions/2026-11-06-cif-document.json) reusing the existing detector CIF parser; eight successful reference load/read observations on two positive fixtures, plus explicit raw/diagnostic qualification for unsupported syntax |

CIF access deliberately says `HeaderProjection`: binary data items, full CIF
syntax and per-block scopes are not reconstructed. Original bytes and diagnostics
remain authoritative. No image, profile or complete-CIF interpretation is invented.
The `.run` corpus is independent fixtures: no upstream acceptance-corpus run files
or original run test existed, and none are claimed.

Three additional pinned helper reads fail on valid independent inputs:
`cf_io.read` raises `OSError: Bad file descriptor`, `datIO.columnfile.read` wraps
that error, and `CIF.loadCHIPLOT` raises a Python 3 string/integer comparison
`TypeError`. Those isolated subprocess observations, input/library/source hashes
and cleanup context are retained in the CIF proof. They are reference API
failures; native table/profile parity is not claimed from an absent result.

## Regression boundary

The [full reader continuation](fabio-parity/2026-11-06-series-companions.json)
repeats the existing 237 codec/opening/header tests and all 626 path/stdin input
comparisons with the new source-binding code. The separate 37-test series audit
is additional; companion checks use direct unchanged reference APIs and
independent fixtures, not invented upstream tests.

Exact FabIO/full reading parity remains unestablished. Remaining boundaries
include full standalone CIF grammar, table/profile formats whose pinned helper
APIs fail, outside-corpus codec/layout variants, and optional low-level/general
NeXus helper semantics. Reference bugs, derived views and missing source spans
retain their explicit classifications. Writing, mutation/statistics and cloning
Python/h5py object identities remain outside this native input crate.

[Final native checks](fabio-parity/2026-11-06-series-companions-native-checks.json)
pass 272 Rust tests, 56 Python controls, all required formatting/Clippy/test commands,
no-default checks/eight tests and 21 isolated optional-feature checks. The complete
237-test/626-input regression preserves prior pixel, metadata and attribute
outcomes. Together with the separate 37-test audit, 274 unchanged reference tests
run with zero failures/errors and one existing slow-test skip. All three
additional proof artifacts and both executables retain their exact source hashes.
