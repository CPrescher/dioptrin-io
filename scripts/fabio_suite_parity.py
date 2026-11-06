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
import io
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
    "sparseimage":"sparse",
    "mar345image":"mar345",
    "esperantoimage":"esperanto", "numpyimage": "npy", "dtrekimage": "dtrek", "fit2dmaskimage": "mask",
    "HiPiCimage": "hipic", "pnmimage": "pnm", "edfimage": "edf",
    "cbfimage": "cbf", "tifimage": "tiff", "binaryimage": "raw",
    "fit2dspreadsheetimage": "spreadsheet", "fit2dimage":"fit2d", "kcdimage":"kcd", "mpaimage":"mpa",
    "hdf5image":"hdf5", "eigerimage":"eiger", "limaimage":"lima", "lambdaimage":"lambda",
    "marccdimage":"marccd", "pilatusimage":"pilatus",
    "speimage":"spe", "mrcimage":"mrc", "dm3image":"dm3",
    "brukerimage":"bruker86-view", "bruker100image":"bruker100",
    "OXDimage":"oxford", "raxisimage":"raxis",
    "jpegimage":"jpeg", "jpeg2kimage":"jpeg2000", "xsdimage":"xsd", "GEimage":"ge", "pixiimage":"pixi",
}


def sha_file(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def capture_rejected_source(path, record, directory):
    """Retain exact rejected bytes before upstream temporary-file cleanup."""
    if directory is None:
        return
    expected = record["file_sha256"]
    destination = directory / (expected + "-" + path.name)
    try:
        if not destination.exists():
            with tempfile.NamedTemporaryFile(dir=directory, delete=False) as temporary:
                staging = pathlib.Path(temporary.name)
            try:
                shutil.copyfile(path, staging)
                if sha_file(staging) != expected:
                    raise ValueError("source changed before rejected-input capture")
                staging.replace(destination)
            finally:
                staging.unlink(missing_ok=True)
        if sha_file(destination) != expected:
            raise ValueError("existing rejected-input capture has a different hash")
        record["rejected_source_copy"] = {"path": str(destination),
                                           "sha256": expected,
                                           "bytes": destination.stat().st_size}
    except (OSError, ValueError) as error:
        record["source_capture_error"] = str(error)


def flatten(suite):
    for item in suite:
        if isinstance(item, unittest.TestSuite):
            yield from flatten(item)
        else:
            yield item


def tiff_component_differences(native, reference):
    differences = []
    def compare(actual, expected, location):
        for key, value in expected.items():
            if actual.get(key) != value:
                differences.append({"field": location + key, "native": actual.get(key), "reference": value})
    compare(native, {key: reference[key] for key in ["shape", "subsampling", "precision", "storage"]}, "")
    if len(native["chunks"]) != len(reference["chunks"]):
        return differences + [{"field": "chunks", "native": len(native["chunks"]), "reference": len(reference["chunks"])}]
    for index, (chunk, expected) in enumerate(zip(native["chunks"], reference["chunks"])):
        prefix = f"chunks[{index}]."
        compare(chunk, {key: expected[key] for key in ["origin", "image_shape"]}, prefix)
        if reference["storage"] == "TiffSamplingUnits":
            if chunk["sampling_units"] is None:
                differences.append({"field": prefix + "sampling_units", "native": None, "reference": expected["sampling_units"]})
            else:
                compare(chunk["sampling_units"], expected["sampling_units"], prefix + "sampling_units.")
        elif chunk["sampling_units"] is not None:
            differences.append({"field": prefix + "sampling_units", "native": "present", "reference": "JPEG has decoded component grids, no stored packed sampling units"})
        if len(chunk["planes"]) != 3:
            differences.append({"field": prefix + "planes", "native": len(chunk["planes"]), "reference": 3})
            continue
        for plane, expected_plane in zip(chunk["planes"], expected["planes"]):
            compare(plane, expected_plane, prefix + expected_plane["component"] + ".")
    return differences


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fabio-source", type=pathlib.Path, required=True)
    parser.add_argument("--data", type=pathlib.Path, required=True)
    parser.add_argument("--report", type=pathlib.Path, required=True)
    parser.add_argument("--probe", type=pathlib.Path, default=ROOT / "target/debug/examples/parity_probe")
    parser.add_argument("--modules", nargs="*", help="test module stems; default all 35 codec modules")
    parser.add_argument("--download-base", default="https://www.silx.org/pub/fabio/testimages")
    parser.add_argument("--probe-timeout", type=float, default=120)
    parser.add_argument("--rejected-source-dir", type=pathlib.Path,
                        help="optional external directory retaining exact rejected inputs before upstream cleanup")
    parser.add_argument("--tiff-reference-library", type=pathlib.Path,
                        help="explicit test-only libtiff library for independent native component comparisons")
    parser.add_argument("--read-policies", type=pathlib.Path,
                        help="explicit source-hash or pinned-test recovery recipes; never an automatic decode retry")
    parser.add_argument("--vds-map", type=pathlib.Path,
                        help="explicit original-to-actual HDF5 VDS filename bindings")
    parser.add_argument("--metadata", action="store_true", help="compare every reference header field separately from pixels")
    parser.add_argument("--reference-filters", choices=["bundled", "default"], default="bundled",
                        help="register the installed test-only hdf5plugin codecs explicitly (avoid host plugin ABI conflicts)")
    args = parser.parse_args()
    policy_files = {name: {"path": str(path.resolve()), "sha256": sha_file(path)}
                    for name, path in [("read_policies", args.read_policies), ("vds_map", args.vds_map)] if path}
    policies = json.loads(args.read_policies.read_text())["policies"] if args.read_policies else []
    for policy in policies:
        if not policy.get("codec") or bool(policy.get("source_sha256")) == bool(policy.get("test_prefix")):
            parser.error("each read policy requires a codec and exactly one source hash or pinned test prefix")
        if not policy.get("reason") or not isinstance(policy.get("arguments"), list) or any(
                not isinstance(value, str) for value in policy["arguments"]):
            parser.error("each read policy requires a reason and an argument list")

    def source_policy(record):
        selected = [policy for policy in policies if policy["codec"] == record["codec"] and (
            policy.get("source_sha256") == record.get("file_sha256") if "source_sha256" in policy else
            (record.get("test") or "").startswith(policy["test_prefix"]))]
        if len(selected) > 1:
            raise ValueError("ambiguous explicit source read policies")
        return selected[0] if selected else None
    if args.rejected_source_dir is not None:
        args.rejected_source_dir = args.rejected_source_dir.resolve()
        if args.rejected_source_dir.is_relative_to(ROOT):
            parser.error("rejected detector inputs must stay outside the Git repository")
        args.rejected_source_dir.mkdir(parents=True, exist_ok=True)
    commit = subprocess.check_output(["git", "-C", str(args.fabio_source), "rev-parse", "HEAD"], text=True).strip()
    if commit != PIN:
        parser.error(f"FabIO checkout must be {PIN}, found {commit}")
    # Cargo checks can rebuild examples while this long-running suite executes.
    # Measure a private immutable executable copy, never a changing build path.
    probe_directory = tempfile.TemporaryDirectory(prefix="dioptrin-fabio-probe-")
    probe_path = pathlib.Path(probe_directory.name) / ("probe.exe" if os.name == "nt" else "probe")
    shutil.copy2(args.probe, probe_path)
    capabilities=json.loads(subprocess.check_output([str(probe_path),"--capabilities"],text=True))
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
    report = {"schema": 8, "reference": ledger["reference"],
              "selection": {"all_codec_modules": not bool(args.modules), "modules": []},
              "contract": "uniform count/dtype/shape and independent native component/unit comparisons, with separate explicit display views, at successful upstream .read calls; not Python API or full header parity",
              "recovery_contract": "explicit source recipes selected before decoding; independently verified partial spans/header-only access are separate from complete count parity and expected input errors",
              "gaps": ["semantic headers not completely compared", "native content autodetection not exercised", "file-like streams materialized by test harness", "read paths bypassing .read not intercepted"],
              "out_of_scope": ["Python mutation/writing APIs and numerical statistics"],
              "reads": [], "reference_rejections": [], "source_files": {}, "reference_import_errors": [], "reference_tests": {}}
    report["environment"] = {"python": sys.version, "packages": {}}
    for name in ["fabio", "numpy", "h5py", "hdf5plugin", "Pillow", "glymur", "imagecodecs", "tifffile"]:
        try:
            report["environment"]["packages"][name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            report["environment"]["packages"][name] = None
    try:
        import glymur
        report["reference_native_libraries"] = {"glymur_openjpeg": glymur.version.openjpeg_version}
    except ImportError:
        report["reference_native_libraries"] = {"glymur_openjpeg": None}
    component_reference = None
    pillow_reference = None
    if args.tiff_reference_library is not None:
        from tiff_component_reference import Reference
        component_reference = Reference(args.tiff_reference_library)
        report["reference_native_libraries"]["tiff_components"] = component_reference.metadata
        from pillow_planar16_reference import Reference as PillowReference
        pillow_reference = PillowReference(args.tiff_reference_library)
        report["reference_native_libraries"]["pillow_planar16"] = pillow_reference.metadata
    report["probe_sha256"] = sha_file(probe_path)
    report["rejected_source_capture"] = str(args.rejected_source_dir) if args.rejected_source_dir else None
    report["runtime_capabilities"] = capabilities
    report["explicit_policy_files"] = policy_files
    report["reference_filters"] = args.reference_filters
    report["metadata_enabled"] = args.metadata
    report["rust_sources_sha256"] = {str(p.relative_to(ROOT)): sha_file(p) for p in sorted((ROOT/"src").rglob("*.rs"))}
    report["vendored_sources_sha256"] = {str(p.relative_to(ROOT)): sha_file(p)
        for p in sorted((ROOT/"vendor").rglob("*")) if p.is_file() and not {"target", ".git"}.intersection(p.relative_to(ROOT/"vendor").parts)}
    report["native_sources_sha256"] = {str(p.relative_to(ROOT)): sha_file(p)
        for p in sorted((ROOT/"native").rglob("*")) if p.is_file()}
    report["build_files_sha256"] = {name: sha_file(ROOT/name) for name in ["Cargo.toml", "Cargo.lock", "build.rs"] if (ROOT/name).is_file()}
    report["bridge_sources_sha256"] = {name: sha_file(ROOT/name) for name in [
        "scripts/fabio_suite_parity.py", "scripts/tiff_component_reference.py", "scripts/edf_recovery_reference.py",
        "scripts/thunderscan_reference.py", "scripts/pillow_planar16_reference.py", "scripts/metadata_parity.py", "scripts/test_metadata_parity.py", "scripts/metadata_attributes.py", "scripts/test_metadata_attributes.py", "scripts/header_reference.py", "scripts/inventory_fabio.py", "examples/parity_probe.rs", "docs/fabio-2026.6.0-inventory.json"]}
    active = False
    current_test = None

    def compare_rejection(codec, source, positional, keywords, captured, error):
        record = {"test": current_test, "codec": codec,
                  "source": str(source) if isinstance(source, (str, os.PathLike)) else type(source).__name__,
                  "error": f"{type(error).__name__}: {error}"}
        report["reference_rejections"].append(record)
        feature = capabilities["known_readers"].get(codec)
        adapter = ADAPTERS.get(codec)
        if feature is None:
            record.update(status="decoder_missing")
            return
        if feature not in capabilities["features"]:
            record.update(status="feature_disabled")
            return
        if adapter not in capabilities["adapters"]:
            record.update(status="test_adapter_missing")
            return
        # A failed selected-frame/configuration call is not equivalent to
        # asking the native probe to decode its default complete scan.
        if (positional and positional != (None,)) or any(key != "frame" or value is not None for key, value in keywords.items()):
            record.update(status="configuration_not_compared", configuration={"positional":list(map(repr,positional)),"keywords":{key:repr(value) for key,value in keywords.items()}})
            return
        environment_failure = codec == "jpegimage" and sys.modules["fabio.jpegimage"].Image is None
        with tempfile.TemporaryDirectory(prefix="dioptrin-fabio-reject-") as temporary:
            if captured is not None:
                path = pathlib.Path(temporary) / "stream.bin"
                path.write_bytes(captured)
                record["stream_materialized"] = True
            elif isinstance(source, (str, os.PathLike)):
                path = pathlib.Path(str(source).split("::", 1)[0])
            else:
                record.update(status="unsupported_source")
                return
            if path.is_file():
                file_hash = sha_file(path)
                record["file_sha256"] = file_hash
                report["source_files"].setdefault(file_hash, {"name": path.name, "bytes": path.stat().st_size})
            else:
                record["source_missing"] = not path.exists()
            argument = str(source) if codec == "hdf5image" and captured is None else str(path)
            try:
                native = subprocess.run([str(probe_path), adapter, argument], capture_output=True, text=True, timeout=args.probe_timeout)
            except subprocess.TimeoutExpired:
                record.update(status="native_timeout")
                return
            record["native_exit_code"] = native.returncode
            record["native_error"] = native.stderr.strip()[:2000]
            if environment_failure and native.returncode == 0:
                record.update(status="reference_environment_difference",
                              reason="upstream test deliberately disables Pillow; native decoding is independent of Python backend globals")
            elif native.returncode == 2:
                record.update(status="rejection_match")
            elif native.returncode == 0:
                record.update(status="native_accepted_reference_rejection")
            else:
                record.update(status="native_process_failure")

    def compare(codec, image, source, positional, keywords, captured):
        nonlocal current_test
        record = {"test": current_test, "codec": codec, "source": str(source) if isinstance(source, (str, os.PathLike)) else type(source).__name__}
        report["reads"].append(record)
        if args.metadata:
            from metadata_parity import encode
            record["reference_header"] = encode(dict(image.header))
            from metadata_attributes import inventory
            record["reference_attributes"] = inventory(codec,image)
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
        policy = source_policy(record)
        if getattr(image, "data", None) is None:
            record["reference"] = None
            if policy is None:
                record.update(status="reference_no_pixels")
                if args.metadata:
                    with tempfile.TemporaryDirectory(prefix="dioptrin-header-only-") as directory:
                        header_path = physical
                        if captured is not None:
                            header_path = pathlib.Path(directory)/"header.bin"
                            header_path.write_bytes(captured)
                        command = {"dtrekimage":"dtrek-header","tifimage":"tiff-header"}.get(codec)
                        if command is None or header_path is None:
                            record["header_only"] = {"status":"unsupported_header_adapter"}
                        else:
                            result = subprocess.run([str(probe_path),command,str(header_path)],capture_output=True,text=True,timeout=args.probe_timeout)
                            if result.returncode:
                                record["header_only"] = {"status":"native_rejected","error":result.stderr[:2000]}
                            else:
                                header_frames = [json.loads(line) for line in result.stdout.splitlines()]
                                from metadata_parity import compare as compare_metadata
                                if codec=="dtrekimage":
                                    record["metadata"] = [{"frame":0,**compare_metadata(encode(dict(image.header)),header_frames[0],codec)}]
                                    record["header_only"] = {"status":record["metadata"][0]["status"],"declared_shape":header_frames[0]["declared_shape"],"pixels_available":False}
                                else:
                                    from header_reference import compare as compare_headers
                                    record["header_only"] = {**compare_headers(header_path,header_frames),"pixels_available":False}
                                    record["metadata"] = [{"frame":0,**compare_metadata(encode(dict(image.header)),header_frames[0],codec,record.get("reference_backend"))}]
                                record["header_only"]["native_headers"] = header_frames
                                record["_attribute_frames"] = header_frames
                                record["_source_bits"] = record["header_only"].get("first_bits")
                if args.metadata and "metadata" not in record:
                    record["metadata"] = [{"status":"not_compared","reference_fields":len(image.header)}]
                return
        else:
            record["reference"] = {"dtype": image.data.dtype.name, "shape": [int(n) for n in image.data.shape], "frames": int(image.nframes)}
        adapter = ADAPTERS.get(codec)
        feature=capabilities["known_readers"].get(codec)
        if feature is None:
            record.update(status="decoder_missing",reason="No format-specific Rust reader is implemented")
            return
        if feature not in capabilities["features"]:
            record.update(status="feature_disabled",feature=feature,reason="Rust reader exists but this test binary omits its optional feature")
            return
        if adapter is None or adapter not in capabilities["adapters"]:
            record.update(status="test_adapter_missing",reason="Rust reader exists; the comparison harness has no adapter for it")
            return
        if codec == "tifimage":
            record["reference_backend"] = getattr(image, "lib", None)
            selected = {"PIL":"tiff-pillow13", "TiffIO":"tiff-tiffio"}.get(record["reference_backend"])
            compression = image.header.get("compression")
            if "jpeg" not in capabilities["features"] and (compression == 6 or compression == 7 and selected == "tiff-pillow13"):
                record.update(status="feature_disabled", required_features=["tiff", "jpeg"],
                              reason="selected TIFF legacy/native-component or exact JPEG display path requires optional jpeg feature; this is not a missing decoder")
                return
            if selected is not None:
                if selected not in capabilities["adapters"]:
                    record.update(status="test_adapter_missing",reason="compiled TIFF reader lacks explicit reference-view adapter")
                    return
                adapter = selected
        record["adapter"] = adapter
        extra = []
        if policy is not None:
            extra.extend(policy["arguments"])
            record["explicit_read_policy"] = policy
        if args.vds_map is not None and codec in {"lambdaimage", "hdf5image", "eigerimage", "limaimage"}:
            extra.extend(["--vds-map", str(args.vds_map.resolve())])
            record["vds_map_sha256"] = policy_files["vds_map"]["sha256"]
        if codec == "sparseimage":
            record["reference_reconstruction_backend"] = "native" if sys.modules["fabio.sparseimage"].cython_densify is not None else "numpy"
            if record["reference_reconstruction_backend"] != "native":
                record.update(status="unsupported_reference_reconstruction",reason="selected native sparse model is not qualified against the distinct NumPy fallback")
                return
            if getattr(image,"noisy",0):
                record.update(status="unsupported_reference_reconstruction",reason="stochastic reference data is not represented as stored detector counts")
                return
            record["reconstruction_policy"]="explicit deterministic native FabIO NOISY=False background view; original sparse arrays compared separately"
        if codec == "dtrekimage" and image.data.ndim != 2:
            record["frame_mapping_policy"] = "stored C-order leading axes mapped to 2D planes; complete original array shape retained"
        if codec == "brukerimage":
            record["interpretation_policy"] = "stored words compared first; explicit Bruker86 LINEAR float decoding reported separately as a reference view"
        if codec == "kcdimage":
            record["interpretation_policy"] = "SumReadouts explicitly selected with checked i32 arithmetic; default native reader exposes original per-readout u16 words"
        if codec == "tifimage":
            record["interpretation_policy"] = "native stored channels and photometric samples; reference may invert WhiteIsZero or convert RGB to grayscale"
        if codec == "raxisimage":
            record["interpretation_policy"] = "PhotomultiplierExpanded explicitly selected; default native reader preserves stored uint16 words and flags"
        if codec == "OXDimage":
            record["interpretation_policy"] = "native signed32 counts; TY5 pinned float64 API representation selected explicitly and exactly, raw signature retained"
        if codec in {"speimage", "mrcimage"}:
            record["reference_frame_policy"] = "fresh .read(path, frame=i) for every declared frame; pinned generic getframe/nframes behavior is incomplete"
        if codec == "edfimage":
            record["source_policy"] = "snapshot external payloads; explicit basename relocation; unique .gz/.bz2 counterpart only if exact path is absent; explicit UpToDeclared external windows bounded to available source bytes with complete samples/codecs mandatory; default native Exact policy remains strict"
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
            elif physical is not None and physical.is_file():
                path = physical
            else:
                record.update(status="unsupported_source")
                return
            file_hash = sha_file(path)
            record["file_sha256"] = file_hash
            report["source_files"].setdefault(file_hash, {"name": path.name, "bytes": path.stat().st_size})
            try:
                argument=str(source) if codec=="hdf5image" else str(path)
                native = subprocess.run([str(probe_path), adapter, argument, *extra], capture_output=True, text=True, timeout=args.probe_timeout)
            except subprocess.TimeoutExpired:
                record.update(status="native_timeout")
                return
            if native.returncode:
                record.update(status="native_rejected" if native.returncode == 2 else "native_process_failure",
                              native_exit_code=native.returncode, error=native.stderr.strip()[:2000])
                capture_rejected_source(path, record, args.rejected_source_dir)
                return
            try:
                frames = [json.loads(line) for line in native.stdout.splitlines()]
                if args.metadata:
                    record["_attribute_frames"] = frames
                    if codec in {"tifimage","pilatusimage","marccdimage"}:
                        with image._open(str(path),"rb") as prefix_source:
                            record["_source_prefix_hex"] = prefix_source.read(64).hex()
                            prefix_source.seek(0)
                            import tifffile
                            with tifffile.TiffFile(prefix_source) as tiff_source:
                                from header_reference import tag_value
                                record["_source_bits"] = tag_value(tiff_source.pages[0].tags[258],prefix_source,tiff_source.byteorder)
                if args.metadata and codec=="edfimage" and frames and "payload_window" in frames[-1] and frames[-1]["payload_window"]["source"]==0:
                    window=frames[-1]["payload_window"]
                    with image._open(str(path),"rb") as padding_source:
                        padding_source.seek(window["offset"]+window["effective_bytes"])
                        tail=padding_source.read(512)
                    record["metadata_integrity_evidence"]={"decoded_tail_offset":window["offset"]+window["effective_bytes"],"tail_hex":tail.hex(),"read_limit":512}
                manifest = {}
                source_paths = {pathlib.Path(name) for frame in frames for name in frame.get("sources", [])}
                for source_path in sorted(source_paths):
                    source_hash = sha_file(source_path)
                    manifest[source_hash] = {"name": source_path.name, "bytes": source_path.stat().st_size}
                if manifest:
                    record["source_manifest"] = manifest
                    report["source_files"].update(manifest)
                if args.metadata:
                    from metadata_parity import compare as compare_metadata, encode
                    comparisons = []
                    try:
                        metadata_frames = frames[0]["source_recovery"]["images"] if codec == "edfimage" and "source_recovery" in frames[0] else frames
                        for index, frame in enumerate(metadata_frames):
                            if codec == "tifimage" and record.get("reference_backend") == "PIL" and len(frames) > 1:
                                from PIL import Image
                                with image._open(str(path), "rb") as header_source, Image.open(header_source) as page:
                                    page.seek(index)
                                    ref_header = image._read_header_from_pil(page)
                                record["metadata_frame_policy"] = "pinned FabIO _read_header_from_pil on each Pillow page; public PIL getframe is unsupported"
                            elif codec in {"speimage", "mrcimage"}:
                                ref_image = type(image)().read(io.BytesIO(captured) if captured is not None else str(source), frame=index)
                                ref_header = ref_image.header
                            elif len(metadata_frames) == 1 or codec == "dtrekimage":
                                ref_header = image.header
                            else:
                                ref_header = image.getframe(index).header
                            if codec == "marccdimage":
                                semantic = type(image)()
                                with semantic._open(str(path), "rb") as header_source:
                                    semantic._readheader(header_source)
                                ref_header = semantic.header
                            comparisons.append({"frame":index, **compare_metadata(encode(dict(ref_header)), frame, codec, record.get("reference_backend"))})
                    except Exception as error:
                        comparisons.append({"status":"comparison_error", "error":f"{type(error).__name__}: {error}"})
                    record["metadata"] = comparisons
                if any("source_recovery" in frame for frame in frames):
                    if policy is None:
                        raise ValueError("source recovery requires a recorded explicit read policy")
                    if codec == "edfimage":
                        from edf_recovery_reference import compare as compare_recovery
                        if len(frames) != 1:
                            raise ValueError("EDF recovery must emit one source report")
                        check = compare_recovery(path, frames[0]["source_recovery"])
                    elif codec == "tifimage":
                        if args.tiff_reference_library is None:
                            record.update(status="reference_recovery_unqualified", reason="ThunderScan recovery requires explicit independent libtiff")
                            return
                        from thunderscan_reference import compare as compare_recovery
                        check = compare_recovery(path, frames, args.tiff_reference_library)
                    else:
                        raise ValueError("unqualified source recovery format")
                    record["source_recovery"] = frames
                    record["independent_recovery_check"] = check
                    record.update(status="source_recovery_mismatch" if check["differences"] else "partial_source_read",
                                  recovery_contract="only available source samples compared; absent counts are unavailable, never full count parity")
                    capture_rejected_source(path, record, args.rejected_source_dir)
                    return
                if any("virtual_dataset" in frame for frame in frames):
                    record["virtual_datasets"] = [{"frame":frame["frame"], **frame["virtual_dataset"]} for frame in frames if "virtual_dataset" in frame]
                if any("tiff_compatibility" in frame for frame in frames):
                    record["tiff_compatibility"] = [{"frame":frame["frame"], **frame["tiff_compatibility"]} for frame in frames if "tiff_compatibility" in frame]
                if codec == "edfimage":
                    record["payload_windows"]=[{"frame":frame["frame"],**frame["payload_window"]} for frame in frames]
                component_frames = [frame for frame in frames if "tiff_components" in frame]
                if component_frames:
                    if component_reference is None:
                        record.update(status="reference_components_unqualified",
                                      reason="native component grids require the explicit independent --tiff-reference-library oracle")
                        return
                    checks = []
                    for frame in component_frames:
                        try:
                            reference_components = component_reference.read(path, frame["frame"])
                        except Exception as error:
                            record.update(status="reference_component_error", error=f"{type(error).__name__}: {error}")
                            return
                        differences = tiff_component_differences(frame["tiff_components"], reference_components)
                        checks.append({"frame": frame["frame"], "differences": differences,
                                       "reference": reference_components})
                    record["source_component_checks"] = checks
                    if any(check["differences"] for check in checks):
                        record.update(status="source_component_mismatch")
                        return
                if codec == "sparseimage":
                    import h5py
                    component_checks=[]
                    entry=image.h5[image.h5.attrs["default"]]
                    nx_data=entry[entry.attrs.get("pyFAI_sparse_frames") or entry.attrs["default"]]
                    expected_components=set()
                    def collect_numeric(group,prefix,ancestors=frozenset()):
                        # visititems deduplicates hard links. Source components
                        # retain every named alias, including errors/intensity.
                        identity=(group.file.filename,h5py.h5o.get_info(group.id).addr)
                        if identity in ancestors:raise ValueError("cyclic reference sparse component group")
                        for name,obj in group.items():
                            if isinstance(obj,h5py.Dataset) and obj.dtype.kind in "iuf":
                                expected_components.add(prefix+name)
                            elif isinstance(obj,h5py.Group):
                                collect_numeric(obj,prefix+name+"/",ancestors|{identity})
                    collect_numeric(nx_data,"")
                    for name in ("sparsify","peaks"):
                        if name in entry:
                            obj=entry[name]
                            if isinstance(obj,h5py.Group):collect_numeric(obj,"entry/"+name+"/")
                            elif obj.dtype.kind in "iuf":expected_components.add("entry/"+name)
                    native_components={item["name"] for item in frames[0].get("source_components",[])}
                    record["source_component_inventory_differences"]={
                        "missing":sorted(expected_components-native_components),
                        "extra":sorted(native_components-expected_components)}
                    for descriptor in frames[0].get("source_components",[]):
                        with h5py.File(descriptor["source_file"],"r") as component_file:
                            array=component_file[descriptor["dataset"]][()]
                            canonical=np.ascontiguousarray(array.astype(array.dtype.newbyteorder("<"),copy=False))
                            expected={"dtype":array.dtype.name,"shape":list(array.shape),"sample_sha256":hashlib.sha256(canonical.tobytes()).hexdigest()}
                            differences={key:{"native":descriptor[key],"reference":value} for key,value in expected.items() if descriptor[key]!=value}
                            component_checks.append({"name":descriptor["name"],"differences":differences})
                    record["source_component_checks"]=component_checks
                    sparse_checks=[]
                    for index,frame in enumerate(frames):
                        start,stop=map(int,image.frame_ptr[index:index+2])
                        stored=frame["stored_sparse"]
                        for name,expected in [("source_range",[start,stop]),("events",stop-start)]:
                            if stored[name]!=expected:
                                sparse_checks.append({"frame":index,"field":name,"native":stored[name],"reference":expected})
                        for name,array in [("index",image.index[start:stop]),("intensity",image.intensity[start:stop])]:
                            canonical=np.ascontiguousarray(array.astype(array.dtype.newbyteorder("<"),copy=False))
                            expected={"dtype":array.dtype.name,"shape":list(array.shape),"sample_sha256":hashlib.sha256(canonical.tobytes()).hexdigest()}
                            native=frame["stored_sparse"][name]
                            differences={key:{"native":native[key],"reference":value} for key,value in expected.items() if native[key]!=value}
                            if differences:sparse_checks.append({"frame":index,"component":name,"differences":differences})
                    record["stored_sparse_differences"]=sparse_checks
                    record["stored_events"]=int(len(image.intensity))
                    if (not component_checks or any(c["differences"] for c in component_checks) or sparse_checks
                            or native_components!=expected_components):
                        record.update(status="source_component_mismatch")
                        return
                # SPE declares its stack in the header but the pinned reader
                # inherits nframes=1. MRC's inherited getframe fails on data=None.
                expected_frames = int(image.header["num_frames"]) if codec == "speimage" else image.nframes
                if codec == "dtrekimage":
                    expected_frames = int(np.prod(image.data.shape[:-2])) if image.data.ndim > 2 else 1
                record["reference"]["compared_frames"] = expected_frames
                if len(frames) != expected_frames:
                    record.update(status="frame_count_mismatch", native=len(frames), reference=expected_frames)
                    return
                failures = []
                stored_differences = []
                view_policies = []
                reconstruction = False
                for index, frame in enumerate(frames):
                    if codec == "dtrekimage":
                        ref = image.data.reshape((-1,) + image.data.shape[-2:])[index] if image.data.ndim > 2 else image.data.reshape((1,-1)) if image.data.ndim == 1 else image.data
                    elif codec in {"speimage", "mrcimage"}:
                        ref_source = io.BytesIO(captured) if captured is not None else str(source)
                        ref = type(image)().read(ref_source, frame=index).data
                    else:
                        ref = image.data if expected_frames == 1 else image.getframe(index).data
                    if ref.dtype == np.dtype("bool"):
                        # Pillow can expose true bools backed by 0xff; Rust bool
                        # and NumPy logical casts use 0x01. Compare logical values
                        # and retain the opaque reference bytes as provenance.
                        physical = np.ascontiguousarray(ref).tobytes()
                        canonical = np.ascontiguousarray(ref.astype(np.uint8))
                        record.setdefault("boolean_reference_storage", []).append({"frame":index,
                            "physical_sha256":hashlib.sha256(physical).hexdigest(),
                            "logical_byte_policy":"false=0,true=1"})
                    else:
                        canonical = np.ascontiguousarray(ref.astype(ref.dtype.newbyteorder("<"), copy=False))
                    expected = {"shape": list(ref.shape), "dtype": ref.dtype.name,
                                "sample_sha256": hashlib.sha256(canonical.tobytes()).hexdigest()}
                    if "tiff_components" in frame:
                        raw_differences = [{"frame": index, "field": "sample_layout",
                                            "native": "independently verified native component grids",
                                            "reference": "derived display raster"}]
                        if frame.get("reference_view") is None:
                            record.update(status="component_view_unavailable", reason=frame.get("view_error", "no explicit reference display view"))
                            return
                    else:
                        raw_differences = [{"frame": index, "field": key, "native": frame[key], "reference": value}
                                           for key, value in expected.items() if frame[key] != value]
                    selected = frame
                    if raw_differences and frame.get("reference_view") is not None:
                        selected = frame["reference_view"]
                        stored_differences.extend(raw_differences)
                        view_policies.append(frame["view_policy"])
                    if frame.get("reconstruction_policy") is not None:
                        reconstruction = True
                        view_policies.append(frame["reconstruction_policy"])
                    if frame.get("view_error") is not None:
                        record.setdefault("view_errors", []).append({"frame": index, "error": frame["view_error"]})
                    for key, value in expected.items():
                        if selected[key] != value:
                            failures.append({"frame": index, "field": key, "native": selected[key], "reference": value})
                status = "mismatch" if failures else "component_view_match" if component_frames else "reconstruction_match" if reconstruction else "reference_view_match" if view_policies else "counts_match"
                if failures and codec == "tifimage" and record.get("reference_backend") == "PIL" and pillow_reference is not None:
                    # Qualification requires every native source word to match
                    # independent libtiff/tifffile, an exact raw-unpacker model,
                    # the actual reference output and a correct controlled
                    # Pillow backend. Neither hashes nor error text alone can
                    # turn a mismatch into a reference defect.
                    evidence = []
                    for index in sorted({failure["frame"] for failure in failures}):
                        ref = image.data if expected_frames == 1 else image.getframe(index).data
                        try:
                            proof = pillow_reference.verify(path, index, frames[index], ref)
                        except Exception as error:
                            proof = {"qualified": False, "error": f"{type(error).__name__}: {error}"}
                        evidence.append({"frame": index, "proof": proof})
                    record["reference_defect_checks"] = evidence
                    if evidence and all(entry["proof"] is not None and entry["proof"]["qualified"] for entry in evidence):
                        status = "reference_decoder_defect"
                record.update(status=status, frames=len(frames), mismatches=failures)
                if stored_differences:
                    record["stored_representation_differences"] = stored_differences
                if view_policies:
                    record["view_policies"] = view_policies
                if codec in {"dtrekimage", "edfimage", "pilatusimage", "marccdimage"}:
                    # These two maps expose directly comparable text fields.
                    # Preserve full header coverage as a separate gate: CBF CIF,
                    # TIFF private detector fields etc. still need schemas.
                    header_differences = []
                    for index, frame in enumerate(frames):
                        ref_header = image.header if len(frames) == 1 or codec == "dtrekimage" else image.getframe(index).header
                        if codec == "marccdimage":
                            semantic = type(image)()
                            with semantic._open(str(path), "rb") as header_source:
                                semantic._readheader(header_source)
                            ref_header = semantic.header
                        for key, value in ref_header.items():
                            key = str(key).upper() if codec == "edfimage" else str(key)
                            if codec == "marccdimage":
                                if isinstance(value, bytes): value = list(value)
                                elif isinstance(value, (tuple, list)):
                                    value = list(b"".join(value)) if value and isinstance(value[0], bytes) else list(value)
                            else:
                                value = str(value)
                            if frame["fields"].get(key) != value:
                                header_differences.append({"frame": index, "key": key, "reference": value, "native": frame["fields"].get(key)})
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
                        compare_rejection(codec, source, positional, keywords, captured, error)
                        raise
                    compare(codec, image, source, positional, keywords, captured)
                    if args.metadata:
                        from metadata_attributes import compare_attributes
                        record=report["reads"][-1]
                        attribute_frames=record.pop("_attribute_frames",[])
                        prefix=record.pop("_source_prefix_hex",None)
                        source_bits=record.pop("_source_bits",None)
                        try:record["attribute_comparison"]=compare_attributes(record,attribute_frames,prefix,source_bits)
                        except Exception as error:record["attribute_comparison"]={"status":"comparison_error","error":f"{type(error).__name__}: {error}"}
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
    matching_statuses = {"counts_match", "reference_view_match", "component_view_match", "reconstruction_match"}
    compared_codecs = {r["codec"] for r in report["reads"] if r["status"] in matching_statuses}
    report["rejection_summary"] = dict(collections.Counter(r["status"] for r in report["reference_rejections"]))
    if any(r["status"] not in {"rejection_match", "reference_environment_difference"} for r in report["reference_rejections"]):
        report["gaps"].append("some reference rejection inputs/configurations remain unmatched")
    required = set(canonical_modules.values())
    report["unexercised_codecs"] = sorted(required - compared_codecs)
    report["full_reading_parity"] = (not args.modules and result.wasSuccessful() and not result.skipped
        and not report["reference_import_errors"] and not report["unexercised_codecs"]
        and not report["gaps"] and all(r["status"] in matching_statuses | {"reference_no_pixels"}
            and not r.get("header_field_differences") for r in report["reads"])
        and all(x["matches_pinned_source"] for x in report["reference_codec_sources"].values()))
    if args.metadata:
        from metadata_parity import summarize, compact
        report["metadata_summary"] = summarize(report["reads"])
        report["metadata_parity"] = all(
            record.get("metadata") and all(item["status"] in {"match", "reference_empty"} for item in record["metadata"])
            for record in report["reads"])
        allowed_metadata = {"match", "reference_empty", "qualified_reference_difference"}
        pixel_reads = [r for r in report["reads"] if r.get("reference") is not None]
        report["metadata_fields_accounted_for_pixel_reads"] = bool(pixel_reads) and all(
            record.get("metadata") and all(item["status"] in allowed_metadata for item in record["metadata"])
            for record in pixel_reads)
        from metadata_attributes import summarize_attributes
        report["attribute_summary"] = summarize_attributes(report["reads"])
        report["metadata_attributes_accounted_for"] = all(record.get("attribute_comparison",{}).get("status")=="accounted" for record in report["reads"])
        header_only_reads=[record for record in report["reads"] if record["status"]=="reference_no_pixels"]
        report["header_only_parity"] = bool(header_only_reads) and all(record.get("header_only",{}).get("status")=="match" for record in header_only_reads)
        report["header_only_summary"] = {"read_events":len(header_only_reads),"status":dict(collections.Counter(record.get("header_only",{}).get("status","not_compared") for record in header_only_reads)),
            "tiff_tag_occurrences":sum(record.get("header_only",{}).get("tag_occurrences",0) for record in header_only_reads)}
        report["full_reading_parity"] &= report["metadata_attributes_accounted_for"] and report["header_only_parity"]
        report["metadata_mapping_schemas"] = compact(report["reads"])
        report["full_reading_parity"] &= report["metadata_parity"]
    else:
        report["metadata_parity"] = False
        report["full_reading_parity"] = False
    report["download_base"] = args.download_base
    report["download_hashes"] = UtilsTest.resources.all_data
    report["probe_unchanged_during_run"] = report["probe_sha256"] == sha_file(probe_path)
    report["sources_unchanged_during_run"] = all(sha_file(ROOT/p) == h for p,h in report["rust_sources_sha256"].items())
    report["vendored_sources_unchanged_during_run"] = all(sha_file(ROOT/p) == h for p,h in report["vendored_sources_sha256"].items())
    report["native_sources_unchanged_during_run"] = all(sha_file(ROOT/p) == h for p,h in report["native_sources_sha256"].items())
    report["build_files_unchanged_during_run"] = all(sha_file(ROOT/p) == h for p,h in report["build_files_sha256"].items())
    report["reference_component_library_unchanged_during_run"] = component_reference is None or component_reference.unchanged()
    report["reference_pillow_libraries_unchanged_during_run"] = pillow_reference is None or pillow_reference.unchanged()
    report["bridge_unchanged_during_run"] = all(sha_file(ROOT/p) == h for p,h in report["bridge_sources_sha256"].items())
    report["explicit_policies_unchanged_during_run"] = all(sha_file(pathlib.Path(item["path"])) == item["sha256"]
        for item in policy_files.values())
    if not all(report[key] for key in ["probe_unchanged_during_run", "sources_unchanged_during_run",
            "bridge_unchanged_during_run", "vendored_sources_unchanged_during_run",
            "build_files_unchanged_during_run", "reference_component_library_unchanged_during_run",
            "reference_pillow_libraries_unchanged_during_run",
            "native_sources_unchanged_during_run",
            "explicit_policies_unchanged_during_run"]):
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
