"""Independent CIF syntax trees and unchanged pinned FabIO observations.

Expected scope/order/value trees are handwritten in the CC0 manifest. Their
token spans are located from explicit ordered literal anchors, not by running
the native parser or reconstructing grammar from its output. FabIO acceptance
is observed separately and never establishes standards conformance.
"""
import argparse
import ast
import hashlib
import importlib
import io
import json
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
DATA = ROOT / "tests/data/cif-syntax"
PIN = "9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece"


def fingerprint(path):
    path = pathlib.Path(path).resolve()
    return {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


class Anchors:
    def __init__(self, payload):
        self.payload, self.position = payload, 0

    def token(self, raw, style="Bare", prefix=0):
        raw = raw.encode("ascii")
        start = self.payload.index(raw, self.position)
        end = start + len(raw)
        self.position = end
        left, right = start + prefix, end
        if style in ("SingleQuoted", "DoubleQuoted"):
            left, right = start + 1, end - 1
        elif style == "TextField":
            left, right = start + 1, end - 1
            if self.payload[right - 2:right] == b"\r\n":
                right -= 2
            elif self.payload[right - 1:right] in (b"\n", b"\r"):
                right -= 1
            else:
                raise ValueError("expected fixture text field has no closing EOL")
        return {"span": [start, end], "content": [left, right], "style": style,
                "text": self.payload[left:right].decode("ascii"), "raw_hex": raw.hex()}

    def value(self, spec):
        token = self.token(spec["raw"], spec["style"])
        null = {"?": "Unknown", ".": "Inapplicable"}.get(spec["raw"]) if spec["style"] == "Bare" else None
        return {"token": token, "null": null}

    def items(self, specs):
        result = []
        for spec in specs:
            kind = spec["kind"]
            if kind == "Scalar":
                name = self.token(spec["name"])
                value = self.value(spec["value"])
                result.append({"kind": kind, "name": name, "value": value,
                               "span": [name["span"][0], value["token"]["span"][1]]})
            elif kind == "Loop":
                heading = self.token("loop_")
                columns = [self.token(column) for column in spec["columns"]]
                values = [self.value(value) for value in spec["values"]]
                if not columns or not values or len(values) % len(columns):
                    raise ValueError("invalid independent fixture tree")
                result.append({"kind": kind, "columns": columns, "values": values,
                               "rows": len(values) // len(columns),
                               "span": [heading["span"][0], values[-1]["token"]["span"][1]]})
            elif kind == "SaveFrame":
                name = self.token(spec["heading"], prefix=5)
                items = self.items(spec["items"])
                closing = self.token("save_")
                result.append({"kind": kind, "name": name, "items": items,
                               "span": [name["span"][0], closing["span"][1]]})
            else:
                raise ValueError(f"unknown independent fixture item {kind}")
        return result


def expected_record(spec, payload, provenance):
    anchors = Anchors(payload)
    blocks = []
    for block in spec["expected_blocks"]:
        name = anchors.token(block["heading"], prefix=5)
        items = anchors.items(block["items"])
        if blocks:
            blocks[-1]["span"][1] = name["span"][0]
        blocks.append({"name": name, "items": items, "span": [name["span"][0], len(payload)]})
    return {"kind": "cif_syntax", "version": "Cif11", "provenance": provenance,
            "blocks": blocks, "source_bytes": len(payload),
            "source_sha256": hashlib.sha256(payload).hexdigest()}


def differences(actual, expected, path="$"):
    if type(actual) is not type(expected):
        return [{"field": path, "actual": actual, "expected": expected}]
    if isinstance(expected, dict):
        delta = []
        for key in sorted(actual.keys() | expected.keys()):
            if key not in actual or key not in expected:
                delta.append({"field": f"{path}.{key}", "kind": "missing_or_extra_field"})
            else:
                delta.extend(differences(actual[key], expected[key], f"{path}.{key}"))
        return delta
    if isinstance(expected, list):
        if len(actual) != len(expected):
            return [{"field": path, "kind": "length", "actual": len(actual), "expected": len(expected)}]
        return [item for index, (a, b) in enumerate(zip(actual, expected))
                for item in differences(a, b, f"{path}[{index}]")]
    return [] if actual == expected else [{"field": path, "actual": actual, "expected": expected}]


def compare_record(native, spec, payload, provenance):
    """No ignored spans, source bytes, raw literals, scope, types or null flags."""
    return differences(native, expected_record(spec, payload, provenance))


def rejection_matches(spec, error):
    unsupported = "unsupported CIF syntax version" in error
    specific = spec.get("expected_error", "MISSING_EXPECTED_ERROR") in error
    return specific and (unsupported if spec["input_condition"] == "unsupported_cif2" else not unsupported)


def flat_projection(record):
    """Explicit consumer projection solely to observe legacy loss of scopes."""
    fields, loops = {}, []
    def visit(items):
        for item in items:
            if item["kind"] == "Scalar":
                fields[item["name"]["text"]] = item["value"]["token"]["text"]
            elif item["kind"] == "Loop":
                columns = [t["text"] for t in item["columns"]]
                values = [v["token"]["text"] for v in item["values"]]
                loops.append({"columns": columns, "rows": [values[n:n+len(columns)] for n in range(0, len(values), len(columns))]})
            elif item["kind"] == "SaveFrame":
                visit(item["items"])
    for block in record["blocks"]:
        visit(block["items"])
    return {"fields": fields, "loops": loops}


def reference_worker(path, method, transport):
    from fabio.cbfimage import CIF
    from cif_document_reference import reference_projection
    try:
        cif = CIF()
        getattr(cif, method)(str(path) if transport == "path" else io.BytesIO(path.read_bytes()))
        def json_safe(value):
            if isinstance(value, (bytes, bytearray)):
                return {"kind": "Bytes", "hex": bytes(value).hex()}
            if isinstance(value, dict):
                return {key: json_safe(item) for key, item in value.items()}
            if isinstance(value, list):
                return [json_safe(item) for item in value]
            return value
        return {"status": "read_completed", "projection": json_safe(reference_projection(cif))}
    except Exception as error:
        return {"status": "read_not_completed", "error": f"{type(error).__name__}: {error}"}


def reference_reads(path):
    records = []
    with tempfile.TemporaryDirectory(prefix="dioptrin-cif-syntax-reference-") as directory:
        for method in ("loadCIF", "readCIF"):
            for transport in ("path", "BytesIO"):
                result = subprocess.run([sys.executable, str(pathlib.Path(__file__).resolve()),
                    "--reference-worker", str(path), "--method", method, "--transport", transport],
                    cwd=directory, capture_output=True, text=True, timeout=15)
                record = {"method": method, "transport": transport, "exit_code": result.returncode}
                if result.returncode:
                    record.update(status="reference_process_failure", stderr=result.stderr[-2000:])
                else:
                    record.update(json.loads(result.stdout))
                records.append(record)
    return records


def gemmi_observation(path, module_directory):
    if module_directory is None:
        return {"status": "not_requested"}
    sys.path.insert(0, str(module_directory))
    gemmi = importlib.import_module("gemmi")
    files = sorted(pathlib.Path(gemmi.__file__).parent.glob("*.so"))
    record = {"version": gemmi.__version__, "module": fingerprint(gemmi.__file__),
              "native_modules": [fingerprint(p) for p in files],
              "role": "independent parser observation; normative CIF 1.1 rules take precedence"}
    try:
        document = gemmi.cif.read_file(str(path))
        def items(block):
            result = []
            for item in block:
                if item.pair is not None:
                    result.append({"kind": "Scalar", "name": item.pair[0], "raw_value": item.pair[1]})
                elif item.loop is not None:
                    result.append({"kind": "Loop", "columns": list(item.loop.tags), "raw_values": list(item.loop.values)})
                elif item.frame is not None:
                    result.append({"kind": "SaveFrame", "name": item.frame.name, "items": items(item.frame)})
            return result
        record.update(status="read_completed", blocks=[{"name": b.name, "items": items(b)} for b in document])
    except Exception as error:
        record.update(status="read_not_completed", error=f"{type(error).__name__}: {error}")
    return record


def verify(probe, path, spec, independent_module_directory=None):
    path = pathlib.Path(path).resolve()
    payload = path.read_bytes()
    source = fingerprint(path)
    record = {"source": source, "input_condition": spec["input_condition"],
              "reference_qualification": spec.get("reference_qualification", []),
              "reference": reference_reads(path), "independent": gemmi_observation(path, independent_module_directory), "apis": []}
    expected_success = spec["input_condition"] == "valid_cif11"
    if expected_success:
        projection = flat_projection(expected_record(spec, payload, "Buffer"))
        record["reference_flat_projection_comparison"] = [
            {"method": r["method"], "transport": r["transport"],
             "differences": differences(r["projection"], projection) if r["status"] == "read_completed" else [{"kind": "reference_failed_on_valid_cif11"}]}
            for r in record["reference"]]
    else:
        record["reference_accepts_malformed_or_unsupported_source"] = any(r["status"] == "read_completed" for r in record["reference"])
    for command, provenance in [("cif-syntax", "PathSnapshot"), ("cif-syntax-bytes", "Buffer"), ("cif-syntax-stream", "StreamCapture")]:
        result = subprocess.run([str(probe), command, str(path)], input=payload if provenance == "StreamCapture" else None,
                                capture_output=True, timeout=30)
        api = {"command": command, "exit_code": result.returncode}
        if result.returncode == 0:
            native = json.loads(result.stdout)
            delta = compare_record(native, spec, payload, provenance) if expected_success else [{"kind": "malformed_or_unsupported_source_was_accepted"}]
            api.update(status="match" if not delta else "difference", native=native, differences=delta)
        else:
            error = result.stderr.decode("utf-8", "replace")
            unsupported = "unsupported CIF syntax version" in error
            match = not expected_success and rejection_matches(spec, error)
            api.update(status="expected_rejection" if match else "unexpected_rejection", error=error,
                       rejection_kind="UnsupportedVersion" if unsupported else "MalformedCif11OrResourceError")
        record["apis"].append(api)
    record["source_unchanged"] = source == fingerprint(path)
    record["status"] = "qualified" if record["source_unchanged"] and all(a["status"] in ("match", "expected_rejection") for a in record["apis"]) else "difference"
    return record


def upstream_fixture(upstream, directory):
    """Exact existing test literal; do not fix its duplicate chemical formula."""
    path = pathlib.Path(upstream) / "src/fabio/test/codecs/test_cbfimage.py"
    tree = ast.parse(path.read_text())
    literals = [node.value for node in ast.walk(tree) if isinstance(node, ast.Constant)
                and isinstance(node.value, str) and node.value.startswith("\ndata_global\n_amcsd_formula_title")]
    if len(literals) != 1:
        raise ValueError("pinned upstream CIF test literal is missing or ambiguous")
    target = pathlib.Path(directory) / "pinned-test-cif-LaB6.cif"
    target.write_bytes(literals[0].encode("ascii"))
    return target, {"input_condition": "malformed_cif11", "expected_error": "duplicate case-insensitive CIF name", "reference_qualification": ["actual_pinned_test_duplicate_chemical_formula_sum"], "upstream_module": fingerprint(path)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=pathlib.Path)
    parser.add_argument("--upstream", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--independent-module-directory", type=pathlib.Path)
    parser.add_argument("--reference-worker", type=pathlib.Path)
    parser.add_argument("--method", choices=("loadCIF", "readCIF"))
    parser.add_argument("--transport", choices=("path", "BytesIO"))
    args = parser.parse_args()
    if args.reference_worker:
        print(json.dumps(reference_worker(args.reference_worker, args.method, args.transport)))
        return
    if args.probe is None or args.upstream is None:
        parser.error("--probe and --upstream are required")
    from fabio import cbfimage
    manifest = json.loads((DATA / "provenance.json").read_text())
    tracked = [pathlib.Path(__file__), ROOT / "scripts/test_cif_syntax_reference.py",
               ROOT / "scripts/cif_document_reference.py", ROOT / "scripts/companion_reference.py",
               ROOT / "tests/cif_syntax.rs", ROOT / "tests/cif_document.rs", ROOT / "examples/api_probe.rs",
               DATA / "provenance.json", pathlib.Path(cbfimage.__file__),
               pathlib.Path(sys.modules[cbfimage.split_tokens.__module__].__file__),
               args.upstream / "src/fabio/cbfimage.py", args.upstream / "src/fabio/ext/_cif.pyx",
               args.upstream / "src/fabio/test/codecs/test_cbfimage.py", *sorted(DATA.glob("*.cif")),
               *sorted((ROOT / "src/cbf").rglob("*.rs")), ROOT / "src/cbf.rs", ROOT / "Cargo.toml", ROOT / "Cargo.lock"]
    before = [fingerprint(p) for p in tracked]
    pinned = fingerprint(args.upstream / "src/fabio/cbfimage.py")
    actual = fingerprint(cbfimage.__file__)
    records = []
    for spec in manifest["fixtures"]:
        path = DATA / spec["file"]
        if fingerprint(path)["sha256"] != spec["sha256"]:
            raise ValueError(f"fixture changed: {path}")
        records.append(verify(args.probe.resolve(), path, spec, args.independent_module_directory))
    with tempfile.TemporaryDirectory(prefix="dioptrin-pinned-cif-") as directory:
        path, spec = upstream_fixture(args.upstream, directory)
        record = verify(args.probe.resolve(), path, spec, args.independent_module_directory)
        record["upstream_module"] = spec["upstream_module"]
        records.append(record)
    report = {"schema": 1, "kind": "standalone_cif_syntax", "pinned_fabio_commit": PIN,
              "reference_matches_pinned": actual["sha256"] == pinned["sha256"],
              "probe": fingerprint(args.probe), "inputs": before, "records": records,
              "inputs_unchanged": before == [fingerprint(p) for p in tracked],
              "supported_version": "CIF 1.1", "remaining_gap": "CIF 2.0 Unicode/composites/triple-quotes/text protocols explicitly UnsupportedVersion"}
    independent_sources = [source for r in records for source in
        ([r["independent"]["module"], *r["independent"]["native_modules"]] if "module" in r["independent"] else [])]
    report["independent_modules_unchanged"] = all(fingerprint(source["path"]) == source for source in independent_sources)
    report["qualified"] = report["reference_matches_pinned"] and report["inputs_unchanged"] and report["independent_modules_unchanged"] and all(r["status"] == "qualified" for r in records)
    text = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end="")
    if not report["qualified"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
