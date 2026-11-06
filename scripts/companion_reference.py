"""Independent, test-only Xcalibur run companion read-API qualification.

Uses unchanged pinned RunDescription.read/loads and Sweep.loads/dumps. A fresh
reference object is used for roundtrip diagnostics; serialization is never used
to construct source fixtures or repair native/reference fields.
"""
import argparse
import dataclasses
import hashlib
import importlib
import inspect
import json
import math
import pathlib
import struct
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
PIN = '9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'
FLOATS = ['domega','dtheta','dkappa','dphi','dstart','dend','dwidth','dunknown2','dexposure']
SWEEP_FIELDS = ['iscanindex','iscantype','dwunknown1',*FLOATS[:8],
                'iunknown3','iunknown4','iunknown5','iunknown6','dexposure']


def fingerprint(path):
    path = pathlib.Path(path)
    with path.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    return {'path': str(path.resolve()), 'bytes': path.stat().st_size, 'sha256': digest}


def normalize(value):
    if dataclasses.is_dataclass(value):
        return {field.name: normalize(getattr(value,field.name)) for field in dataclasses.fields(value)}
    if isinstance(value,float) and not math.isfinite(value):
        return {'kind':'float','value':'nan' if math.isnan(value) else 'inf' if value>0 else '-inf'}
    if isinstance(value,(list,tuple)):
        return [normalize(item) for item in value]
    if isinstance(value,dict):
        return {key:normalize(item) for key,item in value.items()}
    return value


def float_bits(value):
    return struct.pack('>d',value).hex()


def differences(actual, expected, location='$'):
    result = []
    if type(actual) is not type(expected):
        return [{'field':location,'actual':actual,'expected':expected}]
    if isinstance(expected,dict):
        for key in sorted(actual.keys()|expected.keys()):
            if key not in actual or key not in expected:
                result.append({'field':location+'.'+key,'kind':'missing_or_extra'})
            else:
                result.extend(differences(actual[key],expected[key],location+'.'+key))
    elif isinstance(expected,list):
        if len(actual)!=len(expected):
            result.append({'field':location+'.length','actual':len(actual),'expected':len(expected)})
        for index,(left,right) in enumerate(zip(actual,expected)):
            result.extend(differences(left,right,f'{location}[{index}]'))
    elif actual!=expected:
        result.append({'field':location,'actual':actual,'expected':expected})
    return result


def source_projection(payload):
    """Independent test-only fixed-slot oracle, including un-decodable source text."""
    if len(payload)<528:
        raise ValueError('truncated independent source header')
    count,unknown = struct.unpack_from('<Qd',payload,512)
    end=528+count*88
    if end>len(payload):
        raise ValueError('truncated independent counted sweeps')
    def decoded(raw):
        try:
            return raw.decode().strip('\0')
        except UnicodeDecodeError:
            return None
    sweeps=[dict(zip(SWEEP_FIELDS,struct.unpack_from('<HHIddddddddHHHHd',payload,528+index*88))) for index in range(count)]
    return {'fields':normalize({'cprefix':decoded(payload[:256]),'cfolder':decoded(payload[256:512]),
                               'inumofsweeps':count,'dunknown':unknown,'pssweep':sweeps}),
            'float_bits':{'dunknown':payload[520:528][::-1].hex(),
                          'pssweep':[{name:float_bits(sweep[name]) for name in FLOATS} for sweep in sweeps]},
            'raw_text_hex':{'cprefix':payload[:256].hex(),'cfolder':payload[256:512].hex()},
            'source_bytes':len(payload),'parsed_bytes':end,'source_sha256':hashlib.sha256(payload).hexdigest(),
            'trailer_hex':payload[end:].hex(),'trailer_sha256':hashlib.sha256(payload[end:]).hexdigest(),
            'sweep_sha256':[hashlib.sha256(payload[528+index*88:528+(index+1)*88]).hexdigest() for index in range(count)]}


def reference_projection(path,payload):
    from fabio.xcaliburimage import RunDescription,Sweep
    result={'fields':None}
    try:
        loaded=RunDescription.loads(payload)
        read=RunDescription.read(str(path))
        result.update(status='match',fields=normalize(loaded),
                      read_loads_differences=differences(normalize(read),normalize(loaded)))
        # Pinning the original byte object makes reference writer losses visible
        # without mutating the read object or turning it into a fixture generator.
        fresh=RunDescription.loads(payload)
        serialized=fresh.dumps()
        try:
            roundtrip=RunDescription.loads(serialized)
            delta=differences(normalize(roundtrip),normalize(loaded))
            result['roundtrip']={'status':'fields_preserved' if not delta else 'reference_serialization_difference',
                'differences':delta,'source_bytes_preserved':serialized==payload,
                'serialized_bytes':len(serialized),'serialized_sha256':hashlib.sha256(serialized).hexdigest(),
                'qualification':'reference serialization is diagnostic only; source padding/trailers remain native original bytes'}
        except Exception as error:
            result['roundtrip']={'status':'reference_serialization_error','error':f'{type(error).__name__}: {error}'}
    except UnicodeDecodeError as error:
        result.update(status='reference_text_rejection',error=f'{type(error).__name__}: {error}',
                      qualification='native keeps raw fixed text; no successful RunDescription reference field/read claim for undecodable text')
    sweep_checks=[]
    count=struct.unpack_from('<Q',payload,512)[0]
    for index in range(count):
        raw=payload[528+index*88:528+(index+1)*88]
        sweep=Sweep.loads(raw)
        sweep_checks.append({'index':index,'fields':normalize(sweep),
                             'float_bits':{name:float_bits(getattr(sweep,name)) for name in FLOATS},
                             'roundtrip_source_bytes_preserved':sweep.dumps()==raw})
    result['sweeps']=sweep_checks
    return result


def compare_record(native,expected,reference,provenance):
    delta=[]
    for key,value in expected.items():
        delta.extend(differences(native.get(key),value,'$.'+key))
    delta.extend(differences(native.get('kind'),'xcalibur_run','$.kind'))
    delta.extend(differences(native.get('provenance'),provenance,'$.provenance'))
    if reference['status']=='match':
        delta.extend(differences(native.get('fields'),reference['fields'],'$.pinned_reference_fields'))
        delta.extend(reference['read_loads_differences'])
    for sweep in reference['sweeps']:
        index=sweep['index']
        actual=native.get('fields',{}).get('pssweep',[])
        actual_bits=native.get('float_bits',{}).get('pssweep',[])
        if index>=len(actual) or index>=len(actual_bits):
            delta.append({'field':f'$.pinned_sweeps[{index}]','kind':'missing'})
        else:
            delta.extend(differences(actual[index],sweep['fields'],f'$.pinned_sweeps[{index}].fields'))
            delta.extend(differences(actual_bits[index],sweep['float_bits'],f'$.pinned_sweeps[{index}].float_bits'))
    return delta


def verify(probe,path,timeout=30):
    record={'source':fingerprint(path),'status':'error'}
    try:
        payload=pathlib.Path(path).read_bytes()
        expected=source_projection(payload)
        reference=reference_projection(path,payload)
        record['reference']=reference
        record['apis']=[]
        for adapter,provenance in [('xcalibur-run','PathSnapshot'),('xcalibur-run-bytes','Buffer'),('xcalibur-run-stream','StreamCapture')]:
            command=[str(probe),adapter,str(path)]
            if provenance=='StreamCapture':
                with pathlib.Path(path).open('rb') as stream:
                    result=subprocess.run(command,stdin=stream,capture_output=True,text=True,timeout=timeout)
            else:
                result=subprocess.run(command,capture_output=True,text=True,timeout=timeout)
            api={'adapter':adapter,'exit_code':result.returncode,'provenance':provenance,'status':'error'}
            record['apis'].append(api)
            if result.returncode:
                api['error']=result.stderr[:4000]
            else:
                native=json.loads(result.stdout)
                delta=compare_record(native,expected,reference,provenance)
                api.update(status='match' if not delta else 'difference',differences=delta,native=native)
        record['source_after']=fingerprint(path)
        record['source_unchanged']=record['source']==record['source_after']
        record['status']='qualified' if record['source_unchanged'] and all(api['status']=='match' for api in record['apis']) else 'difference'
        record['scope']='native raw preservation plus pinnedSweep fields; pinnedRunDescription rejects text' if reference['status']=='reference_text_rejection' else 'all pinnedRunDescription.read/loads fields plus native raw preservation'
    except Exception as error:
        record['error']=f'{type(error).__name__}: {error}'
    return record


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--probe',type=pathlib.Path,required=True)
    parser.add_argument('--report',type=pathlib.Path,required=True)
    parser.add_argument('--fabio-source',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-2026.6.0'))
    parser.add_argument('--source',type=pathlib.Path,action='append')
    args=parser.parse_args()
    commit=subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()
    if commit!=PIN:
        parser.error('reference checkout is not the pinned FabIO source')
    import fabio
    if fabio.version!='2026.6.0':
        parser.error('reference must be FabIO2026.6.0')
    module=importlib.import_module('fabio.xcaliburimage')
    actual=pathlib.Path(inspect.getfile(module));pinned=args.fabio_source/'src/fabio/xcaliburimage.py'
    sources={str(path):fingerprint(path) for path in [actual,pinned,pathlib.Path(__file__),ROOT/'scripts/test_companion_reference.py',
        ROOT/'scripts/generate_xcalibur_run_fixtures.py',ROOT/'src/xcalibur/run.rs',ROOT/'src/xcalibur.rs',
        ROOT/'tests/xcalibur_run.rs',ROOT/'tests/data/xcalibur/run-provenance.json']}
    for path in [*sorted((ROOT/'src').rglob('*.rs')),ROOT/'Cargo.toml',ROOT/'Cargo.lock']:
        sources[str(path)]=fingerprint(path)
    report={'schema':1,'environment':{'python':sys.version,'fabio':fabio.version},'contract':'separate nonimage companion read APIs; no source counts, writers or statistics qualification',
            'reference_commit':PIN,'reference_source_matches_pinned':sources[str(actual)]['sha256']==sources[str(pinned)]['sha256'],
            'probe':fingerprint(args.probe),'sources':sources}
    paths=args.source or [ROOT/'tests/data/xcalibur'/name for name in ['records.run','zero-sweeps.run','raw-text-floats.run']]
    report['reads']=[verify(args.probe,path) for path in paths]
    report['sources_unchanged']=all(fingerprint(path)==source for path,source in sources.items())
    report['probe_unchanged']=fingerprint(args.probe)==report['probe']
    report['all_qualified']=bool(report['reads']) and all(item['status']=='qualified' for item in report['reads']) and report['reference_source_matches_pinned'] and report['sources_unchanged'] and report['probe_unchanged']
    args.report.parent.mkdir(parents=True,exist_ok=True)
    args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n')
    print(json.dumps({'qualified':report['all_qualified'],'sources':len(report['reads']),'report':str(args.report)}))
    return 0 if report['all_qualified'] else 1


if __name__=='__main__':
    raise SystemExit(main())
