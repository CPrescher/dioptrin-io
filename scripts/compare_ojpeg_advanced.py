"""Qualified remaining OJPEG profiles, raw source words and separate stock views."""
import argparse
import ctypes as C
import hashlib
import json
import pathlib
import subprocess
import sys
import numpy as np
import tifffile
import fabio
from compare_ojpeg_fragments import compare_components,digest,pin_fabio
from generate_ojpeg_advanced_fixtures import bind,source_planes
from tiff_component_reference import Reference
ROOT=pathlib.Path(__file__).resolve().parents[1]
EXPECTED={n+'.tif'for n in ['single-prefix','single-prefix-edge','single-first-header','single-prefix-restarts','header-across-two','header-across-three','header-without-interchange','single-table-restarts','complete-internal-restarts','stored-restarts-prefix','sof1-prefix','sof1-complete','planar444-prefix','planar444-complete','planar444-sof1','planar420-complete','table-grid-explicit-reset']}

HELPERS=['compare_ojpeg_advanced.py','generate_ojpeg_advanced_fixtures.py','compare_ojpeg_fragments.py','tiff_component_reference.py','test_ojpeg_advanced_reference.py','generate_ojpeg_fragment_fixtures.py']
def helper_hashes():return {name:digest(ROOT/'scripts'/name)for name in HELPERS}
def fingerprints():
 files=[ROOT/name for name in ['Cargo.toml','Cargo.lock','build.rs','examples/ojpeg_probe.rs']]
 for root in ['src','vendor','native']:files.extend(p for p in(ROOT/root).rglob('*')if p.is_file())
 return {str(p.relative_to(ROOT)):digest(p)for p in sorted(files)}
def qualified_stock_observations(stock):
 stored=stock['stored-restarts-prefix.tif'];planar=stock['planar420-complete.tif'];tiles=stock['table-grid-explicit-reset.tif']
 expected_fields={f'chunk{chunk}.plane{plane}.sample_sha256'for chunk in [2,3]for plane in range(3)}
 return(stored['status']=='native_error'and stored.get('error')=='incomplete independent source chunk 1: -1 != 3072'
  and 'Corrupt JPEG data: premature end of data segment'in stored.get('diagnostics','')and 'LibJpeg: Unexpected error.'in stored.get('diagnostics','')
  and planar['status']=='native_error'and planar.get('error')=='stock incomplete planar source component1: -1'
  and 'jpeg_start_decompress() returned max_h_samp_factor = 1 and max_v_samp_factor = 1, expected 2 and 2'in planar.get('diagnostics','')
  and tiles['status']=='ok'and len(tiles.get('source_word_differences',[]))==6
  and {d['field']for d in tiles['source_word_differences']}==expected_fields
  and 'Application transferred too many scanlines'in tiles.get('diagnostics',''))

def native(probe,path,policy):
 result=subprocess.run([str(probe),str(path),policy],capture_output=True,text=True,timeout=30)
 if result.returncode:raise ValueError(f'native rejected qualified source: {result.stderr.strip()}')
 return json.loads(result.stdout)

def source_reference(jpeg_library,path,case):
 jpeg=path.with_suffix('.jpg');data=jpeg.read_bytes()
 if hashlib.sha256(data).hexdigest()!=case['source_jpeg_sha256']:raise ValueError('original JPEG authority changed')
 with tifffile.TiffFile(path)as file:
  page=file.pages[0];logical=[page.imagelength,page.imagewidth];factors=list(page.tags[530].value);planar=int(page.planarconfig)==2;tiled=page.is_tiled
  width=page.tilewidth if tiled else logical[1];rows=page.tilelength if tiled else min(page.rowsperstrip,logical[0]);across=(logical[1]+width-1)//width
  count=((logical[0]+rows-1)//rows)*across
  shape=[count*rows,width]if tiled else logical
 if shape!=case['source_jpeg_shape']:raise ValueError('source TIFF/JPEG geometry receipt differs')
 planes=source_planes(jpeg_library,data,shape,factors);chunks=[]
 for index in range(1 if planar else count):
  origin=[index//across*rows,index%across*width];image_shape=[min(rows,logical[0]-origin[0]),min(width,logical[1]-origin[1])];out=[]
  for component,((data,pshape),sampling)in enumerate(zip(planes,[[1,1],[factors[1],factors[0]],[factors[1],factors[0]]])):
   start=0 if planar else index*rows//sampling[0];stop=pshape[0]if planar or index+1==count else(index+1)*rows//sampling[0]
   values=np.frombuffer(data,dtype='u1').reshape(pshape)[start:stop].copy();label=['Y','Cb','Cr'][component]
   golden=path.with_name(f'{path.stem}.chunk{index}.{label}')
   if golden.read_bytes()!=values.tobytes():raise ValueError('golden words differ from current independent originalJPEG decode')
   out.append({'component':label,'shape':list(values.shape),'valid_shape':[(image_shape[a]+sampling[a]-1)//sampling[a]for a in range(2)],'sampling':sampling,'sample_sha256':hashlib.sha256(values.tobytes()).hexdigest()})
  chunks.append({'origin':origin,'image_shape':image_shape,'planes':out})
 return {'shape':logical,'subsampling':factors,'chunks':chunks}

def fabio_observation(path):
 try:
  with fabio.open(str(path))as image:
   if image.data is None:return {'status':'no_pixels','classification':'stock reader logs decoder failure but returns an image object without pixels'}
   values=np.asarray(image.data)
   if values.dtype.kind not in 'buif' or values.ndim!=2:raise TypeError('unexpected stock TIFF display API/dtype')
   return {'status':'ok','lib':image.lib,'shape':list(values.shape),'dtype':str(values.dtype),'sample_sha256':hashlib.sha256(values.astype(values.dtype.newbyteorder('<'),copy=False).tobytes()).hexdigest(),'classification':'stock display/luminance output, separate from raw source components'}
 except OSError as error:return {'status':'decoder_error','error':str(error)}

def stock_native(path,library):
 oracle=Reference(library)
 with tifffile.TiffFile(path)as file:
  page=file.pages[0];planar=int(page.planarconfig)==2;factors=list(page.tags[530].value);height,width=page.imagelength,page.imagewidth
 if not planar:return {'status':'ok','reference':oracle.read(path)}
 lib=oracle.library;handle=lib.TIFFOpen(bytes(path),b'r')
 if not handle:raise ValueError('stock libtiff failed opening planar source')
 calls=[];planes=[]
 try:
  length=height*width
  if length>512*1024*1024//3:raise ValueError('stock native planar reference byte budget')
  for index in range(3):
   buffer=C.create_string_buffer(length);actual=lib.TIFFReadEncodedStrip(handle,index,buffer,length)
   calls.append({'component':['Y','Cb','Cr'][index],'requested_bytes':length,'returned_bytes':actual,'output_sha256':hashlib.sha256(buffer.raw).hexdigest()if actual==length else None})
   if actual!=length:raise ValueError(f'stock incomplete planar source component{index}: {actual}')
   planes.append({'component':['Y','Cb','Cr'][index],'shape':[height,width],'valid_shape':[height,width],'sampling':[1,1],'sample_sha256':hashlib.sha256(buffer.raw).hexdigest()})
 finally:lib.TIFFClose(handle)
 if factors!=[1,1]:return {'status':'unqualified_layout','calls':calls,'classification':'stock codec reconstructs separate full-size gray frames; original JPEG declares subsampled chroma grids'}
 return {'status':'ok','reference':{'shape':[height,width],'subsampling':factors,'chunks':[{'origin':[0,0],'image_shape':[height,width],'planes':planes}]},'calls':calls}

def stock_subprocess(path,library):
 result=subprocess.run([sys.executable,str(pathlib.Path(__file__).resolve()),'--stock',str(path),'--reference-library',str(library)],capture_output=True,text=True,timeout=30)
 if result.returncode:raise RuntimeError('unexpected stock-oracle helper failure: '+result.stderr)
 output=json.loads(result.stdout);output['diagnostics']=result.stderr;return output

def run(args):
 manifest=json.loads((args.fixtures/'provenance.json').read_text());files=manifest['files']
 args.reference_library=args.reference_library.resolve(strict=True);libtiff_before=Reference(args.reference_library)
 if libtiff_before.metadata['sha256']!=manifest['libtiff']['sha256']:raise ValueError('stock libtiff differs from pinned independent generator authority')
 if set(files)!=EXPECTED:raise ValueError('all17 named source profiles are mandatory')
 before=fingerprints();helpers_before=helper_hashes();pin_before=pin_fabio(args.fabio_checkout);probe_before=digest(args.probe);jpeg_binary=args.jpeg_library.resolve();jpeg_before=digest(jpeg_binary)
 if jpeg_before!=manifest['jpeg_library']['sha256']:raise ValueError('JPEG source-word authority binary changed')
 j=bind(jpeg_binary)
 fixture_before={str(p.relative_to(args.fixtures)):digest(p)for p in sorted(args.fixtures.iterdir())if p.is_file()};records=[]
 for name,case in files.items():
  path=args.fixtures/name
  if digest(path)!=case['source_sha256']:raise ValueError('source container differs from fixture provenance')
  expected=source_reference(j,path,case);got=native(args.probe,path,case['policy']);differences=compare_components(got,expected)
  if expected!=case['source_word_reference']:differences.append({'field':'original_jpeg_recomputed_reference'})
  if case['policy']=='independent-reset-tiles':
   declared=subprocess.run([str(args.probe),str(path),'declared'],capture_output=True,text=True,timeout=30)
   default_rejected=declared.returncode>0 and 'explicit source SOF' in declared.stderr
   if not default_rejected or got['partition']!='IndependentChunks' or got['tile_policy']!='IndependentResetTiles':differences.append({'field':'explicit_tile_policy_contract'})
  else:
   default_rejected=False
   if got['tile_policy']!='Declared':differences.append({'field':'declared_policy'})
  with tifffile.TiffFile(path)as file:
   page=file.pages[0]
   for fragment in got['fragments']:
    index=fragment['index']
    if fragment['source_offset']!=page.dataoffsets[index]or fragment['source_bytes']!=page.databytecounts[index]:differences.append({'field':f'fragment{index}.original_window'})
    if not(0<=fragment['header_bytes']<=fragment['source_bytes']and fragment['entropy_offset']==fragment['source_offset']+fragment['header_bytes']and 0<=fragment['entropy_bytes']<=fragment['source_bytes']-fragment['header_bytes']):differences.append({'field':f'fragment{index}.byte_contribution'})
  for window in got['source_windows']:
   if window['bytes']<=0 or window['offset']+window['bytes']>path.stat().st_size:differences.append({'field':'source_window_bounds'})
  if case['planar']==2:
   if got['partition']!='PlanarScans'or len(got['scan_sources'])!=3:differences.append({'field':'planar_scan_provenance'})
   for scan in got['scan_sources']:
    if not scan['header_windows']or not scan['entropy_windows']:differences.append({'field':'missing_planar_source_scan_windows'})
    for window in scan['header_windows']+scan['entropy_windows']:
     if window['bytes']<=0 or window['offset']+window['bytes']>path.stat().st_size:differences.append({'field':'planar_scan_window_bounds'})
  stock=stock_subprocess(path,args.reference_library)
  if stock['status']=='ok':stock['source_word_differences']=compare_components(stock['reference'],expected)
  stock['required_exact']=case['authority']=='libtiff_and_original_jpeg'
  if stock['required_exact']and(stock['status']!='ok'or stock['source_word_differences']):differences.append({'field':'required_independent_libtiff_authority'})
  fabio_result=fabio_observation(path);expected_status='no_pixels'if name=='planar420-complete.tif'else'ok'
  if fabio_result['status']!=expected_status:differences.append({'field':'pinned_fabio_observation','expected':expected_status,'actual':fabio_result['status']})
  records.append({'source':str(path.resolve()),'source_sha256':digest(path),'source_jpeg_sha256':case['source_jpeg_sha256'],'policy':case['policy'],'source_authority':case['authority'],'native':got,'source_word_reference':expected,'stock_libtiff':stock,'stock_fabio':fabio_result,'default_rejected':default_rejected,'differences':differences})
 after=fingerprints();helpers_after=helper_hashes();pin_after=pin_fabio(args.fabio_checkout);probe_after=digest(args.probe);jpeg_after=digest(jpeg_binary);fixture_after={str(p.relative_to(args.fixtures)):digest(p)for p in sorted(args.fixtures.iterdir())if p.is_file()}
 source_ok=not any(r['differences']for r in records);stock_by_name={pathlib.Path(r['source']).name:r['stock_libtiff']for r in records}
 qualified_stock_gate=qualified_stock_observations(stock_by_name)
 manual=[r for r in records if r['policy']!='declared'];stock_required=[r for r in records if r['stock_libtiff']['required_exact']]
 result={'schema':1,'passed':source_ok and helpers_before==helpers_after and qualified_stock_gate and libtiff_before.unchanged()and before==after and pin_before==pin_after and probe_before==probe_after and jpeg_before==jpeg_after and fixture_before==fixture_after,
 'contract':'native8-bit decoded original Y/Cb/Cr grids/source words and padding; literal header/entropy windows; explicit independent-reset interpretation separate from default; stock display observations independent',
 'summary':{'files':len(records),'chunks':sum(len(r['native']['chunks'])for r in records),'planes':sum(len(c['planes'])for r in records for c in r['native']['chunks']),'source_word_differences':sum(len(r['differences'])for r in records),'qualified_stock_libtiff_gate':qualified_stock_gate,'required_libtiff_exact':len(stock_required),'required_libtiff_gate':all(r['stock_libtiff']['status']=='ok'and not r['stock_libtiff']['source_word_differences']for r in stock_required),'manual_policy_sources':len(manual),'manual_policy_gate':all(r['default_rejected']and not r['differences']for r in manual),'stock_fabio_completed':sum(r['stock_fabio']['status']=='ok'for r in records),'stock_fabio_no_pixels':sum(r['stock_fabio']['status']=='no_pixels'for r in records),'stock_fabio_expected_gate':all(r['stock_fabio']['status']==('no_pixels'if pathlib.Path(r['source']).name=='planar420-complete.tif'else'ok')for r in records)},
 'native_probe_before':probe_before,'native_probe_after':probe_after,'probe_stable':probe_before==probe_after,'runtime_before':before,'runtime_after':after,'runtime_stable':before==after,'fixtures_before':fixture_before,'fixtures_after':fixture_after,'fixtures_stable':fixture_before==fixture_after,'pinned_fabio_before':pin_before,'pinned_fabio_after':pin_after,'pinned_fabio_stable':pin_before==pin_after,
 'jpeg_reference':{'path':str(jpeg_binary),'sha256_before':jpeg_before,'sha256_after':jpeg_after,'policy':'raw tj3DecompressToYUV8 only after native header precision/dimension/subsampling and byte admission; no RGB/chroma expansion'},'libtiff_reference':libtiff_before.metadata,'libtiff_reference_stable':libtiff_before.unchanged(),
 'helper_sha256_before':helpers_before,'helper_sha256_after':helpers_after,'helpers_stable':helpers_before==helpers_after,
 'remaining':[{'profile':'12-bit/higher precision or lossless/nonsequential OJPEG','condition':'unqualified coding/representation scope','reason':'existing OJPEG native component API is explicitly8-bit DCT; no promotion, downcast or guessed predictor','action':'explicit rejection'}, {'profile':'header beyond first source part with multiple ambiguous entropy contributors','condition':'unqualified source partition','reason':'no universal literal-vs-implied-boundary interpretation asserted','action':'explicit rejection'}, {'profile':'noncomplete subsampled planar or planar multiple strips/restarts','condition':'unqualified component/source mapping','action':'explicit rejection'}, {'profile':'table-only multi-across without caller reset interpretation','condition':'ambiguous historical reset semantics','action':'default rejection; explicit IndependentResetTiles policy qualified separately'}],
 'records':records}
 args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({'passed':result['passed'],**result['summary']}));return result

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--stock',type=pathlib.Path);p.add_argument('--probe',type=pathlib.Path);p.add_argument('--reference-library',type=pathlib.Path,required=True);p.add_argument('--jpeg-library',type=pathlib.Path,default=pathlib.Path('/opt/homebrew/opt/jpeg-turbo/lib/libturbojpeg.dylib'));p.add_argument('--fixtures',type=pathlib.Path,default=ROOT/'tests/data/ojpeg-advanced');p.add_argument('--fabio-checkout',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-2026.6.0'));p.add_argument('--output',type=pathlib.Path,default=ROOT/'docs/ojpeg/2026-11-06-vds-advanced-ojpeg.json');args=p.parse_args()
 if args.stock:
  try:result=stock_native(args.stock,args.reference_library)
  except (OSError,ValueError)as error:result={'status':'native_error','error':str(error)}
  print(json.dumps(result));return 0
 if args.probe is None:p.error('--probe required for matched proof')
 return 0 if run(args)['passed']else 1
if __name__=='__main__':raise SystemExit(main())
