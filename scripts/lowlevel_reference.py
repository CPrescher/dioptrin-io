"""Test-only Pilatus and read-only NeXus helper qualification.

Raw/source metadata and native exact typed data are compared separately from
unchanged reference calls. h5py object identity, writers, mutation and time helper
APIs are not claimed. Unchanged reference failures remain failures, not parity.
"""
import argparse
import hashlib
import importlib
import inspect
import io
import json
import pathlib
import struct
import subprocess
import sys
import tempfile

from companion_reference import differences,fingerprint,normalize
ROOT=pathlib.Path(__file__).resolve().parents[1]
DATA=ROOT/'tests/data/lowlevel'
PIN='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'
QUERIES=['Detector','sensor','Exposure_time','Count_cutoff','N_oscillations','Start_angle','Unknown','Missing']


def generate_fixtures():
    import h5py
    import numpy as np
    DATA.mkdir(parents=True,exist_ok=True)
    text_cases={'pilatus.txt':(DATA/'pilatus.txt').read_bytes(),
                'pilatus-invalid.txt':(DATA/'pilatus-invalid.txt').read_bytes(),
                'pilatus-empty.txt':b'',
                'pilatus-one-token.txt':b'# Lone\n',
                'pilatus-continuation.txt':b'# Detector: PILATUS \\\n# 100K\n# Exposure_time 0.01 s\n',
                'pilatus-trailing-spaces.txt':b'# Detector abc\\  ',
                'pilatus-cr-only.txt':b'# Exposure_time 1 s\r# Exposure_time 2 s',
                'pilatus-universal-lines.txt':''.join(f'# Exposure_time {index+1} s{separator}' for index,separator in enumerate(['\n','\r\n','\r','\v','\f','\x1c','\x1d','\x1e','\x85','\u2028','\u2029'])).encode('utf8'),
                'pilatus-cr-continuation.txt':b'# Detector: PILATUS\\\r# 100K\r# Exposure_time 1 s\r# Exposure_time 2 s\r'}
    for name,payload in text_cases.items():(DATA/name).write_bytes(payload)
    def attr(obj,name,value):obj.attrs[name]=np.bytes_(value)
    with h5py.File(DATA/'nexus.h5','w',track_order=True) as file:
        attr(file,'default','entry_new')
        for name,time in [('entry_old','2020-01-01T00:00:00Z'),('entry_new','2026-01-01T00:00:00+02:00')]:
            entry=file.create_group(name,track_order=True);attr(entry,'NX_class','NXentry');attr(entry,'default','plot')
            entry['start_time']=np.bytes_(time)
            instrument=entry.create_group('instrument',track_order=True);attr(instrument,'NX_class','NXinstrument')
            detector=instrument.create_group('detector',track_order=True);attr(detector,'NX_class','NXdetector')
            counts=detector.create_dataset('counts',data=np.array([[2**64-1,2**63,2**53+1]],dtype='>u8'))
            counts.attrs.create('reserved',np.array([2**64-1,2**53+1],dtype='>u8'),dtype='>u8')
            counts.attrs['signal']=np.uint8(1);attr(counts,'role','signal');attr(counts,'units','counts')
            plot=entry.create_group('plot');attr(plot,'NX_class','NXdata')
        unrelated=file.create_group('unrelated');unrelated['start_time']=np.array([1,2],dtype='int64')
    import shutil
    shutil.copyfile(DATA/'nexus.h5',DATA/'nexus-unicode.h5')
    with h5py.File(DATA/'nexus-unicode.h5','r+') as file:
        del file.attrs['default'];file.attrs['default']='entry_new'
        for name in ['entry_old','entry_new']:
            del file[name].attrs['default'];file[name].attrs['default']='plot'
    with h5py.File(DATA/'relay.h5','w') as file:file['selected']=h5py.ExternalLink('nexus.h5','/entry_new')
    with h5py.File(DATA/'master.h5','w') as file:
        file['remote']=h5py.ExternalLink('relay.h5','/selected');attr(file,'default','remote')
    files=[{'path':p.name,'bytes':p.stat().st_size,'sha256':fingerprint(p)['sha256']} for p in sorted(DATA.iterdir()) if p.suffix in {'.txt','.h5'}]
    (DATA/'provenance.json').write_text(json.dumps({'schema':1,'license':'CC0-1.0','origin':'independently authored text and h5py metadata/count arrays; no FabIO writer or experimental data','generator':'scripts/lowlevel_reference.py --generate-fixtures','files':files},indent=2)+'\n')


def typed_identity(value):
    if isinstance(value,float):return {'f64_bits':struct.pack('>d',value).hex()}
    if isinstance(value,list):return [typed_identity(v) for v in value]
    if isinstance(value,dict):return {k:typed_identity(v) for k,v in value.items()}
    return value


def pilatus_reference(payload):
    from fabio.cbfimage import PilatusHeader
    text=payload.decode('utf8');result={}
    try:result['clean_string']={'status':'completed','lines':PilatusHeader.clean_string(text)}
    except Exception as error:result['clean_string']={'status':'not_completed','error_type':type(error).__name__,'error':str(error)}
    try:
        reference=PilatusHeader(text)
        result['constructor']={'status':'completed','typed_fields':normalize(dict(reference._dict)),
            'queries':[{'key':key,'contains':key in reference,'value':normalize(reference[key]) if key in reference else None} for key in QUERIES]}
    except Exception as error:result['constructor']={'status':'not_completed','error_type':type(error).__name__,'error':str(error)}
    try:PilatusHeader(text,convention='UNKNOWN')
    except Exception as error:result['unknown_convention']={'status':'rejected','error_type':type(error).__name__}
    return result


def compare_pilatus(native,payload,provenance,reference):
    delta=[]
    for key,value in {'kind':'pilatus_document','original_sha256':hashlib.sha256(payload).hexdigest(),'source_bytes':len(payload),'provenance':provenance,'convention':'PILATUS_1.2','description':payload.decode('utf8')}.items():
        delta.extend(differences(native.get(key),value,'$.'+key))
    # Independent lexical occurrence evidence; source units and duplicate order
    # cannot be replaced with only the last typed assignment.
    expected_entries=[];unparsed=[]
    for line in payload.decode().splitlines():
        value=line.strip(' \t\r\n\0')
        if not value:continue
        if not value.startswith('# '):unparsed.append(line);continue
        import re
        match=re.match(r'([^\s,:=]+)[\s,:=]+(.*)',value[2:].strip())
        if match:expected_entries.append([match[1],match[2].lstrip(' \t,:=')])
        else:unparsed.append(line)
    delta.extend(differences(native.get('entries'),expected_entries,'$.lexical_entries'))
    delta.extend(differences(native.get('fields'),dict(expected_entries),'$.lexical_fields'))
    constructor=reference['constructor']
    if constructor['status']=='completed':
        delta.extend(differences(typed_identity(native.get('typed_fields')),typed_identity(constructor['typed_fields']),'$.constructor_fields'))
        delta.extend(differences(typed_identity(native.get('queries')),typed_identity(constructor['queries']),'$.readonly_mapping'))
    elif not native.get('diagnostics'):
        delta.append({'field':'$.diagnostics','missing':'reference constructor failure qualification'})
    cleaned=native.get('cleaned_view',{})
    if reference['clean_string']['status']=='completed':
        delta.extend(differences(cleaned.get('lines'),reference['clean_string']['lines'],'$.clean_string'))
    elif not cleaned.get('diagnostics'):
        delta.append({'field':'$.cleaned_view.diagnostics','missing':'reference cleaner failure qualification'})
    return delta


def verify_pilatus(probe,path):
    payload=path.read_bytes();reference=pilatus_reference(payload)
    result={'source':fingerprint(path),'reference':reference,'apis':[],'status':'error'}
    for command,provenance in [('pilatus','PathSnapshot'),('pilatus-bytes','Buffer'),('pilatus-stream','StreamCapture')]:
        args=[str(probe),command,str(path),*QUERIES]
        if provenance=='StreamCapture':
            with path.open('rb') as stream:process=subprocess.run(args,stdin=stream,capture_output=True,text=True,timeout=30)
        else:process=subprocess.run(args,capture_output=True,text=True,timeout=30)
        api={'command':command,'exit_code':process.returncode,'status':'error'};result['apis'].append(api)
        if process.returncode:api['error']=process.stderr[:3000]
        else:
            native=json.loads(process.stdout);delta=compare_pilatus(native,payload,provenance,reference)
            api.update(status='match' if not delta else 'difference',differences=delta,native=native)
    result['source_unchanged']=result['source']==fingerprint(path)
    result['status']='qualified' if result['source_unchanged'] and all(api['status']=='match' for api in result['apis']) else 'difference'
    result['constructor_reference_qualified']=reference['constructor']['status']=='completed'
    result['cleaner_reference_qualified']=reference['clean_string']['status']=='completed'
    result['scope']='raw source + unchanged constructor/cleaner/mapping only where explicitly qualified; malformed/dangling reference-failing variants are diagnosed separately'
    return result


def descriptor(dtype):
    import h5py
    string=h5py.check_string_dtype(dtype)
    if string:
        if string.length is None:return 'VarLenUnicode' if string.encoding=='utf-8' else 'VarLenAscii'
        return f'FixedAscii({string.length})'
    names={'u':'Unsigned','i':'Integer','f':'Float'}
    return f'{names[dtype.kind]}(U{dtype.itemsize})' if dtype.kind in names else repr(dtype)


def order(storage):
    import h5py
    if not hasattr(storage,'get_order'):return 'None'
    return {h5py.h5t.ORDER_LE:'LittleEndian',h5py.h5t.ORDER_BE:'BigEndian',h5py.h5t.ORDER_NONE:'None',h5py.h5t.ORDER_VAX:'Vax'}.get(storage.get_order(),'unknown')


def attr_value(value,storage):
    import numpy as np
    import h5py
    dtype=storage.dtype;array=np.asarray(value)
    if h5py.check_string_dtype(dtype):
        return {'kind':'Text','values':[v.decode() if isinstance(v,bytes) else str(v) for v in array.reshape(-1)]}
    canonical=array.astype(dtype.newbyteorder('<'),copy=False).tobytes()
    types={'u':'U','i':'I','f':'F'}
    return {'kind':'Numeric','shape':list(array.shape),'pixel_type':types[dtype.kind]+str(dtype.itemsize*8),'little_endian':order(storage)!='BigEndian',
            'pixels':{'dtype':dtype.name,'sample_sha256':hashlib.sha256(canonical).hexdigest()},'value_hex':canonical.hex()}


def object_projection(obj,path):
    import h5py
    source=pathlib.Path(obj.file.filename).resolve()
    values={};declarations={}
    for key in obj.attrs:
        storage=obj.attrs.get_id(key).get_type();value=obj.attrs[key]
        values[key]=attr_value(value,storage);declarations[key]={'shape':list(obj.attrs.get_id(key).shape),'type_descriptor':descriptor(storage.dtype)}
    return {'source_file':str(source),'path':obj.name,'kind':'Dataset' if isinstance(obj,h5py.Dataset) else 'Group',
            'shape':list(obj.shape) if isinstance(obj,h5py.Dataset) else None,'type_descriptor':descriptor(obj.dtype) if isinstance(obj,h5py.Dataset) else None,
            'layout':('Chunked' if obj.chunks else 'Contiguous') if isinstance(obj,h5py.Dataset) else None,
            'byte_order':order(obj.id.get_type()) if isinstance(obj,h5py.Dataset) else None,
            'attributes':{'source_file':str(source),'path':obj.name,'values':values,'declarations':declarations}}


def inspect_object(native,expected,location='$'):
    # requested_path is the explicit caller namespace; resolved file/path below
    # is compared to HDF5's actual source. No h5py ID equivalence is asserted.
    return [item for key,value in expected.items() for item in differences(native.get(key),value,location+'.'+key)]


def independent_result(file,command,path):
    import h5py
    operation=command['op']
    def projected(obj):return object_projection(obj,path)
    def entries():
        values=[obj for obj in file.values() if isinstance(obj,h5py.Group) and obj.attrs.get('NX_class') in [b'NXentry','NXentry'] and 'start_time' in obj]
        return sorted(values,key=lambda obj:bytes(obj['start_time'][()]).decode()[:19],reverse=True)
    def classes(group,kind):return [obj for obj in group.values() if isinstance(obj,h5py.Group) and obj.attrs.get('NX_class') in [kind,kind.encode()]]
    def data(group):
        result=[obj for obj in group.values() if isinstance(obj,h5py.Dataset)]
        if 'filter' in command:
            match=command['filter']
            def equal(obj):
                if match['name'] not in obj.attrs:return False
                actual=obj.attrs[match['name']]
                if isinstance(actual,bytes) and isinstance(match['value'],str):actual=actual.decode('utf8')
                return actual==match['value']
            result=[obj for obj in result if equal(obj)]
        return result
    if operation=='entries':
        import datetime
        return [{'object':projected(obj),'start_time':bytes(obj['start_time'][()]).decode(),
                 'ordering_key':list(datetime.datetime.strptime(bytes(obj['start_time'][()]).decode()[:19],'%Y-%m-%dT%H:%M:%S').timetuple()[:6])} for obj in entries()]
    if operation=='entry':
        obj=file.get(command['name']);return projected(obj) if isinstance(obj,h5py.Group) and obj.attrs.get('NX_class') in [b'NXentry','NXentry'] and 'start_time' in obj else None
    if operation=='class':return [projected(obj) for obj in classes(file[command['path']],command['class'])]
    if operation=='attr':
        obj=file[command['path']];key=command['name'];return attr_value(obj.attrs[key],obj.attrs.get_id(key).get_type()) if key in obj.attrs else None
    if operation=='data':return [projected(obj) for obj in data(file[command['path']])]
    if operation in ['detector','find_data']:
        result=[]
        for entry in entries():
            if operation=='find_data':result+=data(entry)
            for kind in (['NXsubentry','NXinstrument'] if operation=='detector' else ['NXinstrument','NXsubentry']):
                for instrument in classes(entry,kind):
                    if operation=='find_data':result+=data(instrument)
                    for detector in classes(instrument,'NXdetector'):
                        if operation=='detector':result.append(detector)
                        else:result+=data(detector)
        if not command.get('all',False):result=result[:1]
        return [projected(obj) for obj in result]
    if operation=='default':
        entry=file[file.attrs['default']];target=entry.attrs.get('default');return projected(entry[target]) if target else None
    if operation=='image':
        obj=file[command['path']];array=obj[()]
        canonical=array.astype(array.dtype.newbyteorder('<'),copy=False).tobytes()
        return {'object':projected(obj),'frames':1,'index':0,'shape':list(array.shape),'pixels':{'dtype':array.dtype.name,'sample_sha256':hashlib.sha256(canonical).hexdigest()}}
    raise ValueError('unknown independent operation')


def reference_call(reference,command):
    op=command['op'];h=reference.h5
    try:
        if op=='entries':value=reference.get_entries()
        elif op=='entry':value=reference.get_entry(command['name'])
        elif op=='class':value=reference.get_class(h[command['path']],command['class'])
        elif op=='attr':value=reference.get_attr(h[command['path']],command['name'])
        elif op=='data':
            f=command.get('filter');value=reference.get_data(h[command['path']],f['name'],f['value']) if f else reference.get_data(h[command['path']])
        elif op=='detector':value=reference.find_detector(command.get('all',False))
        elif op=='find_data':value=reference.find_data(command.get('all',False))
        elif op=='default':value=reference.get_default_NXdata()
        else:return {'status':'not_a_reference_helper','scope':'image counts use independent exact h5py assembly plus existing typed reader'}
        import numpy as np
        def view(value):
            if isinstance(value,list):return [view(item) for item in value]
            if hasattr(value,'name') and hasattr(value,'file'):return {'path':value.name,'raw_h5py_filename':value.file.filename}
            if isinstance(value,np.ndarray):return {'shape':list(value.shape),'dtype':value.dtype.name,'canonical_hex':value.astype(value.dtype.newbyteorder('<'),copy=False).tobytes().hex()}
            if isinstance(value,np.generic):value=value.item()
            if isinstance(value,bytes):return {'bytes_hex':value.hex()}
            return value
        return {'status':'completed','value':view(value)}
    except Exception as error:return {'status':'not_completed','error_type':type(error).__name__,'error':str(error),'reading_parity_claimed':False}


def compare_nexus_action(actual,expected,command):
    if actual.get('status')!='ok':return [{'field':'$.status','actual':actual}]
    result=actual['result'];op=command['op']
    if op=='entries':
        if len(result)!=len(expected):return [{'field':'$.entries.length','actual':len(result),'expected':len(expected)}]
        delta=[]
        for index,(entry,wanted) in enumerate(zip(result,expected)):
            delta+=inspect_object(entry['object'],wanted['object'],f'$.entries[{index}]')
            delta+=differences(entry.get('start_time'),wanted['start_time'],f'$.entries[{index}].start_time')
            delta+=differences(entry.get('ordering_key'),wanted['ordering_key'],f'$.entries[{index}].ordering_key')
            if (len(wanted['start_time'])!=19)!=bool(entry.get('ordering_qualification')):
                delta.append({'field':f'$.entries[{index}].ordering_qualification','kind':'missing_naive_time_scope'})
        return delta
    if isinstance(expected,list):
        if len(result)!=len(expected):return [{'field':'$.objects.length','actual':len(result),'expected':len(expected)}]
        return [d for index,(obj,wanted) in enumerate(zip(result,expected)) for d in inspect_object(obj,wanted,f'$.objects[{index}]')]
    if op in ['entry','default']:return inspect_object(result,expected) if expected else differences(result,None)
    if op=='image':
        return inspect_object(result['object'],expected['object'])+differences({k:v for k,v in result.items() if k!='object'},{k:v for k,v in expected.items() if k!='object'})
    return differences(result,expected)


def reference_alignment(action,reference,command):
    if reference['status']!='completed':return {'status':'no_unchanged_reference_read_claim','differences':[]}
    native=action['result'];expected=reference['value'];op=command['op'];delta=[]
    if op=='entries':delta=differences([row['object']['path'] for row in native],[row['path'] for row in expected])
    elif op in ['entry','default']:delta=differences(native['path'] if native else None,expected['path'] if expected else None)
    elif op in ['class','data','detector','find_data']:
        if isinstance(expected,dict):expected=[expected]
        delta=differences([row['path'] for row in native],[row['path'] for row in expected])
    elif op=='attr':
        if native is None:delta=differences(None,expected)
        elif native['kind']=='Text':delta=differences(native['values'][0] if len(native['values'])==1 else native['values'],expected)
        elif isinstance(expected,dict) and 'canonical_hex' in expected:
            delta=differences({'shape':native['shape'],'dtype':native['pixels']['dtype'],'canonical_hex':native['value_hex']},expected)
        else:
            import numpy as np
            value=np.frombuffer(bytes.fromhex(native['value_hex']),dtype=np.dtype(native['pixels']['dtype']).newbyteorder('<')).reshape(native['shape'])
            delta=differences(value.item() if value.shape==() else value.tolist(),expected)
    return {'status':'match' if not delta else 'difference','differences':delta,
            'qualification':'owned resolved paths/values compared; root h5py file-object filename is recorded, not treated as object identity'}


def verify_nexus(probe,path,external=False):
    import h5py
    from fabio.nexus import Nexus
    base='/remote' if external else '/entry_new';detector=base+'/instrument/detector';counts=detector+'/counts'
    commands=[{'op':'entries'},{'op':'entry','name':'remote' if external else 'entry_new'},
              {'op':'class','path':base,'class':'NXinstrument'}, {'op':'attr','path':counts,'name':'reserved'},
              {'op':'attr','path':counts,'name':'units'},{'op':'attr','path':counts,'name':'missing'},
              {'op':'data','path':detector},{'op':'data','path':detector,'filter':{'name':'signal','dtype':'int64','value':1}},
              {'op':'data','path':detector,'filter':{'name':'role','dtype':'text','value':'signal'}},
              {'op':'detector','all':False},{'op':'detector','all':True},{'op':'find_data','all':True},{'op':'default'},{'op':'image','path':counts,'index':0}]
    result={'source':fingerprint(path),'commands':commands,'status':'error'}
    with tempfile.TemporaryDirectory(prefix='dioptrin-nexus-reference-') as directory:
        request=pathlib.Path(directory)/'request.json';request.write_text(json.dumps({'path':str(path.resolve()),'follow_external':external,'commands':commands}))
        process=subprocess.run([str(probe),'nexus-api',str(request)],capture_output=True,text=True,timeout=120)
        result['exit_code']=process.returncode
        if process.returncode:result['error']=process.stderr[:4000];return result
        native=json.loads(process.stdout);result['native']=native;checks=[]
        # Keep independent path-driver inspection and the unchanged reference
        # file-object driver in separate lifetimes, avoiding HDF5 handle/cache
        # interactions as a cause of artificial reference failures.
        with h5py.File(path,'r') as file:
            expected_values=[independent_result(file,command,path) for command in commands]
        with Nexus(str(path),mode='r') as reference:
            observed_values=[reference_call(reference,command) for command in commands]
        for command,action,expected,observed in zip(commands,native['actions'],expected_values,observed_values):
            delta=compare_nexus_action(action,expected,command)+differences(action.get('command'),command,'$.command')
            checks.append({'command':command,'reference':observed,'differences':delta,
                           'reference_alignment':reference_alignment(action,observed,command) if action['status']=='ok' else {'status':'difference','differences':[{'field':'native.status'}]}})
        result['checks']=checks
        expected_sources={str((DATA/name).resolve()) for name in (['master.h5','relay.h5','nexus.h5'] if external else [path.name])}
        source_delta=differences(sorted(row['path'] for row in native['sources']),sorted(expected_sources))
        expected_links=[]
        if external:
            for filename in ['master.h5','relay.h5']:
                with h5py.File(DATA/filename,'r') as file:
                    for key in file:
                        link=file.get(key,getlink=True)
                        if isinstance(link,h5py.ExternalLink):
                            expected_links.append((str((DATA/filename).resolve()),'/'+key,True,(b'\0'+link.filename.encode()+b'\0'+link.path.encode()+b'\0').hex()))
        actual_links=sorted(set((row['source'],row['path'],row['external'],row['raw_hex']) for row in native['links']))
        source_delta+=differences(actual_links,sorted(expected_links),'$.link_values')
        result['source_graph_differences']=source_delta
        result['source_unchanged']=result['source']==fingerprint(path)
        result['status']='qualified_native_readonly_values' if len(native['actions'])==len(commands) and not source_delta and all(not check['differences'] and not check['reference_alignment']['differences'] for check in checks) and result['source_unchanged'] else 'difference'
        result['reference_qualification']='unchanged get_data(default)/find_data None-attribute failures are not parity successes; owned descriptors/exact data have independent source proof; h5py identities/writers excluded'
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--generate-fixtures',action='store_true');parser.add_argument('--probe',type=pathlib.Path);parser.add_argument('--report',type=pathlib.Path);parser.add_argument('--fabio-source',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-2026.6.0'));args=parser.parse_args()
    if args.generate_fixtures:generate_fixtures();return 0
    if not args.probe or not args.report:parser.error('--probe and --report required')
    if subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()!=PIN:parser.error('pinned reference required')
    import fabio
    if fabio.version!='2026.6.0':parser.error('FabIO2026.6.0 required')
    source_paths=[pathlib.Path(__file__),ROOT/'scripts/test_lowlevel_reference.py',ROOT/'tests/pilatus_api.rs',ROOT/'tests/nexus_api.rs',DATA/'provenance.json',ROOT/'Cargo.toml',ROOT/'Cargo.lock',*sorted((ROOT/'src').rglob('*.rs'))]
    source_paths += [DATA/item['path'] for item in json.loads((DATA/'provenance.json').read_text())['files']]
    reference_sources={}
    for name in ['cbfimage','nexus']:
        actual=pathlib.Path(inspect.getfile(importlib.import_module('fabio.'+name)));pinned=args.fabio_source/'src/fabio'/(name+'.py');source_paths += [actual,pinned];reference_sources[name]={'actual':fingerprint(actual),'pinned':fingerprint(pinned),'matches':fingerprint(actual)['sha256']==fingerprint(pinned)['sha256']}
    import h5py
    source_paths += [pathlib.Path(inspect.getfile(h5py.h5f)),pathlib.Path(inspect.getfile(h5py.h5a))]
    sources={str(path):fingerprint(path) for path in source_paths}
    import h5py,numpy as np
    report={'schema':1,'environment':{'python':sys.version,'fabio':fabio.version,'numpy':np.__version__,'h5py':h5py.__version__,'hdf5':h5py.version.hdf5_version},'reference_commit':PIN,'reference_sources':reference_sources,'probe':fingerprint(args.probe),'sources':sources,
            'contract':'owned read-only metadata and existing exact typed image selections; constructor/cleaner/mapping reference results separate from raw preservation and intended broken-helper behavior; no h5py identity/writer/time-helper replacement'}
    report['pilatus']=[verify_pilatus(args.probe,path) for path in sorted(DATA.glob('pilatus*.txt'))]
    report['nexus']=[verify_nexus(args.probe,DATA/'nexus.h5'),verify_nexus(args.probe,DATA/'nexus-unicode.h5'),verify_nexus(args.probe,DATA/'master.h5',True)]
    report['sources_unchanged']=all(fingerprint(path)==value for path,value in sources.items());report['probe_unchanged']=fingerprint(args.probe)==report['probe']
    report['qualified']=all(row['status']=='qualified' for row in report['pilatus']) and all(row['status']=='qualified_native_readonly_values' for row in report['nexus']) and report['sources_unchanged'] and report['probe_unchanged'] and all(row['matches'] for row in reference_sources.values())
    report['all_unchanged_reference_reading_parity_claimed']=False
    report['unqualified_pilatus_variants']=[row['unqualified_variant'] for row in report['pilatus'] if 'unqualified_variant' in row]
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n');print(json.dumps({'qualified':report['qualified'],'pilatus_sources':len(report['pilatus']),'pilatus_api_calls':sum(len(row['apis']) for row in report['pilatus']),'nexus_actions':sum(len(row.get('checks',[])) for row in report['nexus']),'report':str(args.report)}));return 0 if report['qualified'] else 1

if __name__=='__main__':raise SystemExit(main())
