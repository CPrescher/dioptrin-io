"""Read-only audit of the 19 pinned-suite TIFF rejections; test tooling only.
Requires the pinned FabIO/Pillow plus independent tifffile/imagecodecs environment.
"""
import argparse, bz2, collections, contextlib, gzip, hashlib, io, json, pathlib, struct, subprocess, sys, tempfile
import fabio, imagecodecs, numpy as np, tifffile
from PIL import Image, __version__ as pillow_version

def sha(data): return hashlib.sha256(data).hexdigest()
def signature(a):
    a=np.asarray(a); raw=a.astype('u1') if a.dtype.kind=='b' else a.astype(a.dtype.newbyteorder('<'))
    return {'shape':list(a.shape),'dtype':{'uint8':'U8','uint16':'U16','bool':'bool'}.get(str(a.dtype),str(a.dtype)),'hash':sha(raw.tobytes())}
def plain(value):
    if isinstance(value,(tuple,list)):return [plain(v) for v in value]
    if isinstance(value,(int,np.integer)):return int(value)
    if isinstance(value,bytes):return {'hex':value.hex()}
    return str(value)
def entries(data):
    endian='<' if data[:2]==b'II' else '>'; offset=struct.unpack_from(endian+'I',data,4)[0];n=struct.unpack_from(endian+'H',data,offset)[0]
    return {struct.unpack_from(endian+'H',data,p)[0]:data[p:p+12] for p in range(offset+2,offset+2+n*12,12)}
def audit(args):
    baseline=json.loads(args.baseline.read_text());groups=collections.defaultdict(list)
    for event in baseline['reads']:
        if event['status']=='native_rejected' and event.get('adapter','').startswith('tiff'):groups[event['file_sha256']].append(event)
    records=[]
    for original_hash,events in groups.items():
        first=events[0];relative=first['source'].split('dioptrin-io-fabio-corpus/')[-1];path=args.corpus/relative
        recreated=original_hash.startswith('0adac79b')
        if recreated:path=next(args.capture.glob('*-tifimagewrite_test0000.tif'))
        encoded=path.read_bytes();actual_hash=sha(encoded)
        if not recreated:assert actual_hash==original_hash,(path,actual_hash)
        decoded=(gzip.decompress(encoded) if path.suffix=='.gz' else bz2.decompress(encoded) if path.suffix=='.bz2' else encoded)
        record={'source':first['source'],'baseline_sha256':original_hash,'baseline_event_count':len(events),'baseline_tests':[v['test'] for v in events],'baseline_error':first['error']}
        evidence={'audited_file':str(path),'audited_sha256':actual_hash,'source_bytes':len(encoded),'decoded_tiff_sha256':sha(decoded),'decoded_tiff_bytes':len(decoded),'baseline_bytes_available':not recreated}
        record['evidence']=evidence
        with tifffile.TiffFile(io.BytesIO(decoded)) as file:
            page=file.pages[0]
            selected=[256,257,258,259,262,266,273,277,278,279,284,292,322,323,324,325,513,514,519,520,521,530,531]
            evidence['first_ifd_source_tags']={str(k):plain(page.tags[k].value) for k in selected if k in page.tags}
            evidence['ifd_count']=len(file.pages)
            with tempfile.TemporaryDirectory(prefix='dioptrin-tiff-audit-') as directory:
                source=pathlib.Path(directory)/'source.tif';source.write_bytes(decoded)
                probe=subprocess.run([str(args.probe.resolve()),'pillow13',str(source)],capture_output=True,text=True)
                evidence['native_read']={'accepted':probe.returncode==0}
                if probe.returncode:evidence['native_read']['error']=probe.stderr.strip()
                else:
                    native=json.loads(probe.stdout)['frames'][0];evidence['native_read']['signature']=native
                    with contextlib.redirect_stdout(io.StringIO()):reference=fabio.open(source)
                    ref=signature(reference.data);evidence['fabio_reference']={'backend':reference.lib,'signature':ref,'explicit_view_matches':all(native['view'][k]==ref[k] for k in ['shape','dtype','hash'])}
                    assert evidence['fabio_reference']['explicit_view_matches']
                    if page.compression==3:
                        strips=[]
                        for offset,length in zip(page.dataoffsets,page.databytecounts):
                            chunk=decoded[offset:offset+length]
                            if int(page.fillorder)==2:chunk=bytes(int(f'{b:08b}'[::-1],2) for b in chunk)
                            strips.append(imagecodecs.ccittfax3_decode(chunk,height=min(page.rowsperstrip,page.imagelength),width=page.imagewidth,t4options=int(page.tags[292].value)))
                        raw=np.concatenate(strips,axis=0).astype('u1')
                        evidence['independent_source_policy']='imagecodecs.ccittfax3_decode on exact source strip, explicit FillOrder reversal; no photometric display inversion'
                    else:raw=page.asarray()
                    independent=signature(raw);evidence['independent_source_signature']=independent;evidence['all_source_samples_match']=all(native['raw'][k]==independent[k] for k in ['shape','dtype','hash']);assert evidence['all_source_samples_match']
        name=path.name
        if recreated:
            record.update(input_condition='malformed_descriptive_metadata',reader_action='implemented',classification='closed_opaque_metadata_admission_gap')
            rawentries=entries(decoded);evidence['original_inline_tag270']={'ifd_entry_hex':rawentries[270].hex(),'type':2,'count':4,'literal_hex':rawentries[270][8:12].hex(),'condition':'four spaces, no terminating NUL; stored as opaque original bytes, not repaired text'}
            evidence['identity_qualification']='Baseline ephemeral bytes were unavailable. A captured rerun of the same pinned test has a different hash (datetime); baseline identity is never substituted.'
            evidence['pixel_window']={'offset':196,'declared_bytes':1152,'required_bytes':24*24*2,'source_end':1348}
            evidence['source_authority']=['TIFF6 ASCII field-type definition (count includes NUL)','pinned FabIO TestNormalTiffOK.test_read_openimage recreation','independent exact source samples and explicit pinned display view']
        elif name.startswith('Feb09-'):
            record.update(input_condition='malformed_structural_metadata',reader_action='retained_strict_rejection',classification='declared_uncompressed_window_too_short')
            evidence['required_pixel_bytes']=1042*1042*2;evidence['declared_pixel_bytes']=1085764;evidence['bytecount_is_sample_count']=True;evidence['policy']='Never read past declared bytecounts, even when complete bytes exist later in the source. Wrapper does not repair inner TIFF metadata.'
        elif name=='a0009.tif':
            record.update(input_condition='malformed_structural_metadata',reader_action='retained_strict_rejection',classification='missing_required_strip_bytecounts')
            evidence['required_tag_missing']=279;evidence['rowsperstrip_missing_default_allowed']=True;evidence['raw_payload_extent']={'offset':174,'required':1024*1024*2,'file_end':len(decoded)};evidence['policy']='Complete apparent trailing pixels do not define a missing required sample window. No StripByteCounts inference.'
        elif name in ['g3test.tif','fax2d.tif']:
            record.update(input_condition='well_formed_variant',reader_action='implemented',classification='closed_native_codec_and_fillorder_gap')
            evidence['layout']='Compression3, one-dimensional T4Options0/4, single bilevel plane, FillOrder2. fax2d name does not mean T4Options two-dimensional coding.'
            evidence['policy']='Bounded original strips, strict EOL at every row, exact terminating width, no missing-row padding; raw01 U8 and separate pinned bool display view.'
        elif name in ['zackthecat.tif','smallliz.tif']:
            record.update(input_condition='legacy_variant',reader_action='remaining_codec_gap',classification='old_jpeg6_tables_and_layout')
            evidence['primary_authority']='TIFF6 obsolete JPEG Compression6; LibTIFF Technical Note2 replaces it with Compression7. Source tables/offsets require OJPEG-aware native backend, not the current modern-JPEG TIFF path.'
            evidence['decoded_count_qualification']='Reference-suite acceptance establishes an available decoder, not independent raw-source completeness or exact lossy IDCT parity.'
        elif name=='ycbcr-cat.tif':
            record.update(input_condition='well_formed_variant',reader_action='remaining_layout_gap',classification='lzw_subsampled_ycbcr_units')
            evidence['primary_authority']='TIFF6 YCbCr sampling-unit layout with 2x2 chroma subsampling; compressed strips encode sampling units rather than three full-resolution component rows.'
            evidence['policy']='Do not discard photometric/subsampling tags to force RGB-shaped raw counts; explicit component and display contracts required.'
        elif name=='quad-lzw.tif':
            record.update(input_condition='legacy_dialect',reader_action='remaining_codec_gap',classification='old_lsb_late_change_tiff_lzw')
            with tifffile.TiffFile(io.BytesIO(decoded)) as f:offset=f.pages[0].dataoffsets[0]
            evidence['first_strip_prefix_hex']=decoded[offset:offset+8].hex();evidence['primary_authority']='libtiff tif_lzw.c LZWPreDecode recognizes first byte0 and secondbyte bit0=1 and selects LZWDecodeCompat: old LSB packing and late code-width switching. Current backend uses modern MSB/early-change TIFF LZW.'
            evidence['policy']='Legacy dialect, not automatically corrupt because modern LZW rejects it; no normative TIFF6 conformity claim.'
        elif name=='quad-tile.tif':
            record.update(input_condition='legacy_mixed_tag_dialect',reader_action='retained_strict_rejection',classification='tile_dimensions_with_strip_offset_tables')
            evidence['count_consistency']={'declared_strip_entries':12,'strips_for_rowsperstrip':3,'tiles_for_declared_tile_dimensions':12,'tile_offsets_tag_present':False,'tile_bytecounts_tag_present':False}
            evidence['policy']='Do not silently reinterpret StripOffsets/StripByteCounts as TileOffsets/TileByteCounts. Reference compatibility may guess tiles; future admission requires explicit legacy policy.'
        elif name=='text.tif':
            record.update(input_condition='unknown_compressed_integrity',reader_action='remaining_codec_gap',classification='thunderscan_with_incomplete_reference_decode')
            check=subprocess.run([sys.executable,'-c','from PIL import Image; import sys; im=Image.open(sys.argv[1]); im.load(); print(im.size)',str(path)],capture_output=True,text=True)
            evidence['independent_pillow_decode']={'exit_code':check.returncode,'stdout':check.stdout.strip(),'stderr':check.stderr.strip()}
            evidence['primary_authority']='libtiff tif_thunder.c ThunderDecode compares produced row pixels against expected pixels and reports not-enough-data when unequal.'
            evidence['reference_fallback']={'source':'src/fabio/utils/pilutils.py:get_numpy_array','pinned_commit':'9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece','behavior':'numpy.asarray(image) exceptions are caught and followed by image.tobytes() on the same partially loaded image; a returned array therefore does not establish complete source decode'}
            evidence['qualification']='Compression32809 is a codec gap. Pillow/libtiff fails with insufficient row data at scanline320; full unpadded source counts have not been established. Do not label well formed or use repaired display pixels as raw proof.'
        else:raise RuntimeError(name)
        assert sha(path.read_bytes())==actual_hash
        records.append(record)
    pilutils=pathlib.Path(fabio.__file__).parent/'utils/pilutils.py'
    for record in records:
        fallback=record['evidence'].get('reference_fallback')
        if fallback:fallback['installed_source_sha256']=sha(pilutils.read_bytes())
    registry=pathlib.Path.home()/'.cargo/registry/src'
    primary=[]
    for package,relative,reason in [('fax-0.2.7','src/decoder.rs','Group3Decoder permissive EOL; strict framing is supplied by the bounded adapter'),('fax-0.2.7','src/maps.rs','public MIT Huffman tables and codewords reused without duplication'),('libtiff-sys-0.2.0','libtiff/libtiff/tif_lzw.c','LZWPreDecode and LZWDecodeCompat identify legacy LSB/late-change dialect'),('libtiff-sys-0.2.0','libtiff/libtiff/tif_thunder.c','ThunderDecode reports incomplete produced row counts'),('libtiff-sys-0.2.0','libtiff/libtiff/tif_ojpeg.c','native OJPEG implementation handles legacy tables/layout')]:
        candidates=list(registry.glob('*/'+package+'/'+relative))
        if candidates:primary.append({'package':package,'path':relative,'sha256':sha(candidates[0].read_bytes()),'evidence':reason})
    assert len(records)==13 and sum(r['baseline_event_count'] for r in records)==19
    return {'schema':1,'baseline':str(args.baseline),'baseline_sha256':sha(args.baseline.read_bytes()),'audited_events':19,'audited_unique_sources':13,'primary_code_evidence':primary,'reference':{'FabIO':fabio.version,'Pillow':pillow_version,'tifffile':tifffile.__version__,'imagecodecs':imagecodecs.__version__},'summary':{'implemented_events':3,'remaining_events':16,'strict_short_window_events':9},'authority':[{'title':'TIFF6 (1992) source specification','url':'https://image-js.github.io/tiff/media/TIFF6.pdf','sections':['ASCII field type','StripByteCounts','TileOffsets/TileByteCounts','YCbCr subsampling']},{'title':'RFC2306 TIFF-F','url':'https://www.rfc-editor.org/rfc/rfc2306.html','sections':['3.8.3','3.8.4','3.8.5']},{'title':'LibTIFF Technical Note2','url':'https://libtiff.gitlab.io/libtiff/specification/technote2.html'}],'records':records}
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('corpus',type=pathlib.Path);parser.add_argument('probe',type=pathlib.Path);parser.add_argument('capture',type=pathlib.Path);parser.add_argument('--baseline',type=pathlib.Path,default=pathlib.Path('docs/fabio-parity/2026-11-05-mar-sparse.json'));args=parser.parse_args()
    with contextlib.redirect_stdout(io.StringIO()):result=audit(args)
    print(json.dumps(result,indent=2))
