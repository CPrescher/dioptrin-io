"""Run the pinned FabIO codec tests unchanged, compare actual reads to Rust.

Test/reference Python only. Large upstream files remain in --data outside Git.
The default is ALL codec test modules, including unimplemented readers. A
filtered run is explicitly recorded and cannot establish full-suite parity.
Reference unittest results and Rust count/dtype/shape comparisons are separate.
Header/stream/discovery/API gaps remain explicit; this is not a Python API shim.
"""
import argparse
import collections
import functools
import hashlib
import importlib
import importlib.metadata
import inspect
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

PIN = "9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece"
VERSION = "2026.6.0"
ROOT = pathlib.Path(__file__).resolve().parents[1]
ADAPTERS = {
    "numpyimage": "npy", "dtrekimage": "dtrek", "fit2dmaskimage": "mask",
    "HiPiCimage": "hipic", "pnmimage": "pnm", "edfimage": "edf",
    "cbfimage": "cbf", "tifimage": "tiff", "binaryimage": "raw",
    "fit2dspreadsheetimage": "spreadsheet",
}


def sha_file(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def flatten(suite):
    for item in suite:
        if isinstance(item, unittest.TestSuite):
            yield from flatten(item)
        else:
            yield item


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fabio-source", type=pathlib.Path, required=True)
    parser.add_argument("--data", type=pathlib.Path, required=True)
    parser.add_argument("--report", type=pathlib.Path, required=True)
    parser.add_argument("--probe", type=pathlib.Path, default=ROOT / "target/debug/examples/parity_probe")
    parser.add_argument("--modules", nargs="*", help="test module stems; default all 35 codec modules")
    parser.add_argument("--download-base", default="https://www.silx.org/pub/fabio/testimages")
    parser.add_argument("--probe-timeout", type=float, default=120)
    parser.add_argument("--reference-filters", choices=["bundled", "default"], default="bundled",
                        help="register the installed test-only hdf5plugin codecs explicitly (avoid host plugin ABI conflicts)")
    args = parser.parse_args()
    commit = subprocess.check_output(["git", "-C", str(args.fabio_source), "rev-parse", "HEAD"], text=True).strip()
    if commit != PIN:
        parser.error(f"FabIO checkout must be {PIN}, found {commit}")
    # Cargo checks can rebuild examples while this long-running suite executes.
    # Measure a private immutable executable copy, never a changing build path.
    probe_directory = tempfile.TemporaryDirectory(prefix="dioptrin-fabio-probe-")
    probe_path = pathlib.Path(probe_directory.name) / ("probe.exe" if os.name == "nt" else "probe")
    shutil.copy2(args.probe, probe_path)
    args.data.mkdir(parents=True, exist_ok=True)
    # Set before importing the reference test resources.
    os.environ["FABIO_DATA"] = str(args.data.resolve())
    import fabio
    import numpy as np
    import fabio.test.codecs
    from fabio.test.utilstest import UtilsTest
    if args.reference_filters == "bundled":
        import hdf5plugin
        hdf5plugin.register(force=True)
    if fabio.version != VERSION:
        parser.error(f"reference must be FabIO {VERSION}, found {fabio.version}")
    UtilsTest.resources.url_base = args.download_base
    UtilsTest.resources.timeout = 20
    # Execute the exact pinned upstream test .py files, using installed reference
    # native extensions. Module case is taken from source, not a guessed filename.
    test_dir = args.fabio_source / "src/fabio/test/codecs"
    fabio.test.codecs.__path__.insert(0, str(test_dir.resolve()))
    paths = sorted(test_dir.glob("test_*.py"))
    if args.modules:
        requested = set(args.modules)
        paths = [p for p in paths if p.stem in requested]
        if {p.stem for p in paths} != requested:
            parser.error("unknown codec test module")
    ledger = json.loads((ROOT / "docs/fabio-2026.6.0-inventory.json").read_text())
    report = {"schema": 1, "reference": ledger["reference"],
              "selection": {"all_codec_modules": not bool(args.modules), "modules": []},
              "contract": "read-count/dtype/shape comparison at successful upstream .read calls; not Python API or full header parity",
              "gaps": ["semantic headers not completely compared", "native content autodetection not exercised", "file-like streams materialized by test harness", "read paths bypassing .read not intercepted", "reference rejection cases recorded but not yet compared to native rejection"],
              "out_of_scope": ["Python mutation/writing APIs and numerical statistics"],
              "reads": [], "reference_rejections": [], "source_files": {}, "reference_import_errors": [], "reference_tests": {}}
    report["environment"] = {"python": sys.version, "packages": {}}
    for name in ["fabio", "numpy", "h5py", "hdf5plugin", "Pillow", "glymur"]:
        try:
            report["environment"]["packages"][name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            report["environment"]["packages"][name] = None
    report["probe_sha256"] = sha_file(probe_path)
    report["reference_filters"] = args.reference_filters
    report["rust_sources_sha256"] = {str(p.relative_to(ROOT)): sha_file(p) for p in sorted((ROOT/"src").rglob("*.rs"))}
    active = False
    current_test = None

    def compare(codec, image, source, positional, keywords, captured):
        nonlocal current_test
        record = {"test": current_test, "codec": codec, "source": str(source) if isinstance(source, (str, os.PathLike)) else type(source).__name__}
        report["reads"].append(record)
        if getattr(image, "data", None) is None:
            record.update(status="reference_no_pixels")
            return
        record["reference"] = {"dtype": image.data.dtype.name, "shape": [int(n) for n in image.data.shape], "frames": int(image.nframes)}
        # Keep file identity/provenance visible even before a codec has a native
        # adapter. A file::dataset selector still identifies one physical file.
        physical = pathlib.Path(str(source).split("::", 1)[0]) if isinstance(source, (str, os.PathLike)) else None
        if physical is not None and physical.is_file():
            file_hash = sha_file(physical)
            record["file_sha256"] = file_hash
            report["source_files"].setdefault(file_hash, {"name": physical.name, "bytes": physical.stat().st_size})
        elif captured is not None:
            file_hash = hashlib.sha256(captured).hexdigest()
            record["file_sha256"] = file_hash
            report["source_files"].setdefault(file_hash, {"name": "file-like stream", "bytes": len(captured)})
        adapter = ADAPTERS.get(codec)
        if adapter is None:
            record.update(status="unsupported_reader")
            return
        record["adapter"] = adapter
        extra = []
        if codec == "cbfimage" and image.data.dtype != np.dtype("int32"):
            # Explicit source-type policy, never a retry after decoding failure.
            adapter = "cbf64"
            record["adapter"] = adapter
        if codec == "binaryimage":
            params = ["dim1", "dim2", "offset", "bytecode", "endian"]
            config = dict(zip(params, positional))
            config.update(keywords)
            extra = [str(config["dim1"]), str(config["dim2"]), str(config.get("offset", 0)),
                     np.dtype(config.get("bytecode", "int32")).name, config.get("endian", "<")]
            record["raw_configuration"] = extra
        with tempfile.TemporaryDirectory(prefix="dioptrin-fabio-parity-") as temporary:
            if captured is not None:
                path = pathlib.Path(temporary) / "stream.bin"
                path.write_bytes(captured)
                record["stream_materialized"] = True
            elif isinstance(source, (str, os.PathLike)) and pathlib.Path(source).is_file():
                path = pathlib.Path(source)
            else:
                record.update(status="unsupported_source")
                return
            file_hash = sha_file(path)
            record["file_sha256"] = file_hash
            report["source_files"].setdefault(file_hash, {"name": path.name, "bytes": path.stat().st_size})
            try:
                native = subprocess.run([str(probe_path), adapter, str(path), *extra], capture_output=True, text=True, timeout=args.probe_timeout)
            except subprocess.TimeoutExpired:
                record.update(status="native_timeout")
                return
            if native.returncode:
                record.update(status="native_rejected", error=native.stderr.strip()[:2000])
                return
            try:
                frames = [json.loads(line) for line in native.stdout.splitlines()]
                expected_frames = image.nframes
                if len(frames) != expected_frames:
                    record.update(status="frame_count_mismatch", native=len(frames), reference=expected_frames)
                    return
                failures = []
                for index, frame in enumerate(frames):
                    ref = image.data if expected_frames == 1 else image.getframe(index).data
                    canonical = np.ascontiguousarray(ref.astype(ref.dtype.newbyteorder("<"), copy=False))
                    expected = {"shape": list(ref.shape), "dtype": ref.dtype.name,
                                "sample_sha256": hashlib.sha256(canonical.tobytes()).hexdigest()}
                    for key, value in expected.items():
                        if frame[key] != value:
                            failures.append({"frame": index, "field": key, "native": frame[key], "reference": value})
                record.update(status="mismatch" if failures else "counts_match", frames=len(frames), mismatches=failures)
                if codec in {"dtrekimage", "edfimage"}:
                    # These two maps expose directly comparable text fields.
                    # Preserve full header coverage as a separate gate: CBF CIF,
                    # TIFF private detector fields etc. still need schemas.
                    header_differences = []
                    for index, frame in enumerate(frames):
                        ref_header = image.header if len(frames) == 1 else image.getframe(index).header
                        for key, value in ref_header.items():
                            key = str(key).upper() if codec == "edfimage" else str(key)
                            if frame["fields"].get(key) != str(value):
                                header_differences.append({"frame": index, "key": key, "reference": str(value), "native": frame["fields"].get(key)})
                    record["header_field_differences"] = header_differences
            except Exception as error:
                record.update(status="comparison_error", error=f"{type(error).__name__}: {error}")

    # Wrap each defining reader exactly once. Nested implementation reads are
    # suppressed; frame reads performed by the comparisons are suppressed too.
    seen = set()
    canonical_modules = {}
    for row in ledger["formats"]:
        module = importlib.import_module("fabio." + row["module"])
        actual_path = pathlib.Path(inspect.getfile(module))
        report.setdefault("reference_codec_sources", {})[row["module"]] = {
            "sha256": sha_file(actual_path), "matches_pinned_source": sha_file(actual_path) == row["source_sha256"]}
        cls = getattr(module, row["class"])
        if cls in seen:
            report.setdefault("aliases", {})[row["module"]] = canonical_modules[cls]
            continue
        seen.add(cls)
        canonical_modules[cls] = row["module"]
        original = cls.read
        codec = row["module"]

        def make_wrapper(original, codec):
            @functools.wraps(original)
            def read(image, source, *positional, **keywords):
                nonlocal active
                if active:
                    return original(image, source, *positional, **keywords)
                captured = None
                if hasattr(source, "read") and hasattr(source, "seek"):
                    try:
                        position = source.tell()
                        source.seek(0)
                        captured = source.read()
                        source.seek(position)
                    except (OSError, ValueError):
                        pass
                active = True
                try:
                    try:
                        result = original(image, source, *positional, **keywords)
                    except Exception as error:
                        report["reference_rejections"].append({"test": current_test, "codec": codec,
                            "source": str(source) if isinstance(source, (str, os.PathLike)) else type(source).__name__,
                            "error": f"{type(error).__name__}: {error}"})
                        raise
                    compare(codec, image, source, positional, keywords, captured)
                    return result
                finally:
                    active = False
            return read
        cls.read = make_wrapper(original, codec)

    class Result(unittest.TextTestResult):
        def startTest(self, test):
            nonlocal current_test
            current_test = test.id()
            super().startTest(test)

    suite = unittest.TestSuite()
    for path in paths:
        entry = {"name": path.stem, "source_sha256": sha_file(path)}
        report["selection"]["modules"].append(entry)
        try:
            module = importlib.import_module("fabio.test.codecs." + path.stem)
            tests = module.suite() if hasattr(module, "suite") else unittest.defaultTestLoader.loadTestsFromModule(module)
            entry["tests"] = [test.id() for test in flatten(tests)]
            suite.addTests(tests)
        except Exception as error:
            report["reference_import_errors"].append({"module": path.stem, "error": f"{type(error).__name__}: {error}"})
    np.random.seed(20261105)
    result = unittest.TextTestRunner(verbosity=1, resultclass=Result).run(suite)
    report["reference_tests"] = {"run": result.testsRun, "failures": [(t.id(),e) for t,e in result.failures],
                                 "errors": [(t.id(),e) for t,e in result.errors],
                                 "skipped": [(t.id(),why) for t,why in result.skipped]}
    report["summary"] = dict(collections.Counter(r["status"] for r in report["reads"]))
    # A green reference suite alone says nothing about Rust parity. Every gap,
    # absent codec, skipped reference test or unmatched read keeps this false.
    compared_codecs = {r["codec"] for r in report["reads"] if r["status"] == "counts_match"}
    required = set(canonical_modules.values())
    report["unexercised_codecs"] = sorted(required - compared_codecs)
    report["full_reading_parity"] = (result.wasSuccessful() and not result.skipped
        and not report["reference_import_errors"] and not report["unexercised_codecs"]
        and not report["gaps"] and all(r["status"] in {"counts_match", "reference_no_pixels"}
            and not r.get("header_field_differences") for r in report["reads"])
        and all(x["matches_pinned_source"] for x in report["reference_codec_sources"].values()))
    report["download_base"] = args.download_base
    report["download_hashes"] = UtilsTest.resources.all_data
    report["probe_unchanged_during_run"] = report["probe_sha256"] == sha_file(probe_path)
    report["sources_unchanged_during_run"] = all(sha_file(ROOT/p) == h for p,h in report["rust_sources_sha256"].items())
    if not report["probe_unchanged_during_run"] or not report["sources_unchanged_during_run"]:
        report["full_reading_parity"] = False
    args.report.parent.mkdir(parents=True, exist_ok=True)
    def json_scalar(value):
        # Some reference headers/frame counts are NumPy scalar integers.
        if isinstance(value, np.generic):
            return value.item()
        raise TypeError(f"unserializable report value: {type(value).__name__}")
    args.report.write_text(json.dumps(report, indent=2, default=json_scalar) + "\n")
    print(json.dumps({"reference_tests": result.testsRun, "reads": report["summary"], "report": str(args.report)}, indent=2))
    return 0 if report["full_reading_parity"] else 1


if __name__ == "__main__":
    sys.exit(main())
