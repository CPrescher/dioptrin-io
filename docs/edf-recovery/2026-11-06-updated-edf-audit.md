# Updated EDF source-access audit, 2026-11-06

The [updated machine-readable audit](2026-11-06-updated-edf-audit.json) covers the
same original 68 EDF rejection events and 16 baseline source hashes. It preserves
all original input conditions, hashes, test IDs, event multiplicities and primary
source evidence. The [historical audit](../rejection-audits/2026-11-06-edf.json)
and its [explanation](../rejection-audits/2026-11-06-edf.md) remain byte-for-byte
unchanged, preserving previous report fingerprints.

| Original source condition | Events / hashes | Explicit configured action |
| --- | --- | --- |
| rh28a oversized external window, raw/gzip headers | 42 / 2 | Existing `UpToDeclared` complete-source closure, unchanged |
| Split MultiFrame stale physical/header sizes | 15 / 3 | Complete original samples through explicit legacy Size policy |
| d80 raw I32 payload labelled TY1 | 1 / 1 | Complete original samples through explicit matching stored-raw label policy |
| Deliberately truncated BadFiles/BadGzFiles | 10 / 10 | Exact typed prefixes, partial element bytes, missing ranges and compression integrity |

The actions are `explicit_compatibility_closed` (42),
`explicit_source_layout_closed` (16) and `partial_source_access` (10).
The original source inconsistencies/truncations remain visible. Strict default
reader admission is unchanged; these are selected policies and recovery access,
not a claim that malformed sources became well formed.

[The scoped proof](2026-11-06-proof.json) independently validates all remaining
26 events across 14 source hashes. All complete compatibility reads match pinned
FabIO in dtype, shape and every sample byte, and match an independent source-byte
interpretation. All ten truncated-source records match the independent prefix
oracle in every typed sample hash, partial byte, declared/missing/verified range,
member boundary, integrity state and truncated-header/stop record. Available
words also match the pinned test's known original array, without comparing or
reproducing the reference reader's fabricated dense zeros.

Header-only sources return typed empty prefixes and original metadata, with the
entire declared sample range missing. Two incomplete gzip payloads retain 28,057
complete F32 words plus one raw byte; no final scalar is synthesized. A partial
next header retains its exact bytes without inventing another frame's shape.
Complete earlier members remain independently verified and accessible.

The original five BadGz envelopes were deleted by upstream cleanup. Their
recreated captured copies retain explicitly different compressed SHA-256 hashes;
original baseline identities are never overwritten. All other proof inputs match
exact baseline bytes. Preserved immutable captures are preferred over the shared
corpus, avoiding concurrent upstream fixture writers. The proof records each
actual input path/hash and its original-versus-recreated identity.

The [API contract](../EDF_RECOVERY.md) explains strict defaults, original
metadata retention, label assertions, resource/source checks and why integrity
is independent of sample availability. The generator is read-only with respect
to input files:

```sh
cargo build --no-default-features --features edf --example edf_recovery_probe
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/audit_edf_recovery.py
```

It reads the historical audit from immutable Git ref `017afba`, writes only the
new scoped proof/audit paths, and verifies exact original 68-event source/test
coverage. Existing external-window proof and primary-source citations remain in
the updated records. Large original/captured detector files remain outside Git.
