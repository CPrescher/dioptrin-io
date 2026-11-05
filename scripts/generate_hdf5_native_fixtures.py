"""CC0 small independent h5py fixtures for typed HDF5/discovery/source graphs.
This script and h5py are test references only; no production Python dependency.
"""
import hashlib
import json
import pathlib
import h5py
import hdf5plugin
import numpy as np

hdf5plugin.register(force=True)
root=pathlib.Path(__file__).resolve().parents[1]/'tests/data/hdf5-native'
root.mkdir(exist_ok=True)
manifest={'license':'CC0-1.0','h5py':h5py.__version__,'hdf5':h5py.version.hdf5_version,'numpy':np.__version__,'datasets':{},'files':{}}

def record(file,path,array):
    with h5py.File(root/file,'r') as f:
        ref=f[path][:]
        canonical=lambda a:np.ascontiguousarray(a.astype(a.dtype.newbyteorder('<'),copy=False))
        assert canonical(ref).tobytes()==canonical(array).tobytes()
        frames=ref[None] if ref.ndim==2 else ref
        manifest['datasets'][file+'::'+path]={'dtype':ref.dtype.name,'little_endian':ref.dtype.byteorder!='>',
            'shape':list(ref.shape),'frame_sha256':[hashlib.sha256(canonical(a).tobytes()).hexdigest() for a in frames]}

with h5py.File(root/'scalars.h5','w') as f:
    for dtype in ['uint8','uint16','uint32','uint64','int8','int16','int32','int64','float32','float64']:
        if dtype.startswith('float'):
            a=np.array([0.,-0.,np.nan,np.inf,-np.inf,-1.25],dtype=dtype).reshape(2,3)
            a.view('uint'+dtype[5:]).flat[2]=0x7fc12345 if dtype=='float32' else 0x7ff8123456789abc
        else:
            info=np.iinfo(dtype);a=np.array([0,info.min,info.max,1,2,3],dtype=dtype).reshape(2,3)
        for endian,order in [('le','<'),('be','>')]:
            array=a.astype(a.dtype.newbyteorder(order))
            for rank in [2,3]:
                data=array if rank==2 else np.stack([array,array[::-1,::-1]]).astype(array.dtype)
                path=f'{dtype}-{endian}-{rank}d'
                f.create_dataset(path,data=data,chunks=(1,2) if rank==2 else (1,1,2),compression='gzip',shuffle=True,fletcher32=True)
    f.create_dataset('rank1',data=np.arange(6,dtype='uint16'))
    f.create_dataset('compound',data=np.zeros((2,3),dtype=[('value','i4')]))
    f.create_dataset('explicit-fill',shape=(2,2,3),dtype='<i2',chunks=(1,2,3),fillvalue=-123)
    f.create_dataset('default-fill',shape=(2,3),dtype='<f4')
    dtype=h5py.h5t.STD_U16LE.copy();dtype.set_precision(12)
    space=h5py.h5s.create_simple((2,3))
    h5py.h5d.create(f.id,b'custom-precision',dtype,space).close()
    source=h5py.VirtualSource('missing.h5','data',shape=(1,2,3))
    layout=h5py.VirtualLayout((1,2,3),dtype='uint16');layout[:]=source
    f.create_virtual_dataset('virtual',layout)
    f.create_dataset('external-raw',(2,3),dtype='uint16',external=[('raw.bin',0,h5py.h5f.UNLIMITED)])
    plist=h5py.h5p.create(h5py.h5p.DATASET_CREATE);plist.set_chunk((1,2,3));plist.set_fill_time(h5py.h5d.FILL_TIME_NEVER)
    h5py.h5d.create(f.id,b'undefined-unwritten',h5py.h5t.STD_U16LE,h5py.h5s.create_simple((2,2,3)),dcpl=plist).close()
    written=h5py.h5d.create(f.id,b'no-fill-written',h5py.h5t.STD_U16LE,h5py.h5s.create_simple((1,2,3)),dcpl=plist)
    written.write(h5py.h5s.ALL,h5py.h5s.ALL,np.array([0,1,65535,2,3,4],dtype='uint16').reshape(1,2,3));written.close()
    f.create_group('group')['data']=f['uint64-be-3d']
with h5py.File(root/'scalars.h5','r') as f:
    for name in f:
        if name.endswith(('2d','3d')):record('scalars.h5',name,f[name][:])
record('scalars.h5','explicit-fill',np.full((2,2,3),-123,dtype='int16'))
record('scalars.h5','default-fill',np.zeros((2,3),dtype='float32'))
record('scalars.h5','no-fill-written',np.array([0,1,65535,2,3,4],dtype='uint16').reshape(1,2,3))

a=np.array([0,70000,2**32-2,2**32-1,5,6,7,8,9,10,11,12],dtype='uint32').reshape(2,2,3)
b=np.array([-32768,0,32767,7,8,9],dtype='int16').reshape(1,2,3)
with h5py.File(root/'data-a.h5','w') as f:
    f.create_dataset('counts',data=a,**hdf5plugin.Bitshuffle())
    f['alias']=h5py.SoftLink('/counts')
with h5py.File(root/'data-b.h5','w') as f:f.create_dataset('counts',data=b,compression='gzip')
with h5py.File(root/'intermediate.h5','w') as f:f['link']=h5py.ExternalLink('data-a.h5','/alias')
with h5py.File(root/'eiger-master.h5','w') as f:
    group=f.require_group('entry/data')
    group['data_000002']=h5py.ExternalLink('data-b.h5','/counts')
    group['data_000001']=h5py.ExternalLink('intermediate.h5','/link')
with h5py.File(root/'dangling-master.h5','w') as f:
    group=f.require_group('entry/data')
    group['data_000001']=h5py.ExternalLink('data-a.h5','/counts')
    group['data_000002']=h5py.ExternalLink('missing.h5','/counts')
with h5py.File(root/'cycle.h5','w') as f:
    f['first']=h5py.SoftLink('/second');f['second']=h5py.SoftLink('/first')
with h5py.File(root/'lima.h5','w') as f:
    f.attrs['creator']='LIMA';f.attrs['default']='/entry_0001'
    entry=f.require_group('entry_0001');entry.attrs['default']='/entry_0001/instrument/Pilatus/plot'
    entry.require_group('measurement').create_dataset('data',data=a,compression='gzip')
with h5py.File(root/'lambda.h5','w') as f:
    detector=f.require_group('entry/instrument/detector')
    detector.create_dataset('description',data=np.bytes_('Lambda'))
    detector.create_dataset('local_name',data='λ-detector')
    detector.create_dataset('data',data=b,compression='gzip')
record('data-a.h5','counts',a);record('data-b.h5','counts',b)
record('lima.h5','entry_0001/measurement/data',a);record('lambda.h5','entry/instrument/detector/data',b)
for path in sorted(root.glob('*.h5')):manifest['files'][path.name]={'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'bytes':path.stat().st_size}
(root/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
