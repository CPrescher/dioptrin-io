"""Independent CC0 source arrays and Pillow 13 display oracles; test-only."""
import hashlib,json,pathlib
import numpy as np
import tifffile,imagecodecs
from PIL import Image,__version__ as pillow_version
root=pathlib.Path(__file__).resolve().parents[1]/'tests/data/tiff-view';root.mkdir(exist_ok=True)
manifest={'license':'CC0-1.0','generator':'scripts/generate_tiff_view_fixtures.py','versions':{'numpy':np.__version__,'tifffile':tifffile.__version__,'Pillow':pillow_version,'imagecodecs':imagecodecs.__version__},'files':{}}
def record(name,raw,**extra):
    path=root/name;a=np.asarray(raw)
    (root/(name+'.raw')).write_bytes(a.astype(a.dtype.newbyteorder('<')).tobytes())
    info={'source_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'raw_dtype':str(a.dtype),'raw_shape':list(a.shape),'raw_sha256':hashlib.sha256((root/(name+'.raw')).read_bytes()).hexdigest(),**extra}
    try:
        image=Image.open(path)
        expected=np.asarray(image.convert('RGB') if image.mode=='P' else image.convert('F') if image.mode in ['RGB','RGBA','CMYK'] else image)
        (root/(name+'.pillow')).write_bytes(expected.astype('u1' if expected.dtype.kind=='b' else expected.dtype.newbyteorder('<')).tobytes())
        info.update(pillow_dtype=str(expected.dtype),pillow_shape=list(expected.shape),pillow_sha256=hashlib.sha256((root/(name+'.pillow')).read_bytes()).hexdigest())
    except OSError as error:info['pillow_rejection']=str(error).split(':')[0]
    manifest['files'][name]=info

base=np.array([[0,19,31,53,71],[97,113,149,173,211]],dtype='u1')
for color,channels in [('rgb',3),('rgba',4),('cmyk',4)]:
    a=np.stack([(base+offset).astype('u1') for offset in [0,31,127,43][:channels]],axis=-1)
    for bits in [8,16]:
        data=a if bits==8 else a.astype('u4')*257+np.array([0,1,63,127,255],dtype='u4')[None,:,None]
        # RGBA source count preservation is validated with unassociated alpha.
        if bits==16:data=np.minimum(data.astype('u4'),65535).astype('u2')
        for planar in ['contig','separate']:
            name=f'{color}{bits}-{planar}.tif';stored=data if planar=='contig' else data.transpose(2,0,1)
            tifffile.imwrite(root/name,stored,photometric='separated' if color=='cmyk' else 'rgb',planarconfig=planar,extrasamples=['unassalpha'] if color=='rgba' else None,metadata=None)
            record(name,data,reference_defect=bits==16 and planar=='separate')
# Associated alpha is a separate display operation; native counts stay premultiplied.
name='rgba8-associated.tif';data=np.array([[[0,9,20,0],[10,20,30,64],[128,200,255,255]],[[1,2,3,4],[64,32,16,128],[200,100,50,1]]],dtype='u1')
tifffile.imwrite(root/name,data,photometric='rgb',extrasamples=['assocalpha'],metadata=None);record(name,data)
# Original JPEG component samples come from the independent native decoder;
# Pillow RGB/luminance rendering is a separate explicit oracle.
a=(np.arange(17*19*3,dtype='u2')*37%256).astype('u1').reshape(17,19,3)
for tiled in [False,True]:
    name='jpeg-ycbcr-'+('tiles' if tiled else 'strips')+'.tif'
    options={'tile':(16,16)} if tiled else {'rowsperstrip':8}
    tifffile.imwrite(root/name,a,photometric='ycbcr',compression='jpeg',subsampling=(1,1),compressionargs={'colorspace':'RGB','outcolorspace':'YCbCr'},metadata=None,**options)
    encoded=(root/name).read_bytes();raw=np.zeros_like(a)
    with tifffile.TiffFile(root/name) as file:
        page=file.pages[0];tables=page.jpegtables
        for index,(offset,length) in enumerate(zip(page.dataoffsets,page.databytecounts)):
            chunk=encoded[offset:offset+length]
            if tables:chunk=tables[:-2]+chunk[2:]
            block=imagecodecs.jpeg_decode(chunk,outcolorspace='YCbCr')
            y,x=((index//2)*16,(index%2)*16) if tiled else (index*8,0)
            h=min(block.shape[0],17-y);w=min(block.shape[1],19-x);raw[y:y+h,x:x+w]=block[:h,:w]
    record(name,raw,lossy=True,raw_reference_decoder='libjpeg-turbo')
# Palette values deliberately include non-replicated bytes to distinguish
# unscaled 16-bit lookup from an 8-bit display conversion.
for bits in [8,16]:
    size=1<<bits;a=np.array([[0,1,2],[3,4,size-1]],dtype='u1' if bits==8 else 'u2')
    colors=np.zeros((3,size),dtype='u2');colors[:,:5]=[[0,255,256,32768,65535],[65535,511,1500,32001,27],[71,0,65001,12345,65535]];colors[:,-1]=[1001,2011,3019]
    name=f'palette{bits}.tif';tifffile.imwrite(root/name,a,photometric='palette',colormap=colors,metadata=None);record(name,a)
    # FabIO TiffIO first truncates palette values to 8-bit, but allocates output
    # using the stored index scalar dtype, including uint16 palette sources.
    (root/(name+'.palette16')).write_bytes(colors[:,a].transpose(1,2,0).astype('<u2').tobytes())
    expected=(colors[:,a]>>8).transpose(1,2,0).astype(a.dtype)
    (root/(name+'.tiffio')).write_bytes(expected.astype(expected.dtype.newbyteorder('<')).tobytes())
for bits in [1,2,4]:
    for photo in ['black','white']:
        source=pathlib.Path(__file__).resolve().parents[1]/'tests/data/images'/f'packed{bits}-{photo}.tif'
        name=source.name;(root/name).write_bytes(source.read_bytes());a=(np.arange(39,dtype='u1')%(1<<bits)).reshape(3,13);record(name,a)
for byteorder in ['<','>']:
    name='white-float-'+('le' if byteorder=='<' else 'be')+'.tif'
    data=np.array([[0.,-0.,1.5],[-99.,np.inf,np.nan]],dtype='f4')
    tifffile.imwrite(root/name,data,photometric='miniswhite',byteorder=byteorder,metadata=None);record(name,data)
# Pillow13 inverts byte/packed grayscale but retains I;16/F samples under
# WhiteIsZero; these are explicit versioned display semantics, not raw decode.
for dtype in ['u1','u2']:
    name=f'white-{dtype}.tif';data=np.array([[0,1,2],[3,17,np.iinfo(dtype).max]],dtype=dtype)
    tifffile.imwrite(root/name,data,photometric='miniswhite',compression='deflate',metadata=None);record(name,data)
# CFA is retained as a raw scalar sensor plane, never demosaiced.
name='cfa16.tif';data=np.array([[0,65535,12345],[3,4,5]],dtype='u2')
tifffile.imwrite(root/name,data,photometric=32803,metadata=None,extratags=[(33421,'H',2,(2,2),False),(33422,'B',4,(0,1,1,2),False)]);record(name,data)
# Legacy malformed descriptive ASCII is retained as its literal inline byte;
# no pointer175/text repair is performed, and the valid sample window is unchanged.
for label,dtype in [('uint16','u2'),('float32','f4')]:
    name=f'opaque-description-{label}.tif';data=np.arange(6,dtype=dtype).reshape(2,3)
    tifffile.imwrite(root/name,data,description='description bytes',metadata=None,photometric='miniswhite')
    encoded=bytearray((root/name).read_bytes());ifd=int.from_bytes(encoded[4:8],'little');count=int.from_bytes(encoded[ifd:ifd+2],'little')
    for index in range(count):
        entry=ifd+2+index*12
        if int.from_bytes(encoded[entry:entry+2],'little')==270:
            encoded[entry+4:entry+8]=(1).to_bytes(4,'little');encoded[entry+8:entry+12]=(175).to_bytes(4,'little')
    (root/name).write_bytes(encoded);record(name,data,opaque_tag270=True)
# Multi-page palette BigTIFF exercises adapter offsets and page selection.
name='palette-bigtiff.tif';data=np.array([[0,1],[3,2]],dtype='u1');colors=np.tile(np.arange(256,dtype='u2')*257,(3,1))
with tifffile.TiffWriter(root/name,bigtiff=True,byteorder='>') as writer:
    for a in [data,data+1]:writer.write(a,photometric='palette',colormap=colors,metadata=None)
record(name,data)
(root/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
