"""Combine source-level audits without dropping repeated baseline rejections.

Test/provenance tooling only; source detector files remain outside Git.
"""
import argparse
import collections
import hashlib
import json
import pathlib


def sha_file(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=pathlib.Path, required=True)
    parser.add_argument("--audits", type=pathlib.Path, nargs="+", required=True)
    parser.add_argument("--comparison", type=pathlib.Path,
                        help="optional complete-suite report verifying every classified outcome")
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    baseline = json.loads(args.baseline.read_text())
    baseline_hash = sha_file(args.baseline)
    events = collections.defaultdict(list)
    for event in baseline["reads"]:
        if event["status"] == "native_rejected":
            events[event["file_sha256"]].append(event)
    records = {}
    source_audits = []
    for path in args.audits:
        audit = json.loads(path.read_text())
        if audit["baseline_sha256"] != baseline_hash:
            parser.error(f"{path}: audit refers to a different baseline")
        source_audits.append({"path": str(path), "sha256": sha_file(path)})
        for record in audit["records"]:
            identity = record["baseline_sha256"]
            if identity not in events or identity in records:
                parser.error(f"{path}: unknown or multiply classified source {identity}")
            reads = events[identity]
            if record["baseline_event_count"] != len(reads):
                parser.error(f"{path}: event count differs for {identity}")
            tests = sorted({event["test"] for event in reads})
            recorded_tests = record["baseline_tests"]
            if sorted(set(recorded_tests)) != tests:
                parser.error(f"{path}: test identities differ for {identity}")
            if len(recorded_tests) != len(set(recorded_tests)) and collections.Counter(recorded_tests) != collections.Counter(event["test"] for event in reads):
                parser.error(f"{path}: repeated test multiplicities differ for {identity}")
            if not record["input_condition"] or not record["reader_action"] or not record["evidence"]:
                parser.error(f"{path}: incomplete source classification {identity}")
            codecs = sorted({event["codec"] for event in reads})
            if len(codecs) != 1:
                parser.error(f"multiple codecs share {identity}; extend the audit key explicitly")
            records[identity] = {
                **record,
                "codec": codecs[0],
                "baseline_source_name": pathlib.Path(reads[0]["source"]).name,
                "baseline_native_errors": sorted({event["error"] for event in reads}),
                "scoped_audit": str(path),
            }
    if set(records) != set(events):
        parser.error("unclassified source hashes: " + ", ".join(sorted(set(events) - set(records))))
    rows = sorted(records.values(), key=lambda row: (row["codec"], row["baseline_source_name"]))
    comparison = None
    if args.comparison is not None:
        comparison = json.loads(args.comparison.read_text())
        if comparison["reference"]["commit"] != baseline["reference"]["commit"] or any(
                not codec["matches_pinned_source"] for codec in comparison["reference_codec_sources"].values()):
            parser.error("comparison reference differs from the pinned baseline")
        if comparison["reference_tests"]["failures"] or comparison["reference_tests"]["errors"]:
            parser.error("comparison reference tests failed")
        if not comparison["selection"]["all_codec_modules"] or not all(comparison[key] for key in [
                "probe_unchanged_during_run", "sources_unchanged_during_run", "bridge_unchanged_during_run"]):
            parser.error("comparison must be a complete suite with stable source/probe/bridge hashes")
        if comparison.get("schema", 0) >= 4 and not all(comparison[key] for key in [
                "vendored_sources_unchanged_during_run", "build_files_unchanged_during_run",
                "reference_component_library_unchanged_during_run"]):
            parser.error("comparison runtime dependency or native reference hashes changed")
        if comparison.get("schema", 0) >= 5 and not all(comparison[key] for key in [
                "explicit_policies_unchanged_during_run", "native_sources_unchanged_during_run"]):
            parser.error("comparison explicit source policies or native sources changed")
        if comparison.get("schema", 0) >= 6 and not comparison["reference_pillow_libraries_unchanged_during_run"]:
            parser.error("comparison Pillow reference dependency hashes changed")
        for row in rows:
            identity = row["baseline_sha256"]
            tested = [event for event in comparison["reads"] if event["codec"] == row["codec"]
                      and event.get("file_sha256") == identity]
            same_source = bool(tested)
            if not tested:
                # These six old ephemeral inputs were deleted by their tests.
                # Their timestamp-bearing replicas are case evidence, never
                # represented as byte-identical original sources.
                name = row["baseline_source_name"]
                recreated = (row["codec"] == "edfimage" and name.startswith(
                    "fabio.test.codecs.test_edfimage.TestBadGzFiles.") and name.endswith(".edf.gz")) or (
                    row["codec"] == "tifimage" and name == "tifimagewrite_test0000.tif")
                if not recreated:
                    parser.error(f"comparison has no exact original source {identity}")
                tested = [event for event in comparison["reads"] if event["codec"] == row["codec"]
                          and pathlib.Path(event["source"]).name == name
                          and event["test"] in row["baseline_tests"]]
            if collections.Counter(event["test"] for event in tested) != collections.Counter(
                    event["test"] for event in events[identity]):
                parser.error(f"comparison event/test multiplicities differ for {identity}")
            outcomes = collections.Counter(event["status"] for event in tested)
            closed = row["reader_action"] in {"implemented", "explicit_compatibility_closed", "explicit_source_layout_closed"}
            allowed = {"counts_match", "reference_view_match", "component_view_match"} if closed else (
                {"partial_source_read"} if row["reader_action"] == "partial_source_access" else {"native_rejected"})
            if not set(outcomes) <= allowed:
                parser.error(f"comparison contradicts classified action for {identity}: {dict(outcomes)}")
            if row["reader_action"] == "explicit_source_layout_closed" and any(
                    not event.get("explicit_read_policy") and not event.get("virtual_datasets") for event in tested):
                parser.error(f"comparison has no explicit source interpretation for {identity}")
            if row["reader_action"] == "partial_source_access" and any(
                    not event.get("source_recovery") or not event.get("independent_recovery_check") or
                    event["independent_recovery_check"]["differences"] for event in tested):
                parser.error(f"comparison has no independently verified partial source read for {identity}")
            if row["reader_action"] == "explicit_compatibility_closed" and any(not any(
                    window["external_policy"] == "UpToDeclared" and
                    window["declared_bytes"] > window["effective_bytes"] > 0
                    for window in event.get("payload_windows", [])) for event in tested):
                parser.error(f"comparison does not record the explicit compatibility window for {identity}")
            row["combined_suite_verification"] = {
                "source_identity": "exact_baseline_bytes" if same_source else "recreated_pinned_test_case",
                "source_sha256": sorted({event["file_sha256"] for event in tested}),
                "outcomes": dict(sorted(outcomes.items())),
                "events": len(tested),
            }
    def summarize(field):
        result = {}
        for row in rows:
            count = result.setdefault(row[field], {"events": 0, "unique_sources": 0})
            count["events"] += row["baseline_event_count"]
            count["unique_sources"] += 1
        return result
    report = {
        "schema": 1,
        "baseline": str(args.baseline),
        "baseline_sha256": baseline_hash,
        "contract": "All baseline native_rejected events classified by source; input condition and reader action are separate. Explicit compatibility support does not make inconsistent headers well formed.",
        "audited_events": sum(map(len, events.values())),
        "audited_unique_sources": len(events),
        "unclassified_events": 0,
        "input_conditions": summarize("input_condition"),
        "reader_actions": summarize("reader_action"),
        "scoped_audits": source_audits,
        "records": rows,
    }
    if comparison is not None:
        report["comparison"] = {"path": str(args.comparison), "sha256": sha_file(args.comparison),
                                "summary": comparison["summary"], "verified_events": sum(map(len, events.values()))}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: report[key] for key in ["audited_events", "audited_unique_sources", "input_conditions", "reader_actions"]}, indent=2))


if __name__ == "__main__":
    main()
