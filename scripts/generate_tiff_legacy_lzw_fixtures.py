"""Independent CC0 legacy LZW/TIFF fixtures; no runtime Python dependency.

The small encoder and codeword packer below are test oracles, never production
code. imagecodecs independently decodes every legacy stream before inclusion.
"""
import collections, hashlib, io, json, pathlib, struct
import imagecodecs, numpy as np, tifffile
from PIL import Image, __version__ as pillow_version
ROOT=pathlib.Path(__file__).resolve().parents[1]/'tests/data/tiff-legacy-lzw'
ROOT.mkdir(exist_ok=True)

def codewords(data,clear_every=None):
    table={bytes([i]):i for i in range(256)};next_code=258;prefix=b'';codes=[256];since_clear=0
    for byte in data:
        suffix=bytes([byte]);word=prefix+suffix
        if word in table:prefix=word;continue
        codes.append(table[prefix]);since_clear+=1
        if next_code<4096:table[word]=next_code;next_code+=1
        prefix=suffix
        if (clear_every and since_clear>=clear_every) or next_code>=4095:
            codes.append(256);table={bytes([i]):i for i in range(256)};next_code=258;since_clear=0
    if prefix:codes.append(table[prefix])
    return codes+[257]

def pack(codes,early=False):
    output=bytearray();pending=0;count=0;width=9;next_code=258;previous=False;stats=collections.Counter()
    for code in codes:
        assert code < 1<<width
        pending|=code<<count;count+=width;stats['width'+str(width)]+=1
        while count>=8:output.append(pending&255);pending>>=8;count-=8
        if code==256:width=9;next_code=258;previous=False;stats['clears']+=1;continue
        if code==257:continue
        if code==next_code:stats['kwkwk']+=1
        if previous and next_code<4096:
            next_code+=1
            if next_code==(1<<width)-int(early) and width<12:width+=1
        previous=True
    if count:output.append(pending)
    return bytes(output),dict(stats)

def legacy(data,clear_every=None):
    compressed,stats=pack(codewords(data,clear_every))
    assert bytes(imagecodecs.lzw_decode(compressed))==data
    return compressed,stats

def tiff(width,height,bits,channels,chunks,rows=None,tile=None,predictor=1,planar=1,fmt=1,endian='<',orientation=1):
    def field(tag,ty,values):
        if not isinstance(values,(list,tuple)):values=[values]
        return tag,ty,len(values),struct.pack(endian+{3:'H',4:'I'}[ty]*len(values),*values)
    fields=[field(256,4,width),field(257,4,height),field(258,3,[bits]*channels),field(259,3,5),field(262,3,2 if channels==3 else 1),field(274,3,orientation),field(277,3,channels),field(284,3,planar),field(317,3,predictor),field(339,3,[fmt]*channels)]
    offset_tag,count_tag=(324,325) if tile else (273,279)
    if tile:fields.extend([field(322,4,tile[1]),field(323,4,tile[0])])
    else:fields.append(field(278,4,rows or height))
    fields.extend([field(offset_tag,4,[0]*len(chunks)),field(count_tag,4,[len(c) for c in chunks])]);fields.sort()
    start=8+2+12*len(fields)+4
    for tag,ty,n,value in fields:
        if len(value)>4:start+=(len(value)+1)//2*2
    offsets=[];cursor=start
    for chunk in chunks:offsets.append(cursor);cursor+=len(chunk)
    fields=[field(tag,ty,offsets) if tag==offset_tag else (tag,ty,n,value) for tag,ty,n,value in fields]
    data=(b'II' if endian=='<' else b'MM')+struct.pack(endian+'HIH',42,8,len(fields));extra=bytearray();extra_start=8+2+12*len(fields)+4
    for tag,ty,n,value in fields:
        data+=struct.pack(endian+'HHI',tag,ty,n)
        if len(value)<=4:data+=value.ljust(4,b'\0')
        else:
            data+=struct.pack(endian+'I',extra_start+len(extra));extra.extend(value)
            if len(extra)%2:extra.append(0)
    data+=b'\0'*4+extra+b''.join(chunks)
    assert len(data)==cursor
    return data

manifest={'license':'CC0-1.0','generator':'scripts/generate_tiff_legacy_lzw_fixtures.py','versions':{'numpy':np.__version__,'imagecodecs':imagecodecs.__version__,'tifffile':tifffile.__version__,'Pillow':pillow_version},'files':{}}
def record(name,raw,data,stats,**extra):
    (ROOT/name).write_bytes(data);canonical=raw.astype(raw.dtype.newbyteorder('<')).tobytes();(ROOT/(name+'.raw')).write_bytes(canonical)
    # tifffile/imagecodecs is independent of both our tiny test encoder and
    # production weezl. It also checks TIFF layout, endianness and prediction.
    with tifffile.TiffFile(io.BytesIO(data)) as file:
        independent=file.asarray()
        if file.pages[0].planarconfig==2:independent=independent.transpose(1,2,0)
        if file.pages[0].bitspersample<8:independent=independent.astype('u1')
        assert independent.astype(independent.dtype.newbyteorder('<')).tobytes()==canonical,name
    info={'sha256':hashlib.sha256(data).hexdigest(),'dtype':str(raw.dtype),'shape':list(raw.shape),'sample_sha256':hashlib.sha256(canonical).hexdigest(),'stream_statistics':stats,**extra}
    try:
        with Image.open(io.BytesIO(data)) as image:
            display=np.asarray(image.convert('F') if image.mode=='RGB' else image)
        display=display.astype('u1') if display.dtype.kind=='b' else display.astype(display.dtype.newbyteorder('<'))
        (ROOT/(name+'.pillow')).write_bytes(display.tobytes());info['pillow_shape']=list(display.shape);info['pillow_dtype']=str(display.dtype)
    except Exception as error:info['pillow_error']=str(error)
    manifest['files'][name]=info

def predicted(array,predictor,endian='<'):
    if predictor==3:return imagecodecs.floatpred_encode(array,axis=1).tobytes()
    result=array.copy()
    if predictor==2:result[:,1:]=array[:,1:]-array[:,:-1]
    return result.astype(result.dtype.newbyteorder(endian)).tobytes()
def strips(name,array,rows=None,predictor=1,planar=1,endian='<',clear_every=None,mixed=False):
    height,width=array.shape[:2];channels=array.shape[2] if array.ndim==3 else 1;rows=rows or height;chunks=[];stats=[]
    planes=[array[:,:,i] for i in range(channels)] if planar==2 else [array]
    for plane in planes:
        for y in range(0,height,rows):
            source=predicted(plane[y:y+rows],predictor,endian)
            if mixed and len(chunks)%2:
                chunk=imagecodecs.lzw_encode(source);stat={'dialect':'modern_msb_early'}
            else:chunk,stat=legacy(source,clear_every)
            chunks.append(chunk);stats.append(stat)
    data=tiff(width,height,array.dtype.itemsize*8,channels,chunks,rows=rows,predictor=predictor,planar=planar,fmt=3 if array.dtype.kind=='f' else 1,endian=endian,orientation=6 if channels==3 else 1)
    record(name,array,data,stats)

random=np.random.default_rng(20261106).integers(0,256,size=(65,257),dtype='u1')
strips('dictionary-widths.tif',random)
strips('clears-after-growth.tif',random[:37],clear_every=1100)
repeated=np.frombuffer(b'ABABABA'*211+b'\0'*513+b'TOBEORNOTTOBEORTOBEORNOT'*23,dtype='u1')[None,:]
strips('kwkwk.tif',repeated)
a=(np.arange(7*19*3,dtype='u2')*37%256).astype('u1').reshape(7,19,3)
strips('rgb-strips-predictor2.tif',a,rows=3,predictor=2)
strips('rgb-planar-predictor2.tif',a,rows=3,predictor=2,planar=2)
a=(np.arange(4*17,dtype='u4')*8191%65536).astype('u2').reshape(4,17)
strips('uint16-be-predictor2.tif',a,rows=3,predictor=2,endian='>')
a=np.array([0,0x80000000,0x3f800000,0x7f800000,0xff800000,0x7fc0002a,0x00000001,0xc2c60000],dtype='u4').view('f4').reshape(2,4)
strips('float32-predictor3.tif',a,predictor=3)
a=(np.arange(4*17,dtype='u2')*41%256).astype('u1').reshape(4,17)
strips('mixed-modern-legacy.tif',a,rows=2,mixed=True)
a=(np.arange(17*19*3,dtype='u2')*19%256).astype('u1').reshape(17,19,3);chunks=[];stats=[]
for y in range(0,17,16):
    for x in range(0,19,16):
        padded=np.full((16,16,3),205,dtype='u1');h=min(16,17-y);w=min(16,19-x);padded[:h,:w]=a[y:y+h,x:x+w]
        chunk,stat=legacy(padded.tobytes());chunks.append(chunk);stats.append(stat)
record('rgb8-edge-tiles.tif',a,tiff(19,17,8,3,chunks,tile=(16,16)),stats)
a=(np.arange(39,dtype='u1')%4).reshape(3,13);source=b''
for row in a:
    bits=''.join(f'{int(v):02b}' for v in row);bits+='0'*(-len(bits)%8);source+=bytes(int(bits[i:i+8],2) for i in range(0,len(bits),8))
chunk,stat=legacy(source);record('packed2.tif',a,tiff(13,3,2,1,[chunk]),[stat])
# Same source dictionary codewords with the incorrect early-width convention.
codes=codewords(random.tobytes());bad,_=pack(codes,early=True)
(ROOT/'invalid-lsb-early.tif').write_bytes(tiff(257,65,8,1,[bad]))
manifest['malformed']={'invalid-lsb-early.tif':'LSB prefix but incorrect early-change code widths; must not trigger modern retry'}
(ROOT/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
