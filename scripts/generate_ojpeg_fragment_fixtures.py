"""Independent CC0 fragmented OJPEG fixtures, system TurboJPEG + native libtiff."""
import argparse,ctypes as C,hashlib,json,pathlib,struct
import numpy as np
ROOT=pathlib.Path(__file__).resolve().parents[1]/'tests/data/ojpeg-fragments'
def sha(data):return hashlib.sha256(data).hexdigest()
def encode(j,width,height,v,rows):
    yw=(width+1)//2*2;yh=(height+v-1)//v*v;cw=(width+1)//2;ch=(height+v-1)//v
    source=bytes(8+(r*11+c*5)%240 for r in range(yh) for c in range(yw))+bytes(30+(r*7+c*3)%200 for r in range(ch) for c in range(cw))+bytes(220-(r*4+c*8)%180 for r in range(ch) for c in range(cw))
    handle=j.tj3Init(0);assert handle
    try:
        for parameter,value in [(3,91),(4,2 if v==2 else 1),(19,rows//(v*8))]:assert j.tj3Set(handle,parameter,value)==0
        data=C.create_string_buffer(source);output=C.c_void_p();length=C.c_size_t()
        assert j.tj3CompressFromYUV8(handle,data,width,1,height,C.byref(output),C.byref(length))==0
        jpeg=C.string_at(output,length.value);j.tj3Free(output)
    finally:j.tj3Destroy(handle)
    pos=2;qt={};dc={};ac={};frame=scan=None
    while True:
        marker=jpeg[pos+1];n=int.from_bytes(jpeg[pos+2:pos+4],'big');payload=jpeg[pos+4:pos+2+n]
        if marker==0xdb:
            at=0
            while at<len(payload):qt[payload[at]&15]=payload[at+1:at+65];at+=65
        elif marker==0xc4:
            at=0
            while at<len(payload):
                ident=payload[at];amount=sum(payload[at+1:at+17]);(ac if ident&16 else dc)[ident&15]=payload[at+1:at+17+amount];at+=17+amount
        elif marker==0xc0:frame=payload
        elif marker==0xda:scan=payload
        pos+=n+2
        if marker==0xda:break
    head=jpeg[:pos];entropy=jpeg[pos:-2];pieces=[];starts=[];begin=cursor=0
    while cursor<len(entropy):
        if entropy[cursor]==255:
            next=entropy[cursor+1]
            if 0xd0<=next<=0xd7:
                assert next==0xd0+len(pieces)%8;starts.append(pos+begin);pieces.append(entropy[begin:cursor]);begin=cursor+2
            else:assert next==0
            cursor+=1
        cursor+=1
    starts.append(pos+begin);pieces.append(entropy[begin:])
    return jpeg,head,pieces,starts,[(qt,[frame[8+i*3]for i in range(3)]),(dc,[scan[2+i*2]>>4 for i in range(3)]),(ac,[scan[2+i*2]&15 for i in range(3)])]
def build(output,j,name,mode,width,height,rows,v,logical,tiled=False,reverse=False):
    jpeg,head,pieces,starts,tables=encode(j,width,height,v,rows)
    assert len(pieces)==(logical[0]+rows-1)//rows*((logical[1]+width-1)//width if tiled else 1)
    if mode=='legacy-prefix':head=head[:-2]+b'\0'+head[-1:]
    if name=='prefix-inferred-restart':
        stripped=bytearray(head[:2]);at=2
        while at<len(head):
            n=int.from_bytes(head[at+2:at+4],'big')+2
            if head[at+1]!=0xdd:stripped.extend(head[at:at+n])
            at+=n
        head=bytes(stripped)
    tags={256:(4,[logical[1]]),257:(4,[logical[0]]),258:(3,[8,8,8]),259:(3,[6]),262:(3,[6]),274:(3,[6]),277:(3,[3]),284:(3,[1]),512:(3,[1]),515:(3,[((width+15)//16)*(rows//(v*8))]),530:(3,[2,v])}
    if name=='prefix-inferred-restart':tags[515]=(3,[0])
    ot,lt=(324,325)if tiled else(273,279)
    tags.update({322:(4,[width]),323:(4,[rows])}if tiled else{278:(4,[rows])})
    tags[ot]=(4,[0]*len(pieces));tags[lt]=(4,[len(p)for p in pieces])
    if mode=='table':
        for tag in [519,520,521]:tags[tag]=(4,[0,0,0])
    elif mode in ['prefix','legacy-prefix','split','complete']:tags[513]=(4,[0]);tags[514]=(4,[0])
    metadata_end=8+2+12*len(tags)+4;extra=bytearray();external={}
    for tag,(ty,values)in sorted(tags.items()):
        size=len(values)*(2 if ty==3 else 4)
        if size>4:external[tag]=metadata_end+len(extra);extra.extend(bytes(size))
    start=metadata_end+len(extra);body=bytearray()
    if mode=='table':
        for tag,(values,selectors)in zip([519,520,521],tables):
            locations={}
            for ident in dict.fromkeys(selectors):locations[ident]=start+len(body);body.extend(values[ident])
            tags[tag]=(4,[locations[ident]for ident in selectors])
    if mode=='complete':
        tags[513]=(4,[start]);tags[514]=(4,[len(jpeg)]);body.extend(jpeg)
        tags[ot]=(4,[start+n for n in starts])
    else:
        payloads=list(pieces)
        if name=='table-source-eoi':payloads[-1]+=b'\xff\xd9'
        if mode in ['prefix','legacy-prefix']:prefix=head
        elif mode=='split':prefix=head[:37];payloads[0]=head[37:]+payloads[0]
        elif mode=='first':prefix=None;payloads[0]=head+payloads[0]
        else:prefix=None
        if prefix is not None:tags[513]=(4,[start+len(body)]);tags[514]=(4,[len(prefix)]);body.extend(prefix);body.extend(b'opaque-gap-between-windows')
        offsets=[0]*len(payloads)
        for index in reversed(range(len(payloads)))if reverse else range(len(payloads)):
            offsets[index]=start+len(body);body.extend(payloads[index]);body.extend(b'gap\x91\xff\xd9')
        tags[ot]=(4,offsets);tags[lt]=(4,[len(p)for p in payloads])
    out=bytearray(b'II*\0\x08\0\0\0'+struct.pack('<H',len(tags)))
    for tag,(ty,values)in sorted(tags.items()):
        packed=struct.pack('<'+('H'if ty==3 else'I')*len(values),*values);out.extend(struct.pack('<HHI',tag,ty,len(values)))
        if tag in external:
            out.extend(struct.pack('<I',external[tag]));at=external[tag]-metadata_end;extra[at:at+len(packed)]=packed
        else:out.extend(packed.ljust(4,b'\0'))
    out.extend(bytes(4));out.extend(extra);out.extend(body);path=output/(name+'.tif');path.write_bytes(out)
    return {'original_jpeg_header_sha256':sha(head if mode!='table'else b''),'source_sha256':sha(out),'source_bytes':len(out),'shape':logical,'encoded_shape':[height,width],'subsampling':[2,v],'mode':mode,'fragments':len(pieces),'restart_interval':((width+15)//16)*(rows//(v*8)),'physical_order_reversed':reverse}
def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=pathlib.Path,default=ROOT);p.add_argument('--tiff-library',default='/opt/homebrew/opt/libtiff/lib/libtiff.6.dylib');p.add_argument('--jpeg-library',default='/opt/homebrew/opt/jpeg-turbo/lib/libturbojpeg.dylib');args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    j=C.CDLL(args.jpeg_library);j.tj3Init.argtypes=[C.c_int];j.tj3Init.restype=C.c_void_p;j.tj3Set.argtypes=[C.c_void_p,C.c_int,C.c_int];j.tj3Destroy.argtypes=[C.c_void_p];j.tj3Free.argtypes=[C.c_void_p]
    j.tj3CompressFromYUV8.argtypes=[C.c_void_p,C.c_void_p,C.c_int,C.c_int,C.c_int,C.POINTER(C.c_void_p),C.POINTER(C.c_size_t)]
    manifest={}
    for name,mode,width,height,rows,v,logical,tiled,reverse in [
        ('table-strips','table',32,48,16,2,[48,32],False,True),
        ('table-edge-strips','table',32,43,16,2,[43,32],False,False),
        ('table-vertical-tiles','table',32,48,16,2,[43,29],True,True),
        ('table-422-strips','table',32,23,8,1,[23,32],False,False),
        ('prefix-strips','prefix',32,48,16,2,[48,32],False,False),
        ('split-header-strips','split',32,48,16,2,[48,32],False,True),
        ('first-header-strips','first',32,48,16,2,[48,32],False,False),
        ('complete-interchange-strips','complete',32,48,16,2,[48,32],False,False),
        ('table-source-eoi','table',32,48,16,2,[48,32],False,False),
        ('legacy-prefix-strips','legacy-prefix',32,48,16,2,[48,32],False,False),
        ('table-restart-cycle','table',16,160,16,2,[160,16],False,True),
        ('prefix-grid-tiles','prefix',32,64,16,2,[29,51],True,True),
        ('table-odd-width-strips','table',31,43,16,2,[43,31],False,False),
        ('prefix-inferred-restart','prefix',32,48,16,2,[48,32],False,False)]:
        manifest[name+'.tif']=build(args.output,j,name,mode,width,height,rows,v,logical,tiled,reverse)
    import tiff_component_reference
    oracle=tiff_component_reference.Reference(args.tiff_library)
    for name,case in manifest.items():
        path=args.output/name;reference=oracle.read(path);case['reference']=reference
        # Independent native libtiff returns complete original sampling units.
        import tifffile
        with tifffile.TiffFile(path) as file:
            page=file.pages[0];h,v=page.tags[530].value;tile=page.is_tiled
            width=page.tilewidth if tile else page.imagewidth;rows=page.tilelength if tile else page.rowsperstrip
        lib=oracle.library;handle=lib.TIFFOpen(bytes(path),b'r');assert handle
        try:
            for index,chunk in enumerate(reference['chunks']):
                shape=chunk['planes'][0]['shape'];unit_rows=shape[0]//v;columns=(width+h-1)//h
                size=unit_rows*columns*(h*v+2);buffer=C.create_string_buffer(size)
                fn=lib.TIFFReadEncodedTile if tile else lib.TIFFReadEncodedStrip
                assert fn(handle,index,buffer,size)==size
                units=np.frombuffer(buffer.raw,dtype='u1').reshape(unit_rows,columns,h*v+2)
                y=units[:,:,:h*v].reshape(unit_rows,columns,v,h).transpose(0,2,1,3).reshape(unit_rows*v,columns*h)
                for label,array,expected in zip(['Y','Cb','Cr'],[y,units[:,:,h*v],units[:,:,h*v+1]],chunk['planes']):
                    raw=np.ascontiguousarray(array).tobytes();assert sha(raw)==expected['sample_sha256']
                    (args.output/f'{path.stem}.chunk{index}.{label}').write_bytes(raw)
        finally:lib.TIFFClose(handle)
    binary=pathlib.Path(args.jpeg_library).resolve()
    (args.output/'provenance.json').write_text(json.dumps({'license':'CC0-1.0','source':'Independent explicit Y/Cb/Cr component arrays encoded with native TurboJPEG; original TIFF6 table/header fragments and implied restart boundaries assembled manually without production reader or FabIO writer','generator_sha256':sha(pathlib.Path(__file__).read_bytes()),'jpeg_library':{'path':str(binary),'sha256':sha(binary.read_bytes())},'reference_library':oracle.metadata,'files':manifest},indent=2)+'\n')
if __name__=='__main__':main()
