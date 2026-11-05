"""Independent CC0 Esperanto/AGI bytes from explicit source arrays.
Tiny nonstandard shapes deliberately exercise raw stored-order input; FabIO's
file reader pads such shapes, so its AGI decoder is the independent byte oracle.
"""
import hashlib,json,pathlib,struct
import numpy as np, fabio
from fabio.compression.agi_bitfield import decompress
root=pathlib.Path(__file__).resolve().parents[1]/'tests/data/esperanto'
manifest={'license':'CC0-1.0','fabio_version':fabio.version,'files':{}}
def escaped(v):
 if -127<=v<=126:return bytes([v+127])
 if -32768<=v<=32767:return b'\xfe'+struct.pack('<h',v)
 return b'\xff'+struct.pack('<i',v)
def packed(values,width):
 bias=(1<<(width-1))-1
 if width<8:
  assert all(-bias<=v<=(1<<width)-1-bias for v in values)
  n=sum((v+bias)<<(i*width) for i,v in enumerate(values));return n.to_bytes(width,'little'),b''
 raw=bytearray();extra=bytearray()
 for v in values:
  if -127<=v<=126:raw.append(v+127)
  else:
   enc=escaped(v);raw.append(enc[0]);extra.extend(enc[1:])
 return bytes(raw),bytes(extra)
def agi(a,width):
 rows=[];offsets=[];position=0
 for row in a:
  diffs=np.diff(row).astype('int64').tolist();data=bytearray(escaped(int(row[0])))
  for j in range(len(diffs)//16):
   group=diffs[j*16:j*16+16];pa,ea=packed(group[:8],width);pb,eb=packed(group[8:],width);data.append(width|(width<<4));data.extend(pa+pb+ea+eb)
  for v in diffs[len(diffs)//16*16:]:data.extend(escaped(v))
  offsets.append(position);position+=len(data);rows.append(data)
 return struct.pack('<I',position)+b''.join(rows)+b''.join(struct.pack('<I',o) for o in offsets)
def header(a,encoding):
 lines=[f'ESPERANTO FORMAT 1 CONSISTING OF 25 LINES OF 256 BYTES EACH',f'IMAGE {a.shape[1]} {a.shape[0]} 1 1 "{encoding}"','TIMESTAMP "2026-11-05 12:34:56"','HISTORY "Original stored rows"','UNKNOWN_FIELD "Never discard original metadata"']+['']*20
 return b''.join(s.encode().ljust(254,b' ')+(b'\r\x1a' if i==24 else b'\r\n') for i,s in enumerate(lines))
def write(name,a,encoding,width=8):
 raw=a.astype('<i4').tobytes() if encoding=='4BYTE_LONG' else agi(a,width)
 p=root/name;p.write_bytes(header(a,encoding)+raw)
 ref=np.frombuffer(raw,dtype='<i4').reshape(a.shape) if encoding=='4BYTE_LONG' else decompress(raw,a.shape)
 assert np.array_equal(ref.astype('int32'),a)
 manifest['files'][name]={'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'shape':list(a.shape),'encoding':encoding,'native_sha256':hashlib.sha256(a.astype('<i4').tobytes()).hexdigest(),'intermediate_dtype':ref.dtype.name,'intermediate_sha256':hashlib.sha256(ref.astype(ref.dtype.newbyteorder('<')).tobytes()).hexdigest()}
for width in range(1,9):
 bias=(1<<(width-1))-1
 differences=([0,1]*8 if width==1 else [-bias,1,0,2,-1,1,0,(1<<(width-1))]*2)*2
 a=np.array([np.cumsum([0]+differences),np.cumsum([123]+differences)],dtype='int32')
 write(f'agi-field{width}.esperanto',a,'AGI_BITFIELD',width)
for columns in [1,2,16,17,18,34]:
 pattern=np.array([-2147483648,2147483647,0,-32768,32767,-128,127,126,-1,0,1,-2,2,-3,3,65535,32768],dtype='int32')
 a=np.resize(pattern,(2,columns));write(f'agi-extrema-cols{columns}.esperanto',a,'AGI_BITFIELD');write(f'long32-extrema-cols{columns}.esperanto',a,'4BYTE_LONG')
(root/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
