#!/usr/bin/env python3
"""CC0-1.0 independent sparse HDF5 arrays and explicit expected dense views.
No FabIO/pyFAI writer, stochastic generation or upstream counts are used.
"""
import copy
import hashlib
import json
from pathlib import Path
import h5py
import numpy as np
ROOT=Path(__file__).resolve().parents[1]/'tests/data/sparse'
ROOT.mkdir(parents=True,exist_ok=True)
manifest=dict(license='CC0-1.0',generator='scripts/generate_sparse_fixtures.py',h5py=h5py.__version__,files={},invalid={})
def sha(data):return hashlib.sha256(data).hexdigest()
def canonical(array):return np.ascontiguousarray(array.astype(array.dtype.newbyteorder('<'),copy=False)).tobytes()
def record(path,arrays,dense,**extra):
    manifest['files'][path.name]=dict(file_sha256=sha(path.read_bytes()),components={name:dict(shape=list(a.shape),dtype=a.dtype.name,little_endian=a.dtype.byteorder!='>',sha256=sha(canonical(a)))for name,a in arrays.items()},dense=[dict(dtype=a.dtype.name,shape=list(a.shape),sha256=sha(canonical(a)))for a in dense],**extra)
def create(path,arrays):
    with h5py.File(path,'w')as f:
        f.attrs['default']='/entry';f.attrs['NX_class']='NXroot'
        e=f.create_group('entry');e.attrs['default']='plot';e.attrs['pyFAI_sparse_frames']='data';e.attrs['description']='Independent μ counts'
        g=e.create_group('data');g.attrs['dataformat']='sparsify-Bragg';g.attrs['axes']=np.array(['y','x'],dtype=h5py.string_dtype())
        e['plot']=h5py.SoftLink('/entry/data')
        for name,a in arrays.items():
            kwargs=dict(chunks=(1,a.shape[1]),compression='gzip',compression_opts=1,shuffle=True,fletcher32=True)if name.startswith('background_') else {}
            if name in ('index','intensity')and a.size==0:kwargs=dict(chunks=(16,),maxshape=(None,))
            d=g.create_dataset(name,data=a,**kwargs);d.attrs['original_shape']=np.array(a.shape,dtype='>u8')
        c=e.create_group('sparsify').create_group('configuration');c.create_dataset('data',data='{"sparsify":{"cutoff_pick":5.5},"source":"independent"}')
        p=e.create_group('peaks');p.create_dataset('position',data=np.array([[1,2],[3,4]],dtype='>i4'))
mask=np.array([[0,.25,1,1.5],[2,3,np.nan,-1]],dtype='>f8')
arrays=dict(mask=mask,radius=np.array([0,1,2],dtype='>f8'),background_avg=np.array([[-2,-1,2],[1,2,4]],dtype='<f4'),background_std=np.ones((2,3),dtype='<f4')*.25,normalization=np.array([[1.25,2,1.5,.5],[1,2,1,1]],dtype='>f8'),frame_ptr=np.array([0,2,4],dtype='>u8'),index=np.array([6,1,0,4],dtype='>u4'),intensity=np.array([-7,123,-1,32767],dtype='>i2'),dummy=np.array(-32768,dtype='>i2'))
dense=[np.array([[-3,123,-2,0],[2,-32768,-7,-32768]],dtype='int16'),np.array([[-1,3,3,2],[32767,-32768,-32768,-32768]],dtype='int16')]
p=ROOT/'signed-model.h5';create(p,arrays);record(p,arrays,dense,reference_gap='Pinned native densifier rejects non-native endian source buffers')
le={k:a.astype(a.dtype.newbyteorder('<'))for k,a in arrays.items()};p=ROOT/'signed-model-le.h5';create(p,le);record(p,le,dense)
# No background means the explicit native reference model starts from zero,
# including masked positions; stored event words (NaN payload/-0) are copied last.
intensity=np.array([1.25,0,-0.,np.inf],dtype='>f8');intensity[1]=np.array([0x7ff8000000001234],dtype='>u8').view('>f8')[0]
arrays=dict(mask=np.array([[0,np.nan,1],[2,3,4]],dtype='<f4'),frame_ptr=np.array([0,2,4],dtype='<i4'),index=np.array([2,1,4,5],dtype='>u8'),intensity=intensity,normalization=np.ones((2,3),dtype='<u2'))
d0=np.array([[0,0,1.25],[0,0,0]],dtype='float64');d0[0,1]=intensity[1];d1=np.array([[0,0,0],[0,-0.,np.inf]],dtype='float64')
p=ROOT/'float-events.h5';create(p,arrays);record(p,arrays,[d0,d1],reference_gap='Pinned native densifier rejects non-native endian source buffers and uint64 indices')
le={k:a.astype(a.dtype.newbyteorder('<'))for k,a in arrays.items()};le['index']=le['index'].astype('uint32');p=ROOT/'float-events-le.h5';create(p,le);record(p,le,[d0,d1])
arrays=dict(mask=np.arange(4,dtype='float32').reshape(1,4),frame_ptr=np.array([0,4],dtype='uint32'),index=np.arange(4,dtype='uint32'),intensity=np.array([0,2**64-1,2**63,2**53+1],dtype='>u8'),dummy=np.array(2**64-1,dtype='>u8'))
p=ROOT/'uint64-events.h5';create(p,arrays);record(p,arrays,[arrays['intensity'].astype('uint64').reshape(1,4)],reference_gap='Pinned native densifier rejects non-native endian source buffers')
arrays=dict(mask=np.array([[0,.25,.5],[1,1.5,np.nan]],dtype='float32'),radius=np.array([0,.5,1],dtype='float32'),background_avg=np.array([[0,4,8],[8,4,0]],dtype='float32'),background_std=np.ones((2,3),dtype='float32'),frame_ptr=np.array([0,0,0],dtype='uint32'),index=np.array([],dtype='uint32'),intensity=np.array([],dtype='float32'))
p=ROOT/'empty-model.h5';create(p,arrays);record(p,arrays,[np.array([[0,2,4],[8,np.nan,np.nan]],dtype='float32'),np.array([[8,6,4],[0,np.nan,np.nan]],dtype='float32')])
base=dict(mask=np.arange(6,dtype='float32').reshape(2,3),frame_ptr=np.array([0,2,3],dtype='uint32'),index=np.array([1,4,1],dtype='uint32'),intensity=np.array([11,22,33],dtype='uint16'))
for name,key,value in [
 ('pointer-origin','frame_ptr',np.array([1,2,3],dtype='uint32')),
 ('pointer-final','frame_ptr',np.array([0,2,2],dtype='uint32')),
 ('pointer-order','frame_ptr',np.array([0,3,2,3],dtype='uint32')),
 ('pointer-float','frame_ptr',np.array([0,2,3],dtype='float32')),
 ('negative-index','index',np.array([-1,4,1],dtype='int32')),
 ('outside-index','index',np.array([6,4,1],dtype='uint32')),
 ('duplicate-index','index',np.array([1,1,1],dtype='uint32')),
 ('float-index','index',np.array([1,4,1],dtype='float32')),
 ('index-length','index',np.array([1,4],dtype='uint32')),
 ('normalization-shape','normalization',np.ones((3,2),dtype='float32')),
 ('radius-order','radius',np.array([0,2,1],dtype='float32')),
 ('background-shape','background_avg',np.ones((1,3),dtype='float32'))]:
    case=copy.deepcopy(base);case[key]=value;p=ROOT/f'invalid-{name}.h5';create(p,case);manifest['invalid'][p.name]=dict(file_sha256=sha(p.read_bytes()),failure='open')
# Lossy/unsupported storage, undefined fill and link layouts must be rejected
# through the shared native HDF5 admission path before sparse interpretation.
for mode in ['scale-offset','virtual','external-raw','undefined-fill','custom-precision','link-cycle','missing-link']:
    p=ROOT/f'invalid-{mode}.h5';create(p,base)
    with h5py.File(p,'r+')as f:
        g=f['entry/data']
        if mode in ['scale-offset','virtual','external-raw','undefined-fill','custom-precision']:
            del g['intensity']
            if mode=='scale-offset':g.create_dataset('intensity',data=base['intensity'],chunks=True,scaleoffset=0)
            elif mode=='virtual':
                g.create_dataset('original_intensity',data=base['intensity'])
                layout=h5py.VirtualLayout((3,),dtype='uint16');layout[:]=h5py.VirtualSource('.', '/entry/data/original_intensity',shape=(3,))
                g.create_virtual_dataset('intensity',layout)
            elif mode=='external-raw':g.create_dataset('intensity',shape=(3,),dtype='uint16',external=[('unresolved-source.raw',0,h5py.h5f.UNLIMITED)])
            else:
                dcpl=h5py.h5p.create(h5py.h5p.DATASET_CREATE)
                dtype=h5py.h5t.STD_U16LE.copy()
                if mode=='undefined-fill':dcpl.set_fill_time(h5py.h5d.FILL_TIME_NEVER)
                else:dtype.set_precision(12)
                dataset=h5py.h5d.create(g.id,b'intensity',dtype,h5py.h5s.create_simple((3,)),dcpl=dcpl);dataset.close()
        elif mode=='link-cycle':g['again']=h5py.SoftLink('/entry/data')
        else:
            del g['mask'];g['mask']=h5py.ExternalLink('nonexistent-source.h5','/mask')
    manifest['invalid'][p.name]=dict(file_sha256=sha(p.read_bytes()),failure='open')
# A partially present/irregular model is retained in raw form, but the explicit
# uniform-reference derived view must fail instead of silently substituting zeros.
for name,extras in [('partial-model',dict(radius=np.array([0,1,2],dtype='float32'))),('irregular-model',dict(radius=np.array([0,1,3],dtype='float32'),background_avg=np.ones((2,3),dtype='float32'),background_std=np.ones((2,3),dtype='float32'))),('overflow-model',dict(radius=np.array([0,1,2],dtype='float32'),background_avg=np.ones((2,3),dtype='float32')*100000,background_std=np.ones((2,3),dtype='float32')))]:
    case=copy.deepcopy(base);case.update(extras);p=ROOT/f'invalid-{name}.h5';create(p,case);manifest['invalid'][p.name]=dict(file_sha256=sha(p.read_bytes()),failure='dense')
p=ROOT/'growing.h5';create(p,base)
with h5py.File(p,'r+')as f:
    for name in ['frame_ptr','index','intensity']:
        del f['entry/data'][name]
        f['entry/data'].create_dataset(name,data=base[name],chunks=True,maxshape=(None,))
manifest['growing']=dict(file='growing.h5',file_sha256=sha(p.read_bytes()))
# Exact two-hop external-link graph, opt-in snapshot following only.
with h5py.File(ROOT/'external-target.h5','w')as f:
    g=f.create_group('data')
    for name,a in base.items():g.create_dataset(name,data=a)
with h5py.File(ROOT/'external-relay.h5','w')as f:f['forward']=h5py.ExternalLink('external-target.h5','/data')
with h5py.File(ROOT/'external-master.h5','w')as f:
    f.attrs['default']='entry';e=f.create_group('entry');e.attrs['default']='data';e['data']=h5py.ExternalLink('external-relay.h5','/forward')
for name in ['external-master.h5','external-relay.h5','external-target.h5']:manifest.setdefault('links',{})[name]=dict(file_sha256=sha((ROOT/name).read_bytes()))
(ROOT/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
