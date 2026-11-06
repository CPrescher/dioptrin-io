"""Small independent CC0 arrays distinguishing Pillow raw and libtiff paths.

Test/reference only; all encoded images are generated, no detector data copied.
"""
import hashlib
import json
import pathlib

import imagecodecs
import numpy as np
import tifffile
from PIL import Image, TiffImagePlugin, __version__ as pillow_version


def generate(root):
    assert pillow_version == "13.0.0", "this oracle is versioned"
    assert not TiffImagePlugin.READ_LIBTIFF, "measure Pillow's default path"
    root.mkdir(parents=True, exist_ok=True)
    manifest = {"license": "CC0-1.0", "generator": "scripts/generate_pillow_planar16_fixtures.py",
                "versions": {"numpy": np.__version__, "tifffile": tifffile.__version__,
                             "Pillow": pillow_version, "imagecodecs": imagecodecs.__version__}, "files": {}}
    for color, channels in [("rgb", 3), ("rgba", 4), ("cmyk", 4)]:
        # Unequal high/low bytes and odd dimensions expose both byte stride and
        # row-boundary errors. Unassociated alpha remains an explicit view choice.
        values = np.arange(5 * 7 * channels, dtype="u4").reshape(5, 7, channels)
        raw = ((values * 1093 + 0x1287) % 65536).astype("u2")
        for endian, label in [("<", "le"), (">", "be")]:
            for tiled in [False, True]:
                for codec in [None, "deflate", "adobe_deflate", "lzw", "packbits"]:
                    name = f"{color}-{label}-{'tile' if tiled else 'strip'}-{codec or 'raw'}.tif"
                    path = root / name
                    options = {"tile": (16, 16)} if tiled else {"rowsperstrip": 2}
                    if codec in ["deflate", "adobe_deflate", "lzw"]:
                        options["predictor"] = 2
                    tifffile.imwrite(path, raw.transpose(2, 0, 1), byteorder=endian,
                                     photometric="separated" if color == "cmyk" else "rgb",
                                     planarconfig="separate", extrasamples=["unassalpha"] if color == "rgba" else None,
                                     compression=codec, metadata=None, **options)
                    with tifffile.TiffFile(path) as file:
                        page = file.pages[0]
                        assert np.array_equal(page.asarray().transpose(1, 2, 0), raw)
                        compression = int(page.compression)
                    with Image.open(path) as image:
                        use_libtiff = image.use_load_libtiff
                        pixels = np.asarray(image)
                        luminance = np.asarray(image.convert("F"))
                    correct = (raw >> 8).astype("u1")
                    if codec is not None:
                        assert use_libtiff and np.array_equal(pixels, correct)
                    else:
                        assert not use_libtiff and not np.array_equal(pixels, correct)
                    source_bytes = raw.astype("<u2").tobytes()
                    view_bytes = luminance.astype("<f4").tobytes()
                    (root / (name + ".raw")).write_bytes(source_bytes)
                    (root / (name + ".pillow")).write_bytes(view_bytes)
                    manifest["files"][name] = {
                        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                        "shape": list(raw.shape), "source_dtype": "uint16", "byteorder": endian,
                        "source_sha256": hashlib.sha256(source_bytes).hexdigest(),
                        "compression": compression, "pillow_default_libtiff": use_libtiff,
                        "reference_defect": codec is None,
                        "pillow_shape": list(luminance.shape), "pillow_dtype": "float32",
                        "pillow_sha256": hashlib.sha256(view_bytes).hexdigest()}
    (root / "provenance.json").write_text(json.dumps(manifest, indent=2) + "\n")

    # Multiple spatial tiles and several source types make the tile-index fix
    # independent of byte width, sample count and the display conversion above.
    tiled = {"license": "CC0-1.0", "files": {}}
    for dtype in ["u1", "u2", "u4", "i2", "f4", "f8"]:
        for channels in [3, 4]:
            for endian, label in [("<", "le"), (">", "be")]:
                raw = np.arange(19 * 21 * channels).reshape(19, 21, channels)
                raw = (raw % 197 - 51).astype(dtype) if dtype[0] in "if" else (raw % 251).astype(dtype)
                name = f"multi-{dtype}-{channels}-{label}.tif"
                path = root / name
                tifffile.imwrite(path, raw.transpose(2, 0, 1), photometric="rgb", planarconfig="separate",
                                 extrasamples=["unassalpha"] if channels == 4 else None,
                                 byteorder=endian, tile=(16, 16), compression="deflate", metadata=None)
                assert np.array_equal(tifffile.imread(path).transpose(1, 2, 0), raw)
                output = raw.astype(raw.dtype.newbyteorder("<")).tobytes()
                (root / (name + ".raw")).write_bytes(output)
                tiled["files"][name] = {"shape": list(raw.shape), "dtype": raw.dtype.name,
                                        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                                        "source_sha256": hashlib.sha256(output).hexdigest()}
    (root / "multitile-provenance.json").write_text(json.dumps(tiled, indent=2) + "\n")


if __name__ == "__main__":
    generate(pathlib.Path(__file__).resolve().parents[1] / "tests/data/pillow-planar16")
