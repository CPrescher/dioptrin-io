"""Independent CC0 bounded TIFF metadata interpretations/ThunderScan fixtures."""
import hashlib,io,json,pathlib,struct
import numpy as np,tifffile,imagecodecs
ROOT=pathlib.Path(__file__).resolve().parents[1]/'tests/data/tiff-recovery';ROOT.mkdir(exist_ok=True)
manifest={'license':'CC0-1.0','generator':'scripts/generate_tiff_recovery_fixtures.py','versions':{'numpy':np.__version__,'tifffile':tifffile.__version__,'imagecodecs':imagecodecs.__version__},'files':{}}
def ifd_entries(data):
 endian='<' if data[:2]==b'II' else '>';big=struct.unpack_from(endian+'H',data,2)[0]==43;offset=struct.unpack_from(endian+('Q' if big else 'I'),data,8 if big else 4)[0];n=struct.unpack_from(endian+('Q' if big else 'H'),data,offset)[0];start=offset+(8 if big else 2);stride=20 if big else 12
 return endian,big,{struct.unpack_from(endian+'H',data,p)[0]:p for p in range(start,start+n*stride,stride)}
def store(name,data,raw=None,**extra):
 (ROOT/name).write_bytes(data);info={'sha256':hashlib.sha256(data).hexdigest(),**extra}
 if raw is not None:raw=np.asarray(raw);wire=raw.astype(raw.dtype.newbyteorder('<')).tobytes();(ROOT/(name+'.raw')).write_bytes(wire);info.update(shape=list(raw.shape),dtype=str(raw.dtype),sample_sha256=hashlib.sha256(wire).hexdigest())
 manifest['files'][name]=info
# Independent complete source arrays precede metadata corruption.
a=np.array([[0,1,65535,17,32768],[14,13,12,11,10]],dtype='u2');stream=io.BytesIO();tifffile.imwrite(stream,a,rowsperstrip=1,metadata=None);data=bytearray(stream.getvalue());endian,big,entries=ifd_entries(data)
with tifffile.TiffFile(io.BytesIO(data)) as file:tag=file.pages[0].tags[279];position=tag.valueoffset
struct.pack_into('<II',data,position,5,5);store('short-u16.tif',data,a,policy='UncompressedWindowsFromShape')
a=np.array([0,0x80000000,0x3fa00000,0x7f800000,0xff800000,0x7fc0002a,0x00000001,0xc2c60000,0x40800000],dtype='u4').view('f4').reshape(3,3);stream=io.BytesIO();tifffile.imwrite(stream,a,bigtiff=True,byteorder='>',rowsperstrip=2,metadata=None);data=bytearray(stream.getvalue());endian,big,entries=ifd_entries(data);struct.pack_into(endian+'H',data,entries[279],65001);store('missing-count-be-f32.tif',data,a,policy='UncompressedWindowsFromShape')
a=(np.arange(17*19*3,dtype='u2')*37%256).astype('u1').reshape(17,19,3);stream=io.BytesIO();tifffile.imwrite(stream,a,tile=(16,16),compression='lzw',photometric='rgb',metadata=None);data=bytearray(stream.getvalue());endian,big,entries=ifd_entries(data);struct.pack_into(endian+'H',data,entries[324],273);struct.pack_into(endian+'H',data,entries[325],279);store('mixed-modern-lzw-tiles.tif',data,a,policy='TileTablesFromStripTags')
# Small classic TIFF page builder with all metadata before stored sample bytes.
def pages(images):
 fields=[];ifds=[];cursor=8
 for im in images:
  chunks=im['chunks'];tags=[(256,4,[im['width']]),(257,4,[im['height']]),(258,3,[im['bits']]),(259,3,[im['compression']]),(262,3,[im.get('photo',1)]),(273,4,[0]*len(chunks)),(278,4,[im['rows']]),(279,4,im.get('counts',[len(v) for v in chunks]))];tags.sort();ifds.append(cursor);fields.append(tags);cursor+=2+12*len(tags)+4+sum(4*len(v) for t,ty,v in tags if ty==4 and len(v)>1)
 sample_start=cursor;starts=[]
 for im in images:
  offsets=[]
  for chunk in im['chunks']:offsets.append(cursor);cursor+=len(chunk)
  starts.append(offsets)
 data=bytearray(b'II*\0'+struct.pack('<I',8))
 for index,tags in enumerate(fields):
  extra_start=len(data)+2+12*len(tags)+4;extra=bytearray();data+=struct.pack('<H',len(tags))
  for tag,ty,values in tags:
   if tag==273:values=starts[index]
   wire=struct.pack('<'+('H' if ty==3 else 'I')*len(values),*values);data+=struct.pack('<HHI',tag,ty,len(values))
   if len(wire)<=4:data+=wire.ljust(4,b'\0')
   else:data+=struct.pack('<I',extra_start+len(extra));extra+=wire
  data+=struct.pack('<I',ifds[index+1] if index+1<len(ifds) else 0)+extra
 assert len(data)==sample_start
 for im in images:
  for chunk in im['chunks']:data+=chunk
 assert len(data)==cursor
 return data
raw=np.array([[0,1,15,8,7],[5,4,3,2,1]],dtype='u1');packed=bytes([0x01,0xf8,0x70,0x54,0x32,0x10]);store('short-packed4.tif',pages([dict(width=5,height=2,bits=4,compression=1,rows=2,chunks=[packed],counts=[5])]),raw,policy='UncompressedWindowsFromShape')
a=(np.arange(35,dtype='u1')%16).reshape(5,7);full=[bytes(0xc0|int(x) for x in a[y:y+2].ravel()) for y in range(0,5,2)]
base=dict(width=7,height=5,bits=4,compression=32809,rows=2,chunks=full)
store('thunder-complete.tif',pages([base]),a)
partial=full.copy();partial[1]=partial[1][:10];second=np.array([[0,1,1,0,1,0,0],[1,0,1,0,0,1,1]],dtype='u1');second_packed=np.packbits(second,axis=1).tobytes();stack=pages([{**base,'chunks':partial},dict(width=7,height=2,bits=1,compression=1,rows=2,chunks=[second_packed])]);store('thunder-partial-stack.tif',stack,np.concatenate([a[:3].ravel(),a[4:].ravel()]),declared_shape=[5,7],available_rows=[[0,3],[4,1]],missing_rows=[[3,1]])
(ROOT/'thunder-partial-stack.tif.page1.raw').write_bytes(second.tobytes())
mode_bytes=bytes([0xc5,0x03,0x47,0x95,0x66,0xa1,0xcf,0xc0,0x7a,0x9c]);mode_raw=np.array([[5,5,5,5,5,6,5,7,4,5,6,15,0,15,2]],dtype='u1');store('thunder-all-modes.tif',pages([dict(width=15,height=1,bits=4,compression=32809,rows=1,chunks=[mode_bytes])]),mode_raw)
for name,chunks in [('thunder-run-overflow.tif',[b'\x3f']),('thunder-coded-tail.tif',[b'\xc1\xc2\xc3\xc4']),('thunder-noop-tail.tif',[b'\xc1\xc2\xc3\0\x6a\xa4'])]:
 store(name,pages([dict(width=1 if name=='thunder-run-overflow.tif' else 3,height=1,bits=4,compression=32809,rows=1,chunks=chunks)]),None)
(ROOT/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
