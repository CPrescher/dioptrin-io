"""Exact finite nested-VDS qualification using independent authored words.

Unchanged FabIO/h5py calls run cold in separate processes. Invalid cyclic and
resource-exhausting graphs are never submitted to an unbounded reference read.
"""
import argparse
import importlib
import inspect
import itertools
import json
import pathlib
import subprocess

import h5py
import numpy as np
import vds_printf_reference as common

ROOT=common.ROOT
DATA=ROOT/'tests/data/vds-nested'


def request(case):
    return {'path':str((DATA/case['master']).resolve()),'dataset':case['dataset'],
            'vds_filenames':[{'stored_filename':name,'actual_file':str((DATA/name).resolve())} for name in case['bindings']],
            'unmapped_regions':case.get('unmapped_regions','declared_fill')}


def coordinates(record):
    """Test-only finite coordinate oracle; never used by the native decoder."""
    kind=record['kind']
    if kind=='All':return set(itertools.product(*(range(n) for n in record['shape'])))
    if kind=='Regular':
        axes=[sorted({start+i*stride+b for i in range(count) for b in range(block)}) for start,stride,count,block in zip(record['start'],record['stride'],record['count'],record['block'])]
        return set(itertools.product(*axes))
    if kind=='Irregular':
        return {point for first,last in record['blocks'] for point in itertools.product(*(range(a,b+1) for a,b in zip(first,last)))}
    raise ValueError('unknown selection record')


def scalar(file,dataset):
    # Dataset.fillvalue boxes bool and would normalize a reserved storage byte.
    with h5py.File(file,'r') as handle:
        value=handle[dataset];out=np.empty((),dtype=value.dtype)
        value.id.get_create_plist().get_fill_value(out)
    raw=common.canonical(out)
    return {'pixels':{'dtype':out.dtype.name,'sample_sha256':common.sha(raw)},'value_hex':raw.hex()}


def prefixed(prefix,differences):
    return [{**value,'field':prefix+value['field']} for value in differences]


def compare_native(native,case,config):
    expected=common.expected_frames(case)
    differences=common.delta(native,{'kind':'hdf5_layout','frames':len(expected),'links':[]})
    actual=native.get('images',[])
    if len(actual)!=len(expected):differences.append({'field':'images.count','reference':len(expected),'native':len(actual)})
    for index,(a,b) in enumerate(zip(actual,expected)):differences+=prefixed(f'images.{index}.',common.delta(a,{'status':'ok',**b}))
    version=native.get('native_hdf5_version')
    if not isinstance(version,list) or len(version)!=3:differences.append({'field':'native_hdf5_version','native':version})
    paths={config['path'],*(b['actual_file'] for b in config['vds_filenames'])}
    for key,wanted,sortkey in [('sources',[{'path':p,'bytes':pathlib.Path(p).stat().st_size} for p in paths],'path'),('source_bindings',[{'requested':p,'resolved':p} for p in paths],'requested')]:
        observed=native.get(key,[])
        if sorted(observed,key=lambda v:v[sortkey])!=sorted(wanted,key=lambda v:v[sortkey]):differences.append({'field':key,'reference':wanted,'native':observed})
    meta=native['metadata'][0];dtype=np.dtype(case['dtype'])
    differences+=prefixed('metadata.',common.delta(meta,{'source_shape':case['shape'],'shape':case['shape'][1:],'frames':len(expected),'first_frame':0,'pixel_type':common.TYPE_NAMES[dtype.name],'little_endian':dtype.byteorder!='>'}))
    graph=meta['virtual_dataset'];nodes=[graph,*graph['nested_nodes']];byid={node['node_id']:node for node in nodes}
    differences+=prefixed('graph.',common.delta(graph,{'frozen_shape':case['shape'],'source_file':config['path'],'dataset':config['dataset'],'unmapped_regions':'Reject' if config['unmapped_regions']=='reject' else 'DeclaredFill'}))
    if len(byid)!=len(nodes):differences.append({'field':'graph.duplicate_node_id'})
    if 'nested_nodes' in case and len(graph['nested_nodes'])!=case['nested_nodes']:differences.append({'field':'graph.unique_physical_nodes','reference':case['nested_nodes'],'native':len(graph['nested_nodes'])})
    if 'effective_fill_regions' in case and len(graph['effective_fill_regions'])!=case['effective_fill_regions']:differences.append({'field':'graph.effective_fill_regions.count'})
    if case['master']=='grand.h5' and len({r['origin_node'] for r in graph['effective_fill_regions']})!=3:differences.append({'field':'graph.three_original_fill_nodes'})
    physical=set();gaps={}
    for node in nodes:
        with h5py.File(node['source_file'],'r') as handle:
            dataset=handle[node['dataset']];identity=(node['source_file'],h5py.h5o.get_info(dataset.id).addr)
            if identity in physical:differences.append({'field':'graph.duplicate_physical_node'})
            physical.add(identity);prop=dataset.id.get_create_plist()
            if len(node['mappings'])!=prop.get_virtual_count():differences.append({'field':'node.mapping_count'})
            coverage=set()
            for index,mapping in enumerate(node['mappings']):
                decl=common.declaration(node['source_file'],node['dataset'],index,None)
                differences+=prefixed(f'node.{node["node_id"]}.mapping.{index}.',common.delta(mapping,{key:decl[key] for key in ['stored_filename','declared_dataset','source_selection','virtual_selection']}))
                src=coordinates(mapping['frozen_source_selection']);dst=coordinates(mapping['frozen_virtual_selection'])
                if len(src)!=len(dst) or len(dst)!=mapping['selected_elements']:differences.append({'field':'mapping.selected_elements'})
                if coverage & dst:differences.append({'field':'mapping.overlap'})
                coverage|=dst
                if mapping['nested_node'] is not None:
                    child=byid.get(mapping['nested_node'])
                    if child is None or (child['source_file'],child['dataset'])!=(mapping['source_file'],mapping['resolved_dataset']):
                        # A hard alias may have a different path but must share the same physical object.
                        if child is None:differences.append({'field':'mapping.missing_child'})
                        else:
                            with h5py.File(mapping['source_file'],'r') as f:
                                if h5py.h5o.get_info(f[mapping['resolved_dataset']].id).addr!=h5py.h5o.get_info(f[child['dataset']].id).addr:differences.append({'field':'mapping.wrong_physical_child'})
                    if mapping['storage_allocated'] is not False:differences.append({'field':'mapping.virtual_allocation'})
                elif mapping['storage_allocated'] is not True:differences.append({'field':'mapping.concrete_allocation'})
            full=set(itertools.product(*(range(n) for n in node['frozen_shape'])))
            gaps[node['node_id']]=full-coverage
            counts=[sum(p[0]==i for p in gaps[node['node_id']]) for i in range(node['frozen_shape'][0])]
            constant=counts[0] if len(set(counts))==1 else None
            differences+=prefixed('node.coverage.',common.delta(node,{'unmapped_pixels_by_frame':counts,'unmapped_pixels_per_frame':constant}))
    raw=bytes.fromhex(''.join(case['frame_hex']));width=dtype.itemsize;seen=set()
    for region in graph['effective_fill_regions']:
        origin=byid.get(region['origin_node']);destination=coordinates(region['virtual_selection']);source=coordinates(region['origin_selection'])
        if origin is None or (region['source_file'],region['dataset'])!=(origin['source_file'],origin['dataset']):differences.append({'field':'fill.origin_identity'});continue
        if not source<=gaps[origin['node_id']] or len(source)!=len(destination):differences.append({'field':'fill.original_unmapped_selection'})
        if seen&destination:differences.append({'field':'fill.overlap'})
        seen|=destination;value=scalar(region['source_file'],region['dataset'])
        differences+=prefixed('fill.scalar.',common.delta(region['value'],value))
        for point in destination:
            if any(p>=n or p<0 for p,n in zip(point,case['shape'])):differences.append({'field':'fill.destination_bounds'});continue
            offset=int(np.ravel_multi_index(point,case['shape']))*width
            if raw[offset:offset+width].hex()!=value['value_hex']:differences.append({'field':'fill.projected_original_word','coordinate':point})
    return differences


def gate(summary):
    return not common.delta(summary,{'exact_cases':10,'exact_frames':24,'reference_matches':20,'guard_cases_passed':6,'differences':0,'errors':0,'imports_unchanged':True,'sources_stable':True,'probe_stable':True})


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--probe',type=pathlib.Path,required=True);parser.add_argument('--fabio-source',type=pathlib.Path,required=True);parser.add_argument('--report',type=pathlib.Path,required=True);args=parser.parse_args()
    import fabio
    if fabio.version!='2026.6.0' or subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()!=common.PIN:parser.error('requires pinned unchanged FabIO')
    actual=pathlib.Path(inspect.getfile(importlib.import_module('fabio.hdf5image')));pinned=args.fabio_source/'src/fabio/hdf5image.py';gitbytes=subprocess.check_output(['git','-C',str(args.fabio_source),'show',f'{common.PIN}:src/fabio/hdf5image.py'])
    imports={'actual':common.fingerprint(actual),'pinned':common.fingerprint(pinned),'unchanged':actual.read_bytes()==pinned.read_bytes()==gitbytes}
    sources=[ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'build.rs',ROOT/'examples/api_probe.rs',actual,pinned,ROOT/'scripts/vds_printf_reference.py',ROOT/'scripts/vds_nested_reference.py',ROOT/'scripts/test_vds_nested_reference.py',ROOT/'scripts/generate_nested_vds_fixtures.py',ROOT/'tests/vds_nested.rs']+sorted((ROOT/'src').rglob('*.rs'))+sorted(DATA.glob('*'))
    primary=pathlib.Path.home()/'.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hdf5-metno-src-0.10.4/ext/hdf5/src'
    sources += [primary/'H5Dvirtual.c',primary/'H5Shyper.c',primary/'H5Sselect.c']
    sources += [pathlib.Path(module.__file__) for module in [h5py.h5p,h5py.h5s,h5py.h5d,importlib.import_module('h5py._selector')]]
    sources += sorted((pathlib.Path(h5py.__file__).parent/'.dylibs').glob('*hdf5*'))
    before={str(p):common.fingerprint(p) for p in sources};probe_before=common.fingerprint(args.probe);manifest=json.loads((DATA/'provenance.json').read_text());events=[];errors=[];guards=[]
    for name,case in manifest['cases'].items():
        config=request(case)
        try:
            result=common.run_probe(args.probe,config)
            if result['status']!='ok':raise ValueError(result['error'])
            differences=compare_native(result['value'],case,config);refs={}
            for backend in ['h5py','fabio']:
                reference=common.run_reference({**config,'shape':case['shape']},backend)
                differences+=prefixed(backend+'.',common.compare_reference(reference,case));refs[backend]=reference
            events.append({'case':name,'classification':'exact_complete_nested_source_graph','request':config,'native':result['value'],'independent':common.expected_frames(case),'reference':refs,'differences':differences})
        except Exception as error:errors.append({'case':name,'type':type(error).__name__,'error':str(error)})
    invalid=[(name,case,'declared_fill') for name,case in manifest['invalid'].items()]+[('undefined_used_reject',manifest['invalid']['undefined_used'],'reject'),('effective_fill_reject',manifest['cases']['grand-u64'],'reject')]
    for name,case,policy in invalid:
        config={**request(case),'unmapped_regions':policy};result=common.run_probe(args.probe,config)
        guards.append({'case':name,'classification':'explicit_undefined_fill_or_graph_resource_policy' if name!='cycle' else 'cyclic_source_graph','request':config,'native':result,'passed':result['status']=='error'})
    after={str(p):common.fingerprint(p) for p in sources};probe_after=common.fingerprint(args.probe)
    summary={'exact_cases':len(events),'exact_frames':sum(len(e['independent']) for e in events),'reference_matches':sum(not common.compare_reference(r,manifest['cases'][e['case']]) for e in events for r in e['reference'].values()),'guard_cases_passed':sum(g['passed'] for g in guards),'differences':sum(len(e['differences']) for e in events),'errors':len(errors),'imports_unchanged':imports['unchanged'],'sources_stable':before==after,'probe_stable':probe_before==probe_after}
    report={'schema':1,'passed':gate(summary),'scope':'exact nested finite selection composition with unique physical nodes and root-effective original typed fill; no implicit virtual data read or path search','summary':summary,'pin':common.PIN,'imports':imports,'reference_versions':{'fabio':fabio.version,'numpy':np.__version__,'h5py':h5py.__version__,'hdf5':h5py.version.hdf5_version},'probe_before':probe_before,'probe_after':probe_after,'sources_before':before,'sources_after':after,'events':events,'guard_controls':guards,'errors':errors,'license':'CC0 authored source words and fixtures; unchanged MIT FabIO and h5py used only for tests'}
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'passed':report['passed'],'summary':summary,'report':str(args.report)}));return 0 if report['passed'] else 1

if __name__=='__main__':raise SystemExit(main())
