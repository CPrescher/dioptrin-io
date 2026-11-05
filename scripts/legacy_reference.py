#!/usr/bin/env python3
"""Compare the same physical files with pinned FabIO and native legacy_probe.
Run in a Python environment with FabIO 2026.6.0 and NumPy; no runtime dependency.
Large corpus stays outside Git. JSON records source hashes and exact count hashes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import fabio
from fabio.brukerimage import BrukerImage
from fabio.bruker100image import Bruker100Image
from fabio.OXDimage import OxdImage
from fabio.raxisimage import RaxisImage

parser = argparse.ArgumentParser()
parser.add_argument('--probe',type=Path,default=Path('target/debug/examples/legacy_probe'))
parser.add_argument('--corpus',type=Path)
parser.add_argument('--output',type=Path)
args=parser.parse_args()
if fabio.version != '2026.6.0':
    raise SystemExit(f'expected FabIO 2026.6.0, got {fabio.version}')
root=Path(__file__).resolve().parents[1]
manifest=json.loads((root/'tests/data/legacy/provenance.json').read_text())
inputs=[]
for name,case in manifest['files'].items():
    family=case['family']
    native=family
    if family=='raxis':
        native='raxis-little-expanded' if case.get('little_endian') else 'raxis-expanded'
    reference=Bruker100Image if name.startswith('bruker100') else BrukerImage if family=='bruker' else OxdImage if family=='oxford' else RaxisImage
    inputs.append((root/'tests/data/legacy'/name,native,reference,case.get('reference_difference'),case.get('little_endian',False)))
if args.corpus:
    for name,family,reference in [('Cr8F8140k103.0026','bruker',BrukerImage),('NaCl_10_01_0009.sfrm','bruker',Bruker100Image),('b191_1_9_1.img','oxford',OxdImage),('b191_1_9_1_uncompressed.img','oxford',OxdImage),('d80_60s.img','oxford',OxdImage),('pilatus300k.rod_img','oxford',OxdImage),('mgzn-20hpt.img','raxis-expanded',RaxisImage),('100nmfilmonglass_1_1.img','oxford',OxdImage)]:
        inputs.append((args.corpus/name,family,reference,'Oxford TY5 explicitly rejected; known unreliable reference' if '100nm' in name else None,False))
results=[]
for path,native,reference,difference,little in inputs:
    record=dict(file=str(path),file_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),policy=native)
    image=reference()
    if little: image.endianness='<'
    try:
        data=image.read(str(path)).data
        dtype=data.dtype.newbyteorder('<')
        record['reference']=dict(shape=list(data.shape),dtype=str(dtype),frame_sha256=hashlib.sha256(data.astype(dtype,copy=False).tobytes()).hexdigest())
    except Exception as error:
        record['reference_error']=str(error)
    result=subprocess.run([str(args.probe.resolve()),native,str(path)],capture_output=True,text=True,check=False)
    if result.returncode:
        record['native_error']=result.stderr.strip()
    else:
        record['native']=json.loads(result.stdout)
    dtype_names={'U8':'uint8','U16':'uint16','U32':'uint32','I8':'int8','I16':'int16','I32':'int32','I64':'int64'}
    if 'native' in record and 'reference' in record:
        n,r=record['native'],record['reference']
        record['equal']=(n['shape']==r['shape'] and n['frame_sha256']==r['frame_sha256'] and dtype_names[n['dtype']]==r['dtype'])
    else:
        record['equal']=False
    if difference:record['expected_difference']=difference
    results.append(record)
    print(path.name, 'MATCH' if record['equal'] else 'EXPECTED DIFFERENCE' if difference else 'FAIL')
report=dict(reference=dict(version=fabio.version,upstream_commit='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'),files=results)
if args.output: args.output.write_text(json.dumps(report,indent=2)+'\n')
if any(not r['equal'] and 'expected_difference' not in r for r in results):raise SystemExit(1)
