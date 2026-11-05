"""Pinned Esperanto file-reader counts plus its own two writer encodings.
Usage: reference-env/bin/python SCRIPT CORPUS OUTPUT GENERATED_DIRECTORY
"""
import hashlib,json,pathlib,sys
import fabio,numpy as np
from fabio.esperantoimage import EsperantoImage
root=pathlib.Path(sys.argv[1]).resolve();generated=pathlib.Path(sys.argv[3]).resolve();generated.mkdir(parents=True,exist_ok=True)
files=[]
for name in ['reference.esperanto','sucrose_1s__1_1.esperanto']:
 source=root/name;files.append(source);image=EsperantoImage().read(str(source))
 for encoding in ['4BYTE_LONG','AGI_BITFIELD']:
  image.format=encoding;path=generated/f'{source.stem}-{encoding}.esperanto';image.write(str(path));files.append(path)
manifest={'fabio_version':fabio.version,'fabio_commit':'9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece','files':{}}
for path in files:
 ref=EsperantoImage().read(str(path));a=ref.data
 assert a.dtype.name=='int32'
 manifest['files'][str(path)]={'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'encoding':ref.format,'shape':list(a.shape),'native_sha256':hashlib.sha256(a.astype('<i4').tobytes()).hexdigest(),'file_reader_dtype':a.dtype.name}
pathlib.Path(sys.argv[2]).write_text(json.dumps(manifest,indent=2)+'\n')
