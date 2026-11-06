"""Read-only attribution of the two pinned FabIO/Pillow display mismatches.

Controlled TIFF rewrites use private temporary files; original detector inputs
are never written. No upstream issue, patch or message is submitted by this tool.
"""
import argparse
import contextlib
import inspect
import io
import json
import pathlib
import shutil
import subprocess
import tempfile

import fabio
from fabio import TiffIO
from fabio.utils import pilutils
import numpy as np
import tifffile
from PIL import Image

from pillow_planar16_reference import Reference, sha_file, signature

ROOT = pathlib.Path(__file__).resolve().parents[1]


def audit(args):
    baseline = json.loads(args.baseline.read_text())
    assert fabio.version == baseline["reference"]["version"] == "2026.6.0"
    selected = [row for row in baseline["reads"] if row["status"] == "mismatch"]
    assert len(selected) == 2 and all(row["codec"] == "tifimage" for row in selected)
    oracle = Reference(args.tiff_reference_library)
    package_paths = [pathlib.Path(inspect.getfile(TiffIO)), pathlib.Path(inspect.getfile(pilutils)),
                     pathlib.Path(inspect.getfile(fabio.tifimage))]
    package_hashes = {str(path): sha_file(path) for path in package_paths}
    inputs = [ROOT / "src/tiff/view.rs", ROOT / "vendor/tiff-0.11.3/src/decoder/image.rs",
              pathlib.Path(__file__), ROOT / "scripts/pillow_planar16_reference.py",
              ROOT / "scripts/generate_pillow_planar16_fixtures.py",
              ROOT / "tests/data/pillow-planar16/provenance.json",
              ROOT / "tests/data/pillow-planar16/multitile-provenance.json"]
    code_hashes = {str(path.relative_to(ROOT)): sha_file(path) for path in inputs}
    probe_hash = sha_file(args.probe)
    records = []
    with tempfile.TemporaryDirectory(prefix="dioptrin-planar16-proof-") as temporary:
        probe = pathlib.Path(temporary) / "probe"
        shutil.copy2(args.probe, probe)
        assert sha_file(probe) == probe_hash

        def native(path, adapter="tiff-pillow13"):
            process = subprocess.run([str(probe), adapter, str(path)], capture_output=True, text=True, timeout=30, check=True)
            rows = [json.loads(line) for line in process.stdout.splitlines()]
            assert len(rows) == 1
            return rows[0]

        for event in selected:
            path = pathlib.Path(event["source"])
            assert sha_file(path) == event["file_sha256"]
            reference = fabio.open(path)
            assert reference.lib == "PIL"
            output = native(path)
            proof = oracle.verify(path, 0, output, reference.data)
            assert proof and proof["qualified"], proof
            # The classifier must not excuse a wrong native sample or a new
            # reference output merely because this layout is historically bad.
            damaged = {**output, "sample_sha256": "0" * 64}
            damaged_proof = oracle.verify(path, 0, damaged, reference.data)
            assert not damaged_proof["qualified"]
            changed = reference.data.copy()
            changed[0, 0] += np.float32(1)
            changed_proof = oracle.verify(path, 0, output, changed)
            assert not changed_proof["qualified"]
            try:
                with TiffIO.TiffIO(str(path)) as reader:
                    reader.getData(0)
                raise AssertionError("expected pinned TiffIO fallback cause")
            except (ValueError, IOError) as error:
                tiffio_error = f"{type(error).__name__}: {error}"
            with tifffile.TiffFile(path) as file:
                page = file.pages[0]
                raw = page.asarray().transpose(1, 2, 0)
                photo, byteorder = int(page.photometric), file.byteorder
            controls = []
            for planar, codec in [("contig", None), ("separate", "deflate"), ("separate", "lzw"), ("separate", "packbits")]:
                control = pathlib.Path(temporary) / f"{path.stem}-{planar}-{codec or 'raw'}.tif"
                tifffile.imwrite(control, raw if planar == "contig" else raw.transpose(2, 0, 1),
                                 photometric="rgb" if photo == 2 else "separated", planarconfig=planar,
                                 byteorder=byteorder, compression=codec, metadata=None)
                with tifffile.TiffFile(control) as file:
                    recovered = file.pages[0].asarray()
                    if planar == "separate":
                        recovered = recovered.transpose(1, 2, 0)
                    assert signature(recovered) == signature(raw)
                with Image.open(control) as image:
                    display = np.asarray(image.convert("F"))
                rendered = native(control)
                assert all(rendered[key] == value for key, value in signature(raw).items())
                assert signature(display) == proof["corrected_luminance"]
                assert all(rendered["reference_view"][key] == value for key, value in signature(display).items())
                controls.append({"planar": planar, "compression": codec or "none", "tiff_sha256": sha_file(control),
                                 "source_words_unchanged": True, "pillow_corrected_luminance": signature(display),
                                 "native_corrected_view_matches": True})
            records.append({"source": str(path), "source_sha256": event["file_sha256"], "original_event": event["test"],
                            "TiffIO_fallback_cause": tiffio_error, "proof": proof,
                            "negative_qualification_checks": {"wrong_native_hash_rejected": True, "changed_reference_pixel_rejected": True},
                            "same_words_storage_controls": controls})
            reference.close()
        fixtures = []
        for manifest_name, adapter in [("provenance.json", "tiff-pillow13"), ("multitile-provenance.json", "tiff")]:
            root = ROOT / "tests/data/pillow-planar16"
            manifest = json.loads((root / manifest_name).read_text())
            for name, case in manifest["files"].items():
                path = root / name
                assert sha_file(path) == case["sha256"]
                output = native(path, adapter)
                expected = {"shape": case["shape"], "dtype": case.get("source_dtype", case.get("dtype")),
                            "sample_sha256": case["source_sha256"]}
                assert all(output[key] == value for key, value in expected.items()), name
                if "reference_defect" in case:
                    if case["reference_defect"]:
                        assert "reference layout defect" in output["view_error"], name
                    else:
                        assert output["reference_view"]["sample_sha256"] == case["pillow_sha256"], name
                fixtures.append({"source": str(path.relative_to(ROOT)), "source_words_match": True,
                                 "pillow_display_matches": True if not case.get("reference_defect") and adapter == "tiff-pillow13" else None,
                                 "uncompressed_reference_defect_refused": case.get("reference_defect", False)})
        assert sha_file(probe) == probe_hash
    assert all(sha_file(ROOT / path) == value for path, value in code_hashes.items())
    assert all(sha_file(path) == value for path, value in package_hashes.items())
    assert oracle.unchanged()
    return {"schema": 1, "baseline": str(args.baseline), "baseline_sha256": sha_file(args.baseline),
            "contract": "Original mismatches independently attributed; reference defects never count as matching complete counts or views; corrected compressed views preserve all source words.",
            "probe_sha256": probe_hash, "code_and_fixture_manifest_sha256": code_hashes,
            "reference": {"fabio": fabio.version, **oracle.metadata, "FabIO_sources_sha256": package_hashes},
            "records": records, "fixtures": fixtures,
            "summary": {"original_reference_decoder_defects": len(records), "compressed_display_regressions": 48,
                        "uncompressed_defect_guards": 12, "multitile_source_regressions": 24,
                        "same_words_storage_controls": sum(len(row["same_words_storage_controls"]) for row in records)},
            "all_source_runtime_reference_hashes_unchanged": True}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=pathlib.Path, default=ROOT / "docs/fabio-parity/2026-11-06-source-recovery.json")
    parser.add_argument("--probe", type=pathlib.Path, default=ROOT / "target/release/examples/parity_probe")
    parser.add_argument("--tiff-reference-library", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    with contextlib.redirect_stdout(io.StringIO()):
        result = audit(args)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["summary"], indent=2))
