#!/usr/bin/env python3
"""Independent CC0 OJPEG containers from explicit component grids.

Test-only system libjpeg-turbo encodes source JPEG; a separately loaded libtiff
OJPEG decoder supplies complete native component expectations. No production
reader, FabIO writer or external detector pixels generate these fixtures.
"""
import argparse
import ctypes as C
import hashlib
import json
from pathlib import Path
import struct

ROOT=Path(__file__).resolve().parents[1]/'tests/data/ojpeg'

def main():
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--jpeg-library',default='/opt/homebrew/opt/jpeg-turbo/lib/libturbojpeg.dylib')
 p.add_argument('--tiff-library',default='/opt/homebrew/opt/libtiff/lib/libtiff.dylib')
 args=p.parse_args();ROOT.mkdir(parents=True,exist_ok=True)
 j=C.CDLL(args.jpeg_library);j.tj3Init.argtypes=[C.c_int];j.tj3Init.restype=C.c_void_p
 j.tj3Set.argtypes=[C.c_void_p,C.c_int,C.c_int]
 j.tj3CompressFromYUV8.argtypes=[C.c_void_p,C.c_void_p,C.c_int,C.c_int,C.c_int,C.POINTER(C.c_void_p),C.POINTER(C.c_size_t)]
 j.tj3Destroy.argtypes=[C.c_void_p];j.tj3Free.argtypes=[C.c_void_p]
 t=C.CDLL(args.tiff_library);t.TIFFOpen.argtypes=[C.c_char_p,C.c_char_p];t.TIFFOpen.restype=C.c_void_p
 t.TIFFClose.argtypes=[C.c_void_p]
 for fn in ['TIFFTileSize64','TIFFStripSize64']:
  f=getattr(t,fn);f.argtypes=[C.c_void_p];f.restype=C.c_uint64
 for fn in ['TIFFReadEncodedTile','TIFFReadEncodedStrip']:
  f=getattr(t,fn);f.argtypes=[C.c_void_p,C.c_uint32,C.c_void_p,C.c_ssize_t];f.restype=C.c_ssize_t
 manifest={}
 for name,kind,horizontal,vertical,physical,valid in [
  ('interchange-standard','interchange',2,2,[16,16],[16,16]),
  ('interchange-legacy-sos','legacy',2,2,[16,16],[16,16]),
  ('table-edge-tile','table',2,2,[16,16],[11,13]),
  ('table-422-tile','table',2,1,[16,16],[13,15]),
 ]:
  height,width=physical;ch=height//vertical;cw=width//horizontal
  source=bytes(8+(r*11+c*5)%240 for r in range(height)for c in range(width))+bytes(30+(r*7+c*3)%200 for r in range(ch)for c in range(cw))+bytes(220-(r*4+c*8)%180 for r in range(ch)for c in range(cw))
  handle=j.tj3Init(0);assert handle
  assert j.tj3Set(handle,3,91)==0 # TJPARAM_QUALITY
  assert j.tj3Set(handle,4,2 if vertical==2 else 1)==0 # TJPARAM_SUBSAMP 420/422
  data=C.create_string_buffer(source);output=C.c_void_p();length=C.c_size_t()
  assert j.tj3CompressFromYUV8(handle,data,width,1,height,C.byref(output),C.byref(length))==0
  jpeg=bytearray(C.string_at(output,length.value));j.tj3Free(output);j.tj3Destroy(handle)
  segments=[];pos=2;qt={};dc={};ac={};frame=None
  while True:
   marker=jpeg[pos+1];n=int.from_bytes(jpeg[pos+2:pos+4],'big');payload=bytes(jpeg[pos+4:pos+2+n]);segments.append((marker,pos,payload))
   if marker==0xdb:
    at=0
    while at<len(payload):qt[payload[at]&15]=payload[at+1:at+65];at+=65
   if marker==0xc4:
    at=0
    while at<len(payload):
     ident=payload[at];amount=sum(payload[at+1:at+17]);(ac if ident&16 else dc)[ident&15]=payload[at+1:at+17+amount];at+=17+amount
   if marker==0xc0:frame=payload
   pos+=n+2
   if marker==0xda:sos=payload;break
  entropy=bytes(jpeg[pos:-2]);assert jpeg[-2:]==b'\xff\xd9'
  if kind=='legacy':jpeg[pos-2]=0
  qids=[frame[8+i*3] for i in range(3)];dids=[sos[2+i*2]>>4 for i in range(3)];aids=[sos[2+i*2]&15 for i in range(3)]
  tags={256:(4,[valid[1]]),257:(4,[valid[0]]),258:(3,[8,8,8]),259:(3,[6]),262:(3,[6]),
        274:(3,[6]),277:(3,[3]),284:(3,[1]),512:(3,[1]),519:(4,[0,0,0]),520:(4,[0,0,0]),521:(4,[0,0,0]),530:(3,[horizontal,vertical])}
  tile=kind=='table'
  tags.update({322:(4,[width]),323:(4,[height]),324:(4,[0]),325:(4,[len(entropy)])}if tile else{273:(4,[0]),278:(4,[height]),279:(4,[len(entropy)]),513:(4,[0]),514:(4,[len(jpeg)])})
  metadata_end=8+2+12*len(tags)+4
  extra=bytearray();external={}
  for tag,(ty,values)in sorted(tags.items()):
   size=len(values)*(2 if ty==3 else 4)
   if size>4:external[tag]=metadata_end+len(extra);extra.extend(bytes(size))
  source_start=metadata_end+len(extra);body=bytearray();table_offsets=[]
  if tile:
   for table,ids in [(qt,qids),(dc,dids),(ac,aids)]:
    positions={}
    for ident in dict.fromkeys(ids):positions[ident]=source_start+len(body);body.extend(table[ident])
    table_offsets.append([positions[i]for i in ids])
   tags[324]=(4,[source_start+len(body)]);body.extend(entropy)
  else:
   tags[513]=(4,[source_start]);tags[273]=(4,[source_start+pos]);body.extend(jpeg)
   for marker,at,payload in segments:
    if marker in [0xdb,0xc4]:
     cursor=0
     while cursor<len(payload):
      ident=payload[cursor];start=source_start+at+5+cursor
      if marker==0xdb:qt[ident&15]=start;cursor+=65
      else:
       count=sum(payload[cursor+1:cursor+17]);(ac if ident&16 else dc)[ident&15]=start;cursor+=17+count
   table_offsets=[[qt[i]for i in qids],[dc[i]for i in dids],[ac[i]for i in aids]]
  for tag,values in zip([519,520,521],table_offsets):tags[tag]=(4,values)
  out=bytearray(b'II*\0\x08\0\0\0'+struct.pack('<H',len(tags)))
  for tag,(ty,values)in sorted(tags.items()):
   packed=struct.pack('<'+('H'if ty==3 else'I')*len(values),*values)
   out.extend(struct.pack('<HHI',tag,ty,len(values)))
   if tag in external:
    out.extend(struct.pack('<I',external[tag]));at=external[tag]-metadata_end;extra[at:at+len(packed)]=packed
   else:out.extend(packed.ljust(4,b'\0'))
  out.extend(bytes(4));out.extend(extra);out.extend(body);path=ROOT/(name+'.tif');path.write_bytes(out)
  handle=t.TIFFOpen(str(path).encode(),b'r');assert handle
  size=(t.TIFFTileSize64 if tile else t.TIFFStripSize64)(handle);units=C.create_string_buffer(size)
  result=(t.TIFFReadEncodedTile if tile else t.TIFFReadEncodedStrip)(handle,0,units,size);t.TIFFClose(handle);assert result==size
  raw=units.raw[:size];(ROOT/(name+'.units')).write_bytes(raw)
  y=bytearray(height*width);cb=bytearray(ch*cw);cr=bytearray(ch*cw);cursor=0
  for r in range(ch):
   for c in range(cw):
    for dy in range(vertical):
     for dx in range(horizontal):y[(r*vertical+dy)*width+c*horizontal+dx]=raw[cursor];cursor+=1
    cb[r*cw+c]=raw[cursor];cr[r*cw+c]=raw[cursor+1];cursor+=2
  planes=[]
  for label,values,shape,valid_shape in [('Y',y,physical,valid),('Cb',cb,[ch,cw],[(valid[0]+vertical-1)//vertical,(valid[1]+horizontal-1)//horizontal]),('Cr',cr,[ch,cw],[(valid[0]+vertical-1)//vertical,(valid[1]+horizontal-1)//horizontal])]:
   (ROOT/(name+'.'+label)).write_bytes(values);planes.append(dict(component=label,shape=shape,valid_shape=valid_shape,sha256=hashlib.sha256(values).hexdigest()))
  manifest[path.name]=dict(file_sha256=hashlib.sha256(out).hexdigest(),shape=valid,encoded_shape=physical,subsampling=[horizontal,vertical],reconstruction='TableDefinedSingleChunk'if tile else'CompleteInterchange',baseline_sos_normalized=kind=='legacy',planes=planes)
 def lib(path):
  resolved=Path(path).resolve();return dict(path=str(resolved),sha256=hashlib.sha256(resolved.read_bytes()).hexdigest())
 (ROOT/'provenance.json').write_text(json.dumps(dict(license='CC0-1.0',generator='scripts/generate_ojpeg_fixtures.py',source='Independent explicit Y/Cb/Cr grids compressed with systemTurboJPEG then manually wrapped in original TIFF6 layouts; nativeLibTIFF exact component oracle',reference_libraries=dict(jpeg=lib(args.jpeg_library),tiff=lib(args.tiff_library)),files=manifest),indent=2)+'\n')
if __name__=='__main__':main()
