"""Original CBF encoded-byte access; source slices and integrity are independent.

Unchanged FabIO only_raw/read_raw_data observations never stand in for decoded
count correctness. Truncated bytes and unsupported conversion admission remain
separate from successful complete payload captures.
"""
import argparse
import base64
import hashlib
import inspect
import json
import pathlib
import re
import subprocess
import sys

from companion_reference import fingerprint

ROOT = pathlib.Path(__file__).resolve().parents[1]
DATA = ROOT/'tests/data/cbf-encoded'
PIN = '9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'
MARKER = b'\x0c\x1a\x04\xd5'


def signature(data):
    return {'bytes':len(data), 'sha256':hashlib.sha256(data).hexdigest(),
            'prefix_hex':data[:64].hex(), 'computed_md5':hashlib.md5(data).hexdigest()}


def oracle(payload):
    start = payload.index(MARKER)
    header = payload[:start]
    section = header[header.index(b'--CIF-BINARY-FORMAT-SECTION--'):]
    fields={}
    previous=None
    for line in section.splitlines()[1:]:
        if not line.strip():continue
        if line[:1] in {b' ',b'\t'} and previous:
            fields[previous]+=' '+line.strip().decode('utf8')
        else:
            key,value=line.split(b':',1)
            previous=key.strip().decode('ascii').lower()
            fields[previous]=value.strip().decode('utf8')
    size = int(re.search(rb'^X-Binary-Size:\s*(\d+)\s*$', section, re.M).group(1))
    conversion = re.search(rb'conversions\s*=\s*"([^"]+)"',section).group(1).decode('ascii')
    declared = re.search(rb'^Content-MD5:\s*(\S+)\s*$', section, re.M)
    digest = base64.b64decode(declared.group(1),validate=True).hex() if declared else None
    data = payload[start+4:start+4+size]
    result = signature(data)
    result.update(declared_md5=digest, checksum_matches=(digest==result['computed_md5']) if digest else None,
                  declared_bytes=size, available_complete=len(data)==size,
                  conversion=conversion, header_sha256=hashlib.sha256(header).hexdigest())
    types={f'{signed} {bits}-bit integer':f'{prefix}{bits}' for signed,prefix in [('signed','I'),('unsigned','U')] for bits in [8,16,32,64]}
    result.update(binary_fields=fields,shape=[int(fields['x-binary-size-second-dimension']),int(fields['x-binary-size-fastest-dimension'])],
                  source_pixel_type=types[fields['x-binary-element-type'].strip('"').lower()])
    return result


def compare_native(native, expected):
    wanted={k:expected[k] for k in ('bytes','sha256','prefix_hex','computed_md5','declared_md5','checksum_matches','header_sha256','binary_fields','shape','source_pixel_type')}
    wanted.update(kind='cbf_encoded',representation='OriginalEncodedPayload')
    return [{'field':k,'expected':v,'actual':native.get(k)} for k,v in wanted.items() if native.get(k)!=v]


def reference_worker(path, method):
    from fabio.cbfimage import CbfImage
    image=CbfImage()
    try:
        if method=='only_raw':
            data=image.read(str(path),only_raw=True)
        else:
            image.filename=str(path)
            with path.open('rb') as stream:
                image._readheader(stream)
                data=image.read_raw_data(stream)
        return {'status':'read_completed','payload':signature(data)}
    except Exception as error:
        return {'status':'reference_error','type':type(error).__name__,'error':str(error)}
    finally:
        image.close()


def verify(probe,path):
    source=fingerprint(path)
    expected=oracle(path.read_bytes())
    native=subprocess.run([str(probe),'cbf-encoded',str(path)],capture_output=True,text=True,timeout=30)
    row={'source':source,'oracle':expected,'native_exit_code':native.returncode}
    if not expected['available_complete']:
        row['classification']='incomplete_source_payload'
        good=native.returncode!=0 and 'truncated CBF binary payload' in native.stderr
        row['native_error']=native.stderr
    else:
        row['classification']='source_encoded_payload_with_explicit_integrity'
        if native.returncode==0:
            row['native']=json.loads(native.stdout)
            row['differences']=compare_native(row['native'],expected)
            good=not row['differences']
        else:
            row['native_error']=native.stderr
            good=False
    refs=[]
    for method in ('only_raw','read_raw_data'):
        result=subprocess.run([sys.executable,__file__,'--worker',str(path),'--method',method],capture_output=True,text=True,timeout=30)
        if result.returncode:
            observation={'status':'reference_process_failure','stderr':result.stderr}
        else: observation=json.loads(result.stdout)
        observation['method']=method
        observation['expected_available_payload_match']=observation.get('payload')=={k:expected[k] for k in ('bytes','sha256','prefix_hex','computed_md5')}
        refs.append(observation)
    row['reference']=refs
    row['source_unchanged']=fingerprint(path)==source
    row['qualified']=good and row['source_unchanged'] and all(r['expected_available_payload_match'] for r in refs)
    return row


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--probe',type=pathlib.Path)
    parser.add_argument('--fabio-source',type=pathlib.Path)
    parser.add_argument('--data',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-corpus'))
    parser.add_argument('--report',type=pathlib.Path)
    parser.add_argument('--worker',type=pathlib.Path)
    parser.add_argument('--method',choices=('only_raw','read_raw_data'))
    args=parser.parse_args()
    if args.worker:
        print(json.dumps(reference_worker(args.worker,args.method)));return 0
    if not all((args.probe,args.fabio_source,args.report)):parser.error('--probe, --fabio-source and --report required')
    from fabio import cbfimage
    assert subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()==PIN
    module=pathlib.Path(inspect.getfile(cbfimage))
    pinned=args.fabio_source/'src/fabio/cbfimage.py'
    git_source=subprocess.check_output(['git','-C',str(args.fabio_source),'show',PIN+':src/fabio/cbfimage.py'])
    assert hashlib.sha256(git_source).hexdigest()==fingerprint(pinned)['sha256']==fingerprint(module)['sha256']
    manifest=json.loads((DATA/'provenance.json').read_text())
    paths=[]
    for item in manifest['files']:
        path=DATA/item['path'];assert fingerprint(path)['sha256']==item['sha256'];paths.append(path)
    # Exact original corpus inputs, with their transport compression left explicit.
    paths.extend(args.data/name for name in ['Pilatus1M.cbf','XDS.cbf','ref-test_5_0001.cbf','run2_1_00148.cbf','testcbf.cbf'])
    assert all(path.is_file() for path in paths)
    tracked=[pathlib.Path(__file__),ROOT/'scripts/test_cbf_encoded_reference.py',ROOT/'scripts/generate_cbf_encoded_fixtures.py',ROOT/'tests/cbf_encoded.rs',DATA/'provenance.json',
             *sorted((ROOT/'src').rglob('*.rs')),ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'examples/api_probe.rs',module,pinned,*paths]
    before={str(p):fingerprint(p) for p in tracked}
    probe=fingerprint(args.probe)
    report={'schema':1,'contract':__doc__,'reference_commit':PIN,'sources':before,'probe':probe,
            'independent_fixture_provenance':manifest,
            'reads':[verify(args.probe.resolve(),path) for path in paths],
            'remaining_gaps':['Encoded-only admission retains explicit single-image integer dimension/type metadata bounds; unsupported scalar types, volumes and multiple binary sections remain separate source-layout gaps.',
                              'Outer compression is not inferred by the direct CBF path API; existing explicit transport staging remains separate.']}
    report['sources_unchanged']=all(fingerprint(p)==v for p,v in before.items())
    report['probe_unchanged']=fingerprint(args.probe)==probe
    report['all_qualified']=report['sources_unchanged'] and report['probe_unchanged'] and all(r['qualified'] for r in report['reads'])
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'qualified':report['all_qualified'],'sources':len(paths),'report':str(args.report)}))
    return 0 if report['all_qualified'] else 1


if __name__=='__main__':raise SystemExit(main())
