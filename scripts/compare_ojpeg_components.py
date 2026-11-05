#!/usr/bin/env python3
"""Test-only native OJPEG component proof against an explicitly pinned LibTIFF."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
ROOT=Path(__file__).resolve().parents[1]
def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--probe',type=Path,required=True)
 p.add_argument('--reference-library',type=Path,required=True)
 p.add_argument('--reference-script',type=Path,default=ROOT/'scripts/tiff_component_reference.py')
 p.add_argument('--corpus',type=Path,default=Path('/tmp/dioptrin-io-fabio-corpus/libtiffpic.tar.gz__content'))
 p.add_argument('--output',type=Path,default=ROOT/'docs/ojpeg/2026-11-06-components.json')
 args=p.parse_args();spec=importlib.util.spec_from_file_location('dioptrin_test_tiff_reference',args.reference_script);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
 oracle=module.Reference(args.reference_library)
 cases=[]
 fixtures=list((ROOT/'tests/data/ojpeg').glob('*.tif'))
 for path in [args.corpus/'zackthecat.tif',args.corpus/'smallliz.tif']+sorted(fixtures):
  native=json.loads(subprocess.check_output([str(args.probe),str(path)],text=True));reference=oracle.read(path)
  assert native['shape']==reference['shape'] and native['subsampling']==reference['subsampling']
  assert len(native['chunks'])==len(reference['chunks'])
  for got,expected in zip(native['chunks'],reference['chunks']):
   assert got['origin']==expected['origin'] and got['image_shape']==expected['image_shape']
   for actual,wanted in zip(got['planes'],expected['planes']):
    assert all(actual[key]==wanted[key]for key in ['component','shape','valid_shape','sampling','sample_sha256']),(path,actual,wanted)
  cases.append(dict(source=str(path.resolve())if path not in fixtures else str(path.relative_to(ROOT)),file_sha256=digest(path),native=native,reference=reference,all_native_component_samples_match=True))
 result=dict(schema=1,contract='Original Y/Cb/Cr grids and padding; separate decoded-JPEG lossy provenance; no implicit RGB/upsampling',
             reference_library=oracle.metadata,reference_script_sha256=digest(args.reference_script),native_probe_sha256=digest(args.probe),
             authority=dict(libtiff_version='4.7.2',source='libtiff/tif_ojpeg.c',source_sha256='e0dcb8bf12d3b403407155ac84402158c4efb0f19d2c6e10f16329b2bf311bc4',functions=['OJPEGReadHeaderInfoSecTables*','OJPEGWriteStreamSof','OJPEGWriteStreamSos']),
             qualifications=['Source original windows/headers/offsets retained; smallliz baselineSOS Se0->63 follows LibTIFF original-JPEG container interpretation',
              'Native warnings fail; small pre-EOI entropy slack may escape warning through mature libjpeg bit lookahead; no full semantic encoded-integrity claim',
              'Only one complete 8-bit contiguous baselineYCbCr strip/tile; no fragmented or restart reconstruction','Explicit display comparisons belong to separate consumer-view policy'],
             files=cases)
 args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+'\n')
if __name__=='__main__':main()
