"""Small independent CC0-1.0 TIFF/detector/JPEG/JPEG2000 source fixtures.
Python tools are independent test generators/oracles, never runtime dependencies.
"""
import hashlib,json,pathlib,struct
import numpy as np
import tifffile,imagecodecs
from PIL import Image, __version__ as pillow_version
root=pathlib.Path(__file__).resolve().parents[1]/'tests/data/images'
root.mkdir(exist_ok=True)
manifest={'license':'CC0-1.0','generator':'scripts/generate_image_fixtures.py','versions':{'numpy':np.__version__,'tifffile':tifffile.__version__,'Pillow':pillow_version,'imagecodecs':imagecodecs.__version__},'files':{}}
def record(name,a,**extra):
    a=np.asarray(a); raw=a.astype(a.dtype.newbyteorder('<')).tobytes()
    (root/(name+'.samples')).write_bytes(raw)
    manifest['files'][name]={'sha256':hashlib.sha256((root/name).read_bytes()).hexdigest(),'dtype':str(a.dtype),'shape':list(a.shape),'sample_sha256':hashlib.sha256(raw).hexdigest(),**extra}
def tiff(name,a,**kw):
    tifffile.imwrite(root/name,a,metadata=None,**kw)
    record(name,a)
rgb=np.arange(2*5*3,dtype='u2').reshape(2,5,3)*1801
for planar in ['contig','separate']:
    name='rgb-'+planar+'.tif'
    tifffile.imwrite(root/name,rgb if planar=='contig' else rgb.transpose(2,0,1),photometric='rgb',planarconfig=planar,metadata=None,extratags=[(274,'H',1,8,False)])
    record(name,rgb)
tiff('multiband.tif',rgb[:,:,:2],photometric='minisblack',extrasamples=['unspecified'])
a=np.array([0,1,0x8000,0x3c00,0xbc00,0x03ff,0x0400,0x7bff,0x7c00,0xfc00,0x7e01,0x7c01],dtype='u2').view('f2').reshape(2,6)
tiff('float16.tif',a,photometric='minisblack')
# Explicit expected bit expansion includes signaling NaN payload preservation.
out=[]
for bits in a.view('u2').flat:
    bits=int(bits);sign=(bits&0x8000)<<16;exp=(bits>>10)&31;frac=bits&1023
    if exp==0 and frac==0:v=sign
    elif exp==0:
        leading=frac.bit_length()-1;v=sign|((leading+103)<<23)|((frac-(1<<leading))<<(23-leading))
    elif exp==31:v=sign|0x7f800000|(frac<<13)
    else:v=sign|((exp+112)<<23)|(frac<<13)
    out.append(v)
(root/'float16.tif.samples').write_bytes(np.array(out,dtype='<u4').tobytes())
manifest['files']['float16.tif']['output_dtype']='float32';manifest['files']['float16.tif']['sample_sha256']=hashlib.sha256((root/'float16.tif.samples').read_bytes()).hexdigest()
for bits in [1,2,4]:
    for white in [False,True]:
        a=(np.arange(3*13,dtype='u1')%(1<<bits)).reshape(3,13)
        rows=bytearray()
        for row in a:
            packed=np.zeros((len(row)*bits+7)//8,dtype='u1')
            for x,v in enumerate(row):packed[x*bits//8]|=v<<(8-bits-x*bits%8)
            rows.extend(packed.tobytes())
        tags=[(256,4,1,13),(257,4,1,3),(258,3,1,bits),(259,3,1,1),(262,3,1,int(not white)),(273,4,1,8+2+9*12+4),(277,3,1,1),(278,4,1,3),(279,4,1,len(rows))]
        raw=b'II'+struct.pack('<HIH',42,8,len(tags))+b''.join(struct.pack('<HHII',*tag) for tag in tags)+b'\0'*4+rows
        name=f'packed{bits}-'+('white' if white else 'black')+'.tif';(root/name).write_bytes(raw);record(name,a)
        assert np.array_equal(tifffile.imread(root/name).astype('u1'),a)
# Group4 is independently encoded by Pillow/libtiff, reference counts by tifffile.
a=(np.indices((7,19)).sum(axis=0)%3==0)
Image.fromarray(a).save(root/'group4.tif',compression='group4')
record('group4.tif',tifffile.imread(root/'group4.tif').astype('u1'))
# TIFF JPEG grayscale and RGB (RGB encoded without YCbCr conversion).
for color in [False,True]:
    a=(np.arange(7*11*(3 if color else 1),dtype='u2')*37%256).astype('u1').reshape((7,11,3) if color else (7,11))
    name='jpeg-'+('rgb' if color else 'gray')+'.tif'
    tifffile.imwrite(root/name,a,photometric='rgb' if color else 'minisblack',compression='jpeg',compressionargs={'level':91,'outcolorspace':'RGB'} if color else {'level':91},metadata=None)
    record(name,tifffile.imread(root/name))
# Unsupported palette, subsampled YCbCr, subIFD topology and floating WhiteIsZero.
palette=np.array([[0,1,2],[3,2,1]],dtype='u1');colormap=np.tile(np.arange(256,dtype='u2')*257,(3,1))
tiff('palette.tif',palette,photometric='palette',colormap=colormap)
tiff('white-float.tif',np.array([[0.,1.]],dtype='f4'),photometric='miniswhite')
with tifffile.TiffWriter(root/'subifd.tif') as writer:
    writer.write(np.arange(12,dtype='u2').reshape(3,4),subifds=1,metadata=None)
    writer.write(np.array([[99]],dtype='u2'),subfiletype=1,metadata=None)
record('subifd.tif',np.arange(12,dtype='u2').reshape(3,4))
# Detector semantic metadata has no effect on the TIFF counts.
desc='# Detector: PILATUS 100K\r\n# Pixel_size 172e-6 m x 172e-6 m\r\n# Tau = 0 s\r\n# Exposure_time 0.1 s\r\n# Exposure_time 0.2 s\r\n# Unknown\r\nplain retained\r\n'
tiff('pilatus.tif',np.arange(6,dtype='i4').reshape(2,3),photometric='minisblack',description=desc)
for byteorder in ['<','>']:
    name='mar-'+('le' if byteorder=='<' else 'be')+'.mccd';a=np.array([[0,65535,12],[3,4,5]],dtype='u2')
    tifffile.imwrite(root/name,a,metadata=None,photometric='minisblack',byteorder=byteorder,align=4096)
    header=bytearray(3072);header[4:8]=b'MMX\0'
    for offset,value in [(0,2),(20,1),(24,0),(28,1234 if byteorder=='<' else 4321),(32,1234 if byteorder=='<' else 4321),(36,3072),(80,3),(84,2),(88,2),(768,1)]:struct.pack_into(byteorder+'I',header,offset,value)
    struct.pack_into(byteorder+'i',header,640,-12345);struct.pack_into(byteorder+'H',header,384,65535)
    header[1024:1036]=b'CC0 fixture\0'
    raw=bytearray((root/name).read_bytes());raw[1024:4096]=header;(root/name).write_bytes(raw);record(name,a)
# JPEG reference output comes from Pillow/libjpeg-turbo; preserve EXIF orientation.
for color in [False,True]:
    a=(np.arange(7*11*(3 if color else 1),dtype='u2')*37%256).astype('u1').reshape((7,11,3) if color else (7,11))
    for progressive in [False,True]:
        name=('rgb' if color else 'gray')+('-progressive' if progressive else '')+'.jpg'
        exif=Image.Exif();exif[274]=6
        Image.fromarray(a).save(root/name,quality=91,subsampling=0,progressive=progressive,exif=exif,comment=b'CC0 independent image')
        record(name,np.asarray(Image.open(root/name)),oracle='Pillow/libjpeg-turbo')
# JPEG 2000 with 8/16-bit, 12-bit precision, signed 16-bit, raw J2K and RGB.
for dtype in ['uint8','uint16','int16']:
    a=np.array([0,1,2,3,4,5,6,7,8,9,10,11],dtype=dtype).reshape(3,4)
    if dtype=='uint16':a=a*5301
    elif dtype=='int16':a=a*5201-30000
    name=dtype+'.jp2';(root/name).write_bytes(imagecodecs.jpeg2k_encode(a,level=0,reversible=True));record(name,a)
a=(np.arange(36,dtype='u1')*7).reshape(3,4,3)
for codecformat in ['JP2','J2K']:
    name='rgb.'+('jp2' if codecformat=='JP2' else 'j2k');(root/name).write_bytes(imagecodecs.jpeg2k_encode(a,level=0,reversible=True,codecformat=codecformat));record(name,a)
a=np.arange(12,dtype='u2').reshape(3,4)*317
(root/'uint12.jp2').write_bytes(imagecodecs.jpeg2k_encode(a,level=0,reversible=True,bitspersample=12));record('uint12.jp2',a,precision=12)
# Minimal independently authored predictive JPEG streams. A zero difference
# uses Huffman category 0 (one-bit code 0); remaining entropy bits are pad ones.
# This makes point-transform provenance and separate component scans explicit.
def predictive_jpeg(precision, transforms):
    def marker(code,payload):return bytes([255,code])+struct.pack('>H',len(payload)+2)+payload
    components=len(transforms)
    frame=bytes([precision])+struct.pack('>HHB',1,1,components)+b''.join(bytes([i+1,0x11,0]) for i in range(components))
    raw=b'\xff\xd8'+marker(0xc3,frame)+marker(0xc4,bytes([0,1])+bytes(15)+bytes([0]))
    for i,point in enumerate(transforms):
        if i:raw+=marker(0xfe,b'CC0 between component scans')
        raw+=marker(0xda,bytes([1,i+1,0,1,0,point]))+b'\x7f'
    return raw+b'\xff\xd9'
for name,precision,transforms in [('point-transform.jpg',8,[2]),('point-transform12.jpg',12,[3]),('later-point-transform.jpg',8,[0,2,0]),('lossless8-scans.jpg',8,[0,0,0]),('precision7.jpg',7,[0])]:
    (root/name).write_bytes(predictive_jpeg(precision,transforms))
    a=imagecodecs.jpeg_decode((root/name).read_bytes())
    record(name,a,precision=precision,point_transforms=transforms,lossy=any(transforms))
# Irreversible JPEG 2000: retain lossy provenance and compare decoded samples.
a=(np.arange(77,dtype='u2')*37%256).astype('u1').reshape(7,11)
(root/'lossy.jp2').write_bytes(imagecodecs.jpeg2k_encode(a,level=35,reversible=False))
record('lossy.jp2',imagecodecs.jpeg2k_decode((root/'lossy.jp2').read_bytes()),lossy=True)
# Lossless SOF3 native 12/16-bit samples are not intensity-scaled.
for bits in [12,16]:
    a=(np.arange(12,dtype='u4')*((1<<bits)-1)//11).astype('u2').reshape(3,4)
    name=f'lossless{bits}.jpg';(root/name).write_bytes(imagecodecs.jpeg_encode(a,lossless=True,bitspersample=bits));record(name,a,precision=bits)
(root/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
