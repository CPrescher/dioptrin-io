"""CC0 bounded printf VDS fixtures; expected words never read from a VDS."""
import hashlib,json,pathlib
import h5py
import numpy as np
ROOT=pathlib.Path(__file__).resolve().parents[1]/"tests/data/vds-printf"


def raw(array):return array.astype(array.dtype.newbyteorder('<'),copy=False).tobytes()


def values(kind,index,frames=2):
    count=frames*6
    if kind in ('u64','u64-be'):
        words=[2**64-1,2**63,2**53+1,0,1,2]+[index*100+i for i in range(count-6)]
        return np.array(words,dtype='>u8' if kind.endswith('be') else '<u8').reshape(frames,2,3)
    if kind=='i64':return np.array([-(2**63),2**63-1,-(2**53)-1,0,1,-1]+[index*100+i for i in range(count-6)],dtype='<i8').reshape(frames,2,3)
    if kind=='i16':return np.array([-32768,32767,-1,0,index,7]+[index*100+i for i in range(count-6)],dtype='<i2').reshape(frames,2,3)
    if kind=='f16':return np.array(([0x8000,0x7e55,0xfeaa,1,0x7c00,0xfc00]*frames),dtype='<u2').view('<f2').reshape(frames,2,3)
    if kind=='bool':return np.frombuffer(bytes([0,1,2,127,128,255]*frames),dtype='?').reshape(frames,2,3)
    if kind=='c32':return np.array(([0x80000000,0x7fc12345,0xffc54321,1,0x7f800000,0xff800000]*frames*2),dtype='<u4').view('<c8').reshape(frames,2,3)
    raise ValueError(kind)


def store(file,name,array):
    d=file.create_dataset(name,shape=array.shape,dtype=array.dtype,chunks=(1,2,3),compression='gzip',shuffle=True,maxshape=(None,2,3))
    d.id.write(h5py.h5s.ALL,h5py.h5s.ALL,array,mtype=d.id.get_type())


def printf(file,path,filename,dataset,source_shape,dtype,*,axis=0,start=0,stride=None,block=None,source_start=0,source_frames=None,fill=0,declared=0):
    block=list(source_shape if block is None else block);stride=list(block if stride is None else stride)
    shape=list(block);shape[axis]=declared;maximum=shape.copy();maximum[axis]=h5py.h5s.UNLIMITED
    v=h5py.h5s.create_simple(tuple(shape),tuple(maximum));counts=[1,1,1];counts[axis]=h5py.h5s.UNLIMITED
    origin=[0,0,0];origin[axis]=start;v.select_hyperslab(tuple(origin),tuple(counts),stride=tuple(stride),block=tuple(block))
    s=h5py.h5s.create_simple(source_shape)
    if source_start or source_frames is not None:s.select_hyperslab((source_start,0,0),(1,1,1),block=(source_frames,source_shape[1],source_shape[2]))
    dcpl=h5py.h5p.create(h5py.h5p.DATASET_CREATE);dcpl.set_fill_value(np.array(fill,dtype=dtype));dcpl.set_virtual(v,filename.encode(),dataset.encode(),s)
    h5py.h5d.create(file.id,path.encode(),h5py.h5t.py_create(np.dtype(dtype),logical=True),h5py.h5s.create_simple(tuple(shape),tuple(maximum)),dcpl=dcpl)


def generate():
    ROOT.mkdir(parents=True,exist_ok=True);cases={}
    def build(name,kind='u64',n=2,*,self_file=False,filename=None,dataset='/data',axis=0,start=0,stride=None,select=False,declared=0):
        fname=filename or name+'-%b.h5';arrays=[values(kind,k,frames=3 if select else 2) for k in range(n)]
        bindings=[];source_files=[]
        if self_file:
            owner=ROOT/(name+'.h5')
            with h5py.File(owner,'w',libver='latest') as file:
                for k,array in enumerate(arrays):store(file,dataset.replace('%b',str(k)).replace('%%','%'),array)
                printf(file,'/data','.',dataset,arrays[0].shape,arrays[0].dtype)
        else:
            for k,array in enumerate(arrays):
                literal=fname.replace('%b',str(k)).replace('%%','%');literal_dataset=dataset.replace('%b',str(k)).replace('%%','%')
                with h5py.File(ROOT/literal,'w') as file:store(file,literal_dataset,array)
                bindings.append({'stored_filename':literal,'actual_file':literal});source_files.append(literal)
            with h5py.File(ROOT/(name+'.h5'),'w',libver='latest') as file:
                printf(file,'/data',fname,dataset,arrays[0].shape,arrays[0].dtype,axis=axis,start=start,stride=stride,block=(2,2,3) if select else None,source_start=1 if select else 0,source_frames=2 if select else None,fill=np.uint64(2**64-1) if kind=='u64' and start else 0,declared=declared)
        selected=[a[1:3] if select else a for a in arrays]
        if axis==0:
            frames=start+(n-1)*(stride[0] if stride else 2)+2
            expected=[None]*frames
            for k,a in enumerate(selected):
                step=stride[0] if stride else 2
                frame_bytes=len(raw(a))//2
                for j in range(2):expected[start+k*step+j]=raw(a)[j*frame_bytes:(j+1)*frame_bytes]
            if any(v is None for v in expected):
                fillword=np.array([np.uint64(2**64-1)],dtype=arrays[0].dtype).astype(arrays[0].dtype.newbyteorder('<'),copy=False).tobytes()
                expected=[fillword*6 if v is None else v for v in expected]
            shape=[frames,2,3]
        else:
            expected=[]
            for frame in range(2):
                output=b''
                for row in range(2):output+=b''.join(raw(a[frame:frame+1])[row*3*a.dtype.itemsize:(row+1)*3*a.dtype.itemsize] for a in arrays)
                expected.append(output)
            shape=[2,2,3*n]
        cases[name]={'master':name+'.h5','dataset':'/data','kind':kind,'dtype':arrays[0].dtype.str,'shape':shape,'frame_hex':[v.hex() for v in expected],'bindings':bindings,
                     'printf_mappings':[{'source_file':None,'dataset':'/data','mapping_index':0,'stored_filename':'.' if self_file else fname,'declared_dataset':dataset,'blocks':n}],
                     'unmapped_regions':'declared_fill' if start or stride else 'reject','source_files':source_files,'axis':axis,'block_start':start,'source_selection_start':1 if select else 0,'original_declared_unlimited_axis':declared}
    build('files-u64',n=3);build('predeclared-prefix',n=3,declared=100);build('files-u64-be','u64-be');build('self-datasets',n=3,self_file=True,dataset='/part-%b')
    build('both-escaped','f16',filename='percent%%-%b.h5',dataset='/data%%_%b')
    build('repeated','i16',filename='repeat-%b-%b.h5',dataset='/part-%b-%b')
    build('strided','u64',start=1,stride=(3,2,3));build('spatial','u64',axis=2)
    build('source-selection','i64',select=True)
    for kind in ['f16','bool','c32']:build('files-'+kind,kind)
    # Escaping is part of ordinary HDF5 declaration grammar, not a printf-count policy.
    array=values('u64',0)
    with h5py.File(ROOT/'literal%.h5','w') as f:store(f,'/literal%',array)
    with h5py.File(ROOT/'escaped-literal.h5','w',libver='latest') as f:
        v=h5py.h5s.create_simple(array.shape);s=h5py.h5s.create_simple(array.shape);p=h5py.h5p.create(h5py.h5p.DATASET_CREATE);p.set_virtual(v,b'literal%%.h5',b'/literal%%',s)
        h5py.h5d.create(f.id,b'data',h5py.h5t.STD_U64LE,v,dcpl=p)
    cases['escaped-literal']={'master':'escaped-literal.h5','dataset':'/data','kind':'u64','dtype':'<u8','shape':list(array.shape),'frame_hex':[raw(a).hex() for a in array],
                              'bindings':[{'stored_filename':'literal%.h5','actual_file':'literal%.h5'}],'printf_mappings':[],'unmapped_regions':'reject','source_files':['literal%.h5']}
    # Nested physical owner: printf filename '.' resolves child.h5, not root.h5.
    arrays=[values('u64',k) for k in range(2)]
    with h5py.File(ROOT/'nested-child.h5','w',libver='latest') as f:
        for k,a in enumerate(arrays):store(f,f'/part-{k}',a)
        printf(f,'/data','.', '/part-%b',arrays[0].shape,arrays[0].dtype)
    with h5py.File(ROOT/'nested-owner.h5','w',libver='latest') as f:
        v=h5py.h5s.create_simple((4,2,3));s=h5py.h5s.create_simple((4,2,3));s.select_hyperslab((0,0,0),(1,1,1),block=(4,2,3));p=h5py.h5p.create(h5py.h5p.DATASET_CREATE);p.set_virtual(v,b'nested-child.h5',b'/data',s)
        h5py.h5d.create(f.id,b'data',h5py.h5t.STD_U64LE,v,dcpl=p)
    cases['nested-owner']={'master':'nested-owner.h5','dataset':'/data','kind':'u64','dtype':'<u8','shape':[4,2,3],'frame_hex':[raw(a).hex() for array in arrays for a in array],
                           'bindings':[{'stored_filename':'nested-child.h5','actual_file':'nested-child.h5'}],
                           'printf_mappings':[{'source_file':'nested-child.h5','dataset':'/data','mapping_index':0,'stored_filename':'.','declared_dataset':'/part-%b','blocks':2}],
                           'unmapped_regions':'reject','source_files':['nested-child.h5']}
    # Two original infinite mappings share the global output domain. The shorter
    # approved prefix must not become declared fill inside the longer prefix.
    with h5py.File(ROOT/'exhausted-prefixes.h5','w',libver='latest') as f:
        p=h5py.h5p.create(h5py.h5p.DATASET_CREATE)
        for column in (0,3):
            v=h5py.h5s.create_simple((0,2,6),(h5py.h5s.UNLIMITED,2,6))
            v.select_hyperslab((0,0,column),(h5py.h5s.UNLIMITED,1,1),stride=(2,2,3),block=(2,2,3))
            p.set_virtual(v,b'files-u64-%b.h5',b'/data',h5py.h5s.create_simple((2,2,3)))
        h5py.h5d.create(f.id,b'data',h5py.h5t.STD_U64LE,h5py.h5s.create_simple((0,2,6),(h5py.h5s.UNLIMITED,2,6)),dcpl=p)
    with h5py.File(ROOT/'interleaved-prefixes.h5','w',libver='latest') as f:
        p=h5py.h5p.create(h5py.h5p.DATASET_CREATE)
        for lane in (0,1):
            v=h5py.h5s.create_simple((0,2,3),(h5py.h5s.UNLIMITED,2,3))
            v.select_hyperslab((lane,0,0),(h5py.h5s.UNLIMITED,1,1),stride=(2,2,3),block=(1,2,3))
            source=h5py.h5s.create_simple((2,2,3));source.select_hyperslab((lane,0,0),(1,1,1),block=(1,2,3))
            p.set_virtual(v,b'files-u64-%b.h5',b'/data',source)
        h5py.h5d.create(f.id,b'data',h5py.h5t.STD_U64LE,h5py.h5s.create_simple((0,2,3),(h5py.h5s.UNLIMITED,2,3)),dcpl=p)
    files={p.name:{'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(ROOT.glob('*.h5'))}
    (ROOT/'provenance.json').write_text(json.dumps({'license':'CC0-1.0','generator':'scripts/generate_printf_vds_fixtures.py','origin':'Independent authored scalar storage words and explicit HDF5 DCPL mappings; expected words assembled from source bytes without VDS reads','h5py_version':h5py.__version__,'hdf5_version':h5py.version.hdf5_version,'cases':cases,'files':files},indent=2)+'\n')


if __name__=='__main__':generate()
