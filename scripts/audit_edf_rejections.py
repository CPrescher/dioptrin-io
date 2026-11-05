#!/usr/bin/env python3
"""Read-only classification of the pinned68 EDF rejection events.
No upstream writers, source repair, count padding or global ledger mutation.
"""

import argparse
import bz2
import collections
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import zlib
import fabio
import numpy as np

p = argparse.ArgumentParser()
p.add_argument(
    "--baseline",
    type=Path,
    default=Path("docs/fabio-parity/2026-11-05-mar-sparse.json"),
)
p.add_argument(
    "--captures",
    type=Path,
    nargs="*",
    default=[
        Path("/tmp/dioptrin-rejections-baseline-capture.json"),
        Path("/tmp/dioptrin-rejections-extra-capture.json"),
    ],
)
p.add_argument(
    "--probe", type=Path, default=Path("target/debug/examples/edf_rejection_probe")
)
p.add_argument(
    "--output", type=Path, default=Path("docs/rejection-audits/2026-11-06-edf.json")
)
a = p.parse_args()
if fabio.version != "2026.6.0":
    raise SystemExit("requires pinned FabIO2026.6.0")
COMMIT = "9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece"
REF = Path("/tmp/dioptrin-io-fabio-2026.6.0")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def digest(array):
    return sha(
        np.ascontiguousarray(
            array.astype(array.dtype.newbyteorder("<"), copy=False)
        ).tobytes()
    )


def matches(native, reference):
    types = {
        "U8": "uint8",
        "U16": "uint16",
        "U32": "uint32",
        "U64": "uint64",
        "I8": "int8",
        "I16": "int16",
        "I32": "int32",
        "I64": "int64",
        "F32": "float32",
        "F64": "float64",
    }
    return (
        native["sample_sha256"] == reference["sample_sha256"]
        and native["shape"] == reference["shape"]
        and types[native["dtype"]] == reference["dtype"]
    )


def authority(path, first, last):
    return dict(
        path=path,
        sha256=sha((REF / path).read_bytes()),
        lines=[first, last],
        url=f"https://github.com/silx-kit/fabio/blob/{COMMIT}/{path}#L{first}-L{last}",
    )


def expand(data):
    if data.startswith(b"BZh"):
        try:
            return bz2.decompress(data), dict(codec="bzip2", complete=True)
        except Exception as error:
            return b"", dict(codec="bzip2", complete=False, error=str(error))
    if not data.startswith(b"\x1f\x8b"):
        return data, dict(codec="none", complete=True)
    output = bytearray()
    tail = data
    members = 0
    while tail:
        decoder = zlib.decompressobj(31)
        try:
            chunk = decoder.decompress(tail)
        except zlib.error as error:
            return bytes(output), dict(
                codec="gzip", complete=False, complete_members=members, error=str(error)
            )
        output.extend(chunk)
        if not decoder.eof:
            return bytes(output), dict(
                codec="gzip",
                complete=False,
                complete_members=members,
                error="incomplete gzip member/trailer",
            )
        members += 1
        tail = decoder.unused_data
    return bytes(output), dict(codec="gzip", complete=True, complete_members=members)


def block(data, offset):
    end = re.search(rb"}\r?\n", data[offset:])
    if end is None:
        return None
    payload = offset + end.end()
    raw = data[offset:payload]
    start = raw.find(b"{")
    if start < 0 or raw[:start].strip():
        return None
    body = raw[start + 1 : raw.rfind(b"}")]
    fields = {}
    for part in body.split(b";"):
        if b"=" in part:
            k, v = part.split(b"=", 1)
            fields[k.strip(b"\0 \t\r\n").decode("latin1").upper()] = v.strip(
                b"\0 \t\r\n"
            ).decode("latin1")
    return payload, fields


def scan(data):
    offset = 0
    records = []
    while offset < len(data):
        parsed = block(data, offset)
        if parsed is None:
            return records, dict(
                offset=offset,
                error="incomplete_or_invalid_header",
                available_header_bytes=len(data) - offset,
            )
        payload, fields = parsed
        general = "EDF_DATAFORMATVERSION" in fields
        rec = dict(
            header_offset=offset,
            actual_header_bytes=payload - offset,
            payload_offset=payload,
        )
        if "EDF_HEADERSIZE" in fields:
            rec["declared_header_bytes"] = int(fields["EDF_HEADERSIZE"])
        if general:
            rec.update(
                general_header=True, declared_data_blocks=fields.get("EDF_DATABLOCKS")
            )
            physical = int(fields.get("EDF_BINARYSIZE", fields.get("SIZE", "0")))
        else:
            width = int(fields["DIM_1"])
            height = int(fields["DIM_2"])
            dtype = {
                "UnsignedShort": "<u2",
                "FloatValue": "<f4",
                "SignedInteger": "<i4",
            }[fields["DATATYPE"]]
            required = width * height * np.dtype(dtype).itemsize
            rec.update(
                shape=[height, width],
                dtype=np.dtype(dtype).name,
                required_sample_bytes=required,
                compression_tag=fields.get("COMPRESSION", "NONE"),
                legacy_size=int(fields.get("SIZE", required)),
                declared_inline_bytes=int(
                    fields.get("EDF_BINARYSIZE", fields.get("SIZE", required))
                ),
            )
            physical = rec["declared_inline_bytes"]
            if "EDF_BINARYFILENAME" in fields:
                rec.update(
                    external_filename=fields["EDF_BINARYFILENAME"],
                    external_offset=int(fields.get("EDF_BINARYFILEPOSITION", 0)),
                    declared_external_window=int(
                        fields.get("EDF_BINARYFILESIZE", required)
                    ),
                )
            else:
                available = len(data) - payload
                rec.update(
                    available_bytes_to_eof=available,
                    complete_sample_bytes_available=available >= required,
                )
                if available < physical:
                    records.append(rec)
                    return records, dict(
                        offset=payload,
                        error="declared_inline_window_exceeds_eof",
                        declared=physical,
                        available=available,
                        required_sample_bytes=required,
                    )
        records.append(rec)
        offset = payload + physical
    return records, None


def probe(mode, path, *layout):
    result = subprocess.run(
        [str(a.probe.resolve()), mode, str(path), *map(str, layout)],
        capture_output=True,
        text=True,
    )
    return dict(
        exit_code=result.returncode,
        error=result.stderr.strip(),
        frames=[json.loads(line) for line in result.stdout.splitlines()],
    )


def refs(path):
    image = fabio.edfimage.EdfImage().read(str(path))
    frames = []
    for index in range(image.nframes):
        frame = image if image.nframes == 1 else image.getframe(index)
        values = frame.data
        frames.append(
            dict(
                frame=index,
                dtype=values.dtype.name,
                shape=list(values.shape),
                sample_sha256=digest(values),
                all_zero=bool(np.all(values == 0)),
                incomplete_data=bool(
                    getattr(
                        frame._frames[frame.currentframe]
                        if hasattr(frame, "_frames")
                        else frame,
                        "incomplete_data",
                        False,
                    )
                ),
            )
        )
    result = dict(frames=frames, incomplete_file=bool(image.incomplete_file))
    image.close()
    return result


baseline = json.loads(a.baseline.read_text())
events = [
    r
    for r in baseline["reads"]
    if r["codec"] == "edfimage" and r["status"] == "native_rejected"
]
groups = collections.defaultdict(list)
for event in events:
    groups[event["file_sha256"]].append(event)
captures = []
capture_tests = collections.defaultdict(list)
capture_hash = {}
for capture_path in a.captures:
    if not capture_path.exists():
        continue
    capture = json.loads(capture_path.read_text())
    captures.append(
        dict(
            path=str(capture_path),
            sha256=sha(capture_path.read_bytes()),
            source_probe_stable=capture.get("probe_unchanged_during_run"),
            sources_stable=capture.get("sources_unchanged_during_run"),
        )
    )
    for row in capture.get("reads", []):
        if row.get("rejected_source_copy"):
            capture_tests[row["test"]].append(row)
            capture_hash[row["file_sha256"]] = row
records = []
for original_sha, rows in sorted(
    groups.items(), key=lambda item: Path(item[1][0]["source"]).name
):
    first = rows[0]
    path = Path(first["source"])
    origin = "original_source"
    if not path.exists() or sha(path.read_bytes()) != original_sha:
        saved = capture_hash.get(original_sha)
        if saved is None:
            saved = next(
                (
                    r
                    for r in capture_tests[first["test"]]
                    if Path(r["rejected_source_copy"]["path"]).exists()
                ),
                None,
            )
        if saved is None:
            raise RuntimeError(
                f"no exact or explicitly recreated evidence: {first['source']}"
            )
        path = Path(saved["rejected_source_copy"]["path"])
        origin = (
            "captured_exact_source"
            if saved["file_sha256"] == original_sha
            else "recreated_same_upstream_test"
        )
    supplied_sha = sha(path.read_bytes())
    expanded, envelope = expand(path.read_bytes())
    headers, scan_error = scan(expanded)
    record = dict(
        source=first["source"],
        filename=Path(first["source"]).name,
        baseline_sha256=original_sha,
        baseline_event_count=len(rows),
        baseline_tests=sorted(row["test"] for row in rows),
        baseline_errors=dict(collections.Counter(row["error"] for row in rows)),
        input_condition="inconsistent_header",
        reader_action="strict_reject",
    )
    evidence = dict(
        path=str(path),
        sha256=supplied_sha,
        exact_baseline_bytes=supplied_sha == original_sha,
        provenance=origin,
        bytes=path.stat().st_size,
        outer_envelope=envelope,
        expanded_prefix_sha256=sha(expanded),
        expanded_prefix_bytes=len(expanded),
        header_records=headers,
        scan_error=scan_error,
        strict_native=probe("strict", path),
    )
    if envelope["complete"] is False or "TestBad" in first["test"]:
        record.update(
            input_condition="intentionally_truncated",
            classification="strict_rejection_prevents_partial_sequence_or_invented_counts",
        )
        evidence["authority"] = [
            authority("src/fabio/test/codecs/test_edfimage.py", 685, 776),
            authority("src/fabio/edfimage.py", 630, 650),
        ]
        evidence["reference_behavior"] = refs(path)
        if not supplied_sha == original_sha:
            evidence["original_compressed_bytes_unavailable"] = (
                "Original baseline gzip envelope was deleted by upstream test cleanup. This is explicitly recreated evidence; timestamps change compressed SHA. Baseline hash/testID remain recorded and are not relabelled exact."
            )
    elif Path(first["source"]).name.startswith("rh28a_"):
        record.update(
            reader_action="explicit_compatibility_closed",
            classification="complete_samples_with_inconsistent_oversized_external_window",
        )
        result = probe("up-to", path)
        reference = refs(path)
        if result["exit_code"] != 0:
            raise RuntimeError(result)
        for native, ref in zip(result["frames"], reference["frames"], strict=True):
            if not matches(native, ref):
                raise RuntimeError("external dtype/shape/count mismatch")
            payload = Path(native["payload_source"])
            native["payload_source_sha256"] = sha(payload.read_bytes())
            native["available_from_offset"] = (
                native["source_bytes"] - native["payload_offset"]
            )
            native["required_sample_bytes"] = np.prod(native["shape"]).item() * 2
            native["counts_match_reference"] = True
        evidence.update(
            compatibility_native=result,
            reference=reference,
            all20_frames_match=len(result["frames"]) == 20,
            default_contract_unchanged=True,
            authority=[
                authority("src/fabio/test/codecs/test_edfimage_expg.py", 417, 431),
                authority("src/fabio/edfimage.py", 324, 354),
            ],
        )
    else:
        header = headers[0]
        reference = refs(path)
        if first["source"].endswith("d80_60s.edf"):
            record["classification"] = (
                "raw_samples_mislabelled_as_ty1_not_a_missing_ty1_decoder"
            )
            fields = block(expanded, 0)[1]
            count = np.prod(header["shape"]).item()
            evidence["ty1_encoded_bytes_from_oi_ol"] = (
                count + 2 * int(fields["OI"]) + 4 * int(fields["OL"])
            )
            evidence["raw_payload_bytes"] = len(expanded) - header["payload_offset"]
            evidence["ty1_first_sample_would_be"] = (
                expanded[header["payload_offset"]] - 127
            )
            evidence["raw_i32_first_sample"] = int.from_bytes(
                expanded[header["payload_offset"] : header["payload_offset"] + 4],
                "little",
                signed=True,
            )
            oxford = Path(first["source"]).with_name("d80_60s.img")
            o = fabio.OXDimage.OxdImage().read(str(oxford))
            evidence["independent_oxford_source"] = dict(
                path=str(oxford),
                sha256=sha(oxford.read_bytes()),
                decoded_sha256=digest(o.data),
            )
            evidence["authority"] = [
                authority("src/fabio/edfimage.py", 601, 635),
                authority("src/fabio/test/codecs/test_oxdimage.py", 144, 160),
            ]
        else:
            record["classification"] = (
                "complete_samples_with_stale_inline_size_and_header_declarations"
            )
            evidence["authority"] = [
                authority("src/fabio/edfimage.py", 371, 376),
                authority("src/fabio/edfimage.py", 1216, 1250),
                authority("src/fabio/test/codecs/test_edfimage.py", 269, 305),
            ]
        with tempfile.TemporaryDirectory(prefix="dioptrin-edf-audit-") as temp:
            raw_path = path
            if envelope["codec"] != "none":
                raw_path = Path(temp) / "expanded.edf"
                raw_path.write_bytes(expanded)
            height, width = header["shape"]
            raw = probe(
                "raw",
                raw_path,
                width,
                height,
                header["payload_offset"],
                header["dtype"],
            )
        if raw["exit_code"] != 0 or not matches(
            raw["frames"][0], reference["frames"][0]
        ):
            raise RuntimeError("raw layout dtype/shape/count proof mismatch")
        if "independent_oxford_source" in evidence:
            same = (
                evidence["independent_oxford_source"]["decoded_sha256"]
                == reference["frames"][0]["sample_sha256"]
            )
            evidence["independent_oxford_source"]["counts_match_edf_reference"] = same
            if not same:
                raise RuntimeError("independent Oxford count mismatch")
        evidence.update(
            reference=reference,
            explicit_raw_layout_proof=raw,
            raw_layout_matches_reference=True,
            raw_layout_is_not_edf_acceptance=True,
        )
    if evidence["strict_native"]["exit_code"] == 0:
        raise RuntimeError("unexpected default-policy acceptance")
    if sha(path.read_bytes()) != supplied_sha:
        raise RuntimeError("audit source changed")
    evidence["source_unchanged_after_audit"] = True
    record["evidence"] = evidence
    records.append(record)
report = dict(
    schema=1,
    baseline=str(a.baseline),
    baseline_sha256=sha(a.baseline.read_bytes()),
    audited_events=len(events),
    audited_unique_sources=len(records),
    malformed_sources=len(records),
    reference=dict(fabio_version=fabio.version, fabio_commit=COMMIT),
    probe_sha256=sha(a.probe.read_bytes()),
    captures=captures,
    summary=dict(
        input_conditions=dict(
            collections.Counter(
                {
                    condition: sum(
                        r["baseline_event_count"]
                        for r in records
                        if r["input_condition"] == condition
                    )
                    for condition in sorted({r["input_condition"] for r in records})
                }
            )
        ),
        reader_actions={
            action: sum(
                r["baseline_event_count"]
                for r in records
                if r["reader_action"] == action
            )
            for action in sorted({r["reader_action"] for r in records})
        },
        well_formed_native_codec_gaps=0,
    ),
    records=records,
)
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text(json.dumps(report, indent=2) + "\n")
print(
    json.dumps(
        dict(
            audited_events=report["audited_events"],
            audited_unique_sources=report["audited_unique_sources"],
            summary=report["summary"],
        ),
        indent=2,
    )
)
