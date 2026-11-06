"""Test-only standalone CIF header-projection and legacy helper audit.

CIF.loadCIF/readCIF are observed unchanged on fresh paths and BytesIO objects.
Potentially broken C/legacy profile helpers run in disposable subprocesses with
isolated TMPDIR and cwd; their failures are not reader parity successes.
"""
import argparse
import hashlib
import importlib
import inspect
import io
import json
import os
import pathlib
import subprocess
import sys
import tempfile

from companion_reference import differences, fingerprint
ROOT=pathlib.Path(__file__).resolve().parents[1]
PIN='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'
DATA=ROOT/'tests/data/companions'
EXPECTED={
 'scalars-loops.cif':{'data_blocks':['independent'],
  'fields':{'_title':'two words','_double':'double quoted','_numeric_text':'00012.340(5)','_unknown':'?','_inapplicable':'.','_multiline':'first line\nsecond line'},
  'loops':[{'columns':['_atom_label','_atom_x'],'rows':[['A1','1.234(5)'],['B two','-0.000']]}], 'diagnostics':[]},
 'duplicates.cif':{'data_blocks':['duplicates'],
  'fields':{'_duplicate':'final','_numeric_text':'000000000000000000123.400(99)'},'loops':[],
  'diagnostics':['duplicate CIF item _duplicate; all entries retained']},
 'diagnostics.cif':{'data_blocks':['first','second'],'fields':{'_duplicate':'final','_second':'retained'},
  'loops':[{'columns':['_col_a','_col_b'],'rows':[['1','2']]}]},
}
EXPECTED['scalars-loops.cif']['entries']=[[key,value] for key,value in EXPECTED['scalars-loops.cif']['fields'].items()]
EXPECTED['duplicates.cif']['entries']=[['_duplicate','first'],['_duplicate','final'],['_numeric_text','000000000000000000123.400(99)']]
EXPECTED['diagnostics.cif']['entries']=[['_duplicate','first'],['_duplicate','final'],['_second','retained']]


def reference_projection(cif):
    fields={key:value for key,value in cif.items() if key!='loop_'}
    loops=[]
    def text(value):
        return value.decode('ascii') if isinstance(value,bytes) else value
    for columns,rows in cif.get('loop_',[]):
        loops.append({'columns':[text(column) for column in columns],'rows':[[text(row[column]) for column in columns] for row in rows]})
    return {'fields':fields,'loops':loops}


def reference_reads(path):
    from fabio.cbfimage import CIF
    payload=pathlib.Path(path).read_bytes()
    results=[]
    for method in ('loadCIF','readCIF'):
        for transport in ('path','BytesIO'):
            record={'method':method,'transport':transport}
            try:
                obj=CIF()
                getattr(obj,method)(str(path) if transport=='path' else io.BytesIO(payload))
                record.update(status='read_completed',projection=reference_projection(obj),
                    projection_policy='pinned loop byte strings decoded through explicit strict ASCII; values remain strings with no numerical conversion',
                    raw_loops=[{'columns':[bytes(column).hex() for column in columns],
                        'rows':[[bytes(row[column]).hex() for column in columns] for row in rows]}
                        for columns,rows in obj.get('loop_',[])])
            except Exception as error:
                record.update(status='read_not_completed',error=f'{type(error).__name__}: {error}')
            results.append(record)
    return results


def compare_record(native,expected,payload,provenance,reference):
    delta=[]
    for key,value in expected.items():
        delta.extend(differences(native.get(key),value,'$.'+key))
    for key,value in {'kind':'cif_document','coverage':'HeaderProjection','provenance':provenance,
                      'source_bytes':len(payload),'source_sha256':hashlib.sha256(payload).hexdigest()}.items():
        delta.extend(differences(native.get(key),value,'$.'+key))
    if not isinstance(native.get('coverage_warning'),str) or '_array_data.data' not in native['coverage_warning'] or 'full CIF grammar' not in native['coverage_warning']:
        delta.append({'field':'$.coverage_warning','kind':'missing_explicit_projection_scope'})
    if any(key in native for key in ('pixels','sample_sha256','shape','dtype')):
        delta.append({'field':'$','kind':'unexpected_pixel_claim_in_document'})
    if expected is EXPECTED['diagnostics.cif']:
        diagnostics=native.get('diagnostics',[])
        for text in ['duplicate CIF item','unsupported CIF token save_','has no value','incomplete CIF loop row','multiple CIF data blocks']:
            if not any(text in message for message in diagnostics):
                delta.append({'field':'$.diagnostics','missing':text})
    else:
        projection={key:native.get(key) for key in ('fields','loops')}
        for record in reference:
            if record['status']!='read_completed':
                delta.append({'field':'$.reference','method':record['method'],'transport':record['transport'],'kind':'read_not_completed'})
            else:
                delta.extend(differences(projection,record['projection'],f"$.reference.{record['method']}.{record['transport']}"))
    return delta


def verify(probe,path):
    source=fingerprint(path);payload=pathlib.Path(path).read_bytes();reference=reference_reads(path)
    record={'source':source,'reference':reference,'apis':[],'status':'error',
            'scope':'source-preserving scalar/loop HeaderProjection only; no image counts or full CIF grammar claim'}
    expected=EXPECTED[pathlib.Path(path).name]
    for command,provenance in [('cif-document','PathSnapshot'),('cif-document-bytes','Buffer'),('cif-document-stream','StreamCapture')]:
        api={'command':command,'status':'error'};record['apis'].append(api)
        try:
            args=[str(probe),command,str(path)]
            if provenance=='StreamCapture':
                with pathlib.Path(path).open('rb') as stream:
                    result=subprocess.run(args,stdin=stream,capture_output=True,text=True,timeout=30)
            else:
                result=subprocess.run(args,capture_output=True,text=True,timeout=30)
            api['exit_code']=result.returncode
            if result.returncode:
                api['error']=result.stderr[:4000]
            else:
                native=json.loads(result.stdout)
                delta=compare_record(native,expected,payload,provenance,reference)
                api.update(status='match' if not delta else 'difference',differences=delta,native=native)
        except Exception as error:
            api['error']=f'{type(error).__name__}: {error}'
    record['source_unchanged']=source==fingerprint(path)
    record['status']='qualified_projection' if record['source_unchanged'] and all(api['status']=='match' for api in record['apis']) else 'difference'
    if path.name=='diagnostics.cif':
        record['reference_qualification']='unsupported syntax is retained with native diagnostics/raw source; reference observations are not asserted as complete CIF interpretation'
    return record


CHILD=r'''
import hashlib,inspect,json,pathlib,sys
result={'operation':sys.argv[1]}
def fp(path):
 path=pathlib.Path(path)
 return {'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
try:
 if sys.argv[1]=='cf_io.read':
  from fabio.ext import cf_io
  result['module']=fp(inspect.getfile(cf_io))
  with open(sys.argv[2],'rb') as source:
   data,labels=cf_io.read(source)
  result.update(status='read_completed',shape=list(data.shape),dtype=data.dtype.name,labels=labels,data=data.tolist())
 elif sys.argv[1]=='columnfile.read':
  from fabio.datIO import columnfile
  import fabio.datIO
  result['module']=fp(inspect.getfile(fabio.datIO))
  obj=columnfile();obj.read(sys.argv[2])
  result.update(status='read_completed',shape=list(obj.data.shape),dtype=obj.data.dtype.name,labels=obj.clabels,data=obj.data.tolist())
 elif sys.argv[1]=='CIF.loadCHIPLOT':
  from fabio.cbfimage import CIF
  import fabio.cbfimage
  result['module']=fp(inspect.getfile(fabio.cbfimage))
  obj=CIF();obj.loadCHIPLOT(sys.argv[2])
  result.update(status='read_completed',fields=dict(obj))
except Exception as error:
 result.update(status='read_not_completed',error_type=type(error).__name__,error=str(error))
print(json.dumps(result))
'''


def probe_legacy_helpers(python=sys.executable):
    records=[]
    for operation,name in [('cf_io.read','column-table.flt'),('columnfile.read','column-table.flt'),('CIF.loadCHIPLOT','profile.chi')]:
        source=DATA/name
        record={'operation':operation,'source':fingerprint(source),'status':'audit_error'}
        with tempfile.TemporaryDirectory(prefix='dioptrin-companion-legacy-') as directory:
            environment=dict(os.environ,TMPDIR=directory,TEMP=directory,TMP=directory)
            try:
                process=subprocess.run([str(python),'-c',CHILD,operation,str(source)],cwd=directory,env=environment,capture_output=True,text=True,timeout=15)
                record.update(exit_code=process.returncode,stderr=process.stderr[:4000])
                if process.returncode:
                    record['status']='process_failed'
                else:
                    record.update(json.loads(process.stdout))
                record['disposable_files']=[str(path.relative_to(directory)) for path in pathlib.Path(directory).rglob('*') if path.is_file()]
            except Exception as error:
                record['error']=f'{type(error).__name__}: {error}'
        record['source_unchanged']=fingerprint(source)==record['source']
        record['qualification']='reference helper observation only; failed/incomplete reads are not native/API parity successes'
        records.append(record)
    return records


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--probe',type=pathlib.Path,required=True)
    parser.add_argument('--report',type=pathlib.Path,required=True)
    parser.add_argument('--fabio-source',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-2026.6.0'))
    args=parser.parse_args()
    if subprocess.check_output(['git','-C',str(args.fabio_source),'rev-parse','HEAD'],text=True).strip()!=PIN:
        parser.error('reference checkout must be pinned FabIO')
    import fabio
    if fabio.version!='2026.6.0':parser.error('reference package must be FabIO2026.6.0')
    module=importlib.import_module('fabio.cbfimage');actual=pathlib.Path(inspect.getfile(module));pinned=args.fabio_source/'src/fabio/cbfimage.py'
    paths=[actual,pinned,pathlib.Path(__file__),ROOT/'scripts/test_cif_document_reference.py',ROOT/'tests/cif_document.rs',ROOT/'Cargo.toml',ROOT/'Cargo.lock',DATA/'provenance.json',*sorted((ROOT/'src').rglob('*.rs'))]
    paths += [args.fabio_source/'src/fabio/datIO.py',args.fabio_source/'src/fabio/ext/cf_io.pyx']
    sources={str(path):fingerprint(path) for path in paths}
    report={'schema':1,'contract':'standalone CIF header projection; metadata/source fidelity, no pixels or full grammar claim',
            'reference_commit':PIN,'reference_version':fabio.version,'python':sys.version,
            'probe':fingerprint(args.probe),'sources':sources,'reference_matches_pinned':fingerprint(actual)['sha256']==fingerprint(pinned)['sha256']}
    report['documents']=[verify(args.probe,DATA/name) for name in EXPECTED]
    report['legacy_helpers']=probe_legacy_helpers()
    report['legacy_helper_modules_unchanged']=all(fingerprint(row['module']['path'])['sha256']==row['module']['sha256'] for row in report['legacy_helpers'] if 'module' in row)
    report['sources_unchanged']=all(fingerprint(path)==value for path,value in sources.items())
    report['probe_unchanged']=fingerprint(args.probe)==report['probe']
    report['all_projections_qualified']=all(item['status']=='qualified_projection' for item in report['documents']) and report['reference_matches_pinned'] and report['sources_unchanged'] and report['probe_unchanged']
    report['legacy_helper_read_parity_claimed']=False
    args.report.parent.mkdir(parents=True,exist_ok=True)
    args.report.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'qualified_projection':report['all_projections_qualified'],'legacy_helpers':[{key:row.get(key) for key in ['operation','status','error_type','error']} for row in report['legacy_helpers']],'report':str(args.report)}))
    return 0 if report['all_projections_qualified'] else 1


if __name__=='__main__':raise SystemExit(main())
