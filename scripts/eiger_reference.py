"""Independent count hashes + benchmark inputs; test-only h5py/FabIO."""
import hashlib,json,pathlib,sys
import h5py,hdf5plugin,numpy as np,fabio
path=pathlib.Path(sys.argv[1]);root=pathlib.Path(__file__).resolve().parents[1]
out=pathlib.Path('/tmp/dioptrin-io-eiger-20261105');out.mkdir(exist_ok=True)
mask=fabio.open(str(path.with_suffix('.mask'))).data
assert mask.shape==(3262,3108)
# Match the existing benchmark: original mask uses geometry/consumer row order.
(mask!=0).astype('u1').tofile(out/'mask.u8')
reference={'source':str(path),'dataset':'entry_0000/instrument/eiger/data','bytes':path.stat().st_size,
 'h5py':h5py.__version__,'hdf5plugin':hdf5plugin.version,'numpy':np.__version__,'frame_sha256':[]}
with h5py.File(path) as f:
 d=f[reference['dataset']];reference['shape']=list(d.shape)
 for i in range(len(d)):
  a=d[i].astype('<u4',copy=False)
  reference['frame_sha256'].append(hashlib.sha256(a.tobytes()).hexdigest())
reference['sha256']=hashlib.file_digest(path.open('rb'),'sha256').hexdigest()
(root/'docs/benchmarks/2026-11-05-native-eiger/count-reference.json').write_text(json.dumps(reference,indent=2)+'\n')
print('Wrote',len(reference['frame_sha256']),'independent frame hashes',flush=True)
