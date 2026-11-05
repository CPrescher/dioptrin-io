"""Independent small TIFF fixtures. Test-only Python; never a runtime dependency.
Generated arrays/metadata are dedicated to CC0-1.0 by the dioptrin-io authors.
"""
import hashlib
import json
import pathlib
import numpy as np
import tifffile

root = pathlib.Path(__file__).resolve().parents[1] / 'tests/data/tiff'
manifest = {'license': 'CC0-1.0', 'generator': 'scripts/generate_tiff_fixtures.py',
            'versions': {'numpy': np.__version__, 'tifffile': tifffile.__version__}, 'files': {}}

def write(name, data, **options):
    path = root / name
    tifffile.imwrite(path, data, photometric='minisblack', metadata=None, **options)
    with tifffile.TiffFile(path) as f:
        pages = [p.asarray() for p in f.pages]
    assert all(np.array_equal(a.view('u1'), b.view('u1')) for a,b in zip(pages, data if data.ndim == 3 else [data]))
    manifest['files'][name] = {'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
      'shape': list(data.shape), 'dtype': str(data.dtype),
      'sample_sha256': [hashlib.sha256(a.astype(a.dtype.newbyteorder('<')).tobytes()).hexdigest() for a in pages],
      'options': options}

for dtype in ['uint8','uint16','uint32','uint64','int8','int16','int32','int64']:
    info = np.iinfo(dtype)
    a=np.array([0,info.min,info.max,2,3,4], dtype=dtype).reshape(2,3)
    write(dtype+'.tif', a, extratags=[(274,'H',1,6,False),(65000,'s',0,'original header',False)])
for dtype in ['float32','float64']:
    a=np.array([0.0,-0.0,np.nan,np.inf,-np.inf,-1.25],dtype=dtype).reshape(2,3)
    write(dtype+'.tif',a)
a=np.arange(2*17*19,dtype=np.uint32).reshape(2,17,19)*70000
for compression in ['deflate','lzw','packbits']:
    write(compression+'-tiles.tif',a, tile=(16,16),compression=compression)
for order in ['<','>']:
    write(('le' if order=='<' else 'be')+'-bigtiff.tif',a.astype('int32'),bigtiff=True,byteorder=order,compression='deflate',predictor=2)
# Explicit unsupported photometric sample for regression against count inversion.
tifffile.imwrite(root/'white-is-zero.tif', np.array([[0,255]],dtype='u1'),photometric='miniswhite',metadata=None)
manifest['files']['white-is-zero.tif']={'sha256':hashlib.sha256((root/'white-is-zero.tif').read_bytes()).hexdigest(),'expected':'explicit rejection'}
(root/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
