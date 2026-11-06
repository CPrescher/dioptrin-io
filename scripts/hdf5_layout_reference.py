"""Independent, test-only HDF5 type/external-window qualification.

Unchanged FabIO reads are separate from raw source and h5py bit comparisons.
No production Python dependency or native fallback is introduced.
"""
import argparse
import contextlib
import hashlib
import inspect
import json
import os
import pathlib
import subprocess
import tempfile

import h5py
import numpy as np

from companion_reference import differences, fingerprint
ROOT=pathlib.Path(__file__).resolve().parents[1]
DATA=ROOT/'tests/data/hdf5-layouts'
PIN='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'

def canonical(array):
    """Change byte order only; bool bytes and IEEE components remain words."""
    array=np.asarray(array)
    return array.astype(array.dtype.newbyteorder('<'),copy=False).tobytes(order='C')

def pixel_type(dtype):
    return {'complex64':'C32','complex128':'C64','float16':'F16','bool':'Bool'}[np.dtype(dtype).name]

def frame_projection(index,array):
    raw=canonical(array)
    return {'frame':index,'shape':list(array.shape),'pixel_type':pixel_type(array.dtype),
            'value_hex':raw.hex(),'sample_sha256':hashlib.sha256(raw).hexdigest()}

def compare_frame(native,expected):
    result=[]
    if 'status' in native:result+=differences(native['status'],'ok','$.status')
    for key in ['frame','shape','pixel_type','value_hex']:
        result += differences(native.get(key),expected[key],'$.'+key)
    if 'pixels' in native:
        result += differences(native['pixels'].get('sample_sha256'),expected['sample_sha256'],'$.pixels.sample_sha256')
    return result

@contextlib.contextmanager
def source_directory(path):
    """Reference-only exact native HDF5 cwd binding, recorded in each case."""
    original=pathlib.Path.cwd()
    previous=os.environ.pop('HDF5_EXT_PREFIX',None)
    try:
        os.chdir(path)
        yield
    finally:
        os.chdir(original)
        if previous is not None:os.environ['HDF5_EXT_PREFIX']=previous

def run_probe(probe,command,request):
    with tempfile.TemporaryDirectory(prefix='dioptrin-hdf5-layout-') as folder:
        path=pathlib.Path(folder)/'request.json';path.write_text(json.dumps(request))
        process=subprocess.run([str(probe),command,str(path)],capture_output=True,text=True,timeout=30)
        if process.returncode:
            return {'status':'error','exit_code':process.returncode,'error':process.stderr[:4000]}
        try:return {'status':'completed','value':json.loads(process.stdout)}
        except Exception as error:return {'status':'error','error':str(error),'stdout':process.stdout[:4000]}

def reference_frames(path,dataset):
    from fabio.hdf5image import Hdf5Image
    reference=None
    try:
        reference=Hdf5Image();reference.read(str(path)+'::'+dataset)
        result=[]
        for index in range(reference.nframes):
            data=reference.data if index==0 else reference.getframe(index).data
            result.append(frame_projection(index,data))
        return {'status':'completed','frames':result,'reading_parity_claimed':True}
    except Exception as error:
        return {'status':'not_completed','error_type':type(error).__name__,'error':str(error),'reading_parity_claimed':False}
    finally:
        if reference is not None:reference.close()

def verify(probe,path,name,case,raw=False):
    request={'path':str(path),'dataset':name}
    if raw:request['raw_filenames']=[{'stored_filename':n,'actual_file':str(DATA/n)} for n in ['external-a.bin','external-b.bin']]
    result={'source':fingerprint(path),'dataset':name,'request':request,'input_condition':'well_formed_variant','reader_action':'implemented','status':'error'}
    with source_directory(path.parent):
        with h5py.File(path,'r') as file:
            source=file[name][...]
            source_type_hex=file[name].id.get_type().encode().hex()
            frames=source if source.ndim==3 else source[None,...]
            expected=[frame_projection(i,a) for i,a in enumerate(frames)]
            observed_fill=canonical(np.array([file[name].fillvalue],dtype=file[name].dtype)).hex()
            layout={h5py.h5d.CONTIGUOUS:'Contiguous',h5py.h5d.CHUNKED:'Chunked',h5py.h5d.COMPACT:'Compact'}[file[name].id.get_create_plist().get_layout()]
            allocated=file[name].id.get_space_status()==h5py.h5d.SPACE_STATUS_ALLOCATED
            fill_time={h5py.h5d.FILL_TIME_ALLOC:'Alloc',h5py.h5d.FILL_TIME_IFSET:'IfSet',h5py.h5d.FILL_TIME_NEVER:'Never'}[file[name].id.get_create_plist().get_fill_time()]
        reference=reference_frames(path,name)
    delta=[]
    for index,frame in enumerate(expected):delta+=differences(frame['value_hex'],case['frame_hex'][index],f'$.independent_manual_words[{index}]')
    native=run_probe(probe,'hdf5-layout',request);result.update(native=native,reference=reference,independent_expected=expected,independent_source_type_hex=source_type_hex)
    if native['status']!='completed':result['differences']=[{'field':'$.native','error':native}];return result
    output=native['value']
    delta+=differences(output.get('frames'),len(expected),'$.frames')
    images=output.get('images',[]);delta+=differences(len(images),len(expected),'$.image_count')
    for actual,expected_frame in zip(images,expected):delta+=compare_frame(actual,expected_frame)
    for metadata in output.get('metadata',[]):
        for key,value in {'source_file':str(path.resolve()),'dataset':'/'+name,'shape':list(frames[0].shape),'frames':len(expected),'first_frame':0,'layout':layout,'storage_allocated':allocated,'fill_time':fill_time}.items():
            delta+=differences(metadata.get(key),value,'$.metadata.'+key)
        delta+=differences(metadata.get('pixel_type'),pixel_type(source.dtype),'$.metadata.pixel_type')
        delta+=differences(metadata.get('source_shape'),list(source.shape),'$.metadata.source_shape')
        delta+=differences(metadata.get('little_endian'),source.dtype.byteorder!='>','$.metadata.little_endian')
        if not metadata.get('type_descriptor'):delta.append({'field':'$.metadata.type_descriptor','missing':True})
        fill=metadata.get('fill_value')
        delta+=differences(fill.get('value_hex') if fill else None,observed_fill,'$.metadata.fill_value')
        if raw:
            delta+=differences(metadata.get('external_raw',{}).get('frozen_bytes'),source.nbytes,'$.metadata.external_raw.frozen_bytes')
            windows=metadata.get('external_raw',{}).get('windows',[])
            expected_windows=[{'stored_filename':'external-a.bin','source_file':str((DATA/'external-a.bin').resolve()),'file_offset':7,'declared_bytes':13,'logical_offset':0,'effective_bytes':13},
                              {'stored_filename':'external-b.bin','source_file':str((DATA/'external-b.bin').resolve()),'file_offset':12,'declared_bytes':None,'logical_offset':13,'effective_bytes':83}]
            delta+=differences(windows,expected_windows,'$.metadata.external_raw.windows')
    if not output.get('metadata'):delta.append({'field':'$.metadata','missing':True})
    if raw:
        expected_sources=[{'path':str(p.resolve()),'bytes':p.stat().st_size} for p in [path,DATA/'external-a.bin',DATA/'external-b.bin']]
        delta+=differences(output.get('sources'),expected_sources,'$.sources')
        delta+=differences(output.get('source_bindings'),[{'requested':row['path'],'resolved':row['path']} for row in expected_sources],'$.source_bindings')
        result['reference_external_contract']={'cwd':str(path.parent.resolve()),'HDF5_EXT_PREFIX':'unset for reference call','native':'explicit exact filename bindings; no implicit HDF5 lookup'}
    if reference['status']=='completed':delta+=differences(reference['frames'],expected,'$.unchanged_fabio_frames')
    result.update(status='qualified' if not delta else 'difference',differences=delta)
    return result

def verify_attributes(probe,cases):
    commands=[{'op':'attr','path':'/'+name,'name':'original'} for name,case in cases.items() if name!='external' and 'attribute_hex' in case]
    observed=run_probe(probe,'nexus-api',{'path':str(DATA/'types.h5'),'commands':commands})
    delta=[]
    if observed['status']!='completed':return {'status':'error','native':observed}
    actions=observed['value'].get('actions',[]);delta+=differences(len(actions),len(commands),'$.attribute_count')
    for action,command in zip(actions,commands):
        name=command['path'][1:];case=cases[name]
        delta+=differences(action.get('status'),'ok',f'$.{name}.status')
        value=action.get('result',{})
        delta+=differences(value.get('value_hex'),case['attribute_hex'],f'$.{name}.original_words')
        delta+=differences(value.get('pixel_type'),pixel_type(case['dtype']),f'$.{name}.source_type')
        delta+=differences(value.get('shape'),case['shape'],f'$.{name}.source_shape')
        delta+=differences(value.get('little_endian'),not case['dtype'].startswith('>'),f'$.{name}.source_endian')
    return {'status':'qualified' if not delta else 'difference','count':len(actions),'differences':delta,'native':observed}

def audit_unsupported(probe,name):
    from fabio.hdf5image import Hdf5Image
    native=run_probe(probe,'hdf5-layout',{'path':str(DATA/'types.h5'),'dataset':name})
    with h5py.File(DATA/'types.h5') as file:
        dtype=file[name].dtype;source_declaration={'numpy_dtype':str(dtype),'numpy_descr':dtype.descr,'hdf5_type_hex':file[name].id.get_type().encode().hex(),'enum':h5py.check_enum_dtype(dtype)}
    reference=None
    try:
        reference=Hdf5Image();reference.read(str(DATA/'types.h5')+'::'+name)
        observed={'status':'completed','shape':list(reference.data.shape),'dtype':str(reference.data.dtype),'source_bytes_sha256':hashlib.sha256(canonical(reference.data)).hexdigest()}
    except Exception as error:
        observed={'status':'not_completed','error_type':type(error).__name__,'error':str(error)}
    finally:
        if reference is not None:reference.close()
    return {'dataset':name,'input_condition':'well_formed_variant','reader_action':'unsupported','source_declaration':source_declaration,'native':native,'reference':observed,
            'reason':'requires a separate exact dtype contract' if name not in ['rank1','rank4'] else 'outside native and pinned Hdf5Image 2D/3D image selection contract',
            'status':'qualified_rejection' if native['status']=='error' else 'difference'}

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--probe',type=pathlib.Path,required=True);parser.add_argument('--report',type=pathlib.Path,required=True);parser.add_argument('--fabio-source',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-2026.6.0'));args=parser.parse_args()
    from fabio import version
    from fabio import hdf5image
    assert version=='2026.6.0'
    assert subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()==PIN
    reference=pathlib.Path(inspect.getfile(hdf5image));pinned=args.fabio_source/'src/fabio/hdf5image.py'
    manifest=json.loads((DATA/'provenance.json').read_text());paths=[pathlib.Path(__file__),ROOT/'scripts/test_hdf5_layout_reference.py',ROOT/'scripts/generate_hdf5_layout_fixtures.py',ROOT/'scripts/companion_reference.py',ROOT/'tests/hdf5_layouts.rs',ROOT/'examples/api_probe.rs',DATA/'provenance.json',ROOT/'Cargo.toml',ROOT/'Cargo.lock',reference,pinned,pathlib.Path(inspect.getfile(h5py.h5d)),*sorted((ROOT/'src').rglob('*.rs')),*[DATA/f['path'] for f in manifest['files']]]
    paths += [pathlib.Path(inspect.getfile(h5py.h5t)), *pathlib.Path(h5py.__file__).parent.glob('.dylibs/*'), *pathlib.Path(h5py.__file__).parent.parent.glob('h5py.libs/*')]
    before={str(path):fingerprint(path) for path in paths};probe=args.probe.resolve()
    report={'schema':1,'contract':'native exact complex/half/bool storage and explicit raw external windows; unchanged FabIO calls separated from source/manual bit oracles; no throughput claim','reference_commit':PIN,'probe':fingerprint(probe),'sources':before,'environment':{'fabio':version,'h5py':h5py.__version__,'hdf5':h5py.version.hdf5_version,'numpy':np.__version__}}
    report['cases']=[verify(probe,DATA/('external.h5' if name=='external' else 'types.h5'),'data' if name=='external' else name,case,name=='external') for name,case in manifest['datasets'].items()]
    report['attributes']=verify_attributes(probe,manifest['datasets'])
    report['unsupported']=[audit_unsupported(probe,name) for name in ['enum','renamed','mixed-order','padded','rank1','rank4']]
    report['reference_sources_match']=fingerprint(reference)['sha256']==fingerprint(pinned)['sha256'];report['sources_unchanged']=all(fingerprint(path)==value for path,value in before.items());report['probe_unchanged']=fingerprint(probe)==report['probe']
    report['qualified']=all(case['status']=='qualified' for case in report['cases']) and report['attributes']['status']=='qualified' and all(case['status']=='qualified_rejection' for case in report['unsupported']) and all(report[key] for key in ['reference_sources_match','sources_unchanged','probe_unchanged'])
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n');print(json.dumps({'qualified':report['qualified'],'datasets':len(report['cases']),'images':sum(len(case.get('independent_expected',[])) for case in report['cases']),'attributes':report['attributes'].get('count'),'report':str(args.report)}));return 0 if report['qualified'] else 1

if __name__=='__main__':raise SystemExit(main())
