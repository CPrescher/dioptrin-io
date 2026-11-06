"""CC0 exact HDF5 finite selection fixtures; manual sorted-word assembly."""
import hashlib,json,pathlib
import h5py
import numpy as np
ROOT=pathlib.Path(__file__).resolve().parents[1]
DATA=ROOT/'tests/data/vds-irregular'

def selection(shape,boxes):
    space=h5py.h5s.create_simple(shape);space.select_none()
    for start,extent in boxes:space.select_hyperslab(start,(1,1,1),block=extent,op=h5py.h5s.SELECT_OR)
    return space

def coordinates(shape,boxes):
    # Independent tiny-fixture oracle: explicit mathematical set, sorted C order.
    import itertools
    return sorted(set(point for start,extent in boxes for point in itertools.product(*(range(s,s+n) for s,n in zip(start,extent)))))

def create(name,shape,array,sboxes,vboxes,fill,extra=None,maxshape=None,source_file='source.h5',source_dataset=None):
    source_dataset=source_dataset or '/'+name
    source=selection(array.shape,sboxes);virtual=selection(shape,vboxes)
    dcpl=h5py.h5p.create(h5py.h5p.DATASET_CREATE);dcpl.set_fill_value(np.array(fill,dtype=array.dtype))
    dcpl.set_virtual(virtual,source_file.encode(),source_dataset.encode(),source)
    if extra:
        other=selection(shape,extra);dcpl.set_virtual(other,source_file.encode(),source_dataset.encode(),source)
    path=DATA/(name+'.h5')
    with h5py.File(path,'w',libver='latest') as file:h5py.h5d.create(file.id,b'data',h5py.h5t.py_create(array.dtype),h5py.h5s.create_simple(shape,maxshape),dcpl=dcpl)
    expected=np.empty(shape,dtype=array.dtype)
    # Copy complete storage words, including noncanonical bool and NaN bits.
    word=np.dtype(('V',array.dtype.itemsize));flat=expected.view(word).reshape(-1);src=array.view(word)
    fillword=np.array([fill],dtype=array.dtype).view(word)[0];flat[:]=fillword
    src_points=coordinates(array.shape,sboxes);dst_points=coordinates(shape,vboxes)
    assert len(src_points)==len(dst_points)
    for a,b in zip(src_points,dst_points):expected.view(word)[b]=src[a]
    covered=[sum(p[0]==frame for p in dst_points) for frame in range(shape[0])]
    return {'master':name+'.h5','dataset':'/data','source_file':source_file,'source_dataset':source_dataset,'source_shape':list(array.shape),'shape':list(shape),'dtype':array.dtype.str,
            'source_boxes':sboxes,'virtual_boxes':vboxes,'unmapped_by_frame':[shape[1]*shape[2]-n for n in covered],
            'frame_hex':[a.astype(array.dtype.newbyteorder('<'),copy=False).tobytes().hex() for a in expected]}

def generate():
    DATA.mkdir(parents=True,exist_ok=True);shape=(3,4,5)
    values={}
    for label,dtype in [('u64','<u8'),('u64-be','>u8'),('i16','<i2')]:
        words=[2**64-1,2**63,2**53+1]+list(range(3,60)) if 'u64' in label else list(range(-30,30))
        values[label]=np.array(words,dtype=dtype).reshape(shape)
    for label,dtype,width in [('f16','<f2',2),('f16-be','>f2',2),('f32','<f4',4),('f64','<f8',8),('c32','<c8',4),('c32-be','>c8',4),('c64','<c16',8)]:
        special={2:[0x8000,0x7e55,0xfeaa,1,0x7c00,0xfc00],4:[0x80000000,0x7fc12345,0xffc54321,1,0x7f800000,0xff800000],8:[0x8000000000000000,0x7ff8123456789abc,0xfff8abcddcba1234,1,0x7ff0000000000000,0xfff0000000000000]}[width]
        count=60*(2 if label.startswith('c') else 1)
        values[label]=np.array([special[i%6] for i in range(count)],dtype=f'<u{width}').view('<'+np.dtype(dtype).str[1:]).astype(dtype).reshape(shape)
    values['bool']=np.frombuffer(bytes([0,1,2,127,128,255]*10),dtype='?').reshape(shape)
    with h5py.File(DATA/'source.h5','w') as file:
        for name,array in values.items():
            dataset=file.create_dataset(name,shape=array.shape,dtype=array.dtype,chunks=(1,2,3),compression='gzip',shuffle=True,maxshape=(None,4,5))
            dataset.id.write(h5py.h5s.ALL,h5py.h5s.ALL,array,mtype=dataset.id.get_type())
        file.require_group('deep/actual')['data']=file['u64']
    sboxes=[((0,0,0),(3,1,3)),((0,2,1),(3,2,1))]
    vboxes=[((0,1,0),(3,1,2)),((0,3,2),(3,1,3))]
    cases={}
    for name,array in values.items():
        fill=0 if name=='bool' else array.reshape(-1)[0]
        cases[name]=create(name,(3,5,6),array,sboxes,vboxes,fill)
    a=values['u64']
    cases['cross-frame']=create('cross-frame',(4,4,5),a,[((0,0,0),(1,1,4)),((1,2,1),(1,1,2)),((2,0,0),(1,2,2))],[((0,1,0),(1,1,2)),((1,0,0),(1,1,5)),((2,3,2),(1,1,3))],np.uint64(2**64-1),maxshape=(h5py.h5s.UNLIMITED,4,5),source_dataset='/u64')
    cases['nested-path']=create('nested-path',(3,5,6),a,sboxes,vboxes,np.uint64(2**64-1),source_dataset='/deep/actual/data')
    cases['strided']=create('strided',(3,2,3),a,[((n,y,x),(1,1,1)) for n in range(3) for y in [0,2] for x in [0,2,4]],[((0,0,0),(3,2,3))],0,source_dataset='/u64')
    with h5py.File(DATA/'relay.h5','w') as file:file['data']=h5py.ExternalLink('source.h5','/u64')
    with h5py.File(DATA/'bound.h5','w') as file:file['data']=h5py.ExternalLink('relay.h5','/data')
    cases['external-graph']=create('external-graph',(3,5,6),a,sboxes,vboxes,np.uint64(2**64-1),source_file='bound.h5',source_dataset='/data')
    create('overlap',(3,5,6),a,sboxes,vboxes,0,extra=vboxes,source_dataset='/u64')
    with h5py.File(DATA/'partial.h5','w') as file:
        d=file.create_dataset('u64',shape=shape,dtype='<u8',chunks=(1,2,3));d[0]=a[0]
    with h5py.File(DATA/'short.h5','w') as file:file['u64']=a[:,:1,:]
    # Compact regular declaration is valid but deliberately exceeds bounded
    # selection complexity without requiring detector-sized fixture metadata.
    array=np.arange(34_000,dtype='<u8').reshape(1,1,34_000)
    with h5py.File(DATA/'large-source.h5','w') as file:file.create_dataset('data',data=array,compression='gzip')
    space=h5py.h5s.create_simple(array.shape);space.select_hyperslab((0,0,0),(1,1,17_000),stride=(1,1,2))
    dcpl=h5py.h5p.create(h5py.h5p.DATASET_CREATE);dcpl.set_virtual(space,b'large-source.h5',b'/data',space)
    with h5py.File(DATA/'block-limit.h5','w') as file:h5py.h5d.create(file.id,b'data',h5py.h5t.STD_U64LE,h5py.h5s.create_simple(array.shape),dcpl=dcpl)
    files={p.name:{'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(DATA.glob('*.h5'))}
    manifest={'schema':1,'license':'CC0-1.0','generator':'scripts/generate_irregular_vds_fixtures.py','origin':'independent IEEE/enum storage words and explicitly enumerated C-order selection sets; no VDS read or reference writer generates expected counts','cases':cases,'files':files}
    (DATA/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')

if __name__=='__main__':generate()
