"""Independent encoded test vectors; uses stock Python Bitshuffle, not our codec."""
import hashlib
import json
from pathlib import Path

import bitshuffle
import numpy as np


def values(count, width):
    mask = (1 << (8 * width)) - 1
    for i in range(count):
        value = ((i * 0x9E3779B97F4A7C15) & ((1 << 64) - 1)) ^ 0xA5A5A5A5A5A5A5A5
        shift = i % 64
        value = ((value << shift) | (value >> (64 - shift))) & ((1 << 64) - 1)
        yield 0 if i % 17 == 0 else mask if i % 19 == 0 else value & mask


def main():
    root = Path(__file__).resolve().parents[1] / "tests/data/bitshuffle-neon"
    root.mkdir(exist_ok=True)
    encoded = bytearray()
    cases = []
    for width in (1, 2, 4, 8):
        for count in (8, 24, 128, 2048, 2056, 4099):
            array = np.array(list(values(count, width)), dtype=f"<u{width}")
            data = bitshuffle.bitshuffle(array, block_size=0).tobytes()
            cases.append({"element_bytes": width, "elements": count,
                          "offset": len(encoded), "length": len(data)})
            encoded.extend(data)
    (root / "shuffled.bin").write_bytes(encoded)
    manifest = {"encoder": f"Python bitshuffle {bitshuffle.__version__}",
                "numpy": np.__version__, "byte_order": "little", "block_elements": 0,
                "sha256": hashlib.sha256(encoded).hexdigest(), "cases": cases}
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
