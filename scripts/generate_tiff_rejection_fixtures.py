"""CC0 independent Group3/FillOrder fixtures. Python is test-data tooling only."""
import hashlib, io, json, pathlib, struct
import imagecodecs, numpy as np, tifffile
from PIL import Image, __version__ as pillow_version
root=pathlib.Path(__file__).resolve().parents[1]/'tests/data/tiff-rejections'
root.mkdir(exist_ok=True)
def tiff(width,height,bits,compression,fill,photo,rows,chunks,description=None,t4options=0):
    tags=[(256,4,1,width),(257,4,1,height),(258,3,1,bits),(259,3,1,compression),(262,3,1,photo),(266,3,1,fill),(273,4,len(chunks),0),(278,4,1,rows),(279,4,len(chunks),0)]
    if compression==3:tags.append((292,4,1,t4options))
    if description is not None:tags.append((270,2,len(description),int.from_bytes(description.ljust(4,b'\0'),'little')))
    tags.sort(); start=8+2+12*len(tags)+4
    offsets=[];pos=start+(8*len(chunks) if len(chunks)>1 else 0)
    for chunk in chunks:offsets.append(pos);pos+=len(chunk)
    fields={273:offsets[0] if len(chunks)==1 else start,279:len(chunks[0]) if len(chunks)==1 else start+4*len(chunks)}
    data=b'II*\0'+struct.pack('<IH',8,len(tags))
    for tag,ty,n,value in tags:data+=struct.pack('<HHII',tag,ty,n,fields.get(tag,value))
    data+=b'\0'*4
    if len(chunks)>1:data+=struct.pack('<'+'I'*len(chunks),*offsets)+struct.pack('<'+'I'*len(chunks),*[len(c) for c in chunks])
    return data+b''.join(chunks)
def compressed(a,aligned=False):
    stream=io.BytesIO();Image.fromarray(a.astype(bool)).save(stream,format='TIFF',compression='group3',tiffinfo={292:4 if aligned else 0})
    data=stream.getvalue()
    with tifffile.TiffFile(io.BytesIO(data)) as file:
        page=file.pages[0]; assert len(page.dataoffsets)==1
        chunk=data[page.dataoffsets[0]:page.dataoffsets[0]+page.databytecounts[0]]
    independent=imagecodecs.ccittfax3_decode(chunk,height=a.shape[0],width=a.shape[1],t4options=4 if aligned else 0)
    assert np.array_equal(a,independent)
    return chunk
manifest={'license':'CC0-1.0','generator':'scripts/generate_tiff_rejection_fixtures.py','versions':{'Pillow':pillow_version,'imagecodecs':imagecodecs.__version__,'tifffile':tifffile.__version__,'numpy':np.__version__},'files':{}}
def record(name,data,raw):
    (root/name).write_bytes(data);(root/(name+'.raw')).write_bytes(raw.astype('u1').tobytes())
    with Image.open(io.BytesIO(data)) as image: display=np.asarray(image).astype('u1')
    (root/(name+'.pillow')).write_bytes(display.tobytes())
    manifest['files'][name]={'sha256':hashlib.sha256(data).hexdigest(),'raw_sha256':hashlib.sha256(raw.astype('u1').tobytes()).hexdigest(),'pillow_sha256':hashlib.sha256(display.tobytes()).hexdigest(),'shape':list(raw.shape)}
a=np.fromfunction(lambda y,x:(x*3+y*5)%7<3,(7,19)).astype('u1')
for fill in [1,2]:
 for photo in [0,1]:
    chunks=[compressed(a[i:i+3]) for i in range(0,len(a),3)]
    if fill==2:chunks=[bytes(int(f'{b:08b}'[::-1],2) for b in c) for c in chunks]
    record(f'group3-fill{fill}-photo{photo}.tif',tiff(19,7,1,3,fill,photo,3,chunks),a)
record('group3-aligned.tif',tiff(19,7,1,3,1,1,7,[compressed(a,True)],t4options=4),a)
a=np.zeros((2,3000),dtype='u1');a[1,:]=1
record('group3-makeup.tif',tiff(3000,2,1,3,1,1,2,[compressed(a)]),a)
for bits in [1,2,4]:
    a=(np.arange(39,dtype='u1')%(1<<bits)).reshape(3,13)
    chunks=[]
    for row in a:
        bitstring=''.join(f'{int(v):0{bits}b}' for v in row);bitstring+='0'*(-len(bitstring)%8)
        chunks.append(bytes(int(bitstring[i:i+8][::-1],2) for i in range(0,len(bitstring),8)))
    record(f'packed{bits}-fill2.tif',tiff(13,3,bits,1,2,1,1,chunks),a)
for literal in [b'    ',b'\xaf',b'A\xffB\0']:
    name='opaque-'+literal.hex()+'.tif';a=np.array([[1,17,255]],dtype='u1')
    record(name,tiff(3,1,8,1,1,1,1,[a.tobytes()],literal),a)
(root/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
