"""Independent CC0 FabIO legacy eight-bit PCK V2 fixtures; explicit dialect only.

V2 changes the header alphabet of PCK V1: run exponent4bits, width selector4bits,
widths0/4..16/32. Original source words and simple packer remain independent.
"""
import hashlib
import json
import pathlib
import struct

ROOT=pathlib.Path(__file__).resolve().parents[1]/"tests/data/mar345-v2"
WIDTHS=[0,*range(4,17),32]


def signed16(value):return (value+32768)%65536-32768


def prediction(values,index,width):
    if index==0:return 0
    if index<=width:return signed16(values[index-1])
    total=sum(signed16(values[i]) for i in [index-1,index-width-1,index-width,index-width+1])+2
    return -(abs(total)//4) if total<0 else total//4


def residuals(values,width):
    return [signed16(signed16(value)-prediction(values,i,width)) for i,value in enumerate(values)]


def pack(values,forced=None,run=1,dialect="fabio8"):
    widths=WIDTHS if dialect=="fabio8" else [0,*range(3,17),32]
    run_bits=4 if dialect=="fabio8" else 3
    bits=[]
    def put(value,width):bits.extend((value>>i)&1 for i in range(width))
    for start in range(0,len(values),run):
        chunk=values[start:start+run];exponent=len(chunk).bit_length()-1
        assert 0<=exponent<(1<<run_bits) and len(chunk)==1<<exponent
        width=forced if forced is not None else (0 if not any(chunk) else next(w for w in widths[1:] if all(-(1<<(w-1))<=v<(1<<(w-1)) for v in chunk)))
        assert width in widths
        put(exponent,run_bits);put(widths.index(width),4)
        for value in chunk:
            assert (width and -(1<<(width-1))<=value<(1<<(width-1))) or (not width and value==0)
            put(value&((1<<width)-1),width)
    result=bytearray((len(bits)+7)//8)
    for index,bit in enumerate(bits):result[index//8]|=bit<<(index%8)
    return bytes(result),len(bits)


def source(values,shape,*,little=True,compact=False,forced=None,run=1,crlf=False,extended=False,encoded=None,dialect="fabio8"):
    high=[(i+1,v) for i,v in enumerate(values) if v>65535]
    order="<" if little else ">"
    words=[1234,shape[1],len(high),1,1,len(values),150,151,1234567,234567,-123,456,789,-999,1000,2000]
    raw=bytearray(4096);struct.pack_into(order+"16i",raw,0,*words);text_start=128 if extended else 64
    if extended:raw[64:80]=bytes(range(16))
    text=(f"mar research\nPROGRAM Independent CC0 V2 fixture\nDATE 2026-11-06\nFORMAT {shape[1]} PCK {len(values)}\nHIGH {len(high)}\nREMARK Original stored rows; no flip\nUNKNOWN retained V2 source metadata\nEND OF HEADER\n").encode()
    raw[text_start:text_start+len(text)]=text
    table=b"".join(struct.pack(order+"II",*pair) for pair in high)
    if not compact:table+=bytes((-len(high))%8*8)
    marker=f"\nCCP4 packed image V2, X: {shape[1]:04d}, Y: {shape[0]:04d}\n".encode()
    if crlf:marker=marker.replace(b"\n",b"\r\n")
    if encoded is None:encoded,bits=pack(residuals(values,shape[1]),forced,run,dialect)
    else:encoded,bits=encoded
    payload=bytes(raw)+table+marker+encoded
    canonical=struct.pack("<"+str(len(values))+"I",*values)
    metadata={"shape":shape,"little_endian":little,"pck_version":2,"pck_v2_dialect":"FabioLegacyEightBit" if dialect=="fabio8" else "Original","text_start":text_start,"overflow_layout":"Compact" if compact and len(high)%8 else "PaddedEight","overflow_records":[{"one_based_index":i,"value":v} for i,v in high],"marker_hex":marker.hex(),"header_hex":bytes(raw).hex(),"binary_words":words,"overflow_bytes_hex":table.hex(),"encoded_bytes":len(encoded)}
    return payload,values,{"sha256":hashlib.sha256(payload).hexdigest(),"sample_sha256":hashlib.sha256(canonical).hexdigest(),"used_bits":bits,"encoded_hex":encoded.hex(),"metadata":metadata}


def fixtures():
    out={}
    for selector,width in enumerate(WIDTHS):
        values=[0]*32 if not width else [i%5 for i in range(32)]
        out[f"v2-width{width}.mar345"]=source(values,[4,8],forced=width)
    for exponent in range(16):
        count=max(4,1<<exponent);shape=[count//8,8] if count>=16 else [2,count//2]
        out[f"v2-zero-run{1<<exponent}.mar345"]=source([0]*count,shape,run=1<<exponent)
    values=[(i*31)%32768 for i in range(32)]
    for little,name in [(True,"le"),(False,"be")]:
        overlay=values.copy()
        for index,value in [(0,65536),(7,999999),(16,2147483648),(31,4294967295)]:overlay[index]=value
        for compact in [False,True]:
            name2=f"v2-overflow-{'compact' if compact else 'padded'}-{name}.mar345"
            out[name2]=source(overlay,[4,8],little=little,compact=compact,extended=compact)
    values=[32768+i for i in range(9)]+[65535,32768,1,0,65534,17,32767,65535,0,1,65535,32768,32769,2,3]
    out["v2-signed-neighborhood.mar345"]=source(values,[4,6])
    out["v2-partial-last-byte.mar345"]=source([0,1,2,3],[2,2],forced=5)
    out["v2-marker-crlf.mar345"]=source([i%5 for i in range(32)],[4,8],crlf=True)
    # Force nonzero32-bit signed residual boundaries, retaining the U16 source
    # domain and using the unchanged checked predictor for the intended words.
    differences=[-(1<<31),(1<<31)-1,-1,0,1,65535,-65536,32768]*4
    values=[]
    for index,difference in enumerate(differences):values.append((difference+prediction(values,index,8))%65536)
    encoded=pack(differences,forced=32)
    out["v2-residual32-extrema.mar345"]=source(values,[4,8],encoded=encoded)
    return out


def main():
    ROOT.mkdir(parents=True,exist_ok=True);manifest={"license":"CC0-1.0","generator":"scripts/generate_mar345_v2_fixtures.py","contract":"Independent original source words with explicit FabioLegacyEightBit framing and original signed16 predictor; not canonical original V2; no reference writer or LGPL implementation copied","files":{}}
    for name,(data,values,record) in fixtures().items():
        (ROOT/name).write_bytes(data);manifest["files"][name]=record
    (ROOT/"provenance.json").write_text(json.dumps(manifest,indent=2)+"\n")


if __name__=="__main__":main()
