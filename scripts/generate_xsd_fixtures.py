"""CC0 independent XSData records, checked against the pinned FabIO reference."""
import base64, hashlib, json, pathlib
import fabio, numpy as np
root=pathlib.Path(__file__).resolve().parents[1]/'tests/data/xsd'
root.mkdir(exist_ok=True)
manifest={'license':'CC0-1.0','numpy':np.__version__,'fabio':fabio.version,'files':{}}
for dtype in ['uint8','int8','uint16','int16','uint32','int32','uint64','int64','float32','float64','complex64','complex128']:
    dt=np.dtype(dtype).newbyteorder('<')
    if dtype.startswith('complex'):
        a=np.zeros((2,3),dtype=dt)
        a.real[:]=np.array([0.,-0.,np.nan,np.inf,-np.inf,-1.25]).reshape(2,3)
        a.imag[:]=np.array([-0.,0.,np.inf,np.nan,1.25,-np.inf]).reshape(2,3)
    elif dtype.startswith('float'):
        a=np.array([0.,-0.,np.nan,np.inf,-np.inf,-1.25],dtype=dt).reshape(2,3)
    else:
        info=np.iinfo(dt);a=np.array([0,info.min,info.max,1,2,3],dtype=dt).reshape(2,3)
    raw=a.tobytes()
    for coding,encode in [('base64',base64.b64encode),('base32',base64.b32encode),('base16',base64.b16encode)]:
        path=root/f'{dtype}-{coding}.xml'
        path.write_text(f'<XSDataImage><shape>3</shape><shape>2</shape><size>6</size><dtype>{dtype}</dtype><coding><value>{coding}</value></coding><data>{encode(raw).decode()}</data><md5sum><value>{hashlib.md5(raw).hexdigest()}</value></md5sum></XSDataImage>')
        ref=fabio.xsdimage.XsdImage().read(str(path)).data
        assert ref.dtype==a.dtype and ref.shape==a.shape and ref.tobytes()==raw,path
        manifest['files'][path.name]={'file_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'sample_sha256':hashlib.sha256(raw).hexdigest(),'dtype':dtype,'shape':list(a.shape),'fabio_compared':True}
(root/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
