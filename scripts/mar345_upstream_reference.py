"""Record exact external MAR345/MAR555 original and upstream-writer file proof.
Usage: reference-env/bin/python SCRIPT CORPUS OUTPUT GENERATED_DIRECTORY
Production does not depend on FabIO, Python or any native LGPL codec.
"""
import hashlib,json,pathlib,sys
import fabio,numpy as np
from fabio.mar345image import Mar345Image
root=pathlib.Path(sys.argv[1]).resolve();generated=pathlib.Path(sys.argv[3]).resolve();generated.mkdir(parents=True,exist_ok=True)
files=[]
for name in ['example.mar2300','Fe3O4_023_101.mar2560']:
 original=root/name;files.append((original,'<'));image=Mar345Image().read(str(original))
 for order,endian in [('<','le'),('>','be')]:
  image.byteorder=order;target=generated/f'{name}-{endian}.mar345';image.write(str(target));files.append((target,order))
manifest={'fabio_version':fabio.version,'fabio_commit':'9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece','files':{}}
for path,order in files:
 # Pinned _readheader sets byte_order rather than byteorder in its big-endian
 # branch. Explicit reference order keeps overflow words correct for BE files.
 reader=Mar345Image();reader.byteorder=order;image=reader.read(str(path));a=image.data
 assert a.dtype.name=='uint32'
 manifest['files'][str(path)]={'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'shape':list(a.shape),'dtype':'uint32','little_endian':order=='<','sample_sha256':hashlib.sha256(a.astype('<u4').tobytes()).hexdigest(),'num_high':int(image.numhigh)}
pathlib.Path(sys.argv[2]).write_text(json.dumps(manifest,indent=2)+'\n')
