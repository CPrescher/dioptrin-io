"""Independent exact-text/numeric oracle for native CHIPLOT reads.

The unchanged pinned CIF.loadCHIPLOT is observed separately; its exceptions are
reference failures, never native/API parity successes. No patched reference is
used. Source tokens are parsed independently with Python int/Decimal rather
than being rounded through binary floats.
"""
import argparse
import decimal
import hashlib
import importlib
import inspect
import json
import pathlib
import re
import subprocess
import sys

from companion_reference import differences,fingerprint
ROOT=pathlib.Path(__file__).resolve().parents[1]
DATA=ROOT/'tests/data/chiplot'
PIN='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'


def number(text):
    if re.fullmatch(r'[+-]?[0-9]+',text):
        value=int(text)
        if -(2**63)<=value<2**63:return {'kind':'I64','value':value}
        if 0<=value<2**64:return {'kind':'U64','value':value}
        return {'kind':'IntegerText'}
    if text.lower().lstrip('+-') in {'nan','inf','infinity'}:return {'kind':'NonFinite'}
    if not re.fullmatch(r'[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?',text):
        raise ValueError('unsupported independent profile scalar')
    decimal.Decimal(text) # no context precision reduction or float roundtrip
    return {'kind':'Decimal'}


def exact_numeric_identity(text):
    value=decimal.Decimal(text)
    triple=value.as_tuple()
    return {'sign':triple.sign,'digits':''.join(str(digit) for digit in triple.digits),'exponent':triple.exponent}


def oracle(payload):
    lines=payload.splitlines(keepends=True)
    if len(lines)<4:raise ValueError('incomplete profile header')
    result={'headers':[],'rows':[],'comments':[]}
    offset=0
    for raw in lines[:3]:
        content=raw.rstrip(b'\r\n')
        try:text=content.decode('utf8')
        except UnicodeDecodeError:text=None
        result['headers'].append({'span':[offset,offset+len(content)],'text':text,'hex':content.hex()})
        offset+=len(raw)
    declaration=lines[3].rstrip(b'\r\n')
    result['declaration']={'span':[offset,offset+len(declaration)],'text':declaration.decode('ascii'),'hex':declaration.hex()}
    parts=re.split(rb'[\s,]+',declaration.split(b'#',1)[0].strip())
    if len(parts) not in [1,2] or any(not value.isdigit() for value in parts):raise ValueError('unsupported declaration')
    result['declared_points']=int(parts[0]);result['curves']=int(parts[1]) if len(parts)==2 else 1
    if b'#' in declaration:
        index=declaration.index(b'#');result['comments'].append({'line':4,'offset':offset+index,'hex':declaration[index:].hex()})
    offset+=len(lines[3])
    exact=[]
    for index,raw in enumerate(lines[4:],5):
        line=raw.rstrip(b'\r\n')
        if b'#' in line:
            at=line.index(b'#');result['comments'].append({'line':index,'offset':offset+at,'hex':line[at:].hex()});line=line[:at]
        if line.strip():
            tokens=list(re.finditer(rb'[^\s,]+',line))
            if len(tokens)!=1+result['curves']:raise ValueError('independent row width mismatch')
            samples=[]
            for token in tokens:
                text=token.group().decode('ascii');samples.append({'offset':offset+token.start(),'lexeme':text,'number':number(text)})
                exact.append(exact_numeric_identity(text))
            result['rows'].append({'line':index,'samples':samples})
        offset+=len(raw)
    if len(result['rows'])!=result['declared_points']:raise ValueError('independent count mismatch')
    result['exact_numeric_identities']=exact
    return result


def observe_reference(path):
    from fabio.cbfimage import CIF
    image=CIF()
    try:
        image.loadCHIPLOT(str(path))
        return {'status':'read_completed','fields':dict(image),'reading_parity_claimed':False}
    except Exception as error:
        kind='reference_failure'
        if isinstance(error,TypeError) and "'str' and 'int'" in str(error):kind='pinned_string_vs_integer_step_bug'
        elif isinstance(error,ZeroDivisionError):kind='one_point_uniform_projection_undefined'
        elif isinstance(error,AssertionError):kind='reference_row_count_or_delimiter_limitation'
        elif isinstance(error,ValueError):kind='reference_numeric_or_delimiter_limitation'
        return {'status':'read_not_completed','error_type':type(error).__name__,'error':str(error),'classification':kind,
                'partial_fields':dict(image),'reading_parity_claimed':False,
                'qualification':'unchanged pinned failure recorded; native source-reading proof uses primary format and exact independent oracle, no patched reference'}


def compare_record(native,expected,payload,provenance):
    delta=[]
    for key in ['headers','declaration','declared_points','curves','rows','comments']:
        delta.extend(differences(native.get(key),expected[key],'$.'+key))
    for key,value in {'kind':'chiplot','representation':'ExactSourceLexemes','provenance':provenance,'source_bytes':len(payload),'source_sha256':hashlib.sha256(payload).hexdigest()}.items():
        delta.extend(differences(native.get(key),value,'$.'+key))
    nonfinite=any(cell['number']['kind']=='NonFinite' for row in expected['rows'] for cell in row['samples'])
    if nonfinite and not any('nonfinite' in message for message in native.get('diagnostics',[])):
        delta.append({'field':'$.diagnostics','missing':'nonfinite lexical extension qualification'})
    try:
        actual=[exact_numeric_identity(cell['lexeme']) for row in native['rows'] for cell in row['samples']]
        delta.extend(differences(actual,expected['exact_numeric_identities'],'$.exact_numeric_identities'))
    except Exception as error:delta.append({'field':'$.exact_numeric_identities','error':str(error)})
    return delta


def verify(probe,path):
    payload=path.read_bytes();expected=oracle(payload)
    record={'source':fingerprint(path),'independent_numeric_oracle':{'implementation':'Python int + Decimal, no binary64 conversion','identities':expected['exact_numeric_identities']},
            'reference':observe_reference(path),'apis':[],'status':'error'}
    for command,provenance in [('chiplot','PathSnapshot'),('chiplot-bytes','Buffer'),('chiplot-stream','StreamCapture')]:
        api={'command':command,'status':'error'};record['apis'].append(api)
        try:
            args=[str(probe),command,str(path)]
            if provenance=='StreamCapture':
                with path.open('rb') as stream:process=subprocess.run(args,stdin=stream,capture_output=True,text=True,timeout=30)
            else:process=subprocess.run(args,capture_output=True,text=True,timeout=30)
            api['exit_code']=process.returncode
            if process.returncode:api['error']=process.stderr[:3000]
            else:
                native=json.loads(process.stdout);delta=compare_record(native,expected,payload,provenance)
                api.update(status='match' if not delta else 'difference',differences=delta,native=native)
        except Exception as error:api['error']=f'{type(error).__name__}: {error}'
    record['source_unchanged']=record['source']==fingerprint(path)
    record['status']='qualified_native_source_read' if record['source_unchanged'] and all(api['status']=='match' for api in record['apis']) else 'difference'
    return record


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--probe',type=pathlib.Path,required=True);parser.add_argument('--report',type=pathlib.Path,required=True)
    parser.add_argument('--fabio-source',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-2026.6.0'))
    args=parser.parse_args()
    if subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()!=PIN:parser.error('reference must be pinned FabIO checkout')
    import fabio
    if fabio.version!='2026.6.0':parser.error('reference package must be FabIO2026.6.0')
    module=importlib.import_module('fabio.cbfimage');actual=pathlib.Path(inspect.getfile(module));pinned=args.fabio_source/'src/fabio/cbfimage.py'
    provenance=json.loads((DATA/'provenance.json').read_text());authority=provenance['primary_format']
    format_copy=pathlib.Path(authority['local_audit_copy'])
    if not format_copy.is_file() or fingerprint(format_copy)['sha256']!=authority['sha256']:parser.error('primary format audit copy missing or changed')
    paths=[actual,pinned,pathlib.Path(__file__),ROOT/'scripts/test_chiplot_reference.py',ROOT/'scripts/generate_chiplot_fixtures.py',DATA/'provenance.json',ROOT/'tests/chiplot.rs',ROOT/'Cargo.toml',ROOT/'Cargo.lock',format_copy,*sorted((ROOT/'src').rglob('*.rs'))]
    sources={str(path):fingerprint(path) for path in paths}
    report={'schema':1,'contract':'native CHIPLOT profile reading with exact source lexemes and scalar types; derived float/grid views separately tested; unchanged reference failures are not API parity successes',
            'reference_commit':PIN,'reference_version':fabio.version,'python':sys.version,'format_authority':authority,
            'reference_source_matches':fingerprint(actual)['sha256']==fingerprint(pinned)['sha256'],'sources':sources,'probe':fingerprint(args.probe)}
    report['reads']=[verify(args.probe,DATA/row['path']) for row in provenance['files']]
    report['sources_unchanged']=all(fingerprint(path)==value for path,value in sources.items())
    report['probe_unchanged']=fingerprint(args.probe)==report['probe']
    report['native_source_reading_qualified']=all(row['status']=='qualified_native_source_read' for row in report['reads']) and report['reference_source_matches'] and report['sources_unchanged'] and report['probe_unchanged']
    report['unchanged_reference_read_parity_claimed']=False;report['patched_reference_used']=False
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n')
    print(json.dumps({'native_source_reading_qualified':report['native_source_reading_qualified'],'sources':len(report['reads']),'native_api_calls':sum(len(row['apis']) for row in report['reads']),'unchanged_reference_read_parity_claimed':False,'report':str(args.report)}))
    return 0 if report['native_source_reading_qualified'] else 1


if __name__=='__main__':raise SystemExit(main())
