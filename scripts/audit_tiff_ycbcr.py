#!/usr/bin/env python3
"""Read-only complete-source YCbCr proof; no upstream writers or RGB raw oracle."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess

import numpy as np
from PIL import Image, features

p = argparse.ArgumentParser()
p.add_argument("--reference-helper", type=Path, required=True)
p.add_argument("--reference-library", type=Path, required=True)
p.add_argument("--probe", type=Path, default=Path("target/debug/examples/ycbcr_probe"))
p.add_argument(
    "--source",
    type=Path,
    default=Path(
        "/tmp/dioptrin-io-fabio-corpus/libtiffpic.tar.gz__content/ycbcr-cat.tif"
    ),
)
p.add_argument(
    "--output", type=Path, default=Path("docs/tiff-ycbcr/2026-11-06-proof.json")
)
a = p.parse_args()


def digest(data):
    return hashlib.sha256(data).hexdigest()


before = digest(a.source.read_bytes())
spec = importlib.util.spec_from_file_location(
    "explicit_tiff_component_reference", a.reference_helper
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
reference = module.Reference(a.reference_library)
original = reference.read(a.source)
result = subprocess.run(
    [str(a.probe.resolve()), str(a.source)], check=True, capture_output=True, text=True
)
native = json.loads(result.stdout)
unit_storage = native["storage"] == "TiffSamplingUnits"
comparable_reference = json.loads(json.dumps(original))
if not unit_storage:
    for chunk in comparable_reference["chunks"]:
        chunk["sampling_units"] = None
for key in ["shape", "subsampling", "precision", "compression", "chunks"]:
    if native[key] != comparable_reference[key]:
        raise RuntimeError(f"complete independent component mismatch: {key}")
with Image.open(a.source) as image:
    rgb = np.asarray(image)
    tags = {
        str(k): list(v) if isinstance(v, tuple) else v
        for k, v in image.tag_v2.items()
        if k in [258, 259, 262, 274, 277, 278, 284, 529, 530, 531, 532]
    }
    # Rational conversion below is for JSON only; native metadata retains exact
    # numerator/denominator words, visible in the scoped probe's typed tags.
    tags = json.loads(json.dumps(tags, default=float))
if rgb.shape != (*original["shape"], 3) or rgb.dtype != np.uint8:
    raise RuntimeError("unexpected pinned Pillow RGB view")
values = rgb.astype(np.uint32)
luminance = (
    values[:, :, 0] * 299 + values[:, :, 1] * 587 + values[:, :, 2] * 114
).astype(np.float32) / np.float32(1000)
rgb_sha = digest(rgb.tobytes())
luminance_sha = digest(luminance.astype("<f4", copy=False).tobytes())
rgb_match = (
    None
    if "rgb_sha256" not in native["display"]
    else native["display"]["rgb_sha256"] == rgb_sha
)
if rgb_match is False or native["display"]["luminance_f32_sha256"] != luminance_sha:
    raise RuntimeError("explicit Pillow RGB/luminance view mismatch")
if digest(a.source.read_bytes()) != before:
    raise RuntimeError("source changed during proof")
report = dict(
    schema=1,
    source=dict(
        path=str(a.source),
        sha256=before,
        bytes=a.source.stat().st_size,
        unchanged_after_proof=True,
        tags=tags,
    ),
    reference=dict(
        libtiff=reference.metadata,
        helper_sha256=digest(a.reference_helper.read_bytes()),
        pillow_version=Image.__version__,
        pillow_libtiff_version=features.version("libtiff"),
    ),
    native_probe_sha256=digest(a.probe.read_bytes()),
    native=native,
    independent_reference=original,
    proof=dict(
        all_chunks_match=True,
        chunk_count=len(original["chunks"]),
        all_planes_match=True,
        plane_count=3 * len(original["chunks"]),
        all_stored_units_match=True if unit_storage else None,
        rgb_match=rgb_match,
        luminance_f32_match=True,
        raw_components_are_not_rgb=True,
    ),
)
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report["proof"], indent=2))
