"""Matched fragmented-OJPEG source-component proof; independent test tools only."""
import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import numpy as np
import fabio
import tifffile
from tiff_component_reference import Reference

ROOT=pathlib.Path(__file__).resolve().parents[1]

def digest(path):return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()

def fingerprints():
    files=[]
    for name in ['src/tiff.rs','src/tiff','src/jpeg.rs','Cargo.toml','Cargo.lock','vendor/tiff-0.11.3','native']:
        path=ROOT/name
        files.extend([path]if path.is_file()else[p for p in path.rglob('*')if p.is_file()])
    return {str(p.relative_to(ROOT)):digest(p)for p in sorted(files)}

def compare_components(native,reference):
    differences=[]
    def check(field,expected,actual):
        if expected!=actual:differences.append({'field':field,'expected':expected,'actual':actual})
    for field in ['shape','subsampling']:check(field,reference[field],native.get(field))
    check('chunk_count',len(reference['chunks']),len(native.get('chunks',[])))
    for index,(actual,wanted)in enumerate(zip(native.get('chunks',[]),reference['chunks'])):
        for field in ['origin','image_shape']:check(f'chunk{index}.{field}',wanted[field],actual.get(field))
        check(f'chunk{index}.plane_count',3,len(actual.get('planes',[])))
        for plane,(got,expected)in enumerate(zip(actual.get('planes',[]),wanted['planes'])):
            for field in ['component','shape','valid_shape','sampling','sample_sha256']:
                check(f'chunk{index}.plane{plane}.{field}',expected[field],got.get(field))
    return differences

def native(probe,path):
    result=subprocess.run([str(probe),str(path)],capture_output=True,text=True,timeout=30)
    if result.returncode:raise ValueError(f'native rejected source: {result.stderr.strip()}')
    return json.loads(result.stdout)

def pin_fabio(checkout):
    commit=subprocess.check_output(['git','-C',str(checkout),'rev-parse','HEAD'],text=True,timeout=10).strip()
    expected_commit='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'
    if commit!=expected_commit or fabio.version!='2026.6.0':raise ValueError('FabIO checkout/version differs from pinned authority')
    files=verify_package_files(checkout/'src/fabio',pathlib.Path(fabio.__file__).resolve().parent,
                               ['tifimage.py','TiffIO.py','fabioimage.py','openimage.py','utils/pilutils.py'])
    return {'commit':commit,'version':fabio.version,'files':files}

def verify_package_files(expected_root,actual_root,files):
    result={}
    for name in files:
        expected=digest(expected_root/name);actual=digest(actual_root/name)
        if expected!=actual:raise ValueError(f'installed FabIO differs from pinned checkout: {name}')
        result[name]=actual
    return result

def fabio_observation(path):
    try:
        with fabio.open(str(path)) as image:
            array=np.asarray(image.data)
            return {'status':'ok','lib':image.lib,'nframes':image.nframes,'shape':list(array.shape),'dtype':str(array.dtype),
                    'sample_sha256':hashlib.sha256(array.astype(array.dtype.newbyteorder('<'),copy=False).tobytes()).hexdigest(),
                    'classification':'versioned display/luminance output; not commensurate with raw component grids'}
    except OSError as error:
        return {'status':'error','error':f'{type(error).__name__}: {error}',
                'classification':'unexpected decoder rejection of a known reference-readable source'}

def run(args):
    oracle=Reference(args.reference_library)
    manifest=json.loads((args.fixtures/'provenance.json').read_text())
    pinned_before=pin_fabio(args.fabio_checkout)
    probe_before=digest(args.probe)
    if len(manifest['files'])!=14:raise ValueError('expected exactly14 new independent fragmented fixtures')
    cases=[(args.fixtures/name,'new_independent_fixture')for name in sorted(manifest['files'])]
    old=sorted((ROOT/'tests/data/ojpeg').glob('*.tif'))
    if len(old)!=4:raise ValueError('expected exactly4 existing independent fixtures')
    cases += [(p,'existing_independent_fixture')for p in old]
    cases += [(args.corpus/name,'existing_upstream_source')for name in ['smallliz.tif','zackthecat.tif']]
    if len(cases)!=20 or any(not p.is_file()for p,_ in cases):raise ValueError('all20 requested proof sources must exist')
    previous=json.loads((ROOT/'tests/data/ojpeg/provenance.json').read_text())['files']
    actual_hashes={'smallliz.tif':'e8f722bc322b9a7621d1d858bec36ea78d31b85221f314a2b9872d3af91d600a',
                   'zackthecat.tif':'0d31f2acb25bfbde4d79975572369c3e4bea8e978fc1935075a545e3f8321b6e'}
    before=fingerprints();records=[]
    for path,kind in cases:
        source_before=digest(path)
        if kind=='existing_upstream_source' and source_before!=actual_hashes[path.name]:raise ValueError('upstream source differs from unchanged pinned file')
        if kind=='existing_independent_fixture' and source_before!=previous[path.name]['file_sha256']:raise ValueError('existing fixture changed from provenance')
        got=native(args.probe,path);expected=oracle.read(path);differences=compare_components(got,expected)
        if kind=='new_independent_fixture':
            case=manifest['files'][path.name]
            if source_before!=case['source_sha256']:differences.append({'field':'fixture_source_sha256','expected':case['source_sha256'],'actual':source_before})
            if got['original_jpeg_header_sha256']!=case['original_jpeg_header_sha256']:differences.append({'field':'original_jpeg_header_sha256','expected':case['original_jpeg_header_sha256'],'actual':got['original_jpeg_header_sha256']})
            if len(got['fragments'])!=case['fragments']:differences.append({'field':'fragment_count'})
            with tifffile.TiffFile(path)as file:
                page=file.pages[0]
                for index,fragment in enumerate(got['fragments']):
                    for field,wanted in [('index',index),('source_offset',page.dataoffsets[index]),('source_bytes',page.databytecounts[index])]:
                        if fragment[field]!=wanted:differences.append({'field':f'fragment{index}.{field}','expected':wanted,'actual':fragment[field]})
                    start=fragment['entropy_offset'];end=start+fragment['entropy_bytes']
                    if not(fragment['source_offset']<=start<end<=fragment['source_offset']+fragment['source_bytes']):differences.append({'field':f'fragment{index}.entropy_bounds'})
                    if fragment['origin']!=expected['chunks'][index]['origin']:differences.append({'field':f'fragment{index}.logical_origin'})
                    if fragment['stream_origin']!=[index*got['fragments'][0]['encoded_shape'][0],0]:differences.append({'field':f'fragment{index}.stream_origin'})
            for index,chunk in enumerate(expected['chunks']):
                for plane in chunk['planes']:
                    golden=args.fixtures/f'{path.stem}.chunk{index}.{plane["component"]}'
                    if digest(golden)!=plane['sample_sha256']:differences.append({'field':f'chunk{index}.{plane["component"]}.independent_golden'})
        source_after=digest(path)
        if source_before!=source_after:differences.append({'field':'source_changed'})
        records.append({'path':str(path.resolve()),'kind':kind,'source_sha256':source_before,'source_unchanged':source_before==source_after,
                        'input_condition':'independently qualified original-JPEG legacy profile; TIFF6 restart conventions are ambiguous',
                        'reader_action':'exact_native_component_read','native':got,'independent_reference':expected,
                        'fabio':fabio_observation(path),'differences':differences})
    after=fingerprints()
    pinned_after=pin_fabio(args.fabio_checkout)
    probe_after=digest(args.probe)
    helpers=['scripts/compare_ojpeg_fragments.py','scripts/tiff_component_reference.py','scripts/generate_ojpeg_fragment_fixtures.py']
    result={'schema':1,'gate':before==after and pinned_before==pinned_after and probe_before==probe_after and len(records)==20 and all(r['fabio']['status']=='ok'for r in records) and not any(r['differences']for r in records),
            'contract':'Original8-bit Y/Cb/Cr native grids and decoder padding, stored orientation, exact declared fragment/restart contribution; no implicit RGB/luminance/chroma expansion',
            'native_probe_sha256':probe_before,'native_probe_sha256_after':probe_after,'native_probe_unchanged':probe_before==probe_after,'reference_library':oracle.metadata,
            'reference_unchanged':oracle.unchanged(),
            'helper_sha256':{p:digest(ROOT/p)for p in helpers},'fixture_manifest_sha256':digest(args.fixtures/'provenance.json'),
            'runtime_sources_before':before,'runtime_sources_after':after,'runtime_sources_unchanged':before==after,
            'python':sys.version,'fabio_version':fabio.version,'numpy_version':np.__version__,'tifffile_version':tifffile.__version__,
            'pinned_fabio':pinned_before,'pinned_fabio_after':pinned_after,'pinned_fabio_unchanged':pinned_before==pinned_after,
            'authority':{'libtiff_source':'https://gitlab.com/libtiff/libtiff/-/blob/v4.7.2/libtiff/tif_ojpeg.c','libtiff_source_sha256':'e0dcb8bf12d3b403407155ac84402158c4efb0f19d2c6e10f16329b2bf311bc4','functions':['OJPEGReadHeaderInfo','OJPEGWriteStreamDri','OJPEGWriteStreamRst','OJPEGWriteStreamSof','OJPEGWriteStreamSos'],'ambiguity_primary_source':'https://libtiff.gitlab.io/libtiff/specification/technote2.html'},
            'qualifications':['Native libjpeg warnings fail; limited entropy slack before EOI may be absorbed by mature bit lookahead without warning; no complete semantic entropy proof',
                              'Multi-across fragmented tile grids require explicit source SOF; ambiguous table-only stacked geometry is rejected',
                              'No planar separate, nonbaseline, higher precision, arbitrary intra-fragment restart declarations or split headers extending beyond first fragment'],
            'files':len(records),'new_fragmented_files':len(manifest['files']),
            'chunks':sum(len(r['native']['chunks'])for r in records),'planes':sum(len(c['planes'])for r in records for c in r['native']['chunks']),
            'fabio_read':sum(r['fabio']['status']=='ok'for r in records),'fabio_rejected':sum(r['fabio']['status']=='error'for r in records),'records':records}
    result['gate'] &= result['reference_unchanged']
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:result[k]for k in ['gate','files','new_fragmented_files','chunks','planes','fabio_read','fabio_rejected','runtime_sources_unchanged']}))
    return result

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--probe',type=pathlib.Path,required=True);p.add_argument('--reference-library',type=pathlib.Path,required=True)
    p.add_argument('--fabio-checkout',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-2026.6.0'));p.add_argument('--fixtures',type=pathlib.Path,default=ROOT/'tests/data/ojpeg-fragments');p.add_argument('--corpus',type=pathlib.Path,default=pathlib.Path('/tmp/dioptrin-io-fabio-corpus/libtiffpic.tar.gz__content'))
    p.add_argument('--output',type=pathlib.Path,default=ROOT/'docs/ojpeg/2026-11-06-marv2-vds-ojpeg.json')
    args=p.parse_args();return 0 if run(args)['gate']else 1
if __name__=='__main__':raise SystemExit(main())
