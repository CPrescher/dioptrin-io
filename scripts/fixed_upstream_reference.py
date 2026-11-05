"""Independent FabIO GE/Pixi corpus hashes; large payloads remain external.
Usage: reference-env/bin/python SCRIPT CORPUS OUTPUT PIXI_CAPTURE_DIRECTORY
PiXI test-created files are copied byte-for-byte before upstream temp cleanup.
"""
import hashlib,json,pathlib,shutil,sys
import fabio,numpy as np
from fabio.GEimage import GeImage
from fabio.pixiimage import PixiImage
from fabio.test.codecs.test_pixiimage import TestPixiImage
root=pathlib.Path(sys.argv[1]).resolve();capture=pathlib.Path(sys.argv[3]).resolve();capture.mkdir(parents=True,exist_ok=True)
TestPixiImage.create_fake_images()
files=[]
for name in ['GE_aSI_detector_image_1529','GE_image_1frame_intact_header.ge','GE_image_1frame_blanked_header.ge','dark_before_000428.edf.ge5']:files.append(('ge',GeImage,root/name))
for src in [TestPixiImage.single_frame,TestPixiImage.multi_frame,*[TestPixiImage.file_series.rsplit('$',1)[0]+f'${i:04}' for i in range(3)]]:
 path=capture/pathlib.Path(src).name;shutil.copyfile(src,path);files.append(('pixi',PixiImage,path))
manifest={'fabio_version':fabio.version,'fabio_commit':'9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece','files':{}}
for kind,cls,path in files:
 ref=cls().read(str(path));frames=ref.nframes;hashes=[]
 for i in range(frames):
  a=cls().read(str(path),frame=i).data;hashes.append(hashlib.sha256(np.ascontiguousarray(a.astype(a.dtype.newbyteorder('<'))).tobytes()).hexdigest())
 manifest['files'][str(path)]={'format':kind,'dtype':ref.data.dtype.name,'shape':[int(frames),*map(int,ref.data.shape)],'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'frame_sha256':hashes}
pathlib.Path(sys.argv[2]).write_text(json.dumps(manifest,indent=2)+'\n')
