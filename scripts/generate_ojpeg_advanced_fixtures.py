"""Independent CC0 OJPEG source-profile continuation; no production encoder."""
import argparse
import ctypes as C
import hashlib
import json
import pathlib
import struct
import subprocess
import tempfile
import numpy as np
from generate_ojpeg_fragment_fixtures import encode as encode_rows
from tiff_component_reference import Reference
ROOT=pathlib.Path(__file__).resolve().parents[1]/'tests/data/ojpeg-advanced'
def sha(data):return hashlib.sha256(data).hexdigest()
def bind(path):
 j=C.CDLL(str(path));j.tj3Init.argtypes=[C.c_int];j.tj3Init.restype=C.c_void_p;j.tj3Set.argtypes=[C.c_void_p,C.c_int,C.c_int];j.tj3Destroy.argtypes=[C.c_void_p];j.tj3Free.argtypes=[C.c_void_p]
 j.tj3CompressFromYUV8.argtypes=[C.c_void_p,C.c_void_p,C.c_int,C.c_int,C.c_int,C.POINTER(C.c_void_p),C.POINTER(C.c_size_t)]
 j.tj3Get.argtypes=[C.c_void_p,C.c_int];j.tj3Get.restype=C.c_int
 j.tj3DecompressHeader.argtypes=[C.c_void_p,C.c_void_p,C.c_size_t];j.tj3DecompressToYUV8.argtypes=[C.c_void_p,C.c_void_p,C.c_size_t,C.c_void_p,C.c_int]
 return j
def header(jpeg):
 pos=2;segments=[]
 while True:
  assert jpeg[pos]==255;marker=jpeg[pos+1];size=int.from_bytes(jpeg[pos+2:pos+4],'big');end=pos+size+2
  segments.append((marker,pos,end,jpeg[pos+4:end]));pos=end
  if marker==218:return jpeg[:pos],segments

def source_planes(j,jpeg,shape,factors):
 h,v=factors;rows,cols=shape;yshape=((rows+v-1)//v*v,(cols+h-1)//h*h);uvshape=((rows+v-1)//v,(cols+h-1)//h)
 length=yshape[0]*yshape[1]+2*uvshape[0]*uvshape[1];data=C.create_string_buffer(jpeg);handle=j.tj3Init(1);assert handle
 try:
  assert j.tj3DecompressHeader(handle,data,len(jpeg))==0
  if [j.tj3Get(handle,6),j.tj3Get(handle,5)]!=list(shape)or j.tj3Get(handle,7)!=8 or j.tj3Get(handle,4)!={(1,1):0,(2,1):1,(2,2):2}.get(tuple(factors)):
   raise ValueError('independent JPEG source header differs from admitted shape/precision/sampling')
  if length>512*1024*1024 or len(jpeg)>64*1024*1024:raise ValueError('independent JPEG reference budget')
  out=C.create_string_buffer(length)
  assert j.tj3DecompressToYUV8(handle,data,len(jpeg),out,1)==0
 finally:j.tj3Destroy(handle)
 first=yshape[0]*yshape[1];second=uvshape[0]*uvshape[1]
 return [(out.raw[:first],list(yshape)),(out.raw[first:first+second],list(uvshape)),(out.raw[first+second:],list(uvshape))]

def write(path,logical,rows,parts,prefix,*,planar=1,factors=(2,2),restart=0,tile=None,tables=None,complete=False):
 ot,lt=(324,325)if tile else(273,279)
 tags={256:(4,[logical[1]]),257:(4,[logical[0]]),258:(3,[8,8,8]),259:(3,[6]),262:(3,[6]),ot:(4,[0]*len(parts)),274:(3,[6]),277:(3,[3]),lt:(4,[len(p)if isinstance(p,bytes)else p[1]for p in parts]),284:(3,[planar]),512:(3,[1]),515:(3,[restart]),530:(3,list(factors))}
 tags.update({322:(4,[tile[1]]),323:(4,[tile[0]])}if tile else{278:(4,[rows])})
 if prefix is not None:tags[513]=(4,[0]);tags[514]=(4,[len(prefix)])
 if tables is not None:
  for tag in [519,520,521]:tags[tag]=(4,[0,0,0])
 end=8+2+12*len(tags)+4;external={};extra=bytearray()
 for tag,(kind,values)in sorted(tags.items()):
  size=(2 if kind==3 else 4)*len(values)
  if size>4:external[tag]=end+len(extra);extra.extend(bytes(size))
 start=end+len(extra);body=bytearray()
 if tables is not None:
  for tag,(table,selectors)in zip([519,520,521],tables):
   locations={}
   for ident in dict.fromkeys(selectors):locations[ident]=start+len(body);body.extend(table[ident])
   tags[tag]=(4,[locations[ident]for ident in selectors])
 if prefix is not None:tags[513]=(4,[start+len(body)]);base=start+len(body);body.extend(prefix)
 offsets=[]
 for part in parts:
  if complete:offsets.append(base+part[0])
  else:body.extend(b'ignored-gap-\xff\xd9');offsets.append(start+len(body));body.extend(part)
 tags[ot]=(4,offsets);data=bytearray(b'II*\0\x08\0\0\0'+struct.pack('<H',len(tags)))
 for tag,(kind,values)in sorted(tags.items()):
  packed=struct.pack('<'+('H'if kind==3 else'I')*len(values),*values);data.extend(struct.pack('<HHI',tag,kind,len(values)))
  if tag in external:data.extend(struct.pack('<I',external[tag]));at=external[tag]-end;extra[at:at+len(packed)]=packed
  else:data.extend(packed.ljust(4,b'\0'))
 data.extend(bytes(4));data.extend(extra);data.extend(body);path.write_bytes(data)
 return {'source_sha256':sha(data),'source_bytes':len(data),'shape':logical,'source_offsets':offsets,'source_bytecounts':tags[lt][1]}

def sequential_cjpeg(binary,subsampling):
 with tempfile.TemporaryDirectory()as folder:
  p=pathlib.Path(folder);rows,cols=16,32
  raw=bytes((x*3+y*7+c*40)%256 for y in range(rows)for x in range(cols)for c in range(3));(p/'source.ppm').write_bytes(b'P6\n32 16\n255\n'+raw);(p/'scans.txt').write_text('0;\n1;\n2;\n')
  subprocess.run([str(binary),'-quality','91','-sample',subsampling,'-scans',str(p/'scans.txt'),'-outfile',str(p/'image.jpg'),str(p/'source.ppm')],check=True,capture_output=True,timeout=20)
  return (p/'image.jpg').read_bytes()
def scan_ranges(jpeg):
 pos=2;scans=[]
 while pos<len(jpeg):
  assert jpeg[pos]==255;marker=jpeg[pos+1]
  if marker==217:break
  size=int.from_bytes(jpeg[pos+2:pos+4],'big');end=pos+size+2
  if marker==218:
   cursor=end
   while jpeg[cursor]!=255 or jpeg[cursor+1]==0 or 208<=jpeg[cursor+1]<=215:cursor+=2 if jpeg[cursor]==255 else 1
   scans.append((pos,end,cursor));pos=cursor
  else:pos=end
 return scans

def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=pathlib.Path,default=ROOT);p.add_argument('--jpeg-library',type=pathlib.Path,default=pathlib.Path('/opt/homebrew/opt/jpeg-turbo/lib/libturbojpeg.dylib'));p.add_argument('--tiff-library',type=pathlib.Path,default=pathlib.Path('/opt/homebrew/opt/libtiff/lib/libtiff.6.dylib'));p.add_argument('--cjpeg',type=pathlib.Path,default=pathlib.Path('/opt/homebrew/opt/jpeg-turbo/bin/cjpeg'));args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
 j=bind(args.jpeg_library);oracle=Reference(args.tiff_library);manifest={}
 def record(name,jpeg,shape,factors,logical,rows,parts,prefix,**kwargs):
  path=args.output/(name+'.tif');policy=kwargs.pop('policy','declared');authority=kwargs.pop('authority','libtiff_and_original_jpeg');case=write(path,logical,rows,parts,prefix,factors=factors,**kwargs)
  (args.output/(name+'.jpg')).write_bytes(jpeg);planes=source_planes(j,jpeg,shape,factors)
  case.update({'source_jpeg_sha256':sha(jpeg),'source_jpeg_shape':shape,'subsampling':list(factors),'policy':policy,'authority':authority,'planar':kwargs.get('planar',1)})
  tiled=kwargs.get('tile');count=len(parts)if case['planar']==1 else 1;chunk_rows=tiled[0]if tiled else rows;across=(logical[1]+tiled[1]-1)//tiled[1]if tiled else 1
  chunks=[]
  for index in range(count):
   origin=[index//across*chunk_rows,index%across*(tiled[1]if tiled else logical[1])];image_shape=[min(chunk_rows,logical[0]-origin[0]),min(tiled[1]if tiled else logical[1],logical[1]-origin[1])];descriptors=[]
   for component,((raw,pshape),sampling)in enumerate(zip(planes,[[1,1],[factors[1],factors[0]],[factors[1],factors[0]]])):
    start=index*chunk_rows//sampling[0]if case['planar']==1 else 0;stop=((index+1)*chunk_rows//sampling[0]if index+1<count else pshape[0]);data=np.frombuffer(raw,dtype='u1').reshape(pshape)[start:stop].copy();label=['Y','Cb','Cr'][component];(args.output/f'{name}.chunk{index}.{label}').write_bytes(data.tobytes())
    descriptors.append({'component':label,'shape':list(data.shape),'valid_shape':[(image_shape[k]+sampling[k]-1)//sampling[k]for k in range(2)],'sampling':sampling,'sample_sha256':sha(data.tobytes())})
   chunks.append({'origin':origin,'image_shape':image_shape,'planes':descriptors})
  case['source_word_reference']={'shape':logical,'subsampling':list(factors),'chunks':chunks};manifest[path.name]=case
  if authority=='libtiff_and_original_jpeg'and case['planar']==1:
   reference=oracle.read(path)
   for actual,wanted in zip(reference['chunks'],chunks):
    assert actual['origin']==wanted['origin']and actual['image_shape']==wanted['image_shape']
    for got,expected in zip(actual['planes'],wanted['planes']):assert all(got[k]==expected[k]for k in ['shape','valid_shape','sampling','sample_sha256'])
  if case['planar']==2 and authority=='libtiff_and_original_jpeg':
   lib=oracle.library;handle=lib.TIFFOpen(bytes(path),b'r');assert handle
   try:
    for index,plane in enumerate(chunks[0]['planes']):
     length=plane['shape'][0]*plane['shape'][1];buffer=C.create_string_buffer(length);assert lib.TIFFReadEncodedStrip(handle,index,buffer,length)==length;assert sha(buffer.raw)==plane['sample_sha256']
   finally:lib.TIFFClose(handle)
 # Single and multi-window source header cases.
 for name,width,height,source_rows,layout in [('single-prefix',32,16,16,'prefix'),('single-prefix-edge',31,19,32,'prefix'),('single-first-header',32,16,16,'first'),('single-prefix-restarts',32,32,16,'prefix'),('header-across-two',32,32,16,'two'),('header-across-three',32,48,16,'three'),('header-without-interchange',32,48,16,'none')]:
  jpeg,head,_,_,_=encode_rows(j,width,height,2,source_rows);entropy=jpeg[len(head):-2];rows=height if layout in ['prefix','first']else 16
  if layout=='prefix':parts=[entropy];prefix=head
  elif layout=='first':parts=[jpeg];prefix=None
  elif layout=='two':parts=[head[20:100],head[100:]+entropy];prefix=head[:20]
  elif layout=='three':parts=[head[20:100],head[100:],entropy];prefix=head[:20]
  else:parts=[head[:20],head[20:100],head[100:]+entropy];prefix=None
  record(name,jpeg,[height,width],[2,2],[height,width],rows,parts,prefix)
 jpeg,head,pieces,starts,tables=encode_rows(j,16,32,2,16);entropy=jpeg[len(head):-2];record('single-table-restarts',jpeg,[32,16],[2,2],[32,16],32,[entropy],None,tables=tables,restart=1)
 # Complete interchange with several source restart markers inside one TIFF strip.
 jpeg,head,pieces,starts,tables=encode_rows(j,32,48,2,16)
 # Increase restart frequency using encoder MCU-block setting, preserving all source words through fresh encoding.
 # A width64 stream has fourMCUs perrow. Restart everyrow gives oneRST boundary;
 # TIFF strips32rows then contain an internalRST, with exact source marker gaps.
 jpeg,head,pieces,starts,tables=encode_rows(j,64,64,2,16)
 spans=[(starts[0],starts[2]-starts[0]-2),(starts[2],len(jpeg)-2-starts[2])]
 record('complete-internal-restarts',jpeg,[64,64],[2,2],[64,64],32,spans,jpeg,complete=True,restart=4)
 # Literal full source restart stream across TIFF windows: no implied markers.
 restart_parts=[jpeg[len(head):starts[2]],jpeg[starts[2]:-2]]
 record('stored-restarts-prefix',jpeg,[64,64],[2,2],[64,64],32,restart_parts,head,restart=4,authority='original_jpeg_literal_restart_stream')
 for name,complete in [('sof1-prefix',False),('sof1-complete',True)]:
  jpeg,head,pieces,starts,_=encode_rows(j,32,16,2,16);modified=bytearray(jpeg);sof=next(at for marker,at,_,_ in header(jpeg)[1]if marker==192);modified[sof+1]=193;jpeg=bytes(modified);head=jpeg[:len(head)]
  record(name,jpeg,[16,32],[2,2],[16,32],16,[(len(head),len(jpeg)-len(head)-2)]if complete else[jpeg[len(head):-2]],jpeg if complete else head,complete=complete)
 for name,complete,sof1 in [('planar444-prefix',False,False),('planar444-complete',True,False),('planar444-sof1',False,True)]:
  jpeg=sequential_cjpeg(args.cjpeg,'1x1,1x1,1x1')
  if sof1:modified=bytearray(jpeg);sof=next(at for marker,at,_,_ in header(jpeg)[1]if marker==192);modified[sof+1]=193;jpeg=bytes(modified)
  scans=scan_ranges(jpeg)
  if complete:parts=[(start,end-start)for _,start,end in scans];prefix=jpeg
  else:prefix=jpeg[:scans[0][1]];parts=[jpeg[scans[0][1]:scans[1][0]],jpeg[scans[1][0]:scans[2][0]],jpeg[scans[2][0]:-2]]
  record(name,jpeg,[16,32],[1,1],[16,32],16,parts,prefix,planar=2,complete=complete)
 jpeg=sequential_cjpeg(args.cjpeg,'2x2,1x1,1x1');scans=scan_ranges(jpeg)
 record('planar420-complete',jpeg,[16,32],[2,2],[16,32],16,[(start,end-start)for _,start,end in scans],jpeg,planar=2,complete=True,authority='original_jpeg_planar_source_grids')
 # Ambiguous table-only tile stacks are qualified only under the explicit caller policy.
 jpeg,head,pieces,_,tables=encode_rows(j,32,64,2,16)
 record('table-grid-explicit-reset',jpeg,[64,32],[2,2],[29,51],16,pieces,None,tile=[16,32],tables=tables,policy='independent-reset-tiles',authority='original_jpeg_explicit_reset_interpretation')
 result={'license':'CC0-1.0','origin':'Independent component/RGB arrays encoded with native TurboJPEG/cjpeg; originalJPEG source-word oracle; native libtiff component equality where qualified; no production writer or FabIO-generated data','generator_sha256':sha(pathlib.Path(__file__).read_bytes()),'jpeg_library':{'path':str(args.jpeg_library.resolve()),'sha256':sha(args.jpeg_library.resolve().read_bytes())},'cjpeg':{'path':str(args.cjpeg.resolve()),'sha256':sha(args.cjpeg.resolve().read_bytes())},'libtiff':oracle.metadata,'files':manifest}
 (args.output/'provenance.json').write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps({'files':len(manifest),'chunks':sum(len(v['source_word_reference']['chunks'])for v in manifest.values())}))
if __name__=='__main__':main()
