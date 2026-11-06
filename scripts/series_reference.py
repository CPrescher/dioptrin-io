"""Observe unchanged pinned FabIO series tests and compare genuine native APIs.

Observers never seek, consume or alter a reference series. Native batches run
only after the original filename iterator is exhausted, before source teardown.
Fresh independent reference readers establish file boundaries and cursor values.
FilenameObject parsing remains reference-only scope; actual opens are compared.
"""
import argparse
import contextlib
import hashlib
import importlib
import inspect
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import numpy as np
import metadata_parity

PIN = "9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece"
ROOT = pathlib.Path(__file__).resolve().parents[1]
TESTS = ("test_file_series", "test_filename_steps", "test_filenames")
# Expected observer coverage of these unchanged tests at PIN. This is not a
# guessed read count: missing hooks/early-close omissions must fail the audit.
PINNED_COVERAGE = {"reference_tests": 37, "reference_failures": 0, "reference_errors": 0,
                   "reference_skips": 0, "series_instances": 19, "observed_series_requests": 49,
                   "observed_iterator_yields": 20, "observed_series_successes": 40,
                   "observed_series_errors": 9, "object_open_reads": 25, "filename_helper_calls": 36,
                   "numbered_constructors": 8, "unique_numbered_batches": 2,
                   "numbered_paths_compared": 10010}


def coverage_differences(summary):
    return [{"field": key, "expected_pinned_coverage": value, "observed": summary.get(key)}
            for key, value in PINNED_COVERAGE.items() if summary.get(key) != value]


def fingerprint(path):
    path = pathlib.Path(path)
    return {"path": str(path.resolve()), "bytes": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     allow_nan=False).encode()).hexdigest()


def signature(frame):
    array = np.asarray(frame.data)
    canonical = np.ascontiguousarray(array.astype(array.dtype.newbyteorder("<"), copy=False))
    return {"dtype": array.dtype.name, "shape": list(array.shape),
            "sample_sha256": hashlib.sha256(canonical.tobytes()).hexdigest(),
            "fields": metadata_parity.encode(dict(frame.header))}


def frame_record(frame):
    result = signature(frame)
    file = frame.file_container
    result.update(index=frame.index, local_frame=frame.file_index,
                  path=str(file.filename) if file is not None else None)
    return result


def compare_frame(native, reference, location):
    differences = []
    for key in ("dtype", "shape", "sample_sha256", "index", "file_index", "local_frame", "path"):
        expected = location.get(key, reference.get(key))
        if native.get(key) != expected:
            differences.append({"field": key, "reference": expected, "native": native.get(key)})
    metadata = metadata_parity.compare(reference["fields"], native, "edfimage")
    if metadata["unresolved_fields"]:
        differences.append({"field": "metadata", "comparison": metadata})
    return differences, metadata


def expected_move(anchor, count, command):
    """Independent integer model; -1/count are distinct boundary anchors."""
    operation = command["op"]
    if operation == "read":
        target = command["index"]
    elif operation == "first":
        target = 0
    elif operation == "last":
        target = count - 1
    else:
        target = anchor + ({"next": 1, "previous": -1}.get(operation)
                           if operation != "relative" else command["offset"])
    if target < 0:
        return -1, {"status": "error", "position": {"kind": "BeforeFirst"}, "current": None,
                    "pixels_available": False}
    if target >= count:
        return count, {"status": "error", "position": {"kind": "AfterLast"}, "current": None,
                       "pixels_available": False}
    return target, {"status": "ok", "position": {"kind": "Frame", "index": target},
                    "current": target, "pixels_available": True}


def compare_action(native, expected):
    differences = []
    for key in ("status", "position", "current", "pixels_available"):
        if native.get(key) != expected[key]:
            differences.append({"field": key, "reference": expected[key], "native": native.get(key)})
    if expected["status"] == "error":
        if native.get("frame") is not None:
            differences.append({"field": "failed_move_exposes_frame", "native": native["frame"]})
        if not native.get("error"):
            differences.append({"field": "missing_error_diagnostic"})
    elif native.get("frame") is None:
        differences.append({"field": "missing_success_frame"})
    return differences


def control_trace(count):
    return [{"op": "next"}, {"op": "previous"}, {"op": "previous"}, {"op": "next"},
            {"op": "last"}, {"op": "next"}, {"op": "next"}, {"op": "previous"},
            {"op": "relative", "offset": -count}, {"op": "relative", "offset": count},
            {"op": "first"}, {"op": "read", "index": -1}, {"op": "next"},
            {"op": "read", "index": (1 << 63) - 1}, {"op": "previous"},
            {"op": "read", "index": 0}, {"op": "relative", "offset": -(1 << 63)},
            {"op": "relative", "offset": (1 << 63) - 1}, {"op": "first"},
            {"op": "relative", "offset": 0}, {"op": "last"}]


class Probe:
    def __init__(self, executable, timeout=120):
        self.executable, self.timeout = pathlib.Path(executable).resolve(), timeout

    def __call__(self, kind, request):
        with tempfile.TemporaryDirectory(prefix="dioptrin-series-request-") as directory:
            path = pathlib.Path(directory) / "request.json"
            path.write_text(json.dumps(request, allow_nan=False))
            try:
                result = subprocess.run([str(self.executable), kind, str(path)], capture_output=True,
                                        text=True, timeout=self.timeout, check=False)
            except (OSError, subprocess.TimeoutExpired) as error:
                return {"status": "probe_error", "error_type": type(error).__name__, "error": str(error)}
        if result.returncode:
            return {"status": "error", "returncode": result.returncode, "stderr": result.stderr}
        try:
            response = json.loads(result.stdout)
            if not isinstance(response, dict):
                raise ValueError("probe response must be one JSON object")
        except ValueError as error:
            return {"status": "probe_error", "error_type": type(error).__name__, "error": str(error)}
        return {"status": "ok", "response": response}


class Observer:
    def __init__(self, probe):
        import fabio
        self.fabio = fabio
        self.module = importlib.import_module("fabio.file_series")
        self.probe = probe
        self.original_open = fabio.open
        self.current_test = None
        self.suspended = False
        self.states, self.series, self.numbered = {}, [], []
        self.filename_calls, self.object_reads, self.object_batches = [], [], []
        self.errors, self.numbered_cache = [], {}

    def problem(self, scope, error):
        self.errors.append({"test": self.current_test, "scope": scope,
                            "error_type": type(error).__name__, "error": str(error)})

    @contextlib.contextmanager
    def independent(self):
        old = self.suspended
        self.suspended = True
        try:
            yield
        finally:
            self.suspended = old

    def files(self, paths):
        files, count = [], 0
        for path in paths:
            with self.original_open(path) as image:
                frames = image.nframes
            files.append({"path": str(path), "first_frame": count,
                          "frames": frames})
            count += frames
        return files, count

    @staticmethod
    def locate(files, index):
        for ordinal, file in enumerate(files):
            if file["first_frame"] <= index < file["first_frame"] + file["frames"]:
                return {"index": index, "file_index": ordinal,
                        "local_frame": index - file["first_frame"], "path": file["path"]}
        raise IndexError(index)

    def legacy_negative(self, paths):
        """Qualify codec getframe separately; never touch the observed series."""
        if not paths:
            return None
        with self.original_open(paths[0]) as image:
            result = {"scope": "fresh EDF codec getframe(-1); not FileSeries.get_frame",
                      "source": paths[0], "source_frames": image.nframes,
                      "native_policy": "signed absolute negatives reject BeforeFirst without filename traversal"}
            try:
                frame = image.getframe(-1)
                result.update(reference_status="ok", reference_currentframe=frame.currentframe,
                              reference=signature(frame))
                if image.nframes > 1:
                    final = signature(image.getframe(image.nframes - 1))
                    result.update(qualification="legacy EDF accepts Python negative list indexing",
                                  reference_matches_final_declared_frame=result["reference"] == final)
                else:
                    result["qualification"] = "single-frame codec getframe delegates to absolute filename-number lookup"
            except Exception as error:
                result.update(reference_status="error", error_type=type(error).__name__, error=str(error),
                              qualification="single-frame codec getframe delegates to filename lookup; native cursor does not traverse filenames")
        return result

    def audit(self, paths, events, scope):
        before = [fingerprint(path) for path in paths]
        files, count = self.files(paths)
        observed = [{"op": "read", "index": event["requested"]} for event in events]
        commands = observed + control_trace(count)
        native = self.probe("series", {"format": "edf", "paths": paths, "commands": commands})
        record = {"test": self.current_test, "scope": scope, "sources": before, "files": files,
                  "frames": count, "observed_requests": len(events), "actions": [], "differences": []}
        if native["status"] != "ok":
            record["differences"].append({"field": "native_series_open", "native": native})
            return record
        response = native["response"]
        for key, expected in (("frames", count), ("files", files)):
            if response.get(key) != expected:
                record["differences"].append({"field": key, "reference": expected, "native": response.get(key)})
        actions = response.get("actions", [])
        if len(actions) != len(commands):
            record["differences"].append({"field": "action_count", "reference": len(commands), "native": len(actions)})
        anchor = -1
        with self.independent():
            fresh = self.module.FileSeries(filenames=list(paths))
            try:
                for order, (command, action) in enumerate(zip(commands, actions)):
                    anchor, expected = expected_move(anchor, count, command)
                    row = {"origin": "observed_reference_call" if order < len(events) else "independent_cursor_control",
                           "command": command, "native": action, "differences": compare_action(action, expected)}
                    if action.get("command") != command:
                        row["differences"].append({"field": "command", "reference": command, "native": action.get("command")})
                    reference = None
                    if order < len(events):
                        event = events[order]
                        row["reference"] = event
                        if event["status"] != expected["status"]:
                            row["differences"].append({"field": "observed_reference_status", "reference": event["status"], "policy": expected["status"]})
                        reference = event.get("frame")
                    elif expected["status"] == "ok":
                        reference = frame_record(fresh.get_frame(anchor))
                        row["reference"] = {"status": "ok", "frame": reference}
                    if expected["status"] == "ok" and reference is not None and action.get("frame"):
                        location = self.locate(files, anchor)
                        for key in ("index", "local_frame", "path"):
                            if reference.get(key) != location[key]:
                                row["differences"].append({"field": "reference_mapping." + key,
                                                           "reference": reference.get(key), "source_location": location[key]})
                        delta, metadata = compare_frame(action["frame"], reference, location)
                        row["differences"].extend(delta)
                        row["metadata"] = metadata
                    record["actions"].append(row)
            finally:
                fresh.close()
        record["legacy_codec_negative_qualification"] = self.legacy_negative(paths)
        qualification = record["legacy_codec_negative_qualification"]
        if qualification and qualification.get("reference_matches_final_declared_frame") is False:
            record["differences"].append({"field": "legacy_negative_reference_proof", "reference": qualification})
        record["sources_unchanged"] = before == [fingerprint(path) for path in paths]
        if not record["sources_unchanged"]:
            record["differences"].append({"field": "source_changed"})
        record["status"] = "match" if not record["differences"] and all(not row["differences"] for row in record["actions"]) else "difference"
        return record

    def finish_series(self, instance):
        state = self.states[id(instance)]
        if state["closed"]:
            return
        state["closed"] = True
        if instance._FileSeries__filename_generator is not None:
            self.errors.append({"test": state["test"], "scope": "unconsumed_filename_iterator",
                                "reason": "observer refuses to consume reference input early"})
            return
        paths = [str(path) for path in instance._FileSeries__filenames]
        record = self.audit(paths, state["events"], "FileSeries")
        record["test"] = state["test"]
        record["iterator_yields"] = state["yields"]
        self.series.append(record)

    def record_numbered(self, instance, args, kwargs, original):
        bound = inspect.signature(original).bind(instance, *args, **kwargs)
        bound.apply_defaults()
        config = bound.arguments
        # Separate fresh reference list; do not change the observed _current.
        fresh = self.module.numbered_file_series.__new__(self.module.numbered_file_series)
        original(fresh, *args, **kwargs)
        reference = list(fresh)
        numbers = list(range(config["first"], config["last"] + 1, config["step"]))
        request = {"seed": reference[0], "step": config["step"], "numbers": numbers}
        key = digest(request)
        if key not in self.numbered_cache:
            native = self.probe("filename", request)
            differences = []
            if native["status"] != "ok":
                differences.append({"field": "native_filename", "native": native})
            else:
                value = native["response"]
                if value.get("paths") != reference:
                    differences.append({"field": "paths", "reference_sha256": digest(reference), "native_sha256": digest(value.get("paths"))})
                if value.get("first") != config["first"] or value.get("digits") != config["digits"]:
                    differences.append({"field": "numbered_seed_metadata", "native": {k: value.get(k) for k in ("first", "digits")}})
            self.numbered_cache[key] = {"count": len(reference), "reference_paths_sha256": digest(reference),
                                         "first": reference[0], "last": reference[-1], "differences": differences}
        self.numbered.append({"test": self.current_test, "batch": key, "actual_matches_fresh": list(instance) == reference,
                              **self.numbered_cache[key]})

    def filename_hook(self, name, original):
        def observe(*args, **kwargs):
            result = original(*args, **kwargs)
            if not self.suspended:
                try:
                    seed = args[0] if args else kwargs.get("filename", kwargs.get("name"))
                    number = self.fabio.FilenameObject(filename=seed).num
                    if name == "next_filename": number += 1
                    elif name == "previous_filename": number -= 1
                    else:
                        requested = args[1] if len(args) > 1 else kwargs.get("num", kwargs.get("frame"))
                        if requested is not None: number = requested
                    self.filename_calls.append({"test": self.current_test, "api": name, "seed": str(seed),
                                                "number": number, "reference": result})
                except Exception as error:
                    self.problem("filename_observation", error)
            return result
        return observe

    def filename_results(self):
        result, cache = [], {}
        for event in self.filename_calls:
            request = {"seed": event["seed"], "numbers": [] if event["number"] is None else [event["number"]]}
            key = digest(request)
            if key not in cache:
                cache[key] = self.probe("filename", request)
            native = cache[key]
            nonnumeric = event["number"] is None
            matches = (native["status"] == "error" and "seed filename contains no series number" in native.get("stderr", "")) if nonnumeric else (native["status"] == "ok" and native["response"].get("paths") == [event["reference"]])
            result.append({**event, "native": native, "status": "qualified_nonnumeric_rejection" if nonnumeric and matches else "match" if matches else "difference",
                           "qualification": "No native numeric seed exists; reference unchanged string is not an invented series" if nonnumeric else None})
        return result

    def flush_object_reads(self):
        events = [row for row in self.object_reads if row["test"] == self.current_test]
        paths = list(dict.fromkeys(row["path"] for row in events))
        indices = {path: index for index, path in enumerate(paths)}
        requests = []
        for event in events:
            index = indices[event["path"]]
            requests.append({"requested": index, "status": "ok", "api": event["api"],
                             "frame": {**event["frame"], "index": index, "local_frame": 0, "path": event["path"]}})
        self.object_batches.append(self.audit(paths, requests, "FilenameObject actual opens; no object/parser shim"))

    @contextlib.contextmanager
    def installed(self, object_test_class):
        cls = self.module.FileSeries
        init, get, frames, close = cls.__init__, cls._get_frame, cls.frames, cls.close
        numbered = self.module.numbered_file_series
        numbered_init = numbered.__init__
        object_teardown = object_test_class.tearDown

        def on_init(instance, *args, **kwargs):
            init(instance, *args, **kwargs)
            if not self.suspended:
                self.states[id(instance)] = {"instance": instance, "test": self.current_test, "events": [], "closed": False, "yields": 0}

        def on_get(instance, index):
            if self.suspended:
                return get(instance, index)
            state = self.states[id(instance)]
            try:
                frame = get(instance, index)
            except Exception as error:
                state["events"].append({"requested": index, "status": "error", "api": "get_frame", "error_type": type(error).__name__, "error": str(error)})
                raise
            try:
                state["events"].append({"requested": index, "status": "ok", "api": "get_frame", "frame": frame_record(frame)})
            except Exception as error:
                self.problem("frame_capture", error)
            return frame

        def on_frames(instance):
            for frame in frames(instance):
                if not self.suspended:
                    try:
                        state = self.states[id(instance)]
                        state["events"].append({"requested": frame.index, "status": "ok", "api": "frames_yield", "frame": frame_record(frame)})
                        state["yields"] += 1
                    except Exception as error:
                        self.problem("yield_capture", error)
                yield frame

        def on_close(instance):
            if not self.suspended and id(instance) in self.states:
                try:
                    self.finish_series(instance)
                except Exception as error:
                    self.problem("series_close_audit", error)
            return close(instance)

        def on_numbered(instance, *args, **kwargs):
            numbered_init(instance, *args, **kwargs)
            if not self.suspended:
                try: self.record_numbered(instance, args, kwargs, numbered_init)
                except Exception as error: self.problem("numbered_manifest", error)

        def on_open(value, *args, **kwargs):
            image = self.original_open(value, *args, **kwargs)
            if not self.suspended and self.current_test and ".TestFilenameObjects." in self.current_test:
                try:
                    path = value.tostring() if isinstance(value, self.fabio.FilenameObject) else str(value)
                    self.object_reads.append({"test": self.current_test, "path": str(path),
                                              "api": "FilenameObject" if isinstance(value, self.fabio.FilenameObject) else "path", "frame": signature(image)})
                except Exception as error:
                    self.problem("filename_object_open_capture", error)
            return image

        def on_teardown(instance):
            try: self.flush_object_reads()
            except Exception as error: self.problem("filename_object_audit", error)
            return object_teardown(instance)

        with contextlib.ExitStack() as stack:
            for owner, name, value in [(cls, "__init__", on_init), (cls, "_get_frame", on_get),
                                       (cls, "frames", on_frames), (cls, "close", on_close),
                                       (numbered, "__init__", on_numbered), (self.fabio, "open", on_open),
                                       (object_test_class, "tearDown", on_teardown)]:
                stack.enter_context(mock.patch.object(owner, name, value))
            for name in ("next_filename", "previous_filename", "jump_filename", "construct_filename"):
                stack.enter_context(mock.patch.object(self.fabio, name, self.filename_hook(name, getattr(self.fabio, name))))
            yield self


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=pathlib.Path, required=True)
    parser.add_argument("--fabio-source", type=pathlib.Path, required=True)
    parser.add_argument("--report", type=pathlib.Path, required=True)
    parser.add_argument("--timeout", type=float, default=120)
    args = parser.parse_args()
    import fabio
    commit = subprocess.check_output(["git", "-C", str(args.fabio_source), "rev-parse", "HEAD"], text=True).strip()
    if commit != PIN or fabio.version != "2026.6.0":
        parser.error("requires pinned FabIO2026.6.0 source and runtime")
    modules = [importlib.import_module("fabio.test." + name) for name in TESTS]
    runtime = [fabio] + [importlib.import_module("fabio." + name) for name in ("file_series", "fabioimage", "fabioutils", "edfimage", "openimage", "test.utilstest")]
    imports = []
    for module in [*modules, *runtime]:
        actual = pathlib.Path(inspect.getfile(module))
        relative = pathlib.Path("fabio/__init__.py") if module is fabio else pathlib.Path(*module.__name__.split(".")).with_suffix(".py")
        pinned = args.fabio_source / "src" / relative
        imports.append({"module": module.__name__, "actual": fingerprint(actual), "pinned": fingerprint(pinned),
                        "matches_pinned_source": actual.read_bytes() == pinned.read_bytes()})
    tracked = [pathlib.Path(item[which]["path"]) for item in imports for which in ("actual", "pinned")]
    tracked += [pathlib.Path(__file__), ROOT / "scripts/test_series_reference.py", ROOT / "scripts/metadata_parity.py"]
    tracked += [*sorted((ROOT / "src").rglob("*.rs")), ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "examples/api_probe.rs"]
    before = {str(path): fingerprint(path) for path in tracked}
    probe_before = fingerprint(args.probe)
    observer = Observer(Probe(args.probe, args.timeout))
    outcomes = []

    class Result(unittest.TextTestResult):
        def startTest(self, test):
            observer.current_test = test.id()
            outcomes.append({"test": test.id(), "status": "started"})
            super().startTest(test)
        def addSuccess(self, test):
            outcomes[-1]["status"] = "passed"
            super().addSuccess(test)
        def addFailure(self, test, error):
            outcomes[-1].update(status="failed", error=self._exc_info_to_string(error, test))
            super().addFailure(test, error)
        def addError(self, test, error):
            outcomes[-1].update(status="error", error=self._exc_info_to_string(error, test))
            super().addError(test, error)
        def addSkip(self, test, reason):
            outcomes[-1].update(status="skipped", reason=reason)
            super().addSkip(test, reason)

    suite = unittest.TestSuite(module.suite() for module in modules)
    with observer.installed(modules[2].TestFilenameObjects):
        result = unittest.TextTestRunner(stream=sys.stderr, verbosity=1, resultclass=Result).run(suite)
    filenames = observer.filename_results()
    unclosed = [{"test": state["test"], "scope": "unclosed_reference_series"} for state in observer.states.values() if not state["closed"]]
    records = observer.series + observer.object_batches
    observed = [action["reference"] for row in observer.series for action in row["actions"]
                if action["origin"] == "observed_reference_call"]
    summary = {"reference_tests": result.testsRun, "reference_failures": len(result.failures), "reference_errors": len(result.errors),
               "reference_skips": len(result.skipped), "series_instances": len(observer.series),
               "observed_series_requests": sum(row["observed_requests"] for row in observer.series),
               "observed_iterator_yields": sum(row["iterator_yields"] for row in observer.series),
               "observed_series_successes": sum(row["status"] == "ok" for row in observed),
               "observed_series_errors": sum(row["status"] == "error" for row in observed),
               "object_open_reads": len(observer.object_reads), "filename_helper_calls": len(filenames),
               "numbered_constructors": len(observer.numbered), "unique_numbered_batches": len(observer.numbered_cache),
               "numbered_paths_compared": sum(row["count"] for row in observer.numbered_cache.values()),
               "native_cursor_actions": sum(len(row["actions"]) for row in records)}
    report = {"schema": 1, "contract": __doc__, "reference_commit": PIN, "environment": {"python": sys.version, "fabio": fabio.version, "numpy": np.__version__},
              "reference_imports": imports, "tests": outcomes, "summary": summary, "series": observer.series,
              "filename_objects": {"scope": "parser/object identity remains reference-only; actual opens compared via explicit EDF paths", "parsing_cases": len(modules[2].CASES), "read_batches": observer.object_batches},
              "legacy_list_controls": {"scope": "arbitrary string-list object controls are reference-only; numbered manifest paths compared in full"},
              "numbered": observer.numbered, "filename_helpers": filenames, "observer_errors": observer.errors + unclosed,
              "coverage_differences": coverage_differences(summary),
              "sources": before, "probe": probe_before,
              "sources_unchanged": all(fingerprint(path) == value for path, value in before.items()),
              "probe_unchanged": fingerprint(args.probe) == probe_before}
    report["all_qualified"] = (result.wasSuccessful() and all(row["matches_pinned_source"] for row in imports)
                               and report["sources_unchanged"] and report["probe_unchanged"] and not report["observer_errors"]
                               and not report["coverage_differences"]
                               and all(row.get("status") == "match" for row in records)
                               and all(row["status"] != "difference" for row in filenames)
                               and all(row["actual_matches_fresh"] and not row["differences"] for row in observer.numbered)
                               and bool(records) and bool(filenames) and bool(observer.numbered))
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    print(json.dumps({"qualified": report["all_qualified"], **summary, "report": str(args.report)}))
    return 0 if report["all_qualified"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
