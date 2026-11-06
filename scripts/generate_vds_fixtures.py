#!/usr/bin/env python3
"""Generate CC0 frozen-VDS source-contract fixtures with independent h5py.
No production reader or upstream detector file generates these explicit arrays.
"""
import hashlib
import json
from pathlib import Path
import h5py
import numpy as np
ROOT=Path(__file__).resolve().parents[1]/'tests/data/vds'
ROOT.mkdir(parents=True,exist_ok=True)
A='/missing/acquisition/module-a.h5';B='/missing/acquisition/module-b.h5'

def module(name,values,dtype='<i2',chunks=None,maxshape=None):
 with h5py.File(ROOT/name,'w')as f:f.create_dataset('data',data=np.array(values,dtype=dtype),chunks=chunks,maxshape=maxshape)
def source(filename,shape,unlimited=False):
 return h5py.VirtualSource(filename,'/data',shape=(0,*shape[1:])if unlimited else shape,maxshape=(None,*shape[1:])if unlimited else None)
def master(name,dtype='<i2',fill=-32768,unlimited=False,overlap=False,full=False):
 shape=(2,2,6)if full else(2,4,6)
 layout=h5py.VirtualLayout(shape=(0,*shape[1:])if unlimited else shape,maxshape=(None,*shape[1:])if unlimited else None,dtype=dtype)
 if unlimited:
  for filename,y,x,h,w in [(A,0,1,2,3),(B,3,3,1,2)]:
   vspace=h5py.h5s.create_simple((0,4,6),(h5py.h5s.UNLIMITED,4,6))
   sspace=h5py.h5s.create_simple((0,0,0),(h5py.h5s.UNLIMITED,)*3)
   vspace.select_hyperslab((0,y,x),(1,1,1),block=(h5py.h5s.UNLIMITED,h,w))
   sspace.select_hyperslab((0,0,0),(1,1,1),block=(h5py.h5s.UNLIMITED,h,w))
   layout.dcpl.set_virtual(vspace,filename.encode(),b'/data',sspace)
 else:
  layout[:,0:2,1:4]=source(A,(2,2,3))
  if overlap:layout[:,1:2,2:4]=source(B,(2,1,2))
  else:layout[:,3:4,3:5]=source(B,(2,1,2))
 with h5py.File(ROOT/name,'w',libver='latest')as f:
  f.create_virtual_dataset('data',layout,fillvalue=fill)
  d=f.require_group('/entry/instrument/detector');d['description']=np.bytes_('Lambda');d['data']=f['data']

av=np.arange(12,dtype='<i2').reshape(2,2,3)-5;bv=np.array([[[100,200]],[[300,400]]],dtype='<i2')
module('module-a.h5',av);module('module-b.h5',bv)
module('wrong-dtype.h5',av.astype('f4'),'<f4');module('short-shape.h5',av[:,:1,:]);module('wrong-frames.h5',bv[:1])
module('growing-a.h5',av,chunks=(1,2,3),maxshape=(None,2,3))
with h5py.File(ROOT/'unallocated-a.h5','w')as f:f.create_dataset('data',shape=(2,2,3),chunks=(1,2,3),dtype='<i2')
with h5py.File(ROOT/'partial-a.h5','w')as f:
 d=f.create_dataset('data',shape=(2,2,3),chunks=(1,2,3),dtype='<i2');d[0]=av[0]
module('stride-a.h5',np.arange(24).reshape(2,4,3))
master('finite.h5');master('unlimited.h5',unlimited=True);master('overlap.h5',overlap=True)
module('growing-b.h5',bv,chunks=(1,1,2),maxshape=(None,1,2))
with h5py.File(ROOT/'multi-vds.h5','w',libver='latest')as f:
 group=f.require_group('/entry/data')
 for name,filename,array in [('data_000001',A,av),('data_000002',B,bv)]:
  layout=h5py.VirtualLayout(shape=array.shape,dtype='<i2');layout[:]=h5py.VirtualSource(filename,'/data',shape=array.shape)
  group.create_virtual_dataset(name,layout)
 group.create_dataset('data_000003',data=bv,chunks=(1,1,2),maxshape=(None,1,2))
# Strict source hyperslab disallows a gap between selected module rows.
layout=h5py.VirtualLayout(shape=(2,2,3),dtype='<i2');layout[:]=h5py.VirtualSource(A,'/data',shape=(2,4,3))[:,::2,:]
with h5py.File(ROOT/'strided.h5','w',libver='latest')as f:f.create_virtual_dataset('data',layout,fillvalue=0)
# A frame-number shift must never be interpreted as a matching mosaic.
layout=h5py.VirtualLayout(shape=(3,2,3),dtype='<i2');layout[1:3]=h5py.VirtualSource(A,'/data',shape=(2,2,3))
with h5py.File(ROOT/'shifted.h5','w',libver='latest')as f:f.create_virtual_dataset('data',layout,fillvalue=0)
# Explicit logical fill is still unsafe if its creation policy disables fill.
with h5py.File(ROOT/'finite.h5')as old:
 layout=h5py.VirtualLayout(shape=(2,4,6),dtype='<i2');layout[:,0:2,1:4]=h5py.VirtualSource(A,'/data',shape=(2,2,3));layout[:,3:4,3:5]=h5py.VirtualSource(B,'/data',shape=(2,1,2))
 dcpl=layout.dcpl.copy();dcpl.set_fill_time(h5py.h5d.FILL_TIME_NEVER)
 with h5py.File(ROOT/'never-fill.h5','w',libver='latest')as f:h5py.h5d.create(f.id,b'data',h5py.h5t.STD_I16LE,h5py.h5s.create_simple((2,4,6)),dcpl=dcpl)
# Master spatial extents govern logical fill; fixed extra frame extents
# are outside the initial full-frame-preserving mosaic contract.
with h5py.File(ROOT/'finite.h5')as old:
 for name,shape in [('spatial-extra-fill.h5',(2,5,7)),('frame-master-mismatch.h5',(3,4,6))]:
  with h5py.File(ROOT/name,'w',libver='latest')as f:
   h5py.h5d.create(f.id,b'data',h5py.h5t.STD_I16LE,h5py.h5s.create_simple(shape),dcpl=old['data'].id.get_create_plist())
# Integer extrema and a fill scalar that cannot pass through float64 exactly.
u=np.array([[[2**64-1,2**63,2**53+1],[7,8,9]],[[2**64-2,2**53+3,0],[2**63+1,11,12]]],dtype='<u8')
module('module-u64.h5',u,'<u8');layout=h5py.VirtualLayout(shape=(2,2,4),dtype='<u8');layout[:,:,:3]=h5py.VirtualSource(A,'/data',shape=u.shape)
with h5py.File(ROOT/'uint64-fill.h5','w',libver='latest')as f:f.create_virtual_dataset('data',layout,fillvalue=np.uint64(2**64-1))
# Complete coverage can be selected without any logical fill policy.
layout=h5py.VirtualLayout(shape=av.shape,dtype='<i2');layout[:]=h5py.VirtualSource(A,'/data',shape=av.shape)
with h5py.File(ROOT/'full.h5','w',libver='latest')as f:f.create_virtual_dataset('data',layout)
# Source graph: explicit bound file -> external relay -> immutable concrete file.
with h5py.File(ROOT/'link-target.h5','w')as f:f['data']=av
with h5py.File(ROOT/'link-relay.h5','w')as f:f['data']=h5py.ExternalLink('link-target.h5','/data')
with h5py.File(ROOT/'link-bound.h5','w')as f:f['data']=h5py.ExternalLink('link-relay.h5','/data')
# Metadata may not bypass explicit VDS source admission through detector text.
with h5py.File(ROOT/'text-source.h5','w')as f:f['description']=np.array([b'Lambda'],dtype='S6')
text_layout=h5py.VirtualLayout(shape=(1,),dtype='S6');text_layout[:]=h5py.VirtualSource('text-source.h5','/description',shape=(1,))
with h5py.File(ROOT/'metadata-vds.h5','w',libver='latest')as f:
 d=f.require_group('/entry/instrument/detector');d.create_virtual_dataset('description',text_layout);d['data']=av
mosaic_expected=np.full((2,4,6),-32768,dtype='<i2');mosaic_expected[:,0:2,1:4]=av;mosaic_expected[:,3:4,3:5]=bv
u_expected=np.full((2,2,4),2**64-1,dtype='<u8');u_expected[:,:,:3]=u
# Shared scatter/fill must preserve every supported scalar bit pattern.
scalar_cases={}
with h5py.File(ROOT/'scalars-source.h5','w')as source_file, h5py.File(ROOT/'scalars-master.h5','w',libver='latest')as master_file:
 for name,dtype,values,fill in [
  ('u8','u1',[0,255,128,1,2,3,4,5],255),('u16','<u2',[0,65535,32768,1,2,3,4,5],65535),
  ('u32','<u4',[0,2**32-1,2**31,1,2,3,4,5],2**32-1),('u64','<u8',[0,2**64-1,2**63,2**53+1,2,3,4,5],2**64-1),
  ('i8','i1',[-128,127,-1,0,1,2,3,4],-128),('i16','<i2',[-32768,32767,-1,0,1,2,3,4],-32768),
  ('i32','<i4',[-2**31,2**31-1,-1,0,1,2,3,4],-2**31),('i64','<i8',[-2**63,2**63-1,-2**53-1,0,1,2,3,4],-2**63),
  ('f32','<f4',None,None),('f64','<f8',None,None),('be-u16','>u2',[0,65535,32768,1,2,3,4,5],65535)]:
  if name=='f32':
   array=np.array([0x80000000,0x7fa01234,0x7fc05678,0x7f800000,0xff800000,0,0x3fc00000,0xc0200000],dtype='<u4').view('<f4')
   fill=np.array([0x7fc0abcd],dtype='<u4').view('<f4')[0]
  elif name=='f64':
   array=np.array([0x8000000000000000,0x7ff0000000001234,0x7ff8000000005678,0x7ff0000000000000,0xfff0000000000000,0,0x3ff8000000000000,0xc004000000000000],dtype='<u8').view('<f8')
   fill=np.array([0x8000000000000000],dtype='<u8').view('<f8')[0]
  else:array=np.array(values,dtype=dtype)
  array=array.reshape(2,2,2);source_file[name]=array
  layout=h5py.VirtualLayout(shape=(2,2,3),dtype=dtype);layout[:,:,1:3]=h5py.VirtualSource(A,'/'+name,shape=array.shape)
  result=master_file.create_virtual_dataset(name,layout,fillvalue=fill)
  # Read back only the original scalar fill declaration, never VDS data.
  declared_fill=result.fillvalue
  expected=np.full((2,2,3),declared_fill,dtype=dtype);expected[:,:,1:3]=array
  canonical=expected.astype(expected.dtype.newbyteorder('<'),copy=False)
  raw=canonical.tobytes();(ROOT/(name+'.raw')).write_bytes(raw)
  scalar_cases[name]=dict(dtype=dtype,frame_bytes=len(raw)//2,expected_raw=name+'.raw',fill_hex=np.array([declared_fill],dtype=dtype).astype(np.dtype(dtype).newbyteorder('<'),copy=False).tobytes().hex())
(ROOT/'provenance.json').write_text(json.dumps(dict(license='CC0-1.0',generator='scripts/generate_vds_fixtures.py',
 source='Independent explicit native arrays and regular HDF5 virtual mapping declarations; no detector/reference writer input',
 module_filenames=[A,B],scalar_cases=scalar_cases,expected_i16=mosaic_expected.tolist(),expected_u64=u_expected.tolist(),
 files={p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in ROOT.glob('*.h5')},
 frame_sha256=[hashlib.sha256(frame.tobytes()).hexdigest()for frame in mosaic_expected]),indent=2)+'\n')
