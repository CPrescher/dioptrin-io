"""CC0 independent HDF5 type/external-window fixtures; no FabIO writer."""
import hashlib
import json
import pathlib
import h5py
import numpy as np

ROOT = pathlib.Path(__file__).resolve().parents[1]
DATA = ROOT / 'tests/data/hdf5-layouts'

def generate():
    DATA.mkdir(parents=True, exist_ok=True)
    cases = {}
    def retain(name, values, **kwargs):
        dataset = file.create_dataset(name, data=values, **kwargs)
        dataset.attrs.create('original', values, dtype=values.dtype)
        count = 1 if values.ndim == 2 else values.shape[0]
        frames = values.reshape((count, -1))
        dtype = values.dtype.newbyteorder('<')
        cases[name] = {'shape': list(values.shape), 'dtype': values.dtype.str,
                       'frame_hex': [frame.astype(dtype).tobytes().hex() for frame in frames],
                       'attribute_hex': values.astype(dtype).tobytes().hex()}
        return dataset
    with h5py.File(DATA/'types.h5', 'w') as file:
        for width, words in [(4, [0, 0x80000000, 0x7f800000, 0xff800000, 0x7fc12345, 0xffc54321, 1, 0x3f800000, 0x7f7fffff, 0x00800000, 0x40000000, 0xc0400000]),
                             (8, [0, 0x8000000000000000, 0x7ff0000000000000, 0xfff0000000000000, 0x7ff8123456789abc, 0xfff8abcddcba1234, 1, 0x3ff0000000000000, 0x7fefffffffffffff, 0x0010000000000000, 0x4000000000000000, 0xc008000000000000])]:
            native = np.array(words, dtype=f'<u{width}').view(f'<c{width*2}').reshape(1, 2, 3)
            for order in ['<', '>']:
                values = native.astype(f'{order}c{width*2}')
                retain(f'complex{width*16}-'+('le' if order=='<' else 'be'), values, chunks=(1, 1, 2), compression='gzip', shuffle=True)
        half = np.array([0, 0x8000, 0x7c00, 0xfc00, 0x7e55, 0xfeaa, 1, 0x3c00, 0x7bff, 0x0400, 0x4000, 0xc200], dtype='<u2').view('<f2').reshape(2, 2, 3)
        for order in ['<', '>']:
            retain('half-'+('le' if order=='<' else 'be'), half.astype(order+'f2'), chunks=(1, 2, 2), compression='gzip')
        # A bool-typed array deliberately contains all stored bytes, including
        # noncanonical nonzero true representations. No bool cast normalizes it.
        boolean = np.frombuffer(bytes([0, 1, 2, 127, 128, 255]), dtype='?').reshape(2, 3)
        dataset = file.create_dataset('bool', shape=boolean.shape, dtype='?')
        dataset.id.write(h5py.h5s.ALL, h5py.h5s.ALL, boolean, mtype=dataset.id.get_type())
        dataset.attrs.create('original', boolean)
        cases['bool']={'shape':list(boolean.shape),'dtype':'|b1','frame_hex':[boolean.tobytes().hex()],'attribute_hex':boolean.tobytes().hex()}
        fill = np.array([0x80000000, 0x7fc12345], dtype='<u4').view('<c8')[0]
        dataset = file.create_dataset('complex-fill', shape=(2, 2), dtype='>c8', fillvalue=fill)
        cases['complex-fill']={'shape':[2,2],'dtype':'>c8','frame_hex':[np.full((2,2),fill,dtype='<c8').tobytes().hex()]}
        for name,dtype in [('renamed',np.dtype([('real','<f4'),('imag','<f4')])),('mixed-order',np.dtype([('r','<f4'),('i','>f4')])),('padded',np.dtype({'names':['r','i'],'formats':['<f4','<f4'],'offsets':[0,8],'itemsize':16}))]:
            file.create_dataset(name, shape=(2,3), dtype=dtype)
        file.create_dataset('enum', data=np.array([[0,1,255]],dtype='u1'), dtype=h5py.enum_dtype({'OFF':0,'ON':1},basetype='u1'))
        file.create_dataset('rank1', data=np.arange(3, dtype='i2'))
        file.create_dataset('rank4', data=np.zeros((1,1,2,3),dtype='i2'))
    with h5py.File(DATA/'complex-vds.h5','w',libver='latest') as file:
        layout=h5py.VirtualLayout(shape=(1,2,4),dtype='>c8')
        layout[:,:,0:3]=h5py.VirtualSource('types.h5','/complex64-be',shape=(1,2,3))
        fill=np.array([0x80000000,0x7fc12345],dtype='<u4').view('<c8')[0]
        file.create_virtual_dataset('data',layout,fillvalue=fill)
    values = np.array([0,0x80000000,0x7fc12345,0xffc54321,1,0x3f800000,0x40000000,0xc0400000,0x7f800000,0xff800000,0x7f7fffff,0x00800000,0x3f000000,0xbf000000,0,1,2,3,4,5,6,7,8,9],dtype='<u4').view('<c8').reshape(2,2,3)
    encoded = values.astype('>c8').tobytes()
    (DATA/'external-a.bin').write_bytes(b'PREFIX!'+encoded[:13]+b'not-samples')
    (DATA/'external-b.bin').write_bytes(b'prefix-123! '+encoded[13:]+b'not-samples')
    with h5py.File(DATA/'external.h5','w') as file:
        file.create_dataset('data',shape=values.shape,dtype='>c8',external=[('external-a.bin',7,13),('external-b.bin',12,h5py.h5f.UNLIMITED)])
    cases['external']={'shape':list(values.shape),'dtype':'>c8','frame_hex':[frame.astype('<c8').tobytes().hex() for frame in values]}
    with h5py.File(DATA/'sparse-types.h5','w') as file:
        for name,intensity in [('complex',np.array([0x80000000,0x7fc12345],dtype='<u4').view('<c8')),('half',np.array([0x7e55],dtype='<u2').view('<f2')),('bool',np.frombuffer(bytes([255]),dtype='?'))]:
            group=file.create_group(name)
            group['mask']=np.zeros((1,1),dtype='float32')
            group['frame_ptr']=np.array([0,1],dtype='uint32')
            group['index']=np.array([0],dtype='uint32')
            dataset=group.create_dataset('intensity',shape=(1,),dtype=intensity.dtype)
            dataset.id.write(h5py.h5s.ALL,h5py.h5s.ALL,intensity,mtype=dataset.id.get_type())
    files=[{'path':p.name,'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(DATA.iterdir()) if p.suffix in {'.h5','.bin'}]
    (DATA/'provenance.json').write_text(json.dumps({'schema':1,'license':'CC0-1.0','origin':'independent hand-chosen IEEE component/half bit patterns, bool storage bytes and explicit external windows; no reference writer','generator':'scripts/generate_hdf5_layout_fixtures.py','datasets':cases,'files':files},indent=2)+'\n')

if __name__=='__main__': generate()
