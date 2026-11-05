"""Compare explicit TIFF consumer views with pinned FabIO and raw samples with
independent tifffile. Test-only Python; no runtime bridge or global harness edits.
"""
import argparse,bz2,gzip,hashlib,json,pathlib,subprocess,tempfile,contextlib,io
import numpy as np
import fabio,tifffile,imagecodecs
from PIL import __version__ as pillow_version
DTYPES={'uint8':'U8','uint16':'U16','uint32':'U32','uint64':'U64','int8':'I8','int16':'I16','int32':'I32','int64':'I64','float32':'F32','float64':'F64','bool':'bool'}
def signature(array):
    # NumPy mode1 bool arrays may store true as FF rather than 01. Compare their
    # logical bool values explicitly; source packed/native signatures stay raw.
    raw=array.astype('u1') if array.dtype.kind=='b' else array.astype(array.dtype.newbyteorder('<'))
    return {'shape':list(array.shape),'dtype':DTYPES[str(array.dtype)],'hash':hashlib.sha256(raw.tobytes()).hexdigest()}
def compare(args):
    events=json.loads(args.selection.read_text())['reads'];sources={}
    for event in events:
        if event.get('adapter')=='tiff':sources.setdefault(event['file_sha256'],event)
    report=[]
    for source_hash,event in sources.items():
        original=pathlib.Path(event['source']);relative=original.parts[original.parts.index('dioptrin-io-fabio-corpus')+1:] if 'dioptrin-io-fabio-corpus' in original.parts else (original.name,);path=args.corpus.joinpath(*relative)
        item={'name':'/'.join(relative),'source_sha256':source_hash,'previous_status':event['status']}
        if not path.exists():item['status']='source_unavailable';report.append(item);continue
        if hashlib.sha256(path.read_bytes()).hexdigest()!=source_hash:raise RuntimeError(f'changed source: {path}')
        reference=fabio.open(path);item['reference_backend']=reference.lib
        policy={'PIL':'pillow13','TiffIO':'tiffio'}[reference.lib]
        with tempfile.TemporaryDirectory(prefix='dioptrin-tiff-view-') as directory:
            decoded=path
            if path.suffix in ['.gz','.bz2']:
                decoded=pathlib.Path(directory)/'source.tif';decoded.write_bytes((gzip.decompress if path.suffix=='.gz' else bz2.decompress)(path.read_bytes()))
            result=subprocess.run([str(args.probe),policy,str(decoded)],capture_output=True,text=True)
            if result.returncode:item.update(status='native_rejected',error=result.stderr.strip());report.append(item);continue
            native=json.loads(result.stdout)['frames'];item['frames']=len(native);comparisons=[]
            with tifffile.TiffFile(decoded) as file:
                for index,frame in enumerate(native):
                    image=reference.data if reference.nframes==1 else reference.getframe(index).data
                    expected=signature(image);entry={'frame':index,'raw':frame['raw'],'reference':expected}
                    try:
                        page=file.pages[index]
                        if page.photometric==6 and page.compression==7:
                            # tifffile's ordinary JPEG read returns RGB. Request
                            # unchanged JPEG YCbCr components from imagecodecs.
                            encoded=decoded.read_bytes();raw=np.zeros((page.imagelength,page.imagewidth,3),dtype='u1')
                            width=page.tilewidth if page.is_tiled else page.imagewidth
                            height=page.tilelength if page.is_tiled else page.rowsperstrip
                            across=(page.imagewidth+width-1)//width
                            for chunk_index,(offset,length) in enumerate(zip(page.dataoffsets,page.databytecounts)):
                                chunk=encoded[offset:offset+length]
                                if page.jpegtables:chunk=page.jpegtables[:-2]+chunk[2:]
                                block=imagecodecs.jpeg_decode(chunk,outcolorspace='YCbCr')
                                y=(chunk_index//across)*height;x=(chunk_index%across)*width
                                h=min(block.shape[0],page.imagelength-y);w=min(block.shape[1],page.imagewidth-x)
                                raw[y:y+h,x:x+w]=block[:h,:w]
                            entry['source_oracle_policy']='native JPEG YCbCr components, no RGB display conversion'
                        else:raw=page.asarray()
                        if page.planarconfig==2 and raw.ndim==3:raw=raw.transpose(1,2,0)
                        if raw.dtype.kind=='b':raw=raw.astype('u1')
                        if raw.dtype==np.dtype('f2'):raw=raw.astype('f4')
                        entry['independent_source']=signature(raw)
                        entry['source_counts_match']=all(frame['raw'][key]==entry['independent_source'][key] for key in ['shape','dtype','hash'])
                    except Exception as error:entry['source_oracle_error']=str(error)
                    if 'view' in frame:
                        entry['view']=frame['view'];entry['view_matches']=all(frame['view'][key]==expected[key] for key in ['shape','dtype','hash'])
                    else:entry['view_error']=frame['view_error']
                    comparisons.append(entry)
            item['comparison']=comparisons
            if any('reference layout defect' in v.get('view_error','') for v in comparisons):item['status']='reference_layout_defect'
            elif all(v.get('view_matches') for v in comparisons):item['status']='explicit_view_matches'
            else:item['status']='view_difference'
            report.append(item)
    return {'reference':{'fabio':fabio.version,'Pillow':pillow_version,'tifffile':tifffile.__version__,'imagecodecs':imagecodecs.__version__},'contract':'raw source and explicit transformed views are separate; bool view hashes canonicalize logical0/1 only','results':report}
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('corpus',type=pathlib.Path);parser.add_argument('probe',type=pathlib.Path);parser.add_argument('--selection',type=pathlib.Path,default=pathlib.Path(__file__).resolve().parents[1]/'docs/fabio-parity/2026-11-05-parallel-readers.json');args=parser.parse_args()
    with contextlib.redirect_stdout(io.StringIO()):
        report=compare(args)
    print(json.dumps(report,indent=2))
