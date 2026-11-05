"""Read-only exact-count proof for pinned quad-lzw.tif; test tooling only."""
import argparse, collections, contextlib, hashlib, io, json, pathlib, subprocess
import fabio, imagecodecs, numpy as np, tifffile
from PIL import Image, __version__ as pillow_version

def sha(data):return hashlib.sha256(data).hexdigest()
def signature(array):
    array=np.asarray(array)
    return {'shape':list(array.shape),'dtype':{'uint8':'U8','float32':'F32'}[str(array.dtype)],'hash':sha(array.astype(array.dtype.newbyteorder('<')).tobytes())}
def code_boundaries(encoded):
    position=0;width=9;next_code=258;previous=False;statistics=collections.Counter()
    while position+width<=len(encoded)*8:
        code=sum(((encoded[(position+i)//8]>>((position+i)%8))&1)<<i for i in range(width));position+=width;statistics['width'+str(width)]+=1
        if code==256:width=9;next_code=258;previous=False;statistics['clears']+=1;continue
        if code==257:
            assert len(encoded)*8-position<8
            assert all(not(encoded[i//8]>>(i%8)&1) for i in range(position,len(encoded)*8))
            return {'code_statistics':dict(statistics),'end_code_bit':position,'zero_pad_bits':len(encoded)*8-position}
        assert code<=next_code and (previous or code<258)
        if code==next_code:statistics['kwkwk']+=1
        if previous and next_code<4096:
            next_code+=1
            if next_code==1<<width and width<12:width+=1
        previous=True
    raise ValueError('missing EOI')
def run(args):
    source=args.corpus/'libtiffpic.tar.gz__content/quad-lzw.tif';encoded=source.read_bytes();source_hash=sha(encoded)
    assert source_hash=='081118d951224d9909b105bb75f9667fd817e7b95122d141e91b43604dacd78b'
    baseline=json.loads(args.baseline.read_text());events=[event for event in baseline['reads'] if event.get('file_sha256')==source_hash]
    native_run=subprocess.run([str(args.probe.resolve()),'pillow13',str(source)],check=True,capture_output=True,text=True)
    native=json.loads(native_run.stdout)['frames'][0];strips=[]
    with tifffile.TiffFile(source) as file:
        page=file.pages[0];independent=page.asarray();source_tags={str(k):list(v.value) if isinstance(v.value,tuple) else int(v.value) for k,v in page.tags.items() if k in [256,257,258,259,262,266,273,274,277,278,279,284,317,339]}
        independently_decoded=[]
        for index,(offset,count) in enumerate(zip(page.dataoffsets,page.databytecounts)):
            chunk=encoded[offset:offset+count];assert chunk[0]==0 and chunk[1]&1
            rows=min(page.rowsperstrip,page.imagelength-index*page.rowsperstrip);raw=imagecodecs.lzw_decode(chunk);assert len(raw)==rows*page.imagewidth*3
            independently_decoded.append(np.frombuffer(raw,dtype='u1').reshape(rows,page.imagewidth,3))
            strips.append({'offset':offset,'declared_bytes':count,'decoded_source_bytes':len(raw),'prefix_hex':chunk[:8].hex(),'decoded_sha256':sha(raw),**code_boundaries(chunk)})
        assert np.array_equal(np.concatenate(independently_decoded),independent)
    pillow=np.asarray(Image.open(source));assert np.array_equal(pillow,independent)
    with contextlib.redirect_stdout(io.StringIO()):reference=fabio.open(source)
    expected_view=signature(reference.data);expected_source=signature(independent)
    assert all(native['raw'][key]==expected_source[key] for key in ['shape','dtype','hash'])
    assert all(native['view'][key]==expected_view[key] for key in ['shape','dtype','hash'])
    assert sha(source.read_bytes())==source_hash
    vendor=pathlib.Path('vendor/tiff-0.11.3');manifest=json.loads((vendor/'UPSTREAM.json').read_text())
    fixture_manifest=json.loads(pathlib.Path('tests/data/tiff-legacy-lzw/provenance.json').read_text())
    registry=pathlib.Path.home()/'.cargo/registry/src';primary=next(registry.glob('*/libtiff-sys-0.2.0/libtiff/libtiff/tif_lzw.c'))
    return {'schema':1,'source':str(source),'source_sha256':source_hash,'source_bytes':len(encoded),'source_unchanged':True,'baseline':str(args.baseline),'baseline_sha256':sha(args.baseline.read_bytes()),'baseline_events':events,'input_condition':'legacy_dialect','reader_action':'implemented','recognition':'Original first nine-bit clear code256 packed LSB: byte0=0 and byte1 lowbit=1, inspected independently for every chunk before decoding; never modern-error retry','source_tags':source_tags,'reference':{'FabIO':fabio.version,'Pillow':pillow_version,'tifffile':tifffile.__version__,'imagecodecs':imagecodecs.__version__,'fabio_backend':reference.lib},'native':native,'independent_source':expected_source,'pillow_source_rgb':signature(pillow),'pinned_explicit_view':expected_view,'all_native_source_samples_match':True,'all_explicit_view_samples_match':True,'sample_count':int(independent.size),'native_dtype_unchanged':True,'source_orientation_applied':False,'chunks':strips,'dependency':{'TIFF':'0.11.3','weezl':'0.1.12','MIT_upstream_commit':manifest['vcs']['git']['sha1'],'registry_archive_sha256':manifest['registry_archive_sha256'],'baseline_manifest':str(vendor/'UPSTREAM.json'),'patch':str(vendor/'dioptrin-legacy-lzw.patch'),'patch_sha256':sha((vendor/'dioptrin-legacy-lzw.patch').read_bytes()),'new_runtime_dependencies':False},'primary_dialect_evidence':{'package':'libtiff-sys0.2.0','path':'libtiff/libtiff/tif_lzw.c','sha256':sha(primary.read_bytes()),'functions':['LZWPreDecode','LZWDecodeCompat'],'qualification':'Compatibility LSB/late-change dialect, no TIFF6 normative conformity assertion'},'independent_fixtures':fixture_manifest,'integrity_tests':['independent_counts_dictionary_widths_clears_kwkwk_and_predictors','legacy_end_output_and_tail_integrity_are_strict','declared_windows_and_all_tile_padding_are_validated','legacy_scratch_limits_source_identity_and_failed_buffers_are_explicit'],'scope':'Strict EOI/exact decoded count/zero bit padding validation applies to explicitly recognized legacy streams; modern TIFF LZW retains its upstream streaming behavior. Original source windows and all sample/predictor/layout/type/orientation metadata remain unchanged.'}
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('corpus',type=pathlib.Path);parser.add_argument('probe',type=pathlib.Path);parser.add_argument('--baseline',type=pathlib.Path,default=pathlib.Path('docs/fabio-parity/2026-11-06-rejection-continuation.json'));args=parser.parse_args()
    print(json.dumps(run(args),indent=2))
