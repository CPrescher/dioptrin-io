#!/usr/bin/env python3
"""Read-only same-byte EDF compatibility/prefix proof and scoped baseline audit.

The preserved rejection copies are preferred, so concurrent upstream fixture
writers cannot change the subject. No input repair or source writes occur.
"""

import argparse
import collections
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import zlib

import fabio
import numpy as np

import edf_recovery_reference as oracle


def sha(data):
    return hashlib.sha256(data).hexdigest()


def canonical(array):
    return np.ascontiguousarray(
        array.astype(array.dtype.newbyteorder("<"), copy=False)
    ).tobytes()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--probe", type=Path, default=Path("target/debug/examples/edf_recovery_probe")
    )
    parser.add_argument(
        "--captures",
        type=Path,
        default=Path("/tmp/dioptrin-rejection-sources-20261105"),
    )
    parser.add_argument(
        "--baseline",
        type=Path,
        default=Path("docs/fabio-parity/2026-11-05-mar-sparse.json"),
    )
    parser.add_argument("--base-ref", default="017afba")
    parser.add_argument(
        "--base-audit",
        type=Path,
        default=Path("docs/rejection-audits/2026-11-06-edf.json"),
    )
    parser.add_argument(
        "--audit",
        type=Path,
        default=Path("docs/edf-recovery/2026-11-06-updated-edf-audit.json"),
    )
    parser.add_argument(
        "--proof", type=Path, default=Path("docs/edf-recovery/2026-11-06-proof.json")
    )
    args = parser.parse_args()
    if fabio.version != "2026.6.0":
        raise RuntimeError("requires pinned FabIO2026.6.0")
    audit_bytes = subprocess.check_output(
        ["git", "show", f"{args.base_ref}:{args.base_audit}"]
    )
    audit = json.loads(audit_bytes)
    proofs = []
    for record in audit["records"]:
        if record["reader_action"] != "strict_reject":
            continue
        evidence = record["evidence"]
        expected_sha = evidence["sha256"]
        copies = list(args.captures.glob(expected_sha + "-*"))
        path = copies[0] if copies else Path(evidence["path"])
        current_sha = sha(path.read_bytes())
        if current_sha != expected_sha:
            raise RuntimeError("preserved input bytes differ from prior source audit")
        partial = record["input_condition"] == "intentionally_truncated"
        mode = (
            "recovery"
            if partial
            else "raw-ty1"
            if "raw_samples_mislabelled" in record["classification"]
            else "legacy-size"
        )
        output = subprocess.check_output(
            [str(args.probe.resolve()), mode, str(path)], text=True
        )
        native = json.loads(output)
        source_proof = dict(
            filename=record["filename"],
            source=record["source"],
            baseline_sha256=record["baseline_sha256"],
            baseline_event_count=record["baseline_event_count"],
            baseline_tests=record["baseline_tests"],
            proof_source=str(path),
            proof_source_sha256=current_sha,
            proof_source_bytes=path.stat().st_size,
            exact_baseline_bytes=current_sha == record["baseline_sha256"],
            prior_provenance=evidence["provenance"],
            provenance="captured_exact_source"
            if copies and current_sha == record["baseline_sha256"]
            else evidence["provenance"],
            mode=mode,
            native=native,
        )
        if partial:
            comparison = oracle.compare(path, native["source_recovery"])
            if comparison["differences"]:
                raise RuntimeError(comparison["differences"])
            source_proof["independent_reference"] = comparison["reference"]
            source_proof["differences"] = []
            known = np.full(256 * 256, 10, dtype="<f4")
            known[0] = 0
            known[257] = 20
            raw = known.tobytes()
            for image in native["source_recovery"]["images"]:
                available = image["availability"]
                words = available["present_samples"][1]
                if image["dtype"] != "float32" or available["declared_shape"] != [
                    256,
                    256,
                ]:
                    raise RuntimeError(
                        "unexpected pinned independent bad-file source layout"
                    )
                if image["sample_sha256"] != sha(raw[: words * 4]):
                    raise RuntimeError(
                        "available source words differ from pinned original known array"
                    )
                tail = bytes.fromhex(available["partial_element_bytes"])
                if tail != raw[words * 4 : words * 4 + len(tail)]:
                    raise RuntimeError(
                        "raw partial word differs from pinned original known word"
                    )
            images = native["source_recovery"]["images"]
            source_proof["proof"] = dict(
                all_available_words_match_independent_decoder=True,
                all_available_words_match_known_pinned_array=True,
                all_partial_element_bytes_match=True,
                available_samples=sum(
                    i["availability"]["present_samples"][1] for i in images
                ),
                verified_samples=sum(
                    i["availability"]["verified_samples"][1] for i in images
                ),
                known_missing_samples=sum(
                    (
                        i["availability"]["missing_samples"][1]
                        - i["availability"]["missing_samples"][0]
                    )
                    if i["availability"]["missing_samples"]
                    else 0
                    for i in images
                ),
                incomplete_element_bytes=sum(
                    len(i["availability"]["partial_element_bytes"]) // 2 for i in images
                ),
                complete_verified_earlier_frames=sum(
                    i["availability"]["complete_verified_frame"] for i in images
                ),
                unknown_following_header=bool(
                    native["source_recovery"]["stop"]
                    and native["source_recovery"]["stop"]["kind"] == "TruncatedHeader"
                ),
                missing_values_synthesized=False,
                whole_source_count_parity_claimed=False,
            )
            record["reader_action"] = "partial_source_access"
        else:
            independent = oracle.read(
                path,
                inline_size=mode == "legacy-size",
                raw_label="TY1" if mode == "raw-ty1" else None,
            )
            image = fabio.edfimage.EdfImage().read(str(path))
            reference_frames = []
            try:
                for i, frame in enumerate(native["frames"]):
                    reference = image if image.nframes == 1 else image.getframe(i)
                    values = reference.data
                    signature = dict(
                        shape=list(values.shape),
                        dtype=values.dtype.name,
                        sample_sha256=sha(canonical(values)),
                    )
                    if any(frame[k] != signature[k] for k in signature):
                        raise RuntimeError(
                            "explicit complete-layout count/dtype/shape mismatch"
                        )
                    if (
                        frame["sample_sha256"]
                        != independent["images"][i]["sample_sha256"]
                        or not independent["images"][i]["availability"][
                            "complete_verified_frame"
                        ]
                    ):
                        raise RuntimeError(
                            "independent complete-source interpretation mismatch"
                        )
                    reference_frames.append(signature)
            finally:
                image.close()
            source_proof.update(
                independent_reference=independent,
                fabio_reference=reference_frames,
                proof=dict(
                    all_sample_bytes_match=True,
                    source_dtype_shape_match=True,
                    explicit_policy=True,
                    no_decoder_retry=True,
                ),
            )
            record["reader_action"] = "explicit_source_layout_closed"
        if sha(path.read_bytes()) != current_sha:
            raise RuntimeError("proof source changed")
        source_proof["source_unchanged_after_proof"] = True
        index = len(proofs)
        evidence["explicit_access_proof"] = dict(
            path=str(args.proof),
            record=index,
            source_sha256=current_sha,
            policy=mode,
            action=record["reader_action"],
        )
        proofs.append(source_proof)
    if len(proofs) != 14 or sum(r["baseline_event_count"] for r in proofs) != 26:
        raise RuntimeError(
            "EDF recovery proof must cover exact14 sources/26 baseline events"
        )
    proof = dict(
        schema=1,
        baseline=str(args.baseline),
        baseline_sha256=sha(args.baseline.read_bytes()),
        base_audit_ref=args.base_ref,
        base_audit_sha256=sha(audit_bytes),
        probe_sha256=sha(args.probe.read_bytes()),
        oracle_sha256=sha(Path(oracle.__file__).read_bytes()),
        reference=dict(
            fabio=fabio.version,
            fabio_commit="9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece",
            numpy=np.__version__,
            python=sys.version,
            zlib=zlib.ZLIB_RUNTIME_VERSION,
        ),
        baseline_events=26,
        unique_proof_sources=14,
        records=proofs,
    )
    args.proof.parent.mkdir(parents=True, exist_ok=True)
    args.proof.write_text(json.dumps(proof, indent=2) + "\n")
    audit["explicit_access_probe_sha256"] = proof["probe_sha256"]
    audit["recovery_reference_sha256"] = proof["oracle_sha256"]
    audit["explicit_access_proof_sha256"] = sha(args.proof.read_bytes())
    audit["summary"]["reader_actions"] = dict(
        sorted(
            collections.Counter(
                {
                    action: sum(
                        r["baseline_event_count"]
                        for r in audit["records"]
                        if r["reader_action"] == action
                    )
                    for action in {r["reader_action"] for r in audit["records"]}
                }
            ).items()
        )
    )
    audit["summary"]["default_strict_rejections_preserved"] = 68
    args.audit.write_text(json.dumps(audit, indent=2) + "\n")
    baseline = json.loads(args.baseline.read_text())
    expected = collections.Counter(
        (r["file_sha256"], r["test"])
        for r in baseline["reads"]
        if r["codec"] == "edfimage" and r["status"] == "native_rejected"
    )
    actual = collections.Counter(
        (r["baseline_sha256"], test)
        for r in audit["records"]
        for test in r["baseline_tests"]
    )
    if actual != expected:
        raise RuntimeError(
            "updated audit loses original source/test/event multiplicity"
        )
    print(
        json.dumps(
            dict(
                proof_events=26,
                proof_sources=14,
                full_edf_events=68,
                reader_actions=audit["summary"]["reader_actions"],
            ),
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
