"""Independent existing float64 Dioptrin/h5py output, not a runtime dependency.
Run with the previously validated Dioptrin environment, after timing completes.
"""
import argparse,hashlib,json,pathlib
import h5py,hdf5plugin,dioptrin,numpy as np
p=argparse.ArgumentParser();p.add_argument('file');p.add_argument('poni');p.add_argument('output');args=p.parse_args()
r=pathlib.Path(args.output)
integ=dioptrin.Integrator(args.poni,method='pixel_split',unit='2th_deg',polarization_factor=0.965,fast_math=True)
dioptrin.set_rayon_threads(8)
integ.set_mask(np.fromfile(r/'mask.u8',dtype='u1').reshape(3262,3108))
y=[];e=[]
with h5py.File(args.file) as f:
 d=f['entry_0000/instrument/eiger/data']
 for i in range(len(d)):
  result=integ.integrate1d(np.ascontiguousarray(d[i][::-1],dtype='f8'),2000,errors=True)
  y.append(result.intensity);e.append(result.errors)
np.array(y).tofile(r/'h5py-dioptrin-intensity.f64');np.array(e).tofile(r/'h5py-dioptrin-std.f64')
print('Wrote independent float64 Dioptrin/h5py outputs')
