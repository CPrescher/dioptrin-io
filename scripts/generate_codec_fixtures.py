"""Independent stock h5py/Bitshuffle vectors for both scalar widths and byte orders."""
import hashlib
import json
import ctypes
import os
from pathlib import Path

# Disable automatic reloading after unregistering the filter below. The raw
# compressor does not require HDF5 plugin callbacks.
os.environ["HDF5_PLUGIN_PRELOAD"] = "::"

import h5py
import hdf5plugin
import numpy as np

root = Path(__file__).resolve().parents[1] / "tests/data/codec-backends"
root.mkdir(exist_ok=True)
path = root / "counts.h5"
cases = []
# Use the stock filter's standalone compressor and direct-chunk writes. This
# avoids set_local callbacks crossing different HDF5 ABIs on macOS installations.
library = next(Path(hdf5plugin.PLUGINS_PATH).glob("*bshuf.*"))
codec = ctypes.CDLL(str(library))
codec.bshuf_compress_lz4_bound.argtypes = [ctypes.c_size_t, ctypes.c_size_t, ctypes.c_size_t]
codec.bshuf_compress_lz4_bound.restype = ctypes.c_size_t
codec.bshuf_compress_lz4.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                   ctypes.c_size_t, ctypes.c_size_t]
codec.bshuf_compress_lz4.restype = ctypes.c_int64
h5py.h5z.unregister_filter(32008)
while h5py.h5pl.size():
    h5py.h5pl.remove(0)
with h5py.File(path, "w") as file:
    for width in [2, 4]:
        for order in ["<", ">"]:
            dtype = np.dtype(f"{order}u{width}")
            values = np.arange(2 * 8209, dtype="uint64") * 0x9E3779B9
            values = (values & ((1 << (8 * width)) - 1)).astype(dtype)
            values[:4] = [0, 1, 1 << (8 * width - 1), (1 << (8 * width)) - 1]
            name = f"u{width * 8}_{'le' if order == '<' else 'be'}"
            dataset = file.create_dataset(name, shape=(2, 1, 8209), dtype=dtype,
                                          chunks=(1, 1, 8209), compression=32008,
                                          compression_opts=(0, 4, width, 0, 2),
                                          allow_unknown_filter=True)
            for frame in range(2):
                raw = values[frame * 8209:(frame + 1) * 8209].tobytes()
                source = ctypes.create_string_buffer(raw)
                bound = codec.bshuf_compress_lz4_bound(8209, width, 0)
                encoded = ctypes.create_string_buffer(bound)
                size = codec.bshuf_compress_lz4(source, encoded, 8209, width, 0)
                assert 0 <= size <= bound
                payload = len(raw).to_bytes(8, "big") + bytes(4) + encoded.raw[:size]
                dataset.id.write_direct_chunk((frame, 0, 0), payload, filter_mask=0)
            cases.append({"dataset": name, "dtype": str(dtype), "byte_order": order, "width": width,
                          "frame_sha256_le": [hashlib.sha256(a.astype(f"<u{width}").tobytes()).hexdigest()
                                              for a in values.reshape(2, -1)]})
manifest = {"license": "MIT; generated synthetic values, no external detector data",
            "h5py": h5py.__version__, "hdf5plugin": hdf5plugin.version, "numpy": np.__version__,
            "compressor_sha256": hashlib.sha256(library.read_bytes()).hexdigest(),
            "shape": [2, 1, 8209], "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "cases": cases}
(root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
