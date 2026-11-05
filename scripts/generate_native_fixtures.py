"""CC0 independent scalar, array-order, packed-mask and header fixtures.
NumPy/FabIO are generation/reference dependencies only, never Rust runtime.
"""
import hashlib
import json
import pathlib
import struct

import fabio
import numpy as np
from fabio.dtrekimage import DtrekImage
from fabio.fit2dmaskimage import Fit2dMaskImage

root = pathlib.Path(__file__).resolve().parents[1] / "tests/data/native"
root.mkdir(exist_ok=True)
manifest = {"license": "CC0-1.0", "numpy": np.__version__, "fabio": fabio.version, "files": {}}


def record(path, format, a, reference=True):
    if reference:
        image = fabio.open(path)
        ref = image.data if a.ndim <= 2 else np.stack([image.getframe(i).data for i in range(image.nframes)])
        expected = a.reshape((-1,) + a.shape[-2:]) if a.ndim > 2 else a.reshape((1, -1)) if a.ndim < 2 else a
        canonical_ref = np.ascontiguousarray(ref.astype(ref.dtype.newbyteorder('<'), copy=False))
        canonical_expected = np.ascontiguousarray(expected.astype(expected.dtype.newbyteorder('<'), copy=False))
        assert np.array_equal(canonical_ref.view('u1'), canonical_expected.view('u1')), (path, ref, expected)
    frames = a.reshape((-1,) + a.shape[-2:]) if a.ndim > 2 else a.reshape((1, 1, -1)) if a.ndim < 2 else a[None]
    canonical = [np.ascontiguousarray(f.astype(f.dtype.newbyteorder('<'), copy=False)) for f in frames]
    manifest["files"][path.name] = {"format": format, "shape": list(frames.shape[-2:]), "source_shape": list(a.shape),
        "dtype": a.dtype.name, "file_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "frame_sha256": [hashlib.sha256(f.tobytes()).hexdigest() for f in canonical], "fabio_compared": reference}


for dtype in ["uint8", "uint16", "uint32", "uint64", "int8", "int16", "int32", "int64", "float32", "float64"]:
    if dtype.startswith("float"):
        a = np.array([0., -0., np.nan, np.inf, -np.inf, -1.25], dtype=dtype).reshape(2, 3)
    else:
        info = np.iinfo(dtype)
        a = np.array([0, info.min, info.max, 1, 2, 3], dtype=dtype).reshape(2, 3)
    for order in ["<", ">"]:
        endian = "le" if order == "<" else "be"
        for storage in ["C", "F"]:
            data = np.array(np.tile(a, (2, 3, 1, 1)), dtype=np.dtype(dtype).newbyteorder(order), order=storage)
            data[..., 1, 2] = np.arange(6).reshape(2, 3)
            path = root / f"npy-{dtype}-{endian}-{storage}.npy"
            np.save(path, data)
            record(path, "Npy", data)
    if dtype in ["uint8", "uint16", "uint32", "int8", "int16", "int32", "float32"]:
        for endian in ["little", "big"]:
            path = root / f"dtrek-{dtype}-{endian}.img"
            DtrekImage(data=a, header={"BYTE_ORDER": endian + "_endian", "COMMENT": "{ Stored axes {nested} and counts }"}).write(path)
            record(path, "Dtrek", a)

for version in [1, 2, 3]:
    a = np.array([0, 2**53+1, 2**64-1], dtype='uint64')
    path = root / f"npy-v{version}.npy"
    with path.open("wb") as f:
        np.lib.format.write_array(f, a, version=(version, 0))
    record(path, "Npy", a)

for width in [1, 8, 31, 32, 33, 65]:
    a = ((np.arange(3 * width).reshape(3, width) * 7 + 1) % 11 < 4).astype('uint8')
    path = root / f"mask-{width}.msk"
    Fit2dMaskImage(data=a).write(path)
    record(path, "Fit2dMask", a)

a = np.array([[0, 4096, 65535], [1, 32768, 23]], dtype='<u2')
comment = b'Exposure="12.5",Label="First",Label="Synthetic"\r\nCamera=Hamamatsu\x00'
header = b'IM' + struct.pack('<6H', len(comment), 3, 2, 7, 11, 2) + bytes(50)
path = root / "hipic.img"
path.write_bytes(header + comment + a.tobytes())
record(path, "HiPic", a)

for dtype, maxval in [('uint8', 255), ('uint16', 65535), ('uint32', 2147483647)]:
    # Binary starts with whitespace and '#' values: these are pixels, not header.
    a = np.array([[10, 32, 35], [0, maxval, 13]], dtype=dtype)
    path = root / f"p5-{dtype}.pgm"
    path.write_bytes(f'P5\n3 2\n{maxval}\n'.encode() + a.astype('>'+a.dtype.str[1:]).tobytes())
    record(path, "Pnm", a)
for subformat, maxval, dtype in [('P1', 1, 'uint8'), ('P2', 65535, 'uint16')]:
    a = np.array([[0, 1, maxval], [1, 0, 1]], dtype=dtype)
    path = root / f"{subformat.lower()}.pnm"
    path.write_text(f'{subformat}\n# Comment\n3 2\n' + (f'{maxval}\n' if subformat == 'P2' else '') + '0 1 # Inline comment\n' + f'{maxval}\n1 0 1\n')
    # Pinned FabIO P1/P2 implementation never increments the row index and has
    # nonstandard P1 MAXVAL handling. Validate against specification/original
    # values instead of adopting that data-loss bug; keep it a parity gap.
    record(path, "Pnm", a, reference=False)

a = np.array([[0., -0., -1.25], [1e-30, 1e30, -4.5]], dtype='float32')
path = root / 'spreadsheet.spr'
path.write_text('# Original comment\n3 2 Synthetic title\n0 -0 -1.25\n1e-30 1e30 -4.5\n')
record(path, 'Fit2dSpreadsheet', a)
a = np.array(-0., dtype='float64')
path = root / 'npy-scalar.npy'
np.save(path, a)
record(path, 'Npy', a)
(root / "provenance.json").write_text(json.dumps(manifest, indent=2) + '\n')
