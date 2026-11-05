"""Record independent pinned-FabIO counts for the external SPE/MRC/DM3 corpus.
Usage: reference-env/bin/python scripts/container_upstream_reference.py CORPUS OUTPUT
Run CONTAINER_PARITY_MANIFEST=OUTPUT cargo test --features containers --test containers.
The resulting small manifest contains hashes/metadata, never upstream pixel payloads.
"""
import hashlib,json,pathlib,sys
import fabio,numpy as np
from fabio.speimage import SpeImage
from fabio.mrcimage import MrcImage
from fabio.dm3image import Dm3Image
root=pathlib.Path(sys.argv[1]).resolve()
manifest={'directory':str(root),'fabio_version':fabio.version,'fabio_commit':'9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece','files':{}}
for kind,cls,names in [('spe',SpeImage,['v2.SPE','v2_converted.SPE','v3.spe','v3_custom_roi.spe','v3_2frames.spe']),('mrc',MrcImage,['EMD-3001.map','0p67_5s_0000.mrc']),('dm3',Dm3Image,['ref_d20x_310mm.dm3'])]:
 for name in names:
  path=root/name;ref=cls().read(path)
  count=ref.header['num_frames'] if kind=='spe' else ref.nframes
  hashes=[]
  for i in range(count):
   # Pinned SPE.nframes is always 1 and pinned MRC.getframe raises AttributeError.
   # Their public read(frame=i) methods expose every declared stored frame.
   data=cls().read(path,frame=i).data if kind in ('spe','mrc') else ref.data
   hashes.append(hashlib.sha256(np.ascontiguousarray(data.astype(data.dtype.newbyteorder('<'))).tobytes()).hexdigest())
  manifest['files'][name]={'format':kind,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'shape':[int(count),*map(int,ref.data.shape)],'dtype':ref.data.dtype.name,'frame_sha256':hashes}
pathlib.Path(sys.argv[2]).write_text(json.dumps(manifest,indent=2)+'\n')
