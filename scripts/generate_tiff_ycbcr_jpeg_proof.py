#!/usr/bin/env python3
"""Independent CC0 fixture component hashes; source TIFF files are unchanged."""

import hashlib
import json
from pathlib import Path

import imagecodecs
import numpy as np
from PIL import Image

root = Path(__file__).resolve().parents[1] / "tests/data/tiff-view"
cases = {}
for name in ["jpeg-ycbcr-strips.tif", "jpeg-ycbcr-tiles.tif"]:
    source = (root / name).read_bytes()
    with Image.open(root / name) as image:
        tags = image.tag_v2
        horizontal, vertical = tags[530]
        width, height = image.size
        tiled = 324 in tags
        chunk_width = tags[322] if tiled else width
        chunk_height = tags[323] if tiled else min(tags[278], height)
        across = (width + chunk_width - 1) // chunk_width
        chunks = []
        for i, (offset, length) in enumerate(
            zip(tags[324 if tiled else 273], tags[325 if tiled else 279])
        ):
            raster = imagecodecs.jpeg8_decode(
                source[offset : offset + length],
                tables=tags.get(347),
                colorspace="YCbCr",
                outcolorspace="YCbCr",
                fancyupsampling=False,
            )
            # These CC0 fixtures use 1x1 sampling: every native component sample,
            # including entire encoded edge tiles, exists in this raster.
            assert (horizontal, vertical) == (1, 1)
            origin = [i // across * chunk_height, i % across * chunk_width]
            valid = [
                min(chunk_height, height - origin[0]),
                min(chunk_width, width - origin[1]),
            ]
            chunks.append(
                dict(
                    origin=origin,
                    image_shape=valid,
                    planes=[
                        dict(
                            shape=list(raster[:, :, c].shape),
                            sample_sha256=hashlib.sha256(
                                np.ascontiguousarray(raster[:, :, c]).tobytes()
                            ).hexdigest(),
                        )
                        for c in range(3)
                    ],
                )
            )
        cases[name] = dict(
            source_sha256=hashlib.sha256(source).hexdigest(), chunks=chunks
        )
report = dict(
    license="CC0-1.0",
    reference=dict(
        imagecodecs=imagecodecs.__version__,
        jpeg=imagecodecs.jpeg_version(),
        policy="native YCbCr, no fancy upsampling, explicit 1x1 fixture sampling",
    ),
    files=cases,
)
(root / "native-components.json").write_text(json.dumps(report, indent=2) + "\n")
