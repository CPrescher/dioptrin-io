#!/usr/bin/env python3
"""Generate CC0 Oxford TY5 fixtures from explicit arrays, without any reader code."""
import hashlib
import json
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[1] / 'tests/data/ty5'
ROOT.mkdir(parents=True, exist_ok=True)

def encode(rows):
    height, width = len(rows), len(rows[0])
    data, offsets, n16, n32 = bytearray(), [], 0, 0
    for row in rows:
        assert len(row) == width
        offsets.append(len(data))
        previous = 0
        for value in row:
            delta = value - previous
            previous = value
            if -127 <= delta <= 126:
                data.append(delta + 127)
            elif -32768 <= delta <= 32767:
                data.extend(b'\xfe' + struct.pack('<h', delta))
                n16 += 1
            else:
                data.extend(b'\xff' + struct.pack('<i', delta))
                n32 += 1
    size = 5120
    lines = ['OD SAPPHIRE 3.0', 'COMPRESSION=TY5',
             f'NX={width:4d} NY={height:4d} OI={n16:7d} OL={n32:7d}',
             f'NHEADER={size:7d} NG={512:7d} NS={768:7d} NK={1024:7d} NF={512:7d} NH={2048:7d}',
             'NSUPPLEMENT=      0', 'TIME=Thu Nov  5 00:00:00 2026']
    header = bytearray(size)
    header[:256] = ('\r\n'.join(lines) + '\r\n').encode().ljust(256, b' ')
    struct.pack_into('<HH', header, 256, 1, 1)
    struct.pack_into('<HH', header, 256+26, width, height)
    struct.pack_into('<I', header, 256+36, width * height)
    return bytes(header) + data + struct.pack('<' + 'I' * height, *offsets), offsets, [n16, n32]

cases = {}
for name, rows in {
    'non-square': [[1, 2, 3], [-7, -8, 200]],
    'inline-extrema': [[-2147483648, -2147483647, -2147483521],
                       [2147483647, 2147483646, 2147450878], [-300, -173, -47]],
    'single-column': [[32768], [-32768], [127]],
    'single-row': [[0, 126, -1, 126, -1]],
    'negative-only': [[-1000, -999], [-10, -137]],
}.items():
    raw, offsets, exceptions = encode(rows)
    samples = struct.pack('<'+'i' * sum(map(len, rows)), *(v for row in rows for v in row))
    filename = name + '.img'
    (ROOT / filename).write_bytes(raw)
    cases[filename] = dict(shape=[len(rows), len(rows[0])], dtype='I32', values=rows,
                           row_offsets=offsets, exceptions=exceptions,
                           file_sha256=hashlib.sha256(raw).hexdigest(),
                           samples_sha256=hashlib.sha256(samples).hexdigest())
(ROOT/'provenance.json').write_text(json.dumps(dict(
    license='CC0-1.0', generator='scripts/generate_ty5_fixtures.py',
    source='Independent explicit arrays, manually packed inline deltas and row offsets; no upstream detector pixels or reference encoder',
    files=cases), indent=2)+'\n')
