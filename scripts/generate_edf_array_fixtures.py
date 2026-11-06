"""CC0 independent EDF source-rank, codec and recovery fixtures."""
import bz2
import gzip
import hashlib
import json
import pathlib
import struct
import zlib
ROOT=pathlib.Path(__file__).resolve().parents[1]/"tests/data/edf-arrays"


def header(shape,datatype,raw_bytes,stored_bytes,codec="NONE",little=True,dimensions=None,extra=None):
    fields={"HeaderID":"EH:000001:000000:000000","Image":"0","DataType":datatype,"ByteOrder":"LowByteFirst" if little else "HighByteFirst"}
    fields.update({f"Dim_{i}":str(n) for i,n in enumerate(reversed(shape),1)} if dimensions is None else dimensions)
    fields.update(Size=str(stored_bytes),Compression=codec,SourceArrayBytes=str(raw_bytes))
    fields.update(extra or {})
    text=("{\n"+"\n".join(f"{key} = {value} ;" for key,value in fields.items())+"\n").encode()
    length=((len(text)+2+511)//512)*512
    return text+b" "*(length-len(text)-2)+b"}\n"


def record(shape,datatype,code,values,codec="NONE",little=True,dimensions=None):
    raw=struct.pack(("<" if little else ">")+str(len(values))+code,*values)
    encoded={"NONE":lambda x:x,"GZIP":lambda x:gzip.compress(x,mtime=0),"ZLIB":zlib.compress,"BZ2":bz2.compress}[codec](raw)
    return header(shape,datatype,len(raw),len(encoded),codec,little,dimensions)+encoded


def fixtures():
    u16=[0,65535,32769]+list(range(21))
    i64=[-(1<<63),(1<<63)-1,-1,0,(1<<53)+1,-((1<<53)+1)]+list(range(18))
    f64bits=[0,0x8000000000000000,0x7ff8000000000042,0x7ff0000000000000,0xfff0000000000000,0x3ff4000000000000]*2
    float_raw=struct.pack("<12Q",*f64bits);encoded=zlib.compress(float_raw)
    vol=record([2,3,4],"UnsignedShort","H",u16,little=False)
    i32=[-(1<<31),(1<<31)-1]+list(range(10))
    raw=struct.pack("<12H",*range(12));gz=gzip.compress(raw,mtime=0);corrupt=bytearray(gz);corrupt[-8]^=1
    common=header([2,3,4],"Unsigned64",192,192)
    partial=struct.pack("<5Q",0,(1<<64)-1,(1<<53)+1,1<<63,17)+b"\xfe\xff\x00"
    general=header([],"UnsignedShort",0,0,extra={"EDF_DataFormatVersion":"1.0","Dim_1":"3","Dim_2":"2","Dim_3":"2"})
    local=b"{\nImage = 0 ; Size = 24 ;\n";local=local+b" "*(510-len(local))+b"}\n"+raw
    return {
      "volume-u16-be.edf":vol,
      "rank4-gzip.edf":record([2,2,2,3],"Signed64","q",i64,"GZIP"),
      "rank5-zlib.edf":header([2,1,2,1,3],"DoubleValue",len(float_raw),len(encoded),"ZLIB")+encoded,
      "rank3-bzip.edf":record([2,2,3],"SignedInteger","i",i32,"BZ2",False),
      "rank1.edf":record([6],"UnsignedByte","B",[0,255,1,128,17,33]),
      "scalar.edf":record([],"DoubleValue","d",[-0.0]),
      "singleton-leading.edf":record([1,1,2,3],"UnsignedShort","H",range(6)),
      "sparse-dimensions.edf":record([2,1,1,3],"SignedShort","h",[-32768,32767,0,-1,17,-23],little=False,dimensions={"Dim_1":"3","Dim_4":"2"}),
      "mixed-records.edf":vol+record([2,2],"Signed64","q",[-(1<<63),(1<<63)-1,-1,17])+record([],"DoubleValue","d",[-0.0]),
      "general-volume.edf":general+local,
      "volume-truncated.edf":common+partial,
      "volume-header-only.edf":common,
      "volume-corrupt-member.edf":header([2,2,3],"UnsignedShort",24,len(corrupt),"GZIP")+corrupt,
      "volume-footer-short.edf":header([2,2,3],"UnsignedShort",24,len(gz)-1,"GZIP")+gz[:-1],
    }


def main():
    ROOT.mkdir(parents=True,exist_ok=True)
    for name,data in fixtures().items():(ROOT/name).write_bytes(data)
    (ROOT/"provenance.json").write_text(json.dumps({"license":"CC0-1.0","generator":"scripts/generate_edf_array_fixtures.py",
      "contract":"source image records preserve rank/axes; explicit plane views are not extra records",
      "files":{name:{"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()} for name,data in fixtures().items()}},indent=2)+"\n")


if __name__=="__main__":main()
