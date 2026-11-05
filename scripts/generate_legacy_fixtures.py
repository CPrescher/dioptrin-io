#!/usr/bin/env python3
"""Generate independent CC0 detector fixtures using Python's standard library.
No FabIO code/writer or detector data is used. All files encode explicit arrays.
"""
import hashlib
import json
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[1] / 'tests/data/legacy'
ROOT.mkdir(parents=True, exist_ok=True)
cases = {}

def record(key, value):
    return f'{key:<7}:{value}'.encode().ljust(80, b' ')

def bruker(version, width, data, counts, tables=b'', underwidth=1, baseline=0, linear='1 0'):
    fields = [('FORMAT', version), ('HDRBLKS', 5), ('NROWS', 2), ('NCOLS', 3),
              ('NPIXELB', f'{width} {underwidth}' if version == 100 else str(width)), ('NOVERFL', counts),
              ('NEXP', f'1 1 {baseline}'), ('WORDORD', 0), ('LONGORD', 0),
              ('LINEAR', linear), ('TITLE', 'Independent counts'), ('TITLE', 'second record')]
    header = b''.join(record(k, v) for k, v in fields).ljust(2560, b' ')
    fmt = {1:'B', 2:'H', 4:('i' if version == 100 else 'I')}[width]
    return header + struct.pack('<' + fmt * len(data), *data) + tables

def pad(data):
    return data + bytes((-len(data)) % 16)

def oxford(values, ty1=True, version=3):
    small, medium, large = bytearray(), [], []
    if ty1:
        previous = 0
        for value in values:
            delta = value - previous
            previous = value
            if -127 <= delta <= 126:
                small.append(delta + 127)
            elif -32768 <= delta <= 32767:
                small.append(254)
                medium.append(delta)
            else:
                small.append(255)
                large.append(delta)
        payload = bytes(small) + struct.pack('<' + 'h'*len(medium), *medium) + struct.pack('<'+'i'*len(large), *large)
    else:
        payload = struct.pack('<' + 'i'*len(values), *values)
    header_size = 5120 if version == 3 else 6576
    lines = [f'OD SAPPHIRE {version:.1f}', 'COMPRESSION=' + ('TY1' if ty1 else ' NO'),
             f'NX={3:4d} NY={2:4d} OI={len(medium):7d} OL={len(large):7d}',
             f'NHEADER={header_size:7d} NG={512:7d} NS={768:7d} NK={1024:7d} NF={512:7d} NH={2048:7d}', 'NSUPPLEMENT=      0', 'TIME=Thu Nov  5 00:00:00 2026']
    ascii_header = ('\r\n'.join(lines)+'\r\n').encode().ljust(256, b' ')
    header = bytearray(header_size)
    header[:256] = ascii_header
    struct.pack_into('<HH', header, 256, 1, 1)
    struct.pack_into('<HH', header, 256+26, 3, 2)
    struct.pack_into('<I', header, 256+36, 6)
    struct.pack_into('<d', header, 256+512+480, 2.5)
    return bytes(header) + payload

def raxis(words, little=False, ratio=2.5):
    order = '<' if little else '>'
    header = bytearray(1536)
    header[:20] = b'R-AXIS independent 1'[:20].ljust(20)
    struct.pack_into(order+'ii', header, 768, 3, 2)
    struct.pack_into(order+'i', header, 784, 1536)
    struct.pack_into(order+'f', header, 800, ratio)
    header[812:822] = b'PC        ' if little else b'IRIS      '
    struct.pack_into(order+'iii', header, 832, 1, 1, 1)
    header[1400:] = b'P' * (len(header)-1400)
    return bytes(header) + struct.pack(order+'H'*len(words), *words)

def add(name, family, data, values, dtype, **kwargs):
    (ROOT/name).write_bytes(data)
    fmt = {'U8':'B','U16':'H','U32':'I','I8':'b','I16':'h','I32':'i','I64':'q'}[dtype]
    canonical = struct.pack('<'+fmt*len(values), *values)
    cases[name] = dict(family=family, shape=[2,3], dtype=dtype, values=values,
                      file_sha256=hashlib.sha256(data).hexdigest(),
                      frame_sha256=hashlib.sha256(canonical).hexdigest(), **kwargs)

add('bruker86-u8.sfrm','bruker',bruker(86,1,[0,1,255,2,3,4],'0'),[0,1,255,2,3,4],'U8')
add('bruker86-overflow.sfrm','bruker',bruker(86,2,[0,65535,2,65535,4,5],'2', b''.join(f'{v:9d}{p:7d}'.encode() for v,p in [(70000,1),(123456789,3)])),[0,70000,2,123456789,4,5],'U32')
add('bruker86-u32.sfrm','bruker',bruker(86,4,[0,1,2**32-1,2**31,65535,42],'0'),[0,1,2**32-1,2**31,65535,42],'U32')
add('bruker86-linear.sfrm','bruker',bruker(86,2,[0,4,8,12,16,20],'0',linear='0.25 2'),[0,4,8,12,16,20],'U16', reference_difference='FabIO implicitly applies LINEAR and emits float32; native counts stay unchanged')
for uw, under in [(1,[-10,0]),(2,[-32768,-500]),(4,[-2147483648,2147483647])]:
    tables = pad(struct.pack('<'+{1:'bb',2:'hh',4:'ii'}[uw],*under)) + pad(struct.pack('<HH',255,65535)) + pad(struct.pack('<i',80000))
    add(f'bruker100-under{uw}.sfrm','bruker',bruker(100,1,[0,255,255,7,0,1],'2 2 1',tables,uw,32),[under[0],287,80032,39,under[1],33],'I32')
add('bruker100-baseline.sfrm','bruker',bruker(100,1,[0,1,2,255,4,5],'0 0 0',baseline=32),[32,33,34,287,36,37],'I32')
add('bruker100-u16.sfrm','bruker',bruker(100,2,[0,65535,255,7,0,1],'-1 0 1',pad(struct.pack('<i',100000))),[0,100000,255,7,0,1],'I32')
add('bruker100-i32.sfrm','bruker',bruker(100,4,[-2147483648,-12,2147483647,2,3,4],'-1 0 0'),[-2147483648,-12,2147483647,2,3,4],'I32')
for name, values, dtype in [('i8',[1,0,126,127,0,-1],'I8'),('i16',[0,127,200,32767,32700,100],'I16'),('i32',[0,40000,30000,1000000,1000001,42],'I32'),('i64',[2000000000,4000000000,6000000000,6000000001,6000000000,6000000000],'I64'),('negative',[0,-200,-1000,-900,-500,0],'I16')]:
    kwargs = dict(reference_difference='FabIO selects dtype from maximum only and wraps negative counts') if name=='negative' else {}
    add(f'oxford-ty1-{name}.img','oxford',oxford(values),values,dtype,**kwargs)
add('oxford-raw.img','oxford',oxford([-2147483648,-7,0,1,2147483647,9],False),[-2147483648,-7,0,1,2147483647,9],'I32')
add('oxford-v4-raw.img','oxford',oxford([6,5,4,3,2,1],False,4),[6,5,4,3,2,1],'I32')
words=[0,32767,0x8001,0xffff,42,0x8010]
for little in [False,True]:
    add(f'raxis-{"little" if little else "big"}.img','raxis',raxis(words,little),words,'U16',little_endian=little,expanded=[0,32767,2,81917,42,40])
add('raxis-no-flags.img','raxis',raxis([0,1,2,3,4,32767]),[0,1,2,3,4,32767],'U16',expanded=[0,1,2,3,4,32767])
(ROOT/'provenance.json').write_text(json.dumps(dict(license='CC0-1.0', generator='scripts/generate_legacy_fixtures.py', source='Independent explicit arrays and manually packed format records; no upstream detector data', files=cases),indent=2)+'\n')
