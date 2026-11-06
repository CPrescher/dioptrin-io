"""CC0 unlimited VDS mappings; expectations use explicit coordinate arithmetic only."""
import hashlib,itertools,json,pathlib
import h5py
import numpy as np
ROOT=pathlib.Path(__file__).resolve().parents[1]/'tests/data/vds-unlimited'
INF=h5py.h5s.UNLIMITED

def coords(spec,shape):
    start,stride,count,block=spec
    axes=[]
    for a in range(3):
        n=(shape[a]-start[a]+stride[a]-1)//stride[a] if count[a]==INF else count[a]
        axes.append(sorted({start[a]+i*stride[a]+j for i in range(n) for j in range(block[a]) if start[a]+i*stride[a]+j<shape[a]}))
    return list(itertools.product(*axes))

def space(shape,spec):
    s=h5py.h5s.create_simple(tuple(shape),(INF,shape[1],shape[2]))
    s.select_hyperslab(tuple(spec[0]),tuple(spec[2]),stride=tuple(spec[1]),block=tuple(spec[3]))
    return s

def generate():
    ROOT.mkdir(parents=True,exist_ok=True);cases={}
    def case(name,mappings,spatial=(2,3),fill=False,reject=False):
        records=[];selected=[];bindings=[]
        for ordinal,(shape,src,dst) in enumerate(mappings):
            filename=f'{name}-{ordinal}.h5'
            words=np.arange(np.prod(shape),dtype='<u8').reshape(shape)+np.uint64(2**63+ordinal*1000+1)
            words.flat[0]=2**64-1
            with h5py.File(ROOT/filename,'w') as f:f.create_dataset('/data',data=words,chunks=(1,shape[1],shape[2]),maxshape=(None,shape[1],shape[2]),compression='gzip',shuffle=True)
            sourcecoords=coords(src,shape);points=[int(words[c]) for c in sourcecoords]
            rows=dst[2][1]*dst[3][1];cols=dst[2][2]*dst[3][2]
            frames=(len(points)+rows*cols-1)//(rows*cols)
            end=dst[0][0]+((frames-1)//dst[3][0])*dst[1][0]+(frames-1)%dst[3][0]+1
            selected.append((points,dst,end));bindings.append({'stored_filename':filename,'actual_file':filename})
            records.append({'file':filename,'source_shape':list(shape),'source':src,'virtual':dst,'selected_source_coordinates':[list(c) for c in sourcecoords]})
        end=max(v[2] for v in selected);outshape=(end,*spatial)
        output=np.full(outshape,2**64-1,dtype='<u8');covered=set()
        for points,dst,_ in selected:
            target=coords(dst,outshape)
            if len(target)!=len(points):reject=True
            for p,c in zip(points,target):output[c]=p;covered.add(c)
        if len(covered)!=output.size and not fill:reject=True
        with h5py.File(ROOT/(name+'.h5'),'w',libver='latest') as f:
            prop=h5py.h5p.create(h5py.h5p.DATASET_CREATE);prop.set_fill_value(np.array(2**64-1,dtype='<u8'))
            for record in records:
                prop.set_virtual(space((0,*spatial),record['virtual']),record['file'].encode(),b'/data',space(record['source_shape'],record['source']))
            h5py.h5d.create(f.id,b'data',h5py.h5t.STD_U64LE,h5py.h5s.create_simple((0,*spatial),(INF,*spatial)),dcpl=prop)
        with h5py.File(ROOT/(name+'.h5'),'r') as f:
            prop=f['data'].id.get_create_plist()
            for index,record in enumerate(records):
                record['stored_source']=[list(v) for v in prop.get_virtual_srcspace(index).get_regular_hyperslab()]
                record['stored_virtual']=[list(v) for v in prop.get_virtual_vspace(index).get_regular_hyperslab()]
        cases[name]={'master':name+'.h5','dataset':'/data','dtype':'<u8','shape':list(outshape),'frame_hex':[a.tobytes().hex() for a in output],'bindings':bindings,'printf_mappings':[],'unmapped_regions':'declared_fill' if fill else 'reject','mappings':records,'expect':'reject' if reject else 'exact'}
    def spec(start=(0,0,0),stride=(1,2,3),count=(INF,1,1),block=(1,2,3)):return [list(start),list(stride),list(count),list(block)]
    case('contiguous',[((5,2,3),spec(),spec())])
    case('offset-stride',[((8,4,6),spec((1,1,1),(3,2,2),(INF,2,3),(2,1,1)),spec((1,0,0),(3,2,3),(INF,1,1),(2,2,3)))],fill=True)
    case('partial-block',[((4,2,3),spec(stride=(3,2,3),block=(2,2,3)),spec(stride=(2,2,3),block=(2,2,3)))])
    case('spatial-stride',[((3,4,6),spec((0,0,0),(1,2,2),(INF,2,3),(1,1,1)),spec())])
    case('virtual-spatial-stride',[((3,2,3),spec(),spec((0,0,1),(1,2,2),(INF,2,3),(1,1,1)))],spatial=(4,6),fill=True)
    # Per virtual frame has two samples; three source frames each have three.
    # HDF5 rejects unlimited temporal reshaping across unequal spatial counts.
    controls=[]
    for label,source,target in [('reshape',spec(stride=(2,1,3),count=(INF,1,1),block=(2,1,3)),spec(stride=(3,1,2),count=(INF,1,1),block=(3,1,2))),('partial-plane',spec(stride=(1,1,3),count=(INF,1,1),block=(1,1,3)),spec(stride=(1,1,2),count=(INF,1,1),block=(1,1,2)))]:
        prop=h5py.h5p.create(h5py.h5p.DATASET_CREATE)
        try:prop.set_virtual(space((0,1,2),target),b'a.h5',b'/data',space((4,1,3),source))
        except ValueError as error:controls.append({'name':label,'result':'producer_rejected','error':str(error)})
        else:raise AssertionError('unexpected accepted unlimited spatial cardinality change')
    case('interleaved',[((3,2,3),spec(),spec(stride=(2,2,3))),((2,2,3),spec(),spec((1,0,0),(2,2,3)))])
    case('exhausted-maps',[((3,2,3),spec(),spec()),((2,2,3),spec(),spec((0,0,3)))],spatial=(2,6),reject=True)

    files={p.name:{'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(ROOT.glob('*.h5'))}
    (ROOT/'provenance.json').write_text(json.dumps({'license':'CC0-1.0','origin':'Authored U64 words with coordinate-enumerated source and target selections, never expected values from VDS reads','generator':'scripts/generate_vds_unlimited_fixtures.py','h5py_version':h5py.__version__,'hdf5_version':h5py.version.hdf5_version,'cases':cases,'producer_controls':controls,'files':files},indent=2)+'\n')
if __name__=='__main__':generate()
