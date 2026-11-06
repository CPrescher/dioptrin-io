"""CC0 native VDS graph fixtures with independently paired original words."""
import hashlib,itertools,json,pathlib
import h5py
import numpy as np
ROOT=pathlib.Path(__file__).resolve().parents[1]
DATA=ROOT/'tests/data/vds-nested'
def selection(shape,points):
    space=h5py.h5s.create_simple(shape);space.select_none()
    for point in sorted(set(map(tuple,points))):space.select_hyperslab(point,(1,1,1),op=h5py.h5s.SELECT_OR)
    return space
def node(filename,name,source_file,source_path,array,source_points,target_points,shape,fill,never=False):
    dtype=array.dtype;source=selection(array.shape,source_points);target=selection(shape,target_points)
    dcpl=h5py.h5p.create(h5py.h5p.DATASET_CREATE);dcpl.set_fill_value(np.frombuffer(fill,dtype=dtype).reshape(()))
    if never:dcpl.set_fill_time(h5py.h5d.FILL_TIME_NEVER)
    dcpl.set_virtual(target,source_file.encode(),source_path.encode(),source)
    with h5py.File(DATA/filename,'a',libver='latest') as file:h5py.h5d.create(file.id,name.encode(),h5py.h5t.py_create(dtype),h5py.h5s.create_simple(shape),dcpl=dcpl)
    result=np.frombuffer(fill*int(np.prod(shape)),dtype=dtype).reshape(shape).copy();void=np.dtype(('V',dtype.itemsize))
    for a,b in zip(sorted(set(map(tuple,source_points))),sorted(set(map(tuple,target_points)))):result.view(void)[b]=array.view(void)[a]
    return result
def canonical(array):return array.astype(array.dtype.newbyteorder('<'),copy=False).tobytes().hex()
def generate():
    DATA.mkdir(parents=True,exist_ok=True)
    for name in ['source.h5','child.h5','middle.h5','grand.h5','undefined.h5','views.h5','dag.h5','cycle.h5','depth.h5']:
        (DATA/name).unlink(missing_ok=True)
    shape=(3,3,4);size=36
    values={
        'u64':np.array([2**64-1,2**63,2**53+1]+list(range(3,size)),dtype='<u8').reshape(shape),
        'f16':np.array(([0x8000,0x7e55,0xfeaa,1,0x7c00,0xfc00]*6),dtype='<u2').view('<f2').reshape(shape),
        'c32-be':np.array(([0x80000000,0x7fc12345,0xffc54321,1,0x7f800000,0xff800000]*12),dtype='<u4').view('<c8').astype('>c8').reshape(shape),
        'bool':np.frombuffer(bytes([0,1,2,127,128,255]*6),dtype='?').reshape(shape),
    }
    with h5py.File(DATA/'source.h5','w') as file:
        for name,array in values.items():
            d=file.create_dataset(name,shape=array.shape,dtype=array.dtype,chunks=(1,2,2),compression='gzip',maxshape=(None,3,4));d.id.write(h5py.h5s.ALL,h5py.h5s.ALL,array,mtype=d.id.get_type())
    measured=[(0,0,0),(0,1,1),(0,2,3),(1,0,2),(1,2,1),(2,0,0),(2,1,3),(2,2,2)]
    child_points=[(0,0,0),(0,1,2),(0,2,4),(1,0,1),(1,3,3),(2,1,0),(2,2,2),(2,3,4)]
    selected=sorted(set(child_points+[(0,0,1),(0,3,4),(1,1,0),(1,2,4),(2,0,3),(2,1,1),(2,3,0)]))
    middle_points=list(itertools.islice(itertools.product(range(3),range(3),range(6)),0,45,3))
    grand_selected=list(itertools.islice(itertools.product(range(3),range(3),range(6)),0,48,3))
    grand_points=list(itertools.islice(itertools.product(range(2),range(3),range(4)),0,24,1))[:len(grand_selected)]
    fills={'u64':[bytes.fromhex(s) for s in ['ffffffffffffffff','0100000000000080','0100000000002000']],
           'f16':[bytes.fromhex(s) for s in ['0080','557e','aafe']],
           'c32-be':[bytes.fromhex(s) for s in ['800000007fc12345','7fc5432180000000','ff8000007fc12345']],
           'bool':[bytes([v]) for v in [2,127,255]]}
    cases={};children={}
    for name,array in values.items():
        child=node('child.h5','/'+name,'source.h5','/'+name,array,measured,child_points,(3,4,5),fills[name][0])
        children[name]=child
        middle=node('middle.h5','/'+name,'child.h5','/'+name,child,selected,middle_points,(3,3,6),fills[name][1])
        grand=node('grand.h5','/'+name,'middle.h5','/'+name,middle,grand_selected,grand_points,(2,3,4),fills[name][2])
        for label,file,expected,bindings in [('middle','middle.h5',middle,['child.h5','source.h5']),('grand','grand.h5',grand,['middle.h5','child.h5','source.h5'])]:
            cases[label+'-'+name]={'master':file,'dataset':'/'+name,'bindings':bindings,'shape':list(expected.shape),'dtype':expected.dtype.str,'frame_hex':[canonical(a) for a in expected],'unmapped_regions':'declared_fill'}
    u=values['u64'];child=node('undefined.h5','/data','source.h5','/u64',u,measured,child_points,(3,4,5),bytes(8),True)
    expected=node('views.h5','/measured-only','undefined.h5','/data',child,child_points,list(itertools.product(range(2),range(2),range(2))),(2,2,2),bytes(8))
    cases['measured-only-undefined-child']={'master':'views.h5','dataset':'/measured-only','bindings':['undefined.h5','source.h5'],'shape':list(expected.shape),'dtype':'<u8','frame_hex':[canonical(a) for a in expected],'unmapped_regions':'reject','effective_fill_regions':0}
    node('views.h5','/undefined-used','undefined.h5','/data',child,[(0,0,1)],[(0,0,0)],(1,1,1),bytes(8))
    with h5py.File(DATA/'child.h5','a') as file:file['alias-u64']=file['u64']
    first=selected[:4];second=selected[-4:];out=np.zeros((2,2,2),dtype='<u8');vshape=out.shape
    dcpl=h5py.h5p.create(h5py.h5p.DATASET_CREATE)
    child=children['u64']
    for source_name,sp,vp in [('/u64',first,[(0,0,0),(0,0,1),(0,1,0),(0,1,1)]),('/alias-u64',second,[(1,0,0),(1,0,1),(1,1,0),(1,1,1)])]:
        dcpl.set_virtual(selection(vshape,vp),b'child.h5',source_name.encode(),selection(child.shape,sp))
        for a,b in zip(sorted(sp),sorted(vp)):out[b]=child[a]
    with h5py.File(DATA/'dag.h5','w') as file:h5py.h5d.create(file.id,b'data',h5py.h5t.STD_U64LE,h5py.h5s.create_simple(vshape),dcpl=dcpl)
    cases['physical-node-hard-alias-dag']={'master':'dag.h5','dataset':'/data','bindings':['child.h5','source.h5'],'shape':list(out.shape),'dtype':'<u8','frame_hex':[canonical(a) for a in out],'unmapped_regions':'declared_fill','nested_nodes':1}
    with h5py.File(DATA/'cycle.h5','w') as file:
        space=h5py.h5s.create_simple((1,1,1));p=h5py.h5p.create(h5py.h5p.DATASET_CREATE);p.set_virtual(space,b'.',b'/data',space);h5py.h5d.create(file.id,b'data',h5py.h5t.STD_U64LE,space,dcpl=p)
    with h5py.File(DATA/'depth.h5','w') as file:
        file['leaf']=np.array([[[2**64-1]]],dtype='<u8')
        for i in reversed(range(34)):
            space=h5py.h5s.create_simple((1,1,1));p=h5py.h5p.create(h5py.h5p.DATASET_CREATE);p.set_virtual(space,b'.',(f'/n{i+1}' if i<33 else '/leaf').encode(),space);h5py.h5d.create(file.id,f'n{i}'.encode(),h5py.h5t.STD_U64LE,space,dcpl=p)
    with h5py.File(DATA/'dag-expansion.h5','w') as file:
        file['leaf']=np.array([[[2**64-1]]],dtype='<u8')
        for i in range(18):
            width=2**i;source=h5py.h5s.create_simple((1,1,width));target_shape=(1,1,width*2);p=h5py.h5p.create(h5py.h5p.DATASET_CREATE)
            for offset in [0,width]:
                target=h5py.h5s.create_simple(target_shape);target.select_hyperslab((0,0,offset),(1,1,1),block=(1,1,width));p.set_virtual(target,b'.',(f'/n{i-1}' if i else '/leaf').encode(),source)
            h5py.h5d.create(file.id,f'n{i}'.encode(),h5py.h5t.STD_U64LE,h5py.h5s.create_simple(target_shape),dcpl=p)
    manifest={'schema':1,'license':'CC0-1.0','generator':'scripts/generate_nested_vds_fixtures.py','origin':'independent original typed words, exact fill words and sorted mathematical source/destination coordinate pairing; no reference VDS read generates expected image words','cases':cases,'invalid':{'undefined_used':{'master':'views.h5','dataset':'/undefined-used','bindings':['undefined.h5','source.h5']},'cycle':{'master':'cycle.h5','dataset':'/data','bindings':[]},'depth':{'master':'depth.h5','dataset':'/n0','bindings':[]},'dag_expansion':{'master':'dag-expansion.h5','dataset':'/n17','bindings':[]}},'files':{p.name:{'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(DATA.glob('*.h5'))}}
    (DATA/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
if __name__=='__main__':generate()
