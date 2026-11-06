"""CC0 bit-defined NPY v1/v2/v3 files; headers are hand-packed, not np.save."""
import hashlib
import json
import math
import pathlib
import pickle
import struct
import numpy as np

ROOT=pathlib.Path(__file__).resolve().parents[1]
DATA=ROOT/'tests/data/npy-types'

def header(descriptor,shape,fortran,version):
    literal=repr({'descr':descriptor,'fortran_order':fortran,'shape':tuple(shape)}).encode('utf8' if version==3 else 'latin1')
    width=2 if version==1 else 4
    length=len(literal)+1
    length+=(-(8+width+length))%64
    body=literal+b' '*(length-len(literal)-1)+b'\n'
    return b'\x93NUMPY'+bytes([version,0])+struct.pack('<H' if width==2 else '<I',length)+body

def frame_shape(shape):
    return [1,1] if not shape else [1,shape[0]] if len(shape)==1 else list(shape[-2:])

def record(array,fortran,version):
    shape=list(array.shape);plane=frame_shape(shape);frames=math.prod(shape[:-2]) if len(shape)>2 else 1
    logical=array.reshape((frames,*plane))
    descriptor=array.dtype.str
    prefix=header(descriptor,shape,fortran,version)
    raw=array.tobytes(order='F' if fortran else 'C')
    canonical=lambda frame:frame.astype(frame.dtype.newbyteorder('<'),copy=False).tobytes(order='C').hex()
    return prefix+raw,{'version':[version,0],'source_shape':shape,'frame_shape':plane,'shape':plane,'pixel_type':'Bool' if array.dtype.kind=='b' else 'F16','dtype':descriptor,
                       'little_endian':array.dtype.byteorder!='>','fortran_order':fortran,'frames':frames,'frame_hex':[canonical(frame) for frame in logical],
                       'source_data_hex':raw.hex(),'header_hex':prefix.hex()}

def samples(kind,shape,order='<'):
    count=math.prod(shape)
    if kind=='bool':return np.frombuffer(bytes([0,1,2,127,128,255][i%6] for i in range(count)),dtype='?').reshape(shape)
    words=[0x8000,0x7e55,0xfeaa,0x0001,0x0400,0x3c00,0x7bff,0x7c00,0xfc00,0,0xc000,0x3555]
    return np.array([words[i%len(words)] for i in range(count)],dtype='<u2').view('<f2').astype(order+'f2').reshape(shape)

def generate():
    DATA.mkdir(parents=True,exist_ok=True)
    manifest={'schema':1,'license':'CC0-1.0','generator':'scripts/generate_npy_type_fixtures.py','origin':'independent binary16 words and raw bool storage bytes; manual NPY dictionary/padding/length headers; no FabIO or NumPy NPY writer','files':{},'unsupported':{},'malformed':{}}
    for version in [1,2,3]:
        for fortran in [False,True]:
            for rank,shape in enumerate([(),(6,),(2,3),(2,2,3),(2,2,2,3)]):
                for kind,order in [('bool','|'),('half','<'),('half','>')]:
                    name=f'{kind}-v{version}-'+('na' if kind=='bool' else 'le' if order=='<' else 'be')+'-'+('f' if fortran else 'c')+f'-r{rank}.npy'
                    payload,case=record(samples(kind,shape,order),fortran,version);(DATA/name).write_bytes(payload);manifest['files'][name]=case
        for index,shape in enumerate([(0,),(0,3),(2,0),(0,2,3),(2,0,3),(2,3,0),(2,0,2,3),(2,2,0,3)]):
            for kind in ['bool','half']:
                name=f'{kind}-v{version}-empty{index}.npy';payload,case=record(samples(kind,shape),bool(index%2),version);(DATA/name).write_bytes(payload);manifest['files'][name]=case
    structured=np.array([[(1.25,255),(-0.0,0)]],dtype=[('real','<f4'),('flag','u1')])
    string=np.array([['µ','A']],dtype='<U2')
    for name,array in [('structured.npy',structured),('string.npy',string)]:
        descriptor=array.dtype.descr if array.dtype.fields else array.dtype.str
        payload=header(descriptor,array.shape,False,3)+array.tobytes()
        (DATA/name).write_bytes(payload);manifest['unsupported'][name]={'input_condition':'well_formed_variant','kind':name[:-4],'source_shape':list(array.shape),'dtype':str(array.dtype),'source_data_hex':array.tobytes().hex()}
    payload=header('<f16',(1,1),False,1)+bytes(16)
    (DATA/'float128.npy').write_bytes(payload);manifest['unsupported']['float128.npy']={'input_condition':'well_formed_platform_variant','kind':'float128','source_shape':[1,1],'dtype':'<f16','source_data_hex':bytes(16).hex(),'qualification':'current macOS arm64 NumPy has no 16-byte floating dtype; no numerical interpretation claimed'}
    array=np.array([[None]],dtype=object);raw=pickle.dumps(array,protocol=4);payload=header('|O',array.shape,False,1)+raw
    (DATA/'object.npy').write_bytes(payload);manifest['unsupported']['object.npy']={'input_condition':'well_formed_variant','kind':'object','source_shape':[1,1],'dtype':'|O','qualification':'benign independently authored None object pickle; readers must reject without unpickling'}
    full,case=record(samples('half',(2,3)),False,1)
    invalid={'truncated.npy':full[:-1],'negative-shape.npy':header('<f2',(-1,3),False,1),'bad-magic.npy':b'notNPY!\x00'+full[8:]}
    for name,payload in invalid.items():
        (DATA/name).write_bytes(payload);manifest['malformed'][name]={'input_condition':'malformed_or_truncated','reader_action':'reject'}
    for collection in ['files','unsupported','malformed']:
        for name,case in manifest[collection].items():
            payload=(DATA/name).read_bytes();case.update(file=name,bytes=len(payload),sha256=hashlib.sha256(payload).hexdigest())
    (DATA/'provenance.json').write_text(json.dumps(manifest,indent=2,ensure_ascii=False)+'\n')

if __name__=='__main__':generate()
