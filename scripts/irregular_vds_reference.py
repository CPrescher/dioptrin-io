"""Test-only exact finite VDS selections: manual words, h5py and unchanged FabIO."""
import argparse,hashlib,inspect,json,pathlib,subprocess,sys
import h5py
import numpy as np
from companion_reference import differences,fingerprint
from hdf5_layout_reference import canonical,run_probe
ROOT=pathlib.Path(__file__).resolve().parents[1]
DATA=ROOT/'tests/data/vds-irregular'
PIN='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'
TYPES={'uint64':'U64','int16':'I16','float16':'F16','float32':'F32','float64':'F64','complex64':'C32','complex128':'C64','bool':'Bool'}

def frame(index,array):
    raw=canonical(array)
    return {'frame':index,'shape':list(array.shape),'pixel_type':TYPES[array.dtype.name],'value_hex':raw.hex(),'sample_sha256':hashlib.sha256(raw).hexdigest()}

def record(space):
    shape=list(space.get_simple_extent_dims());maximum=list(space.get_simple_extent_dims(maxdims=True))
    if space.get_select_type()==h5py.h5s.SEL_ALL:return {'kind':'All','shape':shape,'maximum_shape':maximum}
    if space.is_regular_hyperslab():
        start,stride,count,block=space.get_regular_hyperslab()
        return {'kind':'Regular','shape':shape,'maximum_shape':maximum,'start':list(start),'stride':list(stride),'count':list(count),'block':list(block)}
    return {'kind':'Irregular','shape':shape,'maximum_shape':maximum,'blocks':space.get_select_hyper_blocklist().tolist()}

def bounds(space):
    begin,end=space.get_select_bounds()
    return list(begin),[b-a+1 for a,b in zip(begin,end)]

def compare_images(native,expected):
    delta=differences(len(native),len(expected),'$.images.length')
    for actual,wanted in zip(native,expected):
        delta+=differences(actual.get('status'),'ok','$.images.status')
        for key in ['frame','shape','pixel_type','value_hex']:delta+=differences(actual.get(key),wanted[key],'$.images.'+key)
        delta+=differences(actual.get('pixels',{}).get('sample_sha256'),wanted['sample_sha256'],'$.images.pixels.sample_sha256')
    return delta

def compare_coverage(native,counts):
    constant=counts[0] if all(n==counts[0] for n in counts) else None
    return differences(native.get('unmapped_pixels_by_frame'),counts,'$.coverage.by_frame')+differences(native.get('unmapped_pixels_per_frame'),constant,'$.coverage.constant')

def reference(path):
    from fabio.hdf5image import Hdf5Image
    image=Hdf5Image()
    try:
        image.read(str(path)+'::/data')
        return {'status':'completed','frames':[frame(i,image.data if i==0 else image.getframe(i).data) for i in range(image.nframes)]}
    except Exception as error:return {'status':'not_completed','error_type':type(error).__name__,'error':str(error),'reading_parity_claimed':False}
    finally:image.close()

def request(case,actual=None):
    filename=case['source_file']
    return {'path':str(DATA/case['master']),'dataset':'/data','vds_filenames':[{'stored_filename':filename,'actual_file':str(actual or DATA/filename)}],
            'unmapped_regions':'declared_fill','follow_external':filename=='bound.h5'}

def verify(probe,name,case):
    path=DATA/case['master'];native=run_probe(probe,'hdf5-layout',request(case))
    result={'name':name,'input_condition':'well_formed_variant','reader_action':'implemented','request':request(case),'source':fingerprint(path),'native':native,'status':'error'}
    with h5py.File(path,'r') as file:
        dataset=file['data'];expected=[frame(i,a) for i,a in enumerate(dataset[...])];plist=dataset.id.get_create_plist()
        selected_source=plist.get_virtual_srcspace(0);selected_virtual=plist.get_virtual_vspace(0)
        source_record=record(selected_source);virtual_record=record(selected_virtual)
        source_origin,source_bounds=bounds(selected_source);virtual_origin,virtual_bounds=bounds(selected_virtual)
        maximum=list(dataset.id.get_space().get_simple_extent_dims(maxdims=True))
        fill=canonical(np.array([dataset.fillvalue],dtype=dataset.dtype)).hex()
    delta=differences([a['value_hex'] for a in expected],case['frame_hex'],'$.manual_source_words')
    observed_reference=reference(path);result.update(independent_expected=expected,reference=observed_reference)
    if observed_reference['status']=='completed':delta+=differences(observed_reference['frames'],expected,'$.unchanged_reference')
    else:delta.append({'field':'$.unchanged_reference','observation':observed_reference})
    if native['status']!='completed':result['differences']=delta+[{'field':'$.native','observation':native}];return result
    output=native['value'];delta+=differences(output.get('frames'),case['shape'][0],'$.frames');delta+=compare_images(output.get('images',[]),expected)
    meta=output.get('metadata',[])
    delta+=differences(len(meta),1,'$.metadata.count')
    if meta:
        m=meta[0];v=m.get('virtual_dataset',{})
        for key,wanted in {'pixel_type':TYPES[np.dtype(case['dtype']).name],'shape':case['shape'][1:],'source_shape':case['shape'],'little_endian':not case['dtype'].startswith('>')}.items():delta+=differences(m.get(key),wanted,'$.metadata.'+key)
        for key,wanted in {'declared_shape':case['shape'],'frozen_shape':case['shape'],'master_max_shape':maximum,'unmapped_regions':'DeclaredFill'}.items():delta+=differences(v.get(key),wanted,'$.virtual_dataset.'+key)
        delta+=compare_coverage(v,case['unmapped_by_frame'])
        mappings=v.get('mappings',[]);delta+=differences(len(mappings),1,'$.mapping.count')
        if mappings:
            mapping=mappings[0]
            for key,wanted in {'stored_filename':case['source_file'],'declared_dataset':case['source_dataset'],'source_shape':case['source_shape'],
                               'source_selection':source_record,'virtual_selection':virtual_record,'source_origin':source_origin,'virtual_origin':virtual_origin,
                               'source_bounding_shape':source_bounds,'virtual_bounding_shape':virtual_bounds,'mapped_shape':virtual_bounds,
                               'selection_layout':'FiniteSelection','selected_elements':selected_source.get_select_npoints(),'storage_allocated':True,'source_layout':'Chunked','resolved_dataset':'/u64' if case['source_file']=='bound.h5' else case['source_dataset'],'source_pixel_type':TYPES[np.dtype(case['dtype']).name],
                               'source_little_endian':not case['dtype'].startswith('>')}.items():delta+=differences(mapping.get(key),wanted,'$.mapping.'+key)
            expected_source=(DATA/'source.h5').resolve();delta+=differences(mapping.get('source_file'),str(expected_source),'$.mapping.source_file')
        delta+=differences(m.get('fill_value',{}).get('value_hex'),fill,'$.fill.original_word')
    names=[case['master'],case['source_file']]+(['relay.h5','source.h5'] if case['source_file']=='bound.h5' else [])
    expected_sources=[{'path':str((DATA/n).resolve()),'bytes':(DATA/n).stat().st_size} for n in names]
    delta+=differences(output.get('sources'),expected_sources,'$.sources')
    delta+=differences(output.get('source_bindings'),[{'requested':row['path'],'resolved':row['path']} for row in expected_sources],'$.source_bindings')
    if case['source_file']=='bound.h5':
        expected_links=[{'source':str((DATA/'bound.h5').resolve()),'path':'/data','external':True,'raw_hex':b'\0relay.h5\0/data\0'.hex()},
                        {'source':str((DATA/'relay.h5').resolve()),'path':'/data','external':True,'raw_hex':b'\0source.h5\0/u64\0'.hex()}]
        delta+=differences(output.get('links'),expected_links,'$.original_link_controls')
    else:delta+=differences(output.get('links'),[],'$.links')
    result.update(status='qualified' if not delta else 'difference',differences=delta)
    return result

def rejected(probe,name,request_,condition,reason):
    native=run_probe(probe,'hdf5-layout',request_)
    return {'name':name,'input_condition':condition,'reader_action':'reject','reason':reason,'request':request_,'native':native,'status':'qualified_rejection' if native['status']=='error' else 'difference'}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--probe',type=pathlib.Path,required=True);p.add_argument('--report',type=pathlib.Path,required=True);p.add_argument('--fabio-source',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-2026.6.0'));args=p.parse_args()
    import fabio,fabio.hdf5image
    assert fabio.version=='2026.6.0';assert subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()==PIN
    manifest=json.loads((DATA/'provenance.json').read_text());probe=args.probe.resolve();actual=pathlib.Path(inspect.getfile(fabio.hdf5image));pinned=args.fabio_source/'src/fabio/hdf5image.py'
    paths=[pathlib.Path(__file__),ROOT/'scripts/test_irregular_vds_reference.py',ROOT/'scripts/generate_irregular_vds_fixtures.py',ROOT/'scripts/hdf5_layout_reference.py',ROOT/'scripts/companion_reference.py',ROOT/'tests/vds_irregular.rs',ROOT/'tests/vds.rs',ROOT/'examples/api_probe.rs',ROOT/'Cargo.toml',ROOT/'Cargo.lock',DATA/'provenance.json',actual,pinned,
           *sorted((ROOT/'src').rglob('*.rs')),*[DATA/n for n in manifest['files']],pathlib.Path(inspect.getfile(h5py.h5d)),pathlib.Path(inspect.getfile(h5py.h5s)),*pathlib.Path(h5py.__file__).parent.glob('.dylibs/*')]
    authority=[]
    for filename in ['H5Sselect.c','H5Shyper.c','H5Spublic.h']:
        matches=list((pathlib.Path.home()/'.cargo/registry/src').glob('*/hdf5-metno-src-0.10.4/ext/hdf5/src/'+filename));paths+=matches;authority+=matches
    before={str(path):fingerprint(path) for path in paths}
    report={'schema':1,'contract':'finite rank3 native HDF5 iteration-order projection with exact bound sources, defined-fill only under explicit policy, no detector-sized scratch/implicit source lookup','reference_commit':PIN,'probe':fingerprint(probe),'sources':before,'environment':{'python':sys.version,'fabio':fabio.version,'h5py':h5py.__version__,'hdf5':h5py.version.hdf5_version,'numpy':np.__version__},'native_projection_authority':{'symbol':'H5Sselect_project_intersection','since':'HDF5 1.10.6','bundled_source':[fingerprint(path) for path in authority]}}
    report['cases']=[verify(probe,n,c) for n,c in manifest['cases'].items()]
    base=manifest['cases']['u64'];negative=[]
    no_policy=request(base);no_policy.pop('vds_filenames');negative.append(rejected(probe,'default_vds_reject',no_policy,'well_formed_variant','explicit source-map policy required'))
    for n in ['partial.h5','short.h5','absent.h5']:negative.append(rejected(probe,n,request(base,DATA/n),'incomplete_exact_source_graph','mapped module must exist, contain every selected point and be fully allocated'))
    overlap=request(base);overlap['path']=str(DATA/'overlap.h5');negative.append(rejected(probe,'overlap',overlap,'well_formed_variant','overlapping mapping precedence remains explicitly unsupported'))
    cap=request(base);cap['path']=str(DATA/'block-limit.h5');cap['vds_filenames']=[{'stored_filename':'large-source.h5','actual_file':str(DATA/'large-source.h5')}];negative.append(rejected(probe,'selection_block_limit',cap,'well_formed_variant','selection exceeds documented complexity budget'))
    gap=request(manifest['cases']['cross-frame']);gap['unmapped_regions']='reject';negative.append(rejected(probe,'undeclared_fill_policy',gap,'well_formed_variant','gaps require explicit DeclaredFill'))
    report['rejections']=negative;report['sources_unchanged']=all(fingerprint(path)==value for path,value in before.items());report['probe_unchanged']=fingerprint(probe)==report['probe'];report['reference_source_matches']=fingerprint(actual)['sha256']==fingerprint(pinned)['sha256']
    report['qualified']=all(row['status']=='qualified' for row in report['cases']) and all(row['status']=='qualified_rejection' for row in negative) and all(report[k] for k in ['sources_unchanged','probe_unchanged','reference_source_matches'])
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n');print(json.dumps({'qualified':report['qualified'],'cases':len(report['cases']),'all_word_frames':sum(len(row['independent_expected']) for row in report['cases']),'rejections':len(negative),'report':str(args.report)}));return 0 if report['qualified'] else 1

if __name__=='__main__':raise SystemExit(main())
