#!/usr/bin/env python3
"""Test-only Lambda VDS proof using explicit bindings and independent assembly.
No production reader invokes Python, native VDS reads or implicit relocation.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import h5py
import hdf5plugin
import numpy as np
ROOT=Path(__file__).resolve().parents[1]
DATA='/entry/instrument/detector/data'
def digest(path):
 with Path(path).open('rb')as source:return hashlib.file_digest(source,'sha256').hexdigest()
def signature(array):
 return dict(dtype=str(array.dtype),shape=list(array.shape),sample_sha256=hashlib.sha256(array.astype(array.dtype.newbyteorder('<'),copy=False).tobytes()).hexdigest())
def regular(space):
 start,stride,count,block=space.get_regular_hyperslab()
 assert count==(1,1,1)and stride==(1,1,1)
 return start,block

def main():
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument('--probe',type=Path,required=True)
 parser.add_argument('--bindings',type=Path,required=True)
 parser.add_argument('--master',type=Path,default=Path('/tmp/dioptrin-io-fabio-corpus/l1_test02_00002_master.nxs'))
 parser.add_argument('--baseline',type=Path,default=ROOT/'docs/fabio-parity/2026-11-06-tiff-codecs.json')
 parser.add_argument('--output',type=Path,default=ROOT/'docs/vds/2026-11-06-lambda.json')
 args=parser.parse_args();hdf5plugin.register(force=True)
 bindings=json.loads(args.bindings.read_text());actual={entry['stored_filename']:Path(entry['actual_file']).resolve(strict=True)for entry in bindings}
 assert len(actual)==len(bindings)==3
 source_records=[dict(path=str(args.master.resolve()),sha256=digest(args.master))]
 for original,path in actual.items():source_records.append(dict(stored_filename=original,path=str(path),sha256=digest(path),original_exact_path_available=Path(original).exists()))
 native=[json.loads(line)for line in subprocess.check_output([str(args.probe),str(args.master),str(args.bindings)],text=True).splitlines()]
 frames=[];mappings=[]
 with h5py.File(args.master)as master:
  dataset=master[DATA];entries=dataset.virtual_sources();assert len(entries)==3
  shapes=[]
  for item in entries:
   assert item.file_name in actual
   with h5py.File(actual[item.file_name])as f:
    source=f[item.dset_name];shapes.append(source.shape)
    allocation_status=source.id.get_space_status();assert allocation_status==h5py.h5d.SPACE_STATUS_ALLOCATED
    vstart,vblock=regular(item.vspace);sstart,sblock=regular(item.src_space)
    assert vblock[0]==sblock[0]==h5py.h5s.UNLIMITED and vblock[1:]==sblock[1:]
    assert source.shape[1:]==sblock[1:] and vstart[0]==sstart[0]==0
    mappings.append(dict(stored_filename=item.file_name,actual_file=str(actual[item.file_name]),dataset=item.dset_name,
                         source_shape=list(source.shape),source_dtype=str(source.dtype),source_allocation_status=allocation_status,
                         source_chunks=list(source.chunks)if source.chunks else None,source_origin=list(sstart),virtual_origin=list(vstart),spatial_shape=list(vblock[1:])))
  count=shapes[0][0];assert all(shape[0]==count for shape in shapes)==True
  shape=list(entries[0].vspace.get_simple_extent_dims()[1:]);pixels=shape[0]*shape[1]
  covered=sum(item['spatial_shape'][0]*item['spatial_shape'][1]for item in mappings)
  for index in range(count):
   expected=np.full(shape,dataset.fillvalue,dtype=dataset.dtype)
   for item in mappings:
    _,sy,sx=item['source_origin'];_,dy,dx=item['virtual_origin'];height,width=item['spatial_shape']
    with h5py.File(item['actual_file'])as source:
     expected[dy:dy+height,dx:dx+width]=source[item['dataset']][index,sy:sy+height,sx:sx+width]
   # Reference-only native VDS access intentionally records HDF5's basename
   # fallback separately; the independent expected image above uses only the
   # caller's exact explicit filename bindings and direct source datasets.
   reference=dataset[index]
   assert np.array_equal(reference,expected)
   wanted=signature(expected);got=native[index]
   assert got['dtype']=='I16'and got['shape']==wanted['shape']and got['sample_sha256']==wanted['sample_sha256']
   assert got['frames']==count and got['unmapped_pixels_per_frame']==pixels-covered
   assert set(map(str,map(Path,got['sources'])))=={item['path']for item in source_records}
   frames.append(dict(frame=index,native=got,independent_assembly=wanted,h5py_vds=signature(reference),
                      all_pixels_equal=True,minimum=int(expected.min()),maximum=int(expected.max())))
  assert len(native)==len(frames)==5
  fill=int(dataset.fillvalue)
 for item in source_records:assert digest(item['path'])==item['sha256']
 original_hash=source_records[0]['sha256'];baseline=json.loads(args.baseline.read_text())
 events=[item for item in baseline['reads']if item.get('file_sha256')==original_hash and item.get('status')=='native_rejected'];assert len(events)==2
 previous=json.loads((ROOT/'docs/rejection-audits/2026-11-05-lambda-ty5.json').read_text())
 old_lambda=next(item for item in previous['records']if item['format']=='Lambda VDS')
 ty5=next(item for item in previous['records']if item['format']=='Oxford TY5')
 result=dict(schema=1,baseline=str(args.baseline.relative_to(ROOT)),baseline_sha256=digest(args.baseline),audited_events=2,audited_unique_sources=1,
             reference=dict(h5py=h5py.__version__,hdf5=h5py.version.hdf5_version,hdf5plugin=hdf5plugin.version,
                            h5py_native_library=dict(path=str(Path(h5py.h5.__file__).resolve()),sha256=digest(h5py.h5.__file__)),
                            h5py_vds_policy='Reference-only implicit HDF5 basename fallback; separately checked against exact bound source assembly'),
             probe_sha256=digest(args.probe),source_code_sha256=digest(ROOT/'src/hdf5/vds.rs'),
             rust_sources_sha256={name:digest(ROOT/name)for name in ['src/hdf5.rs','src/hdf5/vds.rs','src/hdf5/typed.rs','src/hdf5/sources.rs','src/source.rs','src/pixels.rs','Cargo.lock']},
             records=[dict(source=str(args.master.resolve()),baseline_sha256=original_hash,baseline_event_count=2,baseline_tests=[e['test']for e in events],
                           input_condition=old_lambda['input_condition'],reader_action='explicit_source_layout_closed',classification='implemented_explicit_immutable_vds_mosaic',
                           read_policy=dict(filename_bindings=bindings,unmapped_regions='DeclaredFill',mapped_sources_require_complete_allocated_storage=True,default_virtual_datasets='Reject',native_virtual_dataset_reads=False),
                           metadata=dict(frames=count,shape=shape,pixel_type='I16',declared_fill=fill,unmapped_pixels_per_frame=pixels-covered,mappings=mappings,source_graph=source_records),
                           verification=dict(frames=frames,all_five_frames_all_pixels_match_independent_assembly_and_h5py=True))],
             related_preserved_records=[ty5])
 args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+'\n')
if __name__=='__main__':main()
