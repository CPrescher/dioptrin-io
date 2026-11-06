"""Explicit printf VDS source-count proof; no reference object or reader patched.

Default h5py/FabIO reads run in separate fresh processes. A separately labelled
warm-child control diagnoses the pinned nested-printf extent cache defect; it is
never used to manufacture primary reference equality. Expected words are CC0
source-byte assembly, independent of all VDS reads.
"""
import argparse
import copy
import hashlib
import importlib
import inspect
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

import h5py
import numpy as np

ROOT=pathlib.Path(__file__).resolve().parents[1]
DATA=ROOT/'tests/data/vds-printf'
PIN='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'
TYPE_NAMES={'bool':'Bool','float16':'F16','uint64':'U64','int64':'I64','int16':'I16','complex64':'C32'}


def sha(data):return hashlib.sha256(data).hexdigest()


def fingerprint(path):
    path=pathlib.Path(path);before=path.stat();data=path.read_bytes();after=path.stat()
    identity=lambda v:(v.st_dev,v.st_ino,v.st_size,v.st_mtime_ns)
    if identity(before)!=identity(after):raise ValueError('source changed during fingerprint')
    return {'path':str(path.resolve()),'bytes':before.st_size,'sha256':sha(data),'device':before.st_dev,'inode':before.st_ino,'mtime_ns':before.st_mtime_ns}


def same(a,b):
    if type(a) is not type(b):return False
    if isinstance(a,list):return len(a)==len(b) and all(same(x,y) for x,y in zip(a,b))
    if isinstance(a,dict):return a.keys()==b.keys() and all(same(a[k],b[k]) for k in a)
    return a==b


def delta(actual,wanted):
    if not isinstance(actual,dict):return [{'field':'object','reference':wanted,'native':actual}]
    return [{'field':k,'reference':v,'native':actual.get(k)} for k,v in wanted.items() if k not in actual or not same(actual[k],v)]


def canonical(array):
    array=np.asarray(array)
    return array.astype(array.dtype.newbyteorder('<'),copy=False).tobytes(order='C')


def frame(index,array):
    raw=canonical(array)
    return {'frame':index,'shape':list(array.shape),'pixel_type':TYPE_NAMES[array.dtype.name],'pixels':{'dtype':array.dtype.name,'sample_sha256':sha(raw)},'value_hex':raw.hex()}


def expected_frames(case):
    dtype=np.dtype(case['dtype']);result=[]
    for index,raw in enumerate(case['frame_hex']):
        result.append({'frame':index,'shape':case['shape'][1:],'pixel_type':TYPE_NAMES[dtype.name],'pixels':{'dtype':dtype.name,'sample_sha256':sha(bytes.fromhex(raw))},'value_hex':raw})
    return result


def request(case,directory=DATA):
    plans=copy.deepcopy(case['printf_mappings'])
    for plan in plans:
        if plan['source_file'] is not None:plan['source_file']=str((directory/plan['source_file']).resolve())
    return {'path':str((directory/case['master']).resolve()),'dataset':case['dataset'],'vds_filenames':[{'stored_filename':v['stored_filename'],'actual_file':str((directory/v['actual_file']).resolve())} for v in case['bindings']],
            'printf_mappings':plans,'unmapped_regions':case['unmapped_regions']}


def parse_pattern(pattern,index):
    """Only grammar actually implemented by pinned native/reference libraries."""
    result=[];at=0
    while at<len(pattern):
        if pattern[at]!='%':result.append(pattern[at]);at+=1;continue
        if at+1==len(pattern):raise ValueError('trailing percent')
        if pattern[at+1]=='%':result.append('%')
        elif pattern[at+1]=='b':result.append(str(index))
        else:raise ValueError('unsupported HDF5 pattern token')
        at+=2
    return ''.join(result)


def selection(space):
    shape=list(space.get_simple_extent_dims());maximum=list(space.get_simple_extent_dims(maxdims=True));kind=space.get_select_type()
    if kind==h5py.h5s.SEL_ALL:return {'kind':'All','shape':shape,'maximum_shape':maximum}
    if kind!=h5py.h5s.SEL_HYPERSLABS:raise ValueError('unsupported source declaration selection')
    if space.is_regular_hyperslab():
        start,stride,count,block=space.get_regular_hyperslab()
        return {'kind':'Regular','shape':shape,'maximum_shape':maximum,'start':list(start),'stride':list(stride),'count':list(count),'block':list(block)}
    return {'kind':'Irregular','shape':shape,'maximum_shape':maximum,'blocks':space.get_select_hyper_blocklist().tolist()}


def declaration(owner,dataset,index,plan):
    with h5py.File(owner,'r') as file:
        prop=file[dataset].id.get_create_plist()
        filename=prop.get_virtual_filename(index);source_dataset=prop.get_virtual_dsetname(index)
        if isinstance(filename,bytes):filename=filename.decode('utf8')
        if isinstance(source_dataset,bytes):source_dataset=source_dataset.decode('utf8')
        src=selection(prop.get_virtual_srcspace(index));virtual=selection(prop.get_virtual_vspace(index))
    return {'source_file':str(pathlib.Path(owner).resolve()),'dataset':dataset,'mapping_index':index,'stored_filename':filename,'declared_dataset':source_dataset,'source_selection':src,'virtual_selection':virtual,'plan':plan,'blocks':1 if plan is None else plan['blocks']}


def compare_native(native,case,config):
    wanted=expected_frames(case);diff=delta(native,{'kind':'hdf5_layout','frames':len(wanted)})
    if not isinstance(native.get('native_hdf5_version'),list) or len(native['native_hdf5_version'])!=3:diff.append({'field':'native_hdf5_version','error':'missing exact library version'})
    actual=native.get('images',[])
    if len(actual)!=len(wanted):diff.append({'field':'images.count','reference':len(wanted),'native':len(actual)})
    for index,(a,b) in enumerate(zip(actual,wanted)):
        diff += [{**d,'field':f'images.{index}.'+d['field']} for d in delta(a,{'status':'ok',**b})]
    metadata=native.get('metadata',[])
    if len(metadata)!=1:return diff+[{'field':'metadata.count','reference':1,'native':len(metadata)}]
    meta=metadata[0];diff += [{**d,'field':'metadata.'+d['field']} for d in delta(meta,{'source_shape':case['shape'],'shape':case['shape'][1:],'frames':len(wanted),'first_frame':0,'pixel_type':TYPE_NAMES[np.dtype(case['dtype']).name],'little_endian':np.dtype(case['dtype']).byteorder!='>'})]
    paths={str(pathlib.Path(config['path']).resolve()),*(str(pathlib.Path(b['actual_file']).resolve()) for b in config['vds_filenames'])}
    expected_sources=sorted([{'path':p,'bytes':pathlib.Path(p).stat().st_size} for p in paths],key=lambda v:v['path'])
    actual_sources=native.get('sources')
    if not isinstance(actual_sources,list) or sorted(actual_sources,key=lambda v:v.get('path',''))!=expected_sources:diff.append({'field':'sources','reference':expected_sources,'native':actual_sources})
    expected_bindings=sorted([{'requested':p,'resolved':p} for p in paths],key=lambda v:v['requested'])
    actual_bindings=native.get('source_bindings')
    if not isinstance(actual_bindings,list) or sorted(actual_bindings,key=lambda v:v.get('requested',''))!=expected_bindings:diff.append({'field':'source_bindings','reference':expected_bindings,'native':actual_bindings})
    diff+=delta(native,{'links':[]})
    graph=meta.get('virtual_dataset',{})
    original_root=declaration(config['path'],config['dataset'],0,None)['virtual_selection']
    diff += [{**d,'field':'virtual_dataset.'+d['field']} for d in delta(graph,{'frozen_shape':case['shape'],'declared_shape':original_root['shape'],'master_max_shape':original_root['maximum_shape'],'source_file':config['path'],'dataset':config['dataset'],'unmapped_regions':'DeclaredFill' if config['unmapped_regions']=='declared_fill' else 'Reject'})]
    nodes=[graph,*graph.get('nested_nodes',[])];expansions=[]
    for node in nodes:
        for mapping in node.get('mappings',[]):
            if mapping.get('printf_expansion') is not None:expansions.append((node,mapping,mapping['printf_expansion']))
    plans=config['printf_mappings'];expected_count=sum(p['blocks'] for p in plans) if plans else 1
    if len(expansions)!=expected_count:diff.append({'field':'printf_expansions.count','reference':expected_count,'native':len(expansions)})
    seen=set()
    for node,mapping,expanded in expansions:
        decl=expanded.get('declaration',{});block=expanded.get('block_index')
        plan_matches=[p for p in plans if p['dataset']==node.get('dataset') and (p['source_file'] is None and node.get('source_file')==config['path'] or p['source_file']==node.get('source_file')) and p['mapping_index']==decl.get('mapping_index')]
        if not plans:plan=None
        elif len(plan_matches)!=1:
            diff.append({'field':'printf_expansion.owner_plan','error':'missing/conflicting exact owner plan','native':expanded});continue
        else:plan=plan_matches[0]
        owner=node.get('source_file');dataset=node.get('dataset');index=0 if plan is None else plan['mapping_index']
        if type(block) is not int or not 0<=block<(1 if plan is None else plan['blocks']):diff.append({'field':'printf_expansion.block_index','native':block});continue
        key=(owner,dataset,index,block)
        if key in seen:diff.append({'field':'printf_expansion.duplicate_block','native':key})
        seen.add(key)
        wanted_decl=declaration(owner,dataset,index,plan)
        diff += [{**d,'field':f'printf.{key}.declaration.'+d['field']} for d in delta(decl,wanted_decl)]
        literal_file=parse_pattern(wanted_decl['stored_filename'],block);literal_dataset=parse_pattern(wanted_decl['declared_dataset'],block)
        diff += [{**d,'field':f'printf.{key}.mapping.'+d['field']} for d in delta(mapping,{'stored_filename':literal_file,'declared_dataset':literal_dataset})]
    all_expected=[]
    for p in plans:
        owner=config['path'] if p['source_file'] is None else p['source_file']
        all_expected.extend((owner,p['dataset'],p['mapping_index'],i) for i in range(p['blocks']))
    if plans and set(all_expected)!=seen:diff.append({'field':'printf_expansions.identities','reference':all_expected,'native':sorted(seen)})
    return diff


def run_probe(probe,config):
    with tempfile.NamedTemporaryFile('w',suffix='.json') as handle:
        json.dump(config,handle);handle.flush();result=subprocess.run([str(probe),'hdf5-layout',handle.name],capture_output=True,text=True,timeout=30)
    if result.returncode:return {'status':'error','returncode':result.returncode,'error':result.stderr}
    return {'status':'ok','value':json.loads(result.stdout)}


def reference_child(config,backend,warm=False):
    from fabio.hdf5image import Hdf5Image
    path=pathlib.Path(config['path']);shape=config['shape'];keep=None;reader=None
    try:
        if warm:
            keep=h5py.File(path.parent/'nested-child.h5','r');child=keep['data'];_shape=child.shape;_data=child[:]
        if backend=='h5py':
            reader=h5py.File(path,'r');dataset=reader[config['dataset']];full=list(dataset.shape)
            frames=[frame(i,dataset[i,:shape[1],:shape[2]]) for i in range(shape[0])]
        else:
            reader=Hdf5Image().read(str(path)+'::'+config['dataset']);full=list(reader.dataset.shape)
            frames=[frame(i,(reader.data if i==0 else reader.getframe(i).data)[:shape[1],:shape[2]]) for i in range(shape[0])]
        out={'backend':backend,'mode':'independent_warm_child_control' if warm else 'unchanged_cold_default','full_shape':full,'selected_shape':shape,'frames':frames}
        if warm:out['warm_child']={'path':str((path.parent/'nested-child.h5').resolve()),'shape':list(_shape),'dtype':_data.dtype.name,'value_hex':canonical(_data).hex(),'sample_sha256':sha(canonical(_data))}
        return out
    finally:
        if reader is not None:reader.close()
        if keep is not None:keep.close()


def run_reference(config,backend,warm=False):
    request_={'path':config['path'],'dataset':config['dataset'],'shape':config['shape']}
    with tempfile.NamedTemporaryFile('w',suffix='.json') as handle:
        json.dump(request_,handle);handle.flush();result=subprocess.run([sys.executable,str(pathlib.Path(__file__).resolve()),'--reference-child',handle.name,backend,'warm' if warm else 'cold'],capture_output=True,text=True,timeout=30)
    if result.returncode:raise ValueError(f'isolated unchanged reference failed:{result.returncode}:{result.stderr}')
    return json.loads(result.stdout)


def compare_reference(value,case):
    wanted=expected_frames(case);diff=delta(value,{'selected_shape':case['shape']})
    actual=value.get('frames',[])
    if len(actual)!=len(wanted):diff.append({'field':'reference.frames.count','reference':len(wanted),'actual':len(actual)})
    for index,(a,b) in enumerate(zip(actual,wanted)):diff += [{**d,'field':f'reference.frames.{index}.'+d['field']} for d in delta(a,b)]
    return diff


def qualify_nested(cold,warm,case):
    # Cold output must be precisely declared zero fill, not an arbitrary error
    # or unexplained mismatching dataset. Warm read is a separate fresh process.
    expected=expected_frames(case);zero=copy.deepcopy(expected)
    for value in zero:
        raw=bytes(len(bytes.fromhex(value['value_hex'])));value['value_hex']=raw.hex();value['pixels']['sample_sha256']=sha(raw)
    cold_diff=delta(cold,{'mode':'unchanged_cold_default','selected_shape':case['shape'],'frames':zero})
    warm_diff=delta(warm,{'mode':'independent_warm_child_control'})+compare_reference(warm,case)
    full_words=bytes.fromhex(''.join(case['frame_hex']))
    warm_diff+=delta(warm.get('warm_child',{}),{'shape':case['shape'],'dtype':np.dtype(case['dtype']).name,'value_hex':full_words.hex(),'sample_sha256':sha(full_words)})
    if cold_diff or warm_diff:raise ValueError('nested reference discrepancy is not the independently controlled cold-zero/warm-exact defect')
    return {'classification':'qualified_pinned_hdf5_nested_printf_extent_cache_defect','cold_words':'declared zero fill despite complete sources','warm_control':'separate fresh process keeps child reader open after shape/data access; outer data then matches every manual word','primary_reference_warmed_or_patched':False}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--probe',type=pathlib.Path,required=True);p.add_argument('--fabio-source',type=pathlib.Path,required=True);p.add_argument('--report',type=pathlib.Path,required=True);args=p.parse_args()
    import fabio
    if fabio.version!='2026.6.0' or subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()!=PIN:p.error('requires pinned unchanged FabIO source/environment')
    module=importlib.import_module('fabio.hdf5image');actual=pathlib.Path(inspect.getfile(module));pinned=args.fabio_source/'src/fabio/hdf5image.py';gitbytes=subprocess.check_output(['git','-C',str(args.fabio_source),'show',f'{PIN}:src/fabio/hdf5image.py'])
    imports={'actual':fingerprint(actual),'pinned':fingerprint(pinned),'unchanged_git_and_import':actual.read_bytes()==pinned.read_bytes()==gitbytes}
    sources=[ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'build.rs',ROOT/'examples/api_probe.rs',ROOT/'scripts/vds_printf_reference.py',ROOT/'scripts/test_vds_printf_reference.py',ROOT/'scripts/generate_printf_vds_fixtures.py',ROOT/'tests/vds_printf.rs',actual,pinned]
    sources+=sorted((ROOT/'src').rglob('*.rs'))+sorted(DATA.glob('*'))
    native_source=pathlib.Path.home()/'.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hdf5-metno-src-0.10.4/ext/hdf5/src/H5Dvirtual.c'
    sources.append(native_source)
    for module in [h5py.h5p,h5py.h5d,importlib.import_module('h5py._selector')]:sources.append(pathlib.Path(module.__file__))
    sources+=sorted((pathlib.Path(h5py.__file__).parent/'.dylibs').glob('*hdf5*'))
    sources+=sorted((pathlib.Path(h5py.__file__).parent.parent/'h5py.libs').glob('*hdf5*'))
    before={str(f):fingerprint(f) for f in sources};probe_before=fingerprint(args.probe);manifest=json.loads((DATA/'provenance.json').read_text());events=[];errors=[]
    for name,case in manifest['cases'].items():
        config=request(case);snapshots={str(path):fingerprint(path) for path in [DATA/case['master'],*(DATA/n for n in case['source_files'])]}
        try:
            result=run_probe(args.probe,config)
            if result['status']!='ok':raise ValueError(result['error'])
            native=result['value'];differences=compare_native(native,case,config)
            ref_config={'path':config['path'],'dataset':case['dataset'],'shape':case['shape']};references={};qualification=None
            for backend in ['h5py','fabio']:
                cold=run_reference(ref_config,backend);reference_diff=compare_reference(cold,case)
                if name=='nested-owner':
                    warm=run_reference(ref_config,backend,True);qualified=qualify_nested(cold,warm,case);references[backend]={'cold_default':cold,'separate_warm_control':warm,'qualification':qualified};qualification=qualified
                else:
                    if reference_diff:raise ValueError(f'{backend} differs from independent source words:{reference_diff}')
                    references[backend]={'cold_default':cold,'differences':reference_diff}
            events.append({'case':name,'request':config,'source_manifest_before':snapshots,'native':native,'independent':expected_frames(case),'references':references,'qualification':qualification,'differences':differences})
            after_sources={str(path):fingerprint(path) for path in [DATA/case['master'],*(DATA/n for n in case['source_files'])]}
            if snapshots!=after_sources:raise ValueError('source file changed')
            events[-1]['source_manifest_after']=after_sources
        except Exception as error:errors.append({'case':name,'type':type(error).__name__,'error':str(error)})
    guards=[]
    for guard in ['exhausted-prefixes','interleaved-prefixes']:
        case=copy.deepcopy(manifest['cases']['files-u64']);case['master']=guard+'.h5'
        first=copy.deepcopy(case['printf_mappings'][0]);second=copy.deepcopy(first);second['mapping_index']=1
        if guard=='exhausted-prefixes':first['blocks']=2;second['blocks']=3
        else:
            first['blocks']=3;second['blocks']=2;case['shape'][0]=5;case['frame_hex']=case['frame_hex'][:5]
        case['printf_mappings']=[first,second];config=request(case);result=run_probe(args.probe,config)
        if guard=='exhausted-prefixes':
            differences=[] if result['status']=='error' and 'approved printf prefix does not cover' in result['error'] else [{'field':'missing_original_mapped_tail_rejection','native':result}]
        else:differences=compare_native(result['value'],case,config) if result['status']=='ok' else [{'field':'valid_unequal_interleave','native':result}]
        guards.append({'case':guard,'request':config,'native':result,'differences':differences})
    after={str(f):fingerprint(f) for f in sources};probe_after=fingerprint(args.probe)
    summary={'cases':len(events),'frames':sum(len(c['independent']) for c in events),'native_differences':sum(len(c['differences']) for c in events),'direct_reference_matches':sum(c['qualification'] is None for c in events),'qualified_nested_cache_defects':sum(c['qualification'] is not None for c in events),'errors':len(errors),'guard_cases_passed':sum(not g['differences'] for g in guards),'imports_unchanged':imports['unchanged_git_and_import'],'sources_stable':before==after,'probe_stable':probe_before==probe_after}
    passed=gate(summary)
    report={'schema':1,'passed':passed,'scope':'explicit immutable owner-qualified printf block prefix; complete concrete source words before any optional declared fill; no native VDS ambient lookup or discovered extent','pin':PIN,'imports':imports,'summary':summary,'probe_before':probe_before,'probe_after':probe_after,'sources_before':before,'sources_after':after,'reference_versions':{'fabio':fabio.version,'numpy':np.__version__,'h5py':h5py.__version__,'hdf5':h5py.version.hdf5_version},'events':events,'guard_controls':guards,'errors':errors,'license':'CC0 authored fixtures/byte expectations; existing HDF5 native APIs/codecs reused; unchanged MIT FabIO and h5py test references only'}
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'passed':passed,'summary':summary,'report':str(args.report)}));return 0 if passed else 1


def gate(summary):
    return not delta(summary,{'cases':14,'frames':60,'native_differences':0,'direct_reference_matches':13,'qualified_nested_cache_defects':1,'errors':0,'guard_cases_passed':2,'imports_unchanged':True,'sources_stable':True,'probe_stable':True})


if __name__=='__main__':
    if len(sys.argv)==5 and sys.argv[1]=='--reference-child':print(json.dumps(reference_child(json.loads(pathlib.Path(sys.argv[2]).read_text()),sys.argv[3],sys.argv[4]=='warm')))
    else:raise SystemExit(main())
