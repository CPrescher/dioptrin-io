"""CC0 synthetic detector arrays; independent FabIO/NumPy/reference codecs.
Requires fabio 2026.6.0 only for fixture generation and independent verification.
"""
import bz2,gzip,zlib,hashlib,json,pathlib
import numpy as np
import fabio
from fabio.cbfimage import CbfImage
root=pathlib.Path(__file__).resolve().parents[1]/'tests/data'
manifest={'license':'CC0-1.0','fabio':fabio.version,'numpy':np.__version__,'files':{}}
for dtype in ['uint8','uint16','uint32','uint64','int8','int16','int32','int64']:
 info=np.iinfo(dtype)
 values=[0,1,127,min(info.max,32768),min(info.max,2**31),min(info.max,2**53+1)]
 if info.min<0:values[1]=info.min if dtype!='int64' else -2**40
 a=np.array(values,dtype=dtype).reshape(2,3)
 path=root/'cbf'/(dtype+'.cbf');CbfImage(data=a,header={'_array_data.header_contents':'# Original header\n# No flips'}).write(path)
 ref=fabio.open(path).data
 assert np.array_equal(a,ref),(dtype,a,ref)
 manifest['files']['cbf/'+path.name]={'file_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'dtype':str(ref.dtype),'values':[int(x) for x in ref.ravel()]}

types={'uint8':'UnsignedByte','uint16':'UnsignedShort','uint32':'UnsignedInteger','uint64':'Unsigned64','int8':'SignedByte','int16':'SignedShort','int32':'SignedInteger','int64':'Signed64','float32':'FloatValue','float64':'DoubleValue'}
def frame(a,order='<',compression='NONE'):
 payload=a.astype(a.dtype.newbyteorder(order)).tobytes()
 payload={'NONE':lambda x:x,'GZIP':lambda x:gzip.compress(x,mtime=0),'ZLIB':zlib.compress,'BZ2':bz2.compress}[compression](payload)
 h='{\nHeaderID = EH:000001:000000:000000;\nImage = 1;\n'
 h+=f'ByteOrder = {"LowByteFirst" if order=="<" else "HighByteFirst"};\nDataType = {types[a.dtype.name]};\nDim_1 = {a.shape[1]};\nDim_2 = {a.shape[0]};\nSize = {len(payload)};\nCompression = {compression};\nEDF_HeaderSize = 512;\nUserValue = Original header;\n'
 return h.encode()+b' '*(512-len(h)-2)+b'}\n'+payload

for dtype in types:
 if dtype.startswith('float'): a=np.array([0.,-0.,np.nan,np.inf,-np.inf,-1.25],dtype=dtype).reshape(2,3)
 else:
  info=np.iinfo(dtype);a=np.array([0,info.min,info.max,2,3,4],dtype=dtype).reshape(2,3)
 for order,label in [('<','le'),('>','be')]:
  path=root/'edf'/(dtype+'-'+label+'.edf');path.write_bytes(frame(a,order))
  ref=fabio.open(path).data
  assert ref.dtype==a.dtype and np.array_equal(ref.view('u1'),a.view('u1'))
  manifest['files']['edf/'+path.name]={'file_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'sample_sha256':hashlib.sha256(a.tobytes()).hexdigest(),'dtype':str(a.dtype)}
a=np.arange(12,dtype='uint32').reshape(3,4)*70000
path=root/'edf'/'compressed-stack.edf';path.write_bytes(b''.join(frame(a+i,'>',c) for i,c in enumerate(['GZIP','ZLIB','BZ2'])))
ref=fabio.open(path);assert ref.nframes==3
for i in range(3):assert np.array_equal(ref.getframe(i).data,a+i)
manifest['files']['edf/compressed-stack.edf']={'file_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'codecs':['GZIP','ZLIB','BZ2'],'shape':[3,3,4]}
# Legacy size/compression spellings and binary padding observed in FabIO's
# upstream corpus, reproduced independently with tiny CC0 source arrays.
a=np.arange(6,dtype='uint16').reshape(2,3)*11000
for name,compression,alias in [('gzip-alias','GZIP','GzipCompression'),('zlib-alias','ZLIB','ZCompression'),('bzip2-alias','BZ2','BZ2')]:
 data=frame(a,'<',compression).replace(('Compression = '+compression).encode(),('Compression = '+alias).encode())
 # Rebuild the 512-byte header because the alias has a different text length.
 marker=data.index(b'}\n')+2
 body=data[:marker].rstrip(b'\n').rstrip(b'}').rstrip()
 data=body+b' '*(512-len(body)-2)+b'}\n'+data[marker:]
 path=root/'edf'/(name+'.edf');path.write_bytes(data)
 assert np.array_equal(fabio.open(path).data,a)
 manifest['files']['edf/'+path.name]={'file_sha256':hashlib.sha256(data).hexdigest(),'dtype':'uint16','compression_alias':alias}
data=frame(a)
text=data[:512].decode().replace('Size = 12;','')
body=text[:text.index('}')].rstrip().encode()
data=body+b' '*(512-len(body)-2)+b'}\n'+data[512:]
path=root/'edf'/'optional-size.edf';path.write_bytes(data)
assert np.array_equal(fabio.open(path).data,a)
manifest['files']['edf/'+path.name]={'file_sha256':hashlib.sha256(data).hexdigest(),'dtype':'uint16','size':'inferred from dimensions, uncompressed only'}
data=frame(a);text=data[:512].decode().replace('Size = 12;','Size = 32;').replace('UserValue = Original header;','UserValue = Original header;\nEDF_BinarySize = 32;')
body=text[:text.index('}')].rstrip().encode()
data=body+b' '*(512-len(body)-2)+b'}\n'+data[512:]+bytes([0xab])*20
path=root/'edf'/'binary-padding.edf';path.write_bytes(data)
assert np.array_equal(fabio.open(path).data,a)
manifest['files']['edf/'+path.name]={'file_sha256':hashlib.sha256(data).hexdigest(),'dtype':'uint16','binary_padding_bytes':20}
(root/'binary-provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
