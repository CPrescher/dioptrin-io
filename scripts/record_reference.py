#!/usr/bin/env python3
"""Same-file reference comparisons for FIT2D, KCD and MPA.
Pinned FabIO 2026.6.0, NumPy and native record_probe. External files stay outside Git.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import fabio
import numpy as np
from fabio.fit2dimage import Fit2dImage
from fabio.kcdimage import KcdImage
from fabio.mpaimage import MpaImage
p=argparse.ArgumentParser()
p.add_argument('--probe',type=Path,default=Path('target/debug/examples/record_probe'))
p.add_argument('--corpus',type=Path)
p.add_argument('--output',type=Path)
a=p.parse_args()
if fabio.version!='2026.6.0':raise SystemExit('expected FabIO 2026.6.0')
root=Path(__file__).resolve().parents[1]
manifest=json.loads((root/'tests/data/records/provenance.json').read_text())
inputs=[(root/'tests/data/records'/name,case['family'],case)for name,case in manifest['files'].items()]
if a.corpus:
    inputs += [(a.corpus/name,family,{})for name,family in [('fit2d.f2d','fit2d'),('Pilatus1M.f2d','fit2d'),('i01f0001.kcd','kcd'),('mpa_test.mpa','mpa')]]
dtypes={'U8':'uint8','U16':'uint16','I32':'int32','F32':'float32','F64':'float64'}
def frame(name,data):
    dtype=data.dtype.newbyteorder('<')
    return dict(name=name,shape=list(data.shape),dtype=str(dtype),frame_sha256=hashlib.sha256(data.astype(dtype,copy=False).tobytes()).hexdigest())
def native(kind,path):
    result=subprocess.run([str(a.probe.resolve()),kind,str(path)],text=True,capture_output=True)
    if result.returncode:return dict(error=result.stderr.strip())
    return json.loads(result.stdout)
def compare(n,r):
    if 'arrays' not in n:return False
    arrays=[dict(value,dtype=dtypes[value['dtype']])for value in n['arrays']]
    return sorted(arrays,key=lambda x:x['name'])==sorted(r,key=lambda x:x['name'])
results=[]
for path,family,case in inputs:
    record=dict(file=str(path),file_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),family=family)
    if family=='fit2d':
        image=Fit2dImage().read(str(path))
        refs=[frame('data_array',image.data)]+[frame(k,v)for k,v in image.header.items()if isinstance(v,np.ndarray)]
        record.update(native=native('fit2d',path),reference=refs)
        record['equal']=compare(record['native'],refs)
        if case.get('expected_reference_difference'):record['expected_difference']=case['expected_reference_difference']
    elif family=='kcd':
        image=KcdImage().read(str(path))
        refs=[frame('sum',image.data)]
        record.update(native=native('kcd-summed',path),reference=refs,policy='explicit SumReadouts')
        record['equal']=compare(record['native'],refs)
        if case.get('overflow'):
            record['expected_difference']='Native signed32 sum overflow rejected; reference wraps'
        else:
            rows,cols=image.data.shape
            count=int(image.header['Number of readouts'])
            raw=np.frombuffer(path.read_bytes()[4100:],dtype='<u2').reshape(count,rows,cols)
            record['stored_readouts']=native('kcd',path)
            record['stored_reference']=[frame(f'readout{i}',v)for i,v in enumerate(raw)]
            record['stored_equal']=compare(record['stored_readouts'],record['stored_reference'])
    else:
        # Independent section-count parser with NumPy conversion also checks the
        # spectra intentionally omitted by FabIO's primary-image interface.
        lines=path.read_text().splitlines(); refs=[]
        fields={};prefix='';i=0
        while i<len(lines):
            line=lines[i].strip();i+=1
            if line.startswith(('[DATA','[CDAT')):
                name,count=line[1:-1].split(',');count=int(count)
                values=np.array(lines[i:i+count],dtype=np.float64);i+=count
                shape=(int(fields['ADC1_range']),int(fields['ADC2_range']))if name.startswith('CDAT')else(1,count)
                refs.append(frame(name,values.reshape(shape)))
            elif line.startswith('['):prefix=line[1:].split(']',1)[0]
            elif '=' in line:
                k,v=line.split('=',1);fields[(prefix+'_'if prefix else'')+k.strip()]=v.strip()
        record.update(native=native('mpa',path),reference=refs)
        record['equal']=compare(record['native'],refs)
        try:
            image=MpaImage().read(str(path))
            record['fabio_primary']=frame('CDAT0',image.data)
            record['fabio_primary_equal']=record['fabio_primary']==next(v for v in refs if v['name']=='CDAT0')
        except Exception as error:
            record['fabio_error']=str(error)
            record['reference_gap']='FabIO primary API cannot expose multiple CDAT frames; independent section comparison used'
    print(path.name,'MATCH'if record['equal']else'EXPECTED DIFFERENCE'if 'expected_difference'in record else'FAIL')
    results.append(record)
report=dict(reference=dict(version=fabio.version,upstream_commit='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'),files=results)
if a.output:a.output.write_text(json.dumps(report,indent=2)+'\n')
if any((not r['equal']and'expected_difference'not in r)or r.get('stored_equal')is False or r.get('fabio_primary_equal')is False for r in results):raise SystemExit(1)
