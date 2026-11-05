"""Independent CC0 MAR345/PCK V1 source arrays and separate simple bit packer.
The external pinned FabIO MIT decoder is an oracle only, never a runtime backend.
"""
import hashlib,json,pathlib,struct
import numpy as np,fabio
from fabio.mar345image import Mar345Image
root=pathlib.Path(__file__).resolve().parents[1]/'tests/data/mar345'
manifest={'license':'CC0-1.0','fabio':fabio.version,'numpy':np.__version__,'files':{}}
WIDTHS=[0,4,5,6,7,8,16,32]
def signed16(n):return (int(n)+32768)%65536-32768
def predictor_differences(a):
 base=[signed16(v) for v in a.flat];width=a.shape[1];out=[]
 for i,v in enumerate(base):
  if i==0:out.append(v)
  elif i<=width:out.append(v-base[i-1])
  else:
   s=base[i-1]+base[i-width-1]+base[i-width]+base[i-width+1]+2
   pred=(abs(s)//4)*(-1 if s<0 else 1)
   out.append(signed16(v-pred))
 return out
def pack(differences,forced=None,run=1):
 accumulator=0;used=0
 for start in range(0,len(differences),run):
  values=differences[start:start+run];power=(len(values).bit_length()-1);assert len(values)==1<<power and power<=7
  if forced is not None:width=forced
  elif all(v==0 for v in values):width=0
  else:width=next(w for w in WIDTHS[1:] if all(-(1<<(w-1))<=v<(1<<(w-1)) for v in values))
  accumulator|=(power|(WIDTHS.index(width)<<3))<<used;used+=6
  for value in values:
   assert width or value==0
   if width:accumulator|=(value&((1<<width)-1))<<used;used+=width
 return accumulator.to_bytes((used+7)//8,'little')
def write(name,a,order='<',compact=False,forced=None,run=1,extended=False,format_kind="PCK",marker_crlf=False):
 a=np.asarray(a,dtype='uint32');high=[(i+1,int(v)) for i,v in enumerate(a.flat) if v>65535]
 h=bytearray(4096);words=[1234,a.shape[1],len(high),1,1,a.size,150,151,1234567,234567,-123,456,789,-999,1000,2000]
 struct.pack_into(order+'16i',h,0,*words)
 text_start=128 if extended else 64
 if extended:h[64:80]=bytes([0,255,128,1,2,3,4,5,6,7,8,9,10,11,12,13])
 text=('mar research\nPROGRAM Independent CC0 fixture\nDATE 2026-11-05\nFORMAT %d %s %d\nHIGH %d\nPIXEL LENGTH 150 HEIGHT 151\nREMARK Original stored rows; no flip\nUNKNOWN reserved metadata\nEND OF HEADER\n'%(a.shape[1],format_kind,a.size,len(high))).encode()
 h[text_start:text_start+len(text)]=text
 records=b''.join(struct.pack(order+'II',i,v) for i,v in high)
 if not compact:
  records+=b''.join(struct.pack(order+'II',0,0) for _ in range((-len(high))%8))
 data=pack(predictor_differences(a),forced,run);marker=('\nCCP4 packed image, X: %04d, Y: %04d\n'%(a.shape[1],a.shape[0])).encode()
 if marker_crlf:marker=marker.replace(b'\n',b'\r\n')
 p=root/name;p.write_bytes(h+records+marker+data)
 # Qualified oracle defects include binary whitespace trimming, signed first
 # rows and the pinned compiled decoder's overflowing32-bit field mask.
 leading_whitespace=data[:1] in [b' ',b'\t',b'\r',b'\n',b'\x0b',b'\x0c']
 compared=False;reference_hash=None
 differences=predictor_differences(a)
 used32=(forced==32 and any(differences)) or any(v < -32768 or v > 32767 for v in differences)
 first_sign=any(32768<=v<=65535 for v in a.flat[:a.shape[1]+1])
 if not leading_whitespace:
  reader=Mar345Image();reader.byteorder=order;ref=reader.read(str(p)).data
  reference_hash=hashlib.sha256(ref.astype('<u4').tobytes()).hexdigest()
  compared=np.array_equal(ref,a)
  if not compared:
   assert used32 or first_sign
   if not used32:assert np.array_equal(ref.flat[a.shape[1]+1:],a.flat[a.shape[1]+1:])
 record={'shape':list(a.shape),'dtype':'uint32','sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'sample_sha256':hashlib.sha256(a.astype('<u4').tobytes()).hexdigest(),'little_endian':order=='<','overflow_layout':'compact' if compact and len(high)%8 else 'padded8','high':len(high),'fabio_file_equal':bool(compared),'fabio_sample_sha256':reference_hash}
 if leading_whitespace:record['reference_difference']='Pinned uncompress_pck.lstrip() consumes a valid leading packed whitespace byte; reference decoder not called on this case.'
 elif not compared:record['reference_difference']='Pinned unpack_pck loses32-bit fields through C integer-mask overflow.' if used32 else 'Pinned postdec sign-extends first width+1 signed16 values intoU32; native preserves unsigned16 source counts.'
 manifest['files'][name]=record
base=np.array([[0,1,7,15,63],[100,128,255,256,1024],[32767,32768,65535,0,17],[2,3,4,5,6]],dtype='uint32')
for order,endian in [('<','le'),('>','be')]:
 write(f'base-{endian}.mar345',base,order,extended=True)
 a=base.copy();a.flat[[0,7,16,19]]=[65536,999999,2147483647,4294967295]
 write(f'overflow-padded-{endian}.mar345',a,order)
 write(f'overflow-compact-{endian}.mar345',a,order,compact=True,extended=True)
write('marker-crlf.mar345',base,marker_crlf=True)
for kind in ['MAR345','PCK345']:write(f'format-{kind}.mar345',base,format_kind=kind)
first=base.copy();first.flat[:6]=np.arange(32768,32774,dtype='uint32');write('first-row-unsigned.mar345',first)
for width in [4,5,6,7,8,16,32]:
 a=np.zeros((4,8),dtype='uint32');write(f'field{width}.mar345',a,forced=width)
write('field32-nonzero.mar345',np.arange(32,dtype='uint32').reshape(4,8),forced=32)
for power in range(8):write(f'zero-run{1<<power}.mar345',np.zeros((16,16),dtype='uint32'),run=1<<power)
(root/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
