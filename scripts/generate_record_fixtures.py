#!/usr/bin/env python3
"""CC0-1.0. Independent FIT2D/KCD/MPA explicit arrays; stdlib only."""
import hashlib
import json
from pathlib import Path
import struct
ROOT=Path(__file__).resolve().parents[1]/'tests/data/records'
ROOT.mkdir(parents=True,exist_ok=True)
files={}
def array(name,shape,dtype,raw):
    return dict(name=name,shape=shape,dtype=dtype,frame_sha256=hashlib.sha256(raw).hexdigest())
def add(name,family,data,arrays,**extras):
    (ROOT/name).write_bytes(data)
    files[name]=dict(family=family,file_sha256=hashlib.sha256(data).hexdigest(),arrays=arrays,**extras)
def fit2d(block,little,floating):
    output=bytearray()
    order='<' if little else '>'
    def record(name,payload):
        data=b'\\'+name.encode()+b':'+payload.encode()
        assert len(data)<block
        output.extend(data.ljust(block-1,b' ')+b'\n')
    def scalar(name,code,value):record(name,'00000000'+code+value)
    def string(name,value):scalar(name,'s',f'{len(value):08x}'+value)
    def pixels(name,shape,dtype,values):
        count=shape[0]*shape[1]
        perblock=block//4*31 if dtype=='U8' else 128
        blocks=(count+perblock-1)//perblock
        code={'U8':'l','I32':'i','F32':'r'}[dtype]
        record(name,f'{blocks:08x}a{code}{1 if dtype=="U8" else 32:08x}00000002{shape[1]:08x}{shape[0]:08x}')
        raw=bytearray(blocks*block)
        if dtype=='U8':
            for j in range((count+30)//31):
                word=0x80000000
                for k,v in enumerate(values[j*31:(j+1)*31]):word|=v<<k
                struct.pack_into(order+'I',raw,j*4,word)
            canonical=bytes(values)
        else:
            for i,v in enumerate(values):struct.pack_into(order+('I' if floating and name=='data_array' else 'i' if dtype=='I32' else 'I'),raw,(i//128)*block+(i%128)*4,v)
            canonical=struct.pack('<'+('i' if dtype=='I32' else 'I')*len(values),*values)
        output.extend(raw)
        return array(name,shape,dtype,canonical)
    string('$FFF_START','FIT2D Data File')
    string('$BIG_ENDIAN','FALSE' if little else 'TRUE')
    scalar('gain','r','3e800000')
    scalar('flags','i','ffffffff')
    main=pixels('data_array',[2,100] if floating else [2,3],'F32' if floating else 'I32',([0x80000000,0,0x3fc00000,0x7fc01234,0x7f800000,0xff800000]+[0x3f800000]*194) if floating else [-2147483648,-1,0,1,2147483647,42])
    mask=pixels('data_mask',[2,35],'U8',[int(i%3==0 or i in [30,31,61,62]) for i in range(70)])
    axis=pixels('x_axis',[1,3],'F32',[0,0x3f800000,0x40000000])
    string('$FFF_END','End of image data')
    return bytes(output),[main,mask,axis]
for block in [512,2048]:
    for little in [True,False]:
        name=f'fit2d-{block}-{"little" if little else "big"}.f2d'
        data,arrays=fit2d(block,little,block==2048)
        add(name,'fit2d',data,arrays,little_endian=little,block_bytes=block,expected_reference_difference=None if little else 'Pinned FabIO scalar/mask decoder ignores source byte-order flag')

def kcd(shape,readouts):
    header=f'No independent source\nX dimension = {shape[1]}\nY dimension = {shape[0]}\nData type = u16\nNumber of readouts = {len(readouts)}\nBinned mode\n'.encode().ljust(4100,b' ')
    return header+b''.join(struct.pack('<'+'H'*len(values),*values)for values in readouts)
readouts=[[0,1,65535,42,65534,1],[65535,2,1,43,0,65535],[1,3,2,44,1,0]]
add('readouts.kcd','kcd',kcd([2,3],readouts),[array(f'readout{i}',[2,3],'U16',struct.pack('<6H',*values))for i,values in enumerate(readouts)],summed=array('sum',[2,3],'I32',struct.pack('<6i',*[sum(v)for v in zip(*readouts)])))
# More than32768 maximum words exceed signed32 only under explicit summation.
add('overflow.kcd','kcd',kcd([1,1],[[65535]]*32769),[],overflow=True)
mpa_header=b'mpafmt=asc\r\nfmt=asc\r\nsephead=0\r\n[ADC1]\r\nrange=2\r\n[ADC2]\r\nrange=3\r\n[MAP0] source coincidence\r\n'
sections=[('DATA0',[1,2],[1.,2.]),('DATA1',[1,3],[3.,4.,5.]),('CDAT0',[2,3],[0.,-1.,1.5,42.,65535.,9007199254740992.]),('CDAT1',[2,3],[6.,5.,4.,3.,2.,1.])]
mpa=bytearray(mpa_header)
for name,shape,values in sections:
    mpa.extend(f'[{name},{len(values)} ]\r\n'.encode())
    mpa.extend(''.join(f'{value:.17g}\r\n'for value in values).encode())
add('sections.mpa','mpa',bytes(mpa),[array(name,shape,'F64',struct.pack('<'+'d'*len(values),*values))for name,shape,values in sections])
(ROOT/'provenance.json').write_text(json.dumps(dict(license='CC0-1.0',generator='scripts/generate_record_fixtures.py',source='Independent explicit arrays, manually packed records; no upstream data',files=files),indent=2)+'\n')
