#!/usr/bin/env python3
"""Pinned native sparse reconstruction and exact source-array evidence.
Original sparse samples and deterministic derived pixels are separate comparisons.
No throughput inference is made. Large detector data stays outside Git.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import fabio
from fabio import sparseimage
import h5py
import numpy as np
p=argparse.ArgumentParser()
p.add_argument('--probe',type=Path,default=Path('target/release/examples/sparse_probe'))
p.add_argument('--corpus',type=Path)
p.add_argument('--output',type=Path)
a=p.parse_args()
if fabio.version!='2026.6.0' or sparseimage.cython_densify is None:raise SystemExit('requires pinned FabIO2026.6.0 with native densifier')
root=Path(__file__).resolve().parents[1]
manifest=json.loads((root/'tests/data/sparse/provenance.json').read_text())
inputs=[(root/'tests/data/sparse'/name,case)for name,case in manifest['files'].items()]
if a.corpus:inputs.append((a.corpus/'output_sparse_0_00000.h5',{}))
dtype_names={'U8':'uint8','U16':'uint16','U32':'uint32','U64':'uint64','I8':'int8','I16':'int16','I32':'int32','I64':'int64','F32':'float32','F64':'float64'}
def digest(array):return hashlib.sha256(np.ascontiguousarray(array.astype(array.dtype.newbyteorder('<'),copy=False)).tobytes()).hexdigest()
def run(*args):return subprocess.run([str(a.probe.resolve()),*map(str,args)],text=True,capture_output=True,check=True)
results=[]
for path,case in inputs:
    record=dict(file=str(path),file_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),policy='Explicit DeterministicBackground, native reference NOISY=False',dense_is_derived=True)
    components=json.loads(run('--components',path).stdout)
    component_results=[]
    with h5py.File(path)as h5:
        entry=h5[h5.attrs['default']];group=entry[entry.attrs.get('pyFAI_sparse_frames',entry.attrs.get('default'))]
        pointers=group['frame_ptr'][()];indices=group['index'][()];intensity=group['intensity'][()]
        for native in components['components']:
            source=h5[native['dataset']]
            if 'dtype'in native:
                values=source[()]
                equal=(list(values.shape)==native['shape']and values.dtype.name==dtype_names[native['dtype']]and digest(values)==native['sha256'])
                component_results.append(dict(name=native['name'],source_dtype=str(values.dtype),shape=list(values.shape),source_sha256=digest(values),native_sha256=native['sha256'],raw_values_match=equal))
            else:
                values=source.asstr()[()]
                strings=np.asarray(values).ravel().tolist()
                component_results.append(dict(name=native['name'],text=strings,raw_values_match=strings==native['text']))
        native_frames=[json.loads(line)for line in run(path).stdout.splitlines()]
        record['components']=component_results;record['raw_components_match']=all(v['raw_values_match']for v in component_results)
        try:
            reference=sparseimage.SparseImage().read(str(path));reference_error=None
        except Exception as error:reference=None;reference_error=f'{type(error).__name__}: {error}'
        frames=[]
        for native in native_frames:
            i=native['frame'];start,stop=map(int,pointers[i:i+2]);raw_index=digest(indices[start:stop]);raw_intensity=digest(intensity[start:stop])
            frame=dict(native,raw_index_sha256=raw_index,raw_intensity_sha256=raw_intensity,raw_sparse_match=raw_index==native['index_sha256']and raw_intensity==native['intensity_sha256'])
            if case:
                expected=case['dense'][i];frame['independent_dense_sha256']=expected['sha256'];frame['independent_reconstruction_match']=native['dense_sha256']==expected['sha256']
            if reference is not None:
                values=reference.data if i==0 else reference.getframe(i).data
                frame['reference_dense_sha256']=digest(values);frame['reconstruction_matches']=frame['reference_dense_sha256']==native['dense_sha256']and values.dtype.name==dtype_names[native['dtype']]
            frames.append(frame)
        record['frames']=frames;record['declared_frames']=len(pointers)-1
        if reference_error:
            record['reference_error']=reference_error
            record['expected_reference_gap']=case.get('reference_gap')
        if reference is not None:reference.close()
    results.append(record)
    print(path.name,f"{len(frames)} frames: source arrays",'MATCH'if record['raw_components_match']and all(f['raw_sparse_match']for f in frames)else'FAIL',', reconstruction', 'MATCH'if all(f.get('reconstruction_matches',f.get('independent_reconstruction_match',False))for f in frames)else'FAIL',flush=True)
report=dict(probe=dict(sha256=hashlib.sha256(a.probe.read_bytes()).hexdigest(),path=str(a.probe),scope='source-array equality and deterministic derived pixels; no throughput claim'),reference=dict(version=fabio.version,upstream_commit='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece',implementation='native Cython densifier, NOISY=False'),files=results)
if a.output:a.output.write_text(json.dumps(report,indent=2)+'\n')
for record in results:
    if not record['raw_components_match']or len(record['frames'])!=record['declared_frames']:raise SystemExit(1)
    if 'reference_error'in record and not record.get('expected_reference_gap'):raise SystemExit(1)
    for frame in record['frames']:
        if not frame['raw_sparse_match']or frame.get('independent_reconstruction_match')is False or frame.get('reconstruction_matches')is False:raise SystemExit(1)
