"""CC0 tiny GE fixtures and independent runtime-Pixi source hashes.
FabIO is an independent reference only, never a production dependency.
"""
import hashlib,json,pathlib,struct,tempfile
import numpy as np, fabio
from fabio.GEimage import GeImage
from fabio.pixiimage import PixiImage
root=pathlib.Path(__file__).resolve().parents[1]/'tests/data/fixed'
manifest={'license':'CC0-1.0','fabio_version':fabio.version,'ge':{},'pixi':{}}
for depth,dtype in [(8,'uint8'),(16,'uint16'),(32,'uint32')]:
 frames=np.array([0,np.iinfo(dtype).max,np.iinfo(dtype).max//2+1,1,2,3,4,5,6,7,8,9],dtype=dtype).reshape(2,2,3)
 h=bytearray(3332);h[:10]=b'ADEPT\0\0\0\0\0';struct.pack_into('<H',h,10,8001);struct.pack_into('<I',h,12,len(h));struct.pack_into('<I',h,18,16)
 for offset,value in [(22,2),(24,2),(26,3),(28,depth),(288,1),(290,1),(292,1),(294,1)]:struct.pack_into('<H',h,offset,value)
 h[30:50]=b'2026-11-05\0\0\0\0\0\0\0\0\0\0';h[50:70]=b'12:34:56\0\0\0\0\0\0\0\0\0\0\0\0'
 name=f'ge-uint{depth}.ge';p=root/name;p.write_bytes(h+b'ORIGINAL USERHDR'+frames.tobytes())
 hashes=[]
 for i,frame in enumerate(frames):
  ref=GeImage().read(p,frame=i);assert ref.data.dtype.name==dtype and ref.data.tobytes()==frame.tobytes();hashes.append(hashlib.sha256(frame.tobytes()).hexdigest())
 manifest['ge'][name]={'shape':list(frames.shape),'dtype':dtype,'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'frame_sha256':hashes}
# Fixed Pixi dimensions require ~0.5 MB/record. Generate bytes at test runtime;
# retain only independently computed complete file/frame hashes in Git.
with tempfile.TemporaryDirectory() as tmp:
 for count in [1,3]:
  arrays=[];records=[]
  for i in range(count):
   a=((np.arange(512*476,dtype='uint32')*17+i*1009)%65536).astype('uint16').reshape(512,476)
   a[0,:3]=[0,65535,32768]
   header=struct.pack('<i',243722)+bytes((j+i)%256 for j in range(20))
   records.append(header+a.tobytes());arrays.append(a)
  path=pathlib.Path(tmp)/f'pixi-{count}.dat';path.write_bytes(b''.join(records))
  for i,a in enumerate(arrays):
   ref=PixiImage().read(str(path),frame=i);assert ref.data.dtype.name=='uint16' and ref.data.tobytes()==a.tobytes()
  manifest['pixi'][str(count)]={'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'frame_sha256':[hashlib.sha256(a.tobytes()).hexdigest() for a in arrays]}
(root/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
