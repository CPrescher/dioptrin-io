"""Test-only bit/type/layout proof for manual NPY fixtures and native reads."""
import argparse
import hashlib
import inspect
import json
import pathlib
import subprocess
import sys
import numpy as np
from companion_reference import differences,fingerprint

ROOT=pathlib.Path(__file__).resolve().parents[1]
DATA=ROOT/'tests/data/npy-types'
PIN='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'

def canonical(array):
    array=np.asarray(array)
    return array.astype(array.dtype.newbyteorder('<'),copy=False).tobytes(order='C')

def array_frames(array):
    shape=array.shape
    plane=(1,1) if not shape else (1,shape[0]) if len(shape)==1 else shape[-2:]
    count=int(np.prod(shape[:-2],dtype=object)) if len(shape)>2 else 1
    return array.reshape((count,*plane))

def frame_projection(index,array):
    raw=canonical(array)
    return {'frame':index,'shape':list(array.shape),'pixel_type':'Bool' if array.dtype.kind=='b' else 'F16',
            'value_hex':raw.hex(),'sample_sha256':hashlib.sha256(raw).hexdigest()}

def compare_native(native,case,expected):
    delta=differences(native.get('frames'),case['frames'],'$.frames')
    metadata=native.get('metadata',{})
    for key in ['source_shape','fortran_order','pixel_type','little_endian']:
        delta+=differences(metadata.get(key),case[key],'$.metadata.'+key)
    delta+=differences(metadata.get('shape'),case['frame_shape'],'$.metadata.shape')
    delta+=differences(metadata.get('header_hex'),case['header_hex'],'$.metadata.header_hex')
    delta+=differences(metadata.get('fields',{}).get('descr'),case['dtype'],'$.metadata.fields.descr')
    images=native.get('images',[]);delta+=differences(len(images),len(expected),'$.image_count')
    for actual,wanted in zip(images,expected):
        if 'status' in actual:delta+=differences(actual['status'],'ok','$.images.status')
        for key in ['frame','shape','pixel_type','value_hex']:delta+=differences(actual.get(key),wanted[key],'$.images.'+key)
        delta+=differences(actual.get('pixels',{}).get('sample_sha256'),wanted['sample_sha256'],'$.images.pixels.sample_sha256')
    return delta

def native_read(probe,path):
    process=subprocess.run([str(probe),'npy-layout',str(path)],capture_output=True,text=True,timeout=30)
    if process.returncode:return {'status':'error','exit_code':process.returncode,'error':process.stderr[:3000]}
    try:return {'status':'completed','value':json.loads(process.stdout)}
    except Exception as error:return {'status':'error','error':str(error),'stdout':process.stdout[:3000]}

def reference_read(path,case=None):
    from fabio.numpyimage import NumpyImage
    reference=NumpyImage()
    try:
        reference.read(str(path))
        if case is None:
            return {'status':'completed','source_shape_after_reference_projection':list(reference.dataset.shape),
                    'dtype':str(reference.dataset.dtype),'source_data_sha256':hashlib.sha256(canonical(reference.dataset)).hexdigest()}
        images=[]
        for index in range(reference.nframes):
            data=reference.data if index==0 else reference.getframe(index).data
            images.append(frame_projection(index,data))
        return {'status':'completed','frames':reference.nframes,'images':images,'reading_parity_claimed':True}
    except Exception as error:
        return {'status':'not_completed','error_type':type(error).__name__,'error':str(error),'reading_parity_claimed':False}
    finally:reference.close()

def qualify_reference(reference,case,expected):
    if reference['status']=='completed':
        delta=differences(reference.get('frames'),case['frames'],'$.reference.frames')+differences(reference.get('images'),expected,'$.reference.images')
        return {'status':'match' if not delta else 'difference','differences':delta}
    if len(case['source_shape'])>3 and 0 in case['source_shape'][-2:] and reference.get('error_type')=='ValueError' and reference.get('error')==f"cannot reshape array of size 0 into shape ({case['source_shape'][-2]},{case['source_shape'][-1]})":
        return {'status':'reference_bug_valid_empty_rank4','differences':[],
                'qualification':'NumPy loads original complete empty array; pinned slice_dataset uses reshape(-1,0,N), which cannot infer the missing axis. Native retains explicit original leading dimensions; unchanged API reading did not complete.'}
    return {'status':'unqualified_reference_failure','differences':[{'field':'$.reference','observation':reference}]}

def verify(probe,name,case):
    path=DATA/name;array=np.load(path,allow_pickle=False)
    expected=[frame_projection(i,frame) for i,frame in enumerate(array_frames(array))]
    delta=differences(list(array.shape),case['source_shape'],'$.np_load.source_shape')
    delta+=differences(array.dtype.str,case['dtype'],'$.np_load.dtype')
    delta+=differences([frame['value_hex'] for frame in expected],case['frame_hex'],'$.manual_words')
    delta+=differences(array.tobytes(order='F' if case['fortran_order'] else 'C').hex(),case['source_data_hex'],'$.manual_source_storage')
    native=native_read(probe,path)
    if native['status']=='completed':delta+=compare_native(native['value'],case,expected)
    else:delta.append({'field':'$.native','observation':native})
    reference=reference_read(path,case);qualification=qualify_reference(reference,case,expected);delta+=qualification['differences']
    return {'source':fingerprint(path),'input_condition':'well_formed_empty_array' if array.size==0 else 'well_formed_variant','reader_action':'implemented',
            'status':'qualified' if not delta else 'difference','case':case,'independent_expected':expected,'native':native,'reference':reference,'reference_qualification':qualification,'differences':delta}

def audit_rejected(probe,name,case,malformed):
    path=DATA/name;native=native_read(probe,path)
    try:
        data=np.load(path,allow_pickle=False)
        source={'status':'completed','shape':list(data.shape),'dtype':str(data.dtype),'data_sha256':hashlib.sha256(canonical(data)).hexdigest()}
    except Exception as error:source={'status':'not_completed','error_type':type(error).__name__,'error':str(error)}
    reference=reference_read(path)
    return {'source':fingerprint(path),'input_condition':case['input_condition'],'reader_action':'reject_malformed' if malformed else 'unsupported',
            'status':'qualified_rejection' if native['status']=='error' else 'difference','case':case,'numpy_load':source,'reference':reference,
            'classification':'malformed_or_truncated' if malformed else 'object_policy_no_unpickling' if case['kind']=='object' else 'reference_platform_dtype_unavailable' if case['kind']=='float128' else 'valid_unsupported_source_dtype','native':native}

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--probe',type=pathlib.Path,required=True);parser.add_argument('--report',type=pathlib.Path,required=True);parser.add_argument('--fabio-source',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-2026.6.0'));args=parser.parse_args()
    import fabio.numpyimage
    import fabio
    assert fabio.version=='2026.6.0';assert subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()==PIN
    manifest=json.loads((DATA/'provenance.json').read_text());reference=pathlib.Path(inspect.getfile(fabio.numpyimage));pinned=args.fabio_source/'src/fabio/numpyimage.py'
    source_paths=[pathlib.Path(__file__),ROOT/'scripts/test_npy_type_reference.py',ROOT/'scripts/generate_npy_type_fixtures.py',ROOT/'scripts/companion_reference.py',ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'examples/api_probe.rs',DATA/'provenance.json',reference,pinned,
                  *sorted((ROOT/'src').rglob('*.rs')),*[DATA/name for group in ['files','unsupported','malformed'] for name in manifest[group]]]
    source_paths += [pathlib.Path(inspect.getfile(np.lib.format)),pathlib.Path(np._core._multiarray_umath.__file__)]
    paths={str(path):fingerprint(path) for path in source_paths};probe=args.probe.resolve()
    report={'schema':1,'contract':'original Bool/binary16 words and source shapes/orders; empty arrays expose no fabricated samples; unchanged FabIO failures remain failures; no object unpickling','reference_commit':PIN,'probe':fingerprint(probe),'sources':paths,'environment':{'python':sys.version,'numpy':np.__version__,'fabio':fabio.version}}
    report['cases']=[verify(probe,name,case) for name,case in manifest['files'].items()]
    report['rejections']=[audit_rejected(probe,name,case,group=='malformed') for group in ['unsupported','malformed'] for name,case in manifest[group].items()]
    report['sources_unchanged']=all(fingerprint(path)==value for path,value in paths.items());report['probe_unchanged']=fingerprint(probe)==report['probe'];report['reference_source_matches']=fingerprint(reference)['sha256']==fingerprint(pinned)['sha256']
    report['qualified']=all(case['status']=='qualified' for case in report['cases']) and all(case['status']=='qualified_rejection' for case in report['rejections']) and all(report[key] for key in ['sources_unchanged','probe_unchanged','reference_source_matches'])
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n')
    print(json.dumps({'qualified':report['qualified'],'source_arrays':len(report['cases']),'image_frames':sum(len(row['independent_expected']) for row in report['cases']),'reference_completed':sum(row['reference']['status']=='completed' for row in report['cases']),'reference_bugs':sum(row['reference_qualification']['status']=='reference_bug_valid_empty_rank4' for row in report['cases']),'rejections':len(report['rejections']),'report':str(args.report)}));return 0 if report['qualified'] else 1

if __name__=='__main__':raise SystemExit(main())
