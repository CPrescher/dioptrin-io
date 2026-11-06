#!/usr/bin/env python3
"""CC0 independent byte-layout fixture; no FabIO import or serialization."""
from pathlib import Path
import struct


def fixture():
    pack = lambda fmt, *v: struct.pack('<' + fmt, *v)
    data = bytearray(pack('Idd', 0x1000c, -0.0, 3.25))
    for text in ['independent.ccd', 'producer', 'chip', 'serial', 'taper', 'type', '尾']:
        data.extend(text.encode().ljust(256, b'\0'))
    data.extend(pack('3H', 9, 10, 11))
    data.extend(pack('9H', 1, 1, 2, 3, 4, 5, 6, 7, 8))
    data.extend(pack('9H', 0, 8, 7, 6, 5, 4, 3, 2, 1))
    assert len(data) == 1854
    data.extend(pack('H14H', 1, 1, 4, 1, 2, 91, 92, 93, 94, 0, 1, 95, 96, 97, 98))
    data.extend(pack('H7H', 1, 3, 2, 4, 5, 6, 7, 0xabcd))
    row = lambda seed: pack('9H', *range(seed, seed + 9))
    for count in [1, 0, 1, 0]:
        data.extend(pack('H', count))
        for _ in range(count): data.extend(row(10) + pack('2H', 111, 222))
    data.extend(pack('H', 1) + row(20))
    data.extend(pack('HddHdd', 7, 1.5, 2.5, 1, 3.5, 4.5))
    # Deliberately differ from the binned column counts to expose reference bug.
    for count, seed in [(1, 30), (0, 40), (2, 50)]:
        data.extend(pack('H', count))
        for i in range(count): data.extend(row(seed + i * 10))
    return bytes(data) + b'opaque trailer\0\xff'


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1] / 'tests/data/xcalibur'
    root.mkdir(parents=True, exist_ok=True)
    (root / 'records.ccd').write_bytes(fixture())
