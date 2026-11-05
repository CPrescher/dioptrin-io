"""Scoped external FabIO oracle; test-only, never a runtime dependency.
Build image_probe with tiff,jpeg,jpeg2000; run with CORPUS PROBE paths.
"""
import argparse
import hashlib
import json
import pathlib
import subprocess

import fabio
from fabio.marccdimage import MarccdImage
from fabio.pilatusimage import PilatusImage

DTYPES = {
    "uint8": "U8", "uint16": "U16", "uint32": "U32", "uint64": "U64",
    "int8": "I8", "int16": "I16", "int32": "I32", "int64": "I64",
    "float32": "F32", "float64": "F64",
}
SOURCES = {
    "jpeg": ["rand_uint8.jpg"],
    "jpeg2000": ["binned_data_uint8.jp2", "binned_data_uint16.jp2"],
    "mar": ["corkcont2_H_0089.mccd", "somedata_0001.mccd"],
    "pilatus": ["lysb_5mg-1.90s_SAXS", "Pilatus1M.tif"],
}


def mar_fields(path):
    # FabIO's normal MAR TIFF read does not invoke the semantic binary header
    # parser. Exercise that parser explicitly rather than comparing pixel tags.
    image = MarccdImage()
    with path.open("rb") as source:
        image._readheader(source)
    result = {}
    for key, value in image.header.items():
        if isinstance(value, bytes):
            value = list(value)
        elif isinstance(value, (tuple, list)):
            if value and isinstance(value[0], bytes):
                value = list(b"".join(value))
            else:
                value = list(value)
        result[key] = value
    return result


def compare(corpus, probe):
    report = []
    for format_name, names in SOURCES.items():
        for name in names:
            path = corpus / name
            if not path.exists():
                raise FileNotFoundError(path)
            if format_name in ("pilatus", "mar"):
                image = {"pilatus": PilatusImage, "mar": MarccdImage}[format_name]()
                image.read(path)
            else:
                image = fabio.open(path)
            try:
                result = json.loads(subprocess.check_output(
                    [probe, format_name, path], text=True, stderr=subprocess.PIPE))
            except subprocess.CalledProcessError as error:
                report.append({"format": format_name, "name": name,
                               "error": error.stderr})
                continue
            arrays = ([image.data] if image.nframes == 1 else
                      [image.getframe(i).data for i in range(image.nframes)])
            expected = [{
                "shape": list(array.shape), "dtype": DTYPES[str(array.dtype)],
                "hash": hashlib.sha256(array.astype(
                    array.dtype.newbyteorder("<")).tobytes()).hexdigest(),
            } for array in arrays]
            same = (len(result["frames"]) == len(expected) and all(
                all(actual[key] == reference[key] for key in ("shape", "dtype", "hash"))
                for actual, reference in zip(result["frames"], expected)))
            semantic = None
            if format_name == "pilatus":
                semantic = result["header"] == dict(image.header)
            elif format_name == "mar":
                reference = mar_fields(path)
                differences = {
                    key: {"rust": result["header"].get(key), "fabio": value}
                    for key, value in reference.items()
                    if result["header"].get(key) != value
                }
                semantic = {"keys": len(reference), "differences": differences}
            report.append({
                "format": format_name, "name": name,
                "source_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "frames": len(expected), "exact_pixels": same, "semantic": semantic,
                "sample_hashes": [frame["hash"] for frame in expected],
            })
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=pathlib.Path)
    parser.add_argument("probe", type=pathlib.Path)
    args = parser.parse_args()
    report = compare(args.corpus, args.probe)
    print(json.dumps(report, indent=2))
    if any(not item.get("exact_pixels") or item.get("semantic") is False or
           isinstance(item.get("semantic"), dict) and
           item["semantic"]["differences"] for item in report):
        raise SystemExit(1)
