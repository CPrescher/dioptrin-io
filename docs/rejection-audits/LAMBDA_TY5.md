# Lambda VDS and Oxford TY5 rejection audit

This audit covers four events from `docs/fabio-parity/2026-11-05-mar-sparse.json`,
representing two unique source hashes. Neither source is classified as malformed.
The adjacent `2026-11-05-lambda-ty5.json` records baseline hashes/event counts,
input conditions, reader actions, source evidence and complete sample checks.
No global inventory or acceptance totals are changed by this scoped report.

| Source | Baseline events | Input condition | Reader action |
| --- | ---: | --- | --- |
| `100nmfilmonglass_1_1.img` | 2 | Well formed TY5 image | Native checked TY5 implemented |
| `l1_test02_00002_master.nxs` | 2 | Well formed VDS descriptor with unavailable exact source paths | Separate VDS mapping/source contract remains unsupported |

## Oxford TY5: native signed counts

The independent authority is dxtbx commit
`41fc3d3cfd26c12e8cb3595b68360a9a9bdbcb05`, specifically its
[`rod_TY5_TY6_decompress` mode5](https://github.com/cctbx/dxtbx/blob/41fc3d3cfd26c12e8cb3595b68360a9a9bdbcb05/src/dxtbx/boost_python/compression.cc)
and [`FormatROD` file layout](https://github.com/cctbx/dxtbx/blob/41fc3d3cfd26c12e8cb3595b68360a9a9bdbcb05/src/dxtbx/format/FormatROD.py).
TY5 support was merged in [dxtbx PR879](https://github.com/cctbx/dxtbx/pull/879).
The BSD3 notice is retained in
`licenses/native-readers/dxtbx-TY5-BSD-3-Clause.txt`; production uses a bounded
Rust adaptation of that existing native decoder, without a Python runtime.

The encoded payload contains `NX*NY + 2*OI + 4*OL` bytecode bytes, followed by
`NY` little-endian unsigned32 row offsets. Codes0..253 mean signed differences
minus127; code254 carries an inline signed16 and code255 an inline signed32.
Each row starts with an absolute signed sample, then accumulates differences.
The reader returns I32, retains original row offsets and encoding metadata,
keeps stored orientation, and preserves the complete header and remaining trailer.
`Compression::Ty5` identifies this successful decode explicitly; `source_type`
is None because the original source is a variable-length encoding. There is no
production float representation view or retry after failure.

Admission validates bounded bytecode and row-table sizes, exact marker counts,
row ordering/ranges and signed32 accumulation. Rows must occupy contiguous,
nonoverlapping declared ranges without unexplained padding. Unsupported padding
is rejected rather than skipped. Table bytes count toward encoded and retained
metadata budgets; source snapshots apply before/after indexing and every read.
No partial output survives malformed input, overflow or source mutation.

All1,048,576 pixels in the actual upstream source match a separately compiled
pinned dxtbx C++ decoder and pinned FabIO2026.6.0 exactly. Native I32 sample SHA256
is `739f8a8e11f602331d874db23e0c4a2c34ad919217a12e9eed5e03b8a00e6bbd`;
counts span -172..460. FabIO promotes those integer counts to float64, so exact
reference representation comparison requires a separately labelled test-only
I32-to-F64 conversion. This is distinct from stored/source signature comparison.
Pinned FabIO also ignores the row table and resets its predictor on image height;
the square upstream fixture hides that defect. The five independent CC0 fixtures
include a nonsquare image, signed extrema, inline exceptions, single rows/columns
and negative-only samples. Every fixture also matches the independent native
oracle, without inheriting FabIO's nonsquare behavior.

## Lambda VDS: unavailable original source graph

The master contains three regular unlimited-frame mappings to acquisition-system
absolute `/gpfs/current/raw/_test_lambda/_fabio_test/..._m0[123].nxs` paths.
None of those exact paths exists in the acceptance environment. The three corpus
modules with matching basenames are available, but their directory is not the
stored source location. The master declares signed16 storage and zero fill,
with mapped module regions separated by unmapped detector gaps. That declared
unmapped-region fill is legitimate metadata; missing module samples cannot be
silently relabelled as those measured counts.

The h5py/HDF5 control experiment copies the unmodified master to an isolated
directory. With no matching modules, its unlimited frame extent becomes zero.
With only two basename modules, its frame extent reappears and the missing
module contributes fill, changing sample hashes. With all three basename files,
all five frame hashes match the original corpus read. This proves the reference
read depends on implicit HDF5 basename lookup and available source state, despite
its stored absolute source names. The audit records exact module names/hashes,
regular selections, declared fill and all control signatures.

Existing `ExternalLinks::FollowSnapshot` is deliberately not a VDS policy.
Both external-link policies continue rejecting virtual datasets before sample
reads, including a complete relative VDS and a missing-absolute VDS with a local
basename decoy in independent CC0 fixtures. Closing this remaining gap requires
an explicit caller-approved filename mapping plus complete immutable snapshots,
validated bounded source/virtual selections, native dtype equality, frame-extent
checks and a clear unmapped-region fill policy. Native path search, unavailable
module fill and implicit basename relocation remain inadmissible. There is no
claim that this valid descriptor or its available module files are corrupt.

## Reproduction

Generate independent fixtures with `scripts/generate_ty5_fixtures.py` and
`scripts/generate_lambda_vds_rejection_fixtures.py` (the latter uses test-only
h5py/NumPy). Build the existing `legacy_probe` with `bruker,oxford,raxis`, then run:

```sh
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/audit_lambda_ty5_rejections.py \
  --dxtbx /tmp/dioptrin-io-dxtbx-ty5-audit \
  --native-probe /tmp/dioptrin-ty5-native-target/debug/examples/legacy_probe \
  --output docs/rejection-audits/2026-11-05-lambda-ty5.json
```

The script verifies the pinned dxtbx checkout, compiles only its existing native
mode5 decoder as an isolated reference library, checks full native signatures,
compares every actual source pixel with FabIO and runs the isolated VDS controls.
Pinned authority file hashes and fixture provenance remain in the JSON report.
No performance claim follows from these correctness checks.
