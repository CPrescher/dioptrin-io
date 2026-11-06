"""CIF2 source/AST/logical-view proof against handwritten trees and pinned oracles.

FabIO is called unchanged. PyCifRW is an optional independent semantic reader;
its acceptance is not normative validation and its dictionaries cannot retain
duplicate table entries, original quoting or source spans.
"""
import argparse
import contextlib
import importlib
import importlib.metadata
import io
import json
import pathlib
import subprocess
import sys

from cif_syntax_reference import differences, fingerprint, reference_reads

ROOT = pathlib.Path(__file__).resolve().parents[1]
DATA = ROOT / "tests/data/cif2"
PIN = "9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece"


class Anchors:
    """Locate ordered literals from an explicitly supplied tree, not CIF grammar."""
    def __init__(self, source):
        self.source, self.position = source, 0

    def token(self, raw, style="Bare", prefix=0):
        raw = raw.encode("utf8")
        start = self.source.index(raw, self.position)
        end = start + len(raw)
        self.position = end
        left, right = start + prefix, end
        if style in ("SingleQuoted", "DoubleQuoted"):
            left, right = start + 1, end - 1
        elif style in ("TripleSingleQuoted", "TripleDoubleQuoted"):
            left, right = start + 3, end - 3
        elif style == "TextField":
            left, right = start + 1, end - 1
            if self.source[right-2:right] == b"\r\n":
                right -= 2
            elif self.source[right-1:right] in (b"\r", b"\n"):
                right -= 1
            else:
                raise ValueError("text field fixture requires explicit closing EOL")
        return {"span": [start, end], "content": [left, right], "style": style,
                "text": self.source[left:right].decode("utf8"), "raw_hex": raw.hex()}

    def logical(self, token, spec=None):
        text = token["text"]
        expected = {"text": text.replace("\r\n", "\n").replace("\r", "\n"),
                    "normalized_eol": "\r" in text, "prefix": None, "folded": False}
        # Prefix/folding output is handwritten, never derived by native code.
        expected.update((spec or {}).get("logical_text", {}))
        return expected

    def value(self, spec):
        kind = spec.get("kind")
        if kind not in ("List", "Table"):
            token = self.token(spec["raw"], spec["style"])
            null = {"?": "Unknown", ".": "Inapplicable"}.get(spec["raw"]) if spec["style"] == "Bare" else None
            return {"token": token, "null": null, "composite": None, "logical_text": self.logical(token, spec)}
        opening = self.token("[" if kind == "List" else "{")
        if kind == "List":
            composite = {"kind": kind, "values": [self.value(v) for v in spec["values"]]}
        else:
            entries = []
            for entry in spec["entries"]:
                key = self.token(entry["key"]["raw"], entry["key"]["style"])
                colon = self.token(":")
                if colon["span"][0] != key["span"][1]:
                    raise ValueError("independent positive table fixture has separated colon")
                entries.append({"key": key, "value": self.value(entry["value"]),
                                "key_logical_text": self.logical(key, entry["key"])})
            composite = {"kind": kind, "entries": entries}
        closing = self.token("]" if kind == "List" else "}")
        start, end = opening["span"][0], closing["span"][1]
        left, right = opening["span"][1], closing["span"][0]
        token = {"span": [start, end], "content": [left, right], "style": kind,
                 "text": self.source[left:right].decode("utf8"), "raw_hex": self.source[start:end].hex()}
        return {"token": token, "null": None, "composite": composite}

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
                columns = [self.token(c) for c in spec["columns"]]
                values = [self.value(v) for v in spec["values"]]
                if not columns or not values or len(values) % len(columns):
                    raise ValueError("independent fixture loop has bad cardinality")
                result.append({"kind": kind, "columns": columns, "values": values,
                               "rows": len(values)//len(columns), "span": [heading["span"][0], values[-1]["token"]["span"][1]]})
            elif kind == "SaveFrame":
                name = self.token(spec["heading"], prefix=5)
                items = self.items(spec["items"])
                closing = self.token("save_")
                result.append({"kind": kind, "name": name, "items": items,
                               "span": [name["span"][0], closing["span"][1]]})
            else:
                raise ValueError(f"unknown independent tree kind {kind}")
        return result


def expected_record(spec, source, provenance):
    import hashlib
    anchors = Anchors(source)
    blocks = []
    for block in spec["expected_blocks"]:
        name = anchors.token(block["heading"], prefix=5)
        items = anchors.items(block["items"])
        if blocks:
            blocks[-1]["span"][1] = name["span"][0]
        blocks.append({"name": name, "items": items, "span": [name["span"][0], len(source)]})
    return {"kind": "cif_syntax", "version": "Cif20", "provenance": provenance,
            "blocks": blocks, "source_bytes": len(source), "source_sha256": hashlib.sha256(source).hexdigest()}


def compare_record(native, spec, source, provenance):
    return differences(native, expected_record(spec, source, provenance))


def semantic_projection(native):
    """Explicit lossy map projection for comparison with PyCifRW only."""
    blocks = []
    def value(v):
        composite = v["composite"]
        if composite is None:
            return v["logical_text"]["text"]
        if composite["kind"] == "List":
            return [value(x) for x in composite["values"]]
        # The actual independent oracle overwrites duplicate map keys. Native
        # source AST comparison above never uses this projection.
        return {entry["key_logical_text"]["text"]: value(entry["value"]) for entry in composite["entries"]}
    def visit(name, items, parent):
        fields = {}
        frames = []
        for item in items:
            if item["kind"] == "Scalar":
                fields[item["name"]["text"].lower()] = value(item["value"])
            elif item["kind"] == "Loop":
                width = len(item["columns"])
                for index, column in enumerate(item["columns"]):
                    fields[column["text"].lower()] = [value(v) for v in item["values"][index::width]]
            else:
                frames.append(item)
        blocks.append({"name": name, "parent": parent, "fields": fields})
        for frame in frames:
            visit(frame["name"]["text"], frame["items"], name)
    for block in native["blocks"]:
        visit(block["name"]["text"], block["items"], None)
    # The oracle's container dictionary has no source-order contract. Complete
    # source order is checked above, not by this separate named-scope view.
    return sorted(blocks,key=lambda b:(b["parent"] or "",b["name"]))


def independent_observation(path, directory, transport="StringIO"):
    if directory is None:
        return {"status": "not_requested"}
    sys.path.insert(0, str(directory))
    CifFile = importlib.import_module("CifFile")
    package=pathlib.Path(CifFile.__file__).parent
    modules = [fingerprint(p) for p in sorted(package.rglob("*")) if p.is_file() and p.suffix in (".py",".so")]
    log = io.StringIO()
    record = {"name": "PyCifRW", "version": importlib.metadata.version("PyCifRW"), "modules": modules,"transport":transport,
              "role": "independent semantic parser; unordered named scopes/lowercased fields/map overwrite/styles and spans are explicit representation limits"}
    try:
        with contextlib.redirect_stdout(log):
            argument=str(path) if transport=="path" else io.StringIO(path.read_bytes().decode("utf8"))
            cif = CifFile.ReadCif(argument, grammar="2.0", scoping="dictionary")
        def clean(value):
            if isinstance(value, dict):
                return {key: clean(v) for key, v in value.items()}
            if isinstance(value, list):
                return [clean(v) for v in value]
            if isinstance(value, str):
                return value
            raise ValueError(f"unexpected independent value type {type(value).__name__}")
        blocks = []
        for name in cif.keys():
            scope = cif.child_table[name]
            parent = cif.child_table[scope.parent].block_id if scope.parent is not None else None
            block = cif[name]
            blocks.append({"name": scope.block_id, "parent": parent, "fields": {key: clean(block[key]) for key in block.keys()}})
        record.update(status="read_completed", blocks=sorted(blocks,key=lambda b:(b["parent"] or "",b["name"])))
    except Exception as error:
        record.update(status="read_not_completed", error=f"{type(error).__name__}: {error}")
    record["diagnostic_stdout"] = log.getvalue()[-2000:]
    return record


def verify(probe, spec, directory=None):
    path = (DATA/spec["file"]).resolve()
    source = path.read_bytes()
    before = fingerprint(path)
    if before["sha256"] != spec["sha256"]:
        raise ValueError(f"fixture changed: {path}")
    positive = spec["reader_action"] == "read"
    record = {"source": before, "input_condition": spec["input_condition"], "notes": spec.get("notes", []),
              "reference": reference_reads(path), "independent": independent_observation(path, directory),
              "independent_path":independent_observation(path,directory,"path"),"apis": []}
    for oracle,comparison in [("independent","independent_semantic_differences"),("independent_path","independent_path_semantic_differences")]:
        if positive and record[oracle]["status"] == "read_completed":
            expected = semantic_projection(expected_record(spec, source, "Buffer"))
            record[comparison] = differences(record[oracle]["blocks"], expected)
    for command, provenance in [("cif-syntax", "PathSnapshot"), ("cif-syntax-bytes", "Buffer"), ("cif-syntax-stream", "StreamCapture")]:
        result = subprocess.run([str(probe), command, str(path)], input=source if provenance == "StreamCapture" else None,
                                capture_output=True, timeout=30)
        api = {"command": command, "exit_code": result.returncode}
        if result.returncode == 0:
            native = json.loads(result.stdout)
            delta = compare_record(native, spec, source, provenance) if positive else [{"kind": "source_was_accepted_despite_expected_rejection"}]
            api.update(status="match" if not delta else "difference", differences=delta, native=native)
        else:
            error = result.stderr.decode("utf8", "replace")
            matched = not positive and spec["expected_error"] in error
            api.update(status="expected_rejection" if matched else "unexpected_rejection", error=error)
        record["apis"].append(api)
    record["source_unchanged"] = before == fingerprint(path)
    record["status"] = "qualified" if record["source_unchanged"] and all(a["status"] in ("match", "expected_rejection") for a in record["apis"]) else "difference"
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=pathlib.Path, required=True)
    parser.add_argument("--upstream", type=pathlib.Path, required=True)
    parser.add_argument("--independent-module-directory", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path)
    args = parser.parse_args()
    from fabio import cbfimage
    manifest = json.loads((DATA/"provenance.json").read_text())
    paths = [pathlib.Path(__file__), ROOT/"scripts/test_cif2_reference.py", ROOT/"scripts/cif_syntax_reference.py",
             ROOT/"scripts/cif_document_reference.py", ROOT/"scripts/companion_reference.py", DATA/"provenance.json",
             ROOT/"tests/cif2_syntax.rs", ROOT/"tests/cif_syntax.rs", ROOT/"examples/api_probe.rs",
             ROOT/"Cargo.toml", ROOT/"Cargo.lock", *sorted((ROOT/"src").rglob("*.rs")),
             pathlib.Path(cbfimage.__file__), pathlib.Path(sys.modules[cbfimage.split_tokens.__module__].__file__),
             args.upstream/"src/fabio/cbfimage.py", args.upstream/"src/fabio/ext/_cif.pyx",
             *[(DATA/s["file"]).resolve() for s in manifest["fixtures"]]]
    before = [fingerprint(p) for p in paths]
    records = [verify(args.probe.resolve(), spec, args.independent_module_directory) for spec in manifest["fixtures"]]
    independent = [m for record in records for oracle in ("independent","independent_path") for m in record[oracle].get("modules", [])]
    report = {"schema": 1, "kind": "cif2_syntax_continuation", "pinned_fabio_commit": PIN,
              "probe": fingerprint(args.probe), "inputs": before, "records": records,
              "reference_matches_pinned": fingerprint(cbfimage.__file__)["sha256"] == fingerprint(args.upstream/"src/fabio/cbfimage.py")["sha256"],
              "inputs_unchanged": before == [fingerprint(p) for p in paths],
              "independent_modules_unchanged": all(fingerprint(m["path"]) == m for m in independent),
              "representation": "Original UTF8 tokens/spans/ordered scoped trees; logical views explicit; no numeric coercion",
              "implementation_profile": "Unicode16 canonical caseless identifiers; default64/max128 composite nesting; explicit source/metadata/value/view bounds"}
    report["qualified"] = report["reference_matches_pinned"] and report["inputs_unchanged"] and report["independent_modules_unchanged"] and all(r["status"] == "qualified" for r in records)
    text = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end="")
    if not report["qualified"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
