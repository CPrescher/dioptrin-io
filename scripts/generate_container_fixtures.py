"""CC0 small independent SPE/MRC/DM3 fixtures from struct/NumPy source arrays.
FabIO is an independent reference only, not a production dependency.
"""
import hashlib,json,pathlib,struct
import fabio,numpy as np
root=pathlib.Path(__file__).resolve().parents[1]/'tests/data/containers';root.mkdir(exist_ok=True)
manifest={'license':'CC0-1.0','numpy':np.__version__,'fabio':fabio.version,'files':{}}
def record(name,kind,a,compared=True):
 p=root/name;frames=a if a.ndim==3 else a[None]
 manifest['files'][name]={'format':kind,'shape':list(frames.shape),'dtype':a.dtype.name,'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'frame_sha256':[hashlib.sha256(np.ascontiguousarray(f.astype(f.dtype.newbyteorder('<'))).tobytes()).hexdigest() for f in frames],'fabio_compared':compared}
 if compared:
  ref={"spe":fabio.speimage.SpeImage,"mrc":fabio.mrcimage.MrcImage,"dm3":fabio.dm3image.Dm3Image}[kind]().read(p)
  for i,f in enumerate(frames):
   if kind in ('spe','mrc'):r=type(ref)().read(p,frame=i).data
   elif len(frames)>1:r=ref.getframe(i).data
   else:r=ref.data
   assert r.dtype.name==a.dtype.name and np.ascontiguousarray(r.astype(r.dtype.newbyteorder('<'))).tobytes()==np.ascontiguousarray(f.astype(f.dtype.newbyteorder('<'))).tobytes(),(name,i)

for code,dtype in [(0,'float32'),(1,'int32'),(2,'int16'),(3,'uint16')]:
 a=np.array([0,-0.,np.nan,np.inf,-np.inf,-1.25],dtype=dtype).reshape(2,3) if code==0 else np.array([0,np.iinfo(dtype).min,np.iinfo(dtype).max,1,2,3],dtype=dtype).reshape(2,3)
 frames=np.stack([a,a[::-1,::-1]])
 for version in [2,3]:
  h=bytearray(4100);struct.pack_into('<H',h,42,3);struct.pack_into('<H',h,656,2);struct.pack_into('<H',h,108,code);struct.pack_into('<i',h,1446,2);struct.pack_into('<f',h,1992,float(version))
  struct.pack_into('<f',h,10,0.5);h[20:29]=b'05Nov2026';h[172:178]=b'121314';struct.pack_into('<f',h,72,750);struct.pack_into('<f',h,650,300);struct.pack_into('<6d',h,3263,0,0.5,0,0,0,0)
  payload=frames.tobytes();xml=b''
  if version==3:
   struct.pack_into('<Q',h,678,4100+len(payload));size=a.nbytes;fmt={0:'MonochromeFloating32',1:'MonochromeSigned32',2:'MonochromeSigned16',3:'MonochromeUnsigned16'}[code]
   xml=f'<SpeFormat><DataFormat><DataBlock type="Frame" count="2" pixelFormat="{fmt}" size="{size}" stride="{size}"><DataBlock type="Region" count="1" width="3" height="2" size="{size}" stride="{size}"/></DataBlock></DataFormat><Calibrations><WavelengthMapping><Wavelength>0.5,1,1.5</Wavelength></WavelengthMapping></Calibrations><Origin created="2026-11-05T12:13:14.1234567+01:00"/><Camera model="Synthetic"/><ShutterTiming><ExposureTime>100</ExposureTime></ShutterTiming></SpeFormat>'.encode()
  name=f'spe-v{version}-{dtype}.spe';(root/name).write_bytes(h+payload+xml);record(name,'spe',frames,version==2)

for mode,dtype in [(0,'int8'),(1,'int16'),(2,'float32'),(4,'complex64'),(6,'uint16')]:
 a=np.array([0,-0.,np.nan,np.inf,-np.inf,-1.25],dtype=dtype).reshape(2,3) if mode==2 else np.array([0,1+2j,-1.25-2.5j,complex(np.nan,np.inf),3-0j,complex(-0.,-0.)],dtype=dtype).reshape(2,3) if mode==4 else np.array([0,np.iinfo(dtype).min,np.iinfo(dtype).max,1,2,3],dtype=dtype).reshape(2,3)
 frames=np.stack([a,a[::-1,::-1]])
 for order,endian in [('<','le'),('>','be')]:
  h=bytearray(1024)
  for pos,value in [(0,3),(4,2),(8,2),(12,mode),(28,3),(32,2),(36,2),(64,3),(68,1),(72,2),(92,16)]:struct.pack_into(order+'i',h,pos,value)
  h[208:212]=b'MAP ';h[212:216]=b'\x44\x44\0\0' if order=='<' else b'\x11\x11\0\0';h[224:241]=b'Stored rows only!'
  name=f'mrc-mode{mode}-{endian}.mrc';(root/name).write_bytes(h+b'OriginalExtHdr!!'+frames.astype(frames.dtype.newbyteorder(order)).tobytes());record(name,'mrc',frames,order=='<')
# True MRC mode3 uses i16 component pairs, unlike the pinned FabIO 8B interpretation.
pairs=np.array([[[-32768,32767],[0,-1],[1,2]],[[3,4],[5,6],[7,8]]],dtype='int16')
for order,endian in [('<','le'),('>','be')]:
 h=bytearray(1024)
 for pos,value in [(0,3),(4,2),(8,1),(12,3)]:struct.pack_into(order+'i',h,pos,value)
 h[208:212]=b'MAP ';h[212:216]=b'\x44\x44\0\0' if order=='<' else b'\x11\x11\0\0'
 name=f'mrc-mode3-{endian}.mrc';(root/name).write_bytes(h+pairs.astype(order+'i2').tobytes())
 manifest['files'][name]={'format':'mrc','shape':[1,2,3],'dtype':'complex_int16','sha256':hashlib.sha256((root/name).read_bytes()).hexdigest(),'frame_sha256':[hashlib.sha256(pairs.astype('<i2').tobytes()).hexdigest()],'fabio_compared':False,'reference_difference':'Pinned FabIO treats mode3 as 8-byte complex64; MRC specification uses two signed16 components (4 bytes).'}

def dmdata(name,info,raw):return b'\x15'+struct.pack('>H',len(name))+name.encode()+b'%%%%'+struct.pack('>I',len(info))+b''.join(struct.pack('>I',v) for v in info)+raw
def dmgroup(name,entries):return b'\x14'+struct.pack('>H',len(name))+name.encode()+b'\x01\x00'+struct.pack('>I',len(entries))+b''.join(entries)
for code,dtype in [(2,'int16'),(3,'int32'),(4,'uint16'),(5,'uint32'),(6,'float32'),(7,'float64'),(8,'int8')]:
 a=np.array([0,-0.,np.nan,np.inf,-np.inf,-1.25],dtype=dtype).reshape(2,3) if code in [6,7] else np.array([0,np.iinfo(dtype).min,np.iinfo(dtype).max,1,2,3],dtype=dtype).reshape(2,3)
 for order,endian in [('<','le'),('>','be')]:
  dim=dmgroup('Dimensions',[dmdata('',[5],struct.pack(order+'I',3)),dmdata('',[5],struct.pack(order+'I',2))])
  img=dmgroup('ImageData',[dmdata('Data',[20,code,6],a.astype(a.dtype.newbyteorder(order)).tobytes()),dim])
  size=dmdata('Active Size (pixels)',[15,0,2,0,3,0,3],struct.pack(order+'ii',3,2));binning=dmdata('Binning',[15,0,2,0,3,0,3],struct.pack(order+'ii',1,1))
  body=b'\x01\x00'+struct.pack('>I',3)+img+size+binning
  blob=struct.pack('>III',3,len(body)+4,1 if order=='<' else 0)+body+b'\0'*8
  name=f'dm3-{dtype}-{endian}.dm3';(root/name).write_bytes(blob);record(name,'dm3',a,True)
# Multiple SPE regions have explicit byte offsets; padding is not a pixel.
frames=np.arange(12,dtype='uint16').reshape(2,2,3)
regions=np.array([[100,200],[300,400]],dtype='uint16')
h=bytearray(4100)
for pos,value in [(42,3),(656,2),(108,3)]:struct.pack_into('<H',h,pos,value)
struct.pack_into('<i',h,1446,2);struct.pack_into('<f',h,1992,3.)
payload=b''.join(frames[i].tobytes()+b'PP'+regions[i].tobytes()+b'METADATA' for i in range(2))
struct.pack_into('<Q',h,678,4100+len(payload))
xml=b'<SpeFormat><DataFormat><DataBlock type="Frame" count="2" pixelFormat="MonochromeUnsigned16" size="18" stride="26"><DataBlock type="Region" count="1" width="3" height="2" size="12" stride="14"/><DataBlock type="Region" count="1" width="2" height="1" size="4" stride="4"/></DataBlock></DataFormat></SpeFormat>'
name='spe-v3-multiple-regions.spe';(root/name).write_bytes(h+payload+xml);record(name,'spe',frames,False)
manifest['files'][name]['second_region_sha256']=[hashlib.sha256(a.tobytes()).hexdigest() for a in regions]
# Byte/character arrays retain their octets; FabIO skips OCTET arrays entirely.
for code in [9,10]:
 a=np.array([0,255,128,1,2,3],dtype='uint8').reshape(2,3)
 dim=dmgroup('Dimensions',[dmdata('',[5],struct.pack('<I',3)),dmdata('',[5],struct.pack('<I',2))])
 body=b'\x01\x00'+struct.pack('>I',1)+dmgroup('ImageData',[dmdata('Data',[20,code,6],a.tobytes()),dim])
 blob=struct.pack('>III',3,len(body)+4,1)+body+b'\0'*8
 name=f'dm3-byte-type{code}.dm3';(root/name).write_bytes(blob);record(name,'dm3',a,False)
# Separate thumbnail and original stack, plus string/struct/nested-array tags.
for order,endian in [('<','le'),('>','be')]:
 frames=np.array([0,-2147483648,2147483647,1,2,3,4,5,6,7,8,9],dtype='int32').reshape(2,2,3)
 thumb=dmgroup('',[dmgroup('ImageData',[dmdata('Data',[20,4,1],struct.pack(order+'H',999)),dmgroup('Dimensions',[dmdata('',[5],struct.pack(order+'I',1))])])])
 dim=dmgroup('Dimensions',[dmdata('',[5],struct.pack(order+'I',n)) for n in [3,2,2]])
 main=dmgroup('',[dmgroup('ImageData',[dmdata('Data',[20,3,12],frames.astype(order+'i4').tobytes()),dim])])
 text=dmdata('Unicode',[18,3],'AμZ'.encode('utf-16le' if order=='<' else 'utf-16be'))
 struct_tag=dmdata('Named Struct',[15,3,1,2,3],b'abcxy'+struct.pack(order+'i',-123))
 nested=dmdata('Nested Array',[20,20,4,2,3],np.arange(6,dtype=order+'u2').tobytes())
 compound=dmdata('Struct Array',[20,15,0,2,0,3,0,6,2],struct.pack(order+'ifif',1,2.,3,4.))
 body=b'\x01\x00'+struct.pack('>I',5)+dmgroup('ImageList',[thumb,main])+text+struct_tag+nested+compound
 name=f'dm3-multiple-images-stack-{endian}.dm3';(root/name).write_bytes(struct.pack('>III',3,len(body)+4,1 if order=='<' else 0)+body+b'\0'*8);record(name,'dm3',frames,False)

(root/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
