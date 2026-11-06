"""Bounded unlimited VDS proof: independent source coordinates and unchanged references."""
import argparse,copy,hashlib,importlib,inspect,itertools,json,pathlib,subprocess,sys
import h5py
import numpy as np
import vds_printf_reference as common
ROOT=pathlib.Path(__file__).resolve().parents[1]
DATA=ROOT/'tests/data/vds-unlimited'

def points(selection):
    kind=selection['kind'];shape=selection['shape']
    if len(shape)!=3 or any(type(v) is not int or not 0<=v<=10000 for v in shape):raise ValueError('unbounded effective shape')
    if kind=='All':return list(itertools.product(*(range(v) for v in shape)))
    if kind=='Regular':
        start,stride,count,block=[selection[k] for k in ['start','stride','count','block']]
        if any(type(v) is not int or not 0<v<=10000 for v in [*stride,*count,*block]):raise ValueError('unbounded effective selection')
        axes=[sorted({start[a]+i*stride[a]+j for i in range(count[a]) for j in range(block[a])}) for a in range(3)]
        result=list(itertools.product(*axes))
    elif kind=='Irregular':
        result=sorted({p for lo,hi in selection['blocks'] for p in itertools.product(*(range(lo[a],hi[a]+1) for a in range(3)))})
    else:raise ValueError('unknown effective selection')
    if any(not 0<=p[a]<shape[a] for p in result for a in range(3)):raise ValueError('effective coordinate out of bounds')
    return result

def desired_target(record,shape):
    start,stride,count,block=record['virtual'];axes=[]
    for a in range(3):
        n=(shape[a]-start[a]+stride[a]-1)//stride[a] if count[a]==h5py.h5s.UNLIMITED else count[a]
        axes.append(sorted({start[a]+i*stride[a]+j for i in range(n) for j in range(block[a]) if start[a]+i*stride[a]+j<shape[a]}))
    return list(itertools.product(*axes))

def compare_native(native,case,config):
    diff=common.delta(native,{'kind':'hdf5_layout','frames':case['shape'][0],'links':[]})
    if not isinstance(native.get('native_hdf5_version'),list) or len(native['native_hdf5_version'])!=3:diff.append({'field':'native_hdf5_version','error':'missing version'})
    images=native.get('images',[]);wanted=common.expected_frames(case)
    if len(images)!=len(wanted):diff.append({'field':'images.count','reference':len(wanted),'native':len(images)})
    for index,(a,b) in enumerate(zip(images,wanted)):
        diff += [{**d,'field':f'images.{index}.'+d['field']} for d in common.delta(a,{'status':'ok',**b})]
    meta=native.get('metadata',[])
    if len(meta)!=1:return diff+[{'field':'metadata.count','reference':1,'native':len(meta)}]
    diff+=common.delta(meta[0],{'source_shape':case['shape'],'shape':case['shape'][1:],'frames':case['shape'][0],'first_frame':0,'pixel_type':'U64','little_endian':True})
    paths={config['path'],*(b['actual_file'] for b in config['vds_filenames'])}

    actual=sorted(native.get('sources',[]),key=lambda v:v['path']);expected=sorted([{'path':p,'bytes':pathlib.Path(p).stat().st_size} for p in paths],key=lambda v:v['path'])
    if not common.same(actual,expected):diff.append({'field':'sources','native':actual,'reference':expected})
    bindings=sorted(native.get('source_bindings',[]),key=lambda v:v['requested']);expected_bindings=sorted([{'requested':p,'resolved':p} for p in paths],key=lambda v:v['requested'])
    if not common.same(bindings,expected_bindings):diff.append({'field':'source_bindings','native':bindings,'reference':expected_bindings})
    graph=meta[0].get('virtual_dataset',{})
    diff+=common.delta(graph,{'frozen_shape':case['shape'],'declared_shape':[0,*case['shape'][1:]],'master_max_shape':[2**64-1,*case['shape'][1:]],'source_file':config['path'],'dataset':'/data','unmapped_regions':'DeclaredFill' if case['unmapped_regions']=='declared_fill' else 'Reject','nested_nodes':[]})
    for key in ['node_id','fill_value','effective_fill_regions','unmapped_pixels_by_frame','unmapped_pixels_per_frame']:
        if key not in graph:diff.append({'field':'virtual_dataset.'+key,'error':'required provenance missing'})
    mappings=graph.get('mappings',[])
    if len(mappings)!=len(case['mappings']):diff.append({'field':'mappings.count','reference':len(case['mappings']),'native':len(mappings)})
    with h5py.File(config['path'],'r') as file:
        prop=file['/data'].id.get_create_plist()
        for index,(actual,record) in enumerate(zip(mappings,case['mappings'])):
            declarations={'source_selection':common.selection(prop.get_virtual_srcspace(index)),'virtual_selection':common.selection(prop.get_virtual_vspace(index))}
            required={'stored_filename':record['file'],'declared_dataset':'/data','source_file':str((DATA/record['file']).resolve()),'resolved_dataset':'/data','source_shape':record['source_shape'],'source_pixel_type':'U64','source_little_endian':True,'storage_allocated':True,'nested_node':None,'printf_expansion':None,'selected_elements':len(record['selected_source_coordinates']),**declarations}
            diff += [{**d,'field':f'mapping.{index}.'+d['field']} for d in common.delta(actual,required)]
            for key,wanted_points in [('frozen_source_selection',[tuple(v) for v in record['selected_source_coordinates']]),('frozen_virtual_selection',desired_target(record,case['shape']))]:
                expected_shape=record['source_shape'] if key=='frozen_source_selection' else case['shape']
                diff += [{**d,'field':f'mapping.{index}.{key}.'+d['field']} for d in common.delta(actual.get(key,{}),{'shape':expected_shape,'maximum_shape':expected_shape})]
                try:found=points(actual[key])
                except (KeyError,ValueError,TypeError) as error:diff.append({'field':f'mapping.{index}.{key}','error':str(error)});continue
                if found!=wanted_points:diff.append({'field':f'mapping.{index}.{key}.coordinates','reference':wanted_points,'native':found})
    return diff

def producer_controls():
    from generate_vds_unlimited_fixtures import space,INF
    spec=[[0,0,0],[2,2,3],[INF,1,1],[1,2,3]];results=[]
    for op in ['OR','XOR','NOTB','AND']:
        s=space((0,2,3),spec)
        try:s.select_hyperslab((1,0,0),(1,1,1),stride=(1,2,3),block=(1,2,3),op=getattr(h5py.h5s,'SELECT_'+op))
        except (ValueError,RuntimeError) as error:results.append({'operation':op,'result':'producer_rejected','error':str(error)})
        else:results.append({'operation':op,'result':'finite_result','selection_type':s.get_select_type(),'selected_points':s.get_select_npoints()})
    s=space((0,2,3),spec)
    try:s.select_hyperslab((1,0,0),(INF,1,1),stride=(2,2,3),block=(1,2,3),op=h5py.h5s.SELECT_OR)
    except (ValueError,RuntimeError) as error:results.append({'operation':'OR_infinite','result':'producer_rejected','error':str(error)})
    else:raise AssertionError('reference unexpectedly permits infinite union')
    return results

def gate(summary):
    return not common.delta(summary,{'exact_cases':6,'exact_frames':27,'strict_missing_mapped_samples_cases':1,'differences':0,'errors':0,'imports_unchanged':True,'sources_stable':True,'probe_stable':True})

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--probe',type=pathlib.Path,required=True);p.add_argument('--fabio-source',type=pathlib.Path,required=True);p.add_argument('--report',type=pathlib.Path,required=True);args=p.parse_args()
    import fabio
    if fabio.version!='2026.6.0' or subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()!=common.PIN:p.error('requires pinned FabIO source/environment')
    actual=pathlib.Path(inspect.getfile(importlib.import_module('fabio.hdf5image')));pinned=args.fabio_source/'src/fabio/hdf5image.py';gitbytes=subprocess.check_output(['git','-C',str(args.fabio_source),'show',f'{common.PIN}:src/fabio/hdf5image.py'])
    imports={'actual':common.fingerprint(actual),'pinned':common.fingerprint(pinned),'unchanged':actual.read_bytes()==pinned.read_bytes()==gitbytes}
    sources=[ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'build.rs',ROOT/'examples/api_probe.rs',actual,pinned,ROOT/'scripts/vds_printf_reference.py',ROOT/'scripts/vds_unlimited_reference.py',ROOT/'scripts/test_vds_unlimited_reference.py',ROOT/'scripts/generate_vds_unlimited_fixtures.py',ROOT/'tests/vds_unlimited.rs']+sorted((ROOT/'src').rglob('*.rs'))+sorted(DATA.glob('*'))
    primary=pathlib.Path.home()/'.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hdf5-metno-src-0.10.4/ext/hdf5/src'
    sources+=[primary/'H5Dvirtual.c',primary/'H5Shyper.c']
    for module in [h5py.h5p,h5py.h5s,h5py.h5d,importlib.import_module('h5py._selector')]:sources.append(pathlib.Path(module.__file__))
    sources+=sorted((pathlib.Path(h5py.__file__).parent/'.dylibs').glob('*hdf5*'))
    before={str(f):common.fingerprint(f) for f in sources};probe_before=common.fingerprint(args.probe);manifest=json.loads((DATA/'provenance.json').read_text());events=[];errors=[]
    for name,case in manifest['cases'].items():
        config=common.request(case,DATA)
        try:
            result=common.run_probe(args.probe,config);refs={}
            for backend in ['h5py','fabio']:
                reference=common.run_reference({'path':config['path'],'dataset':'/data','shape':case['shape']},backend)
                differences=common.compare_reference(reference,case)
                if differences:raise ValueError(f'{backend} differs from authored source coordinates: {differences}')
                refs[backend]=reference
            if case['expect']=='reject':
                differences=[] if result['status']=='error' and 'exhausted source cannot become declared fill' in result['error'].lower() else [{'field':'strict_missing_mapped_samples','native':result}]
                classification='strict_missing_mapped_samples: the shorter original infinite mapping exhausts before the global domain; reference supplies declared fill, native does not substitute missing source counts'
            else:
                if result['status']!='ok':raise ValueError(result['error'])
                differences=compare_native(result['value'],case,config);classification='exact_complete_bound_source_selection'
            events.append({'case':name,'request':config,'native':result,'independent':common.expected_frames(case),'reference':refs,'classification':classification,'differences':differences})
        except Exception as error:errors.append({'case':name,'type':type(error).__name__,'error':str(error)})
    after={str(f):common.fingerprint(f) for f in sources};probe_after=common.fingerprint(args.probe)
    summary={'exact_cases':sum(v['classification']=='exact_complete_bound_source_selection' for v in events),'exact_frames':sum(len(v['independent']) for v in events if v['classification']=='exact_complete_bound_source_selection'),'strict_missing_mapped_samples_cases':sum(v['classification'].startswith('strict_missing') for v in events),'differences':sum(len(v['differences']) for v in events),'errors':len(errors),'imports_unchanged':imports['unchanged'],'sources_stable':before==after,'probe_stable':probe_before==probe_after}
    report={'schema':1,'passed':gate(summary),'scope':'finite source-bound projection of frame-axis unlimited declarations; no source-discovered missing-count fill','summary':summary,'pin':common.PIN,'imports':imports,'reference_versions':{'fabio':fabio.version,'numpy':np.__version__,'h5py':h5py.__version__,'hdf5':h5py.version.hdf5_version},'probe_before':probe_before,'probe_after':probe_after,'sources_before':before,'sources_after':after,'producer_controls':producer_controls(),'rejected_cardinality_controls':manifest['producer_controls'],'events':events,'errors':errors}
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'passed':report['passed'],'summary':summary,'report':str(args.report)}));return 0 if report['passed'] else 1
if __name__=='__main__':raise SystemExit(main())
