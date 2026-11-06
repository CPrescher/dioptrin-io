"""CC0 small independent TIFF API fixtures, written with struct (no image writer)."""
import hashlib
import json
import pathlib
import struct

ROOT=pathlib.Path(__file__).resolve().parents[1]/"tests/data/tiff-api"


def tiff(pages, offsets, order="<"):
    count=15
    data=bytearray(max(offsets)+2+12*(count+1)+4)
    data[:8]=(b"II" if order=="<" else b"MM")+struct.pack(order+"HI",42,offsets[0])
    def payload(value):
        if len(data)%2:data.append(0)
        pos=len(data);data.extend(value);return pos
    for ordinal,(page,offset) in enumerate(zip(pages,offsets)):
        h,w,c=page["shape"]
        code=page["code"];bits=struct.calcsize(code)*8
        raw=struct.pack(order+str(len(page["values"]))+code,*page["values"])
        rps=page.get("rowsperstrip",h);stride=w*c*(bits//8)
        strips=[raw[start*stride:min(start+rps,h)*stride] for start in range(0,h,rps)]
        if page.get("packbits"):
            strips=[bytes([len(strip)-1])+strip for strip in strips]
        strip_offsets=[payload(strip) for strip in strips]
        tags={256:(4,[w]),257:(4,[h]),258:(3,[bits]*c),259:(3,[32773 if page.get("packbits") else 1]),
              262:(3,[page.get("photo",1 if c==1 else 2)]),270:(2,b"Title=independent\n\0"),273:(4,strip_offsets),
              274:(3,[6]),277:(3,[c]),278:(4,[rps]),279:(4,list(map(len,strips))),284:(3,[1]),
              305:(2,b"PyMca independent CC0\0"),306:(2,b"2026:11:06 00:00:00\0"),65000:(1,[0,1,255])}
        if code in "bhiqfd":tags[339]=(3,[3 if code in "fd" else 2])
        if "palette" in page:tags[320]=(3,page["palette"])
        if page.get("missing_counts"):del tags[279]
        assert len(tags)<=count+1
        struct.pack_into(order+"H",data,offset,len(tags))
        for index,(tag,(kind,value)) in enumerate(sorted(tags.items())):
            if kind==2:body=value;n=len(body)
            else:
                code={1:"B",3:"H",4:"I"}[kind];n=len(value);body=struct.pack(order+str(n)+code,*value)
            word=body.ljust(4,b"\0") if len(body)<=4 else struct.pack(order+"I",payload(body))
            struct.pack_into(order+"HHI4s",data,offset+2+index*12,tag,kind,n,word)
        struct.pack_into(order+"I",data,offset+2+len(tags)*12,offsets[ordinal+1] if ordinal+1<len(offsets) else 0)
    return bytes(data)


def fixtures():
    palette=[i*257 for i in range(256)]+[65535-i*257 for i in range(256)]+[0]*256
    return {
      "stack-nonmonotonic.tif":tiff([dict(shape=(3,2,1),code="H",values=[0,65535,32769,1,2,3]),dict(shape=(2,3,1),code="i",values=[-2147483648,2147483647,0,-1,17,-23])],[512,8],">"),
      "rgb8.tif":tiff([dict(shape=(3,2,3),code="B",values=[0,255,17,255,0,17,17,33,65,254,1,3,1,2,3,128,129,130])],[8]),
      "rgb-budget.tif":tiff([dict(shape=(16,16,3),code="B",values=list(range(256))*3)],[8]),
      "palette8.tif":tiff([dict(shape=(2,2,1),code="B",values=[0,1,255,2],photo=3,palette=palette)],[8]),
      "white.tif":tiff([dict(shape=(3,2,1),code="B",values=[0,1,255,128,17,254],photo=0)],[8]),
      "missing-bytecounts.tif":tiff([dict(shape=(3,2,1),code="B",values=[0,1,255,128,17,254],photo=0,missing_counts=True)],[8]),
      "strips.tif":tiff([dict(shape=(4,3,1),code="B",values=list(range(12)),rowsperstrip=1)],[8]),
      "packbits-one.tif":tiff([dict(shape=(2,3,1),code="B",values=list(range(6)),packbits=True)],[8]),
      "empty-ifd.tif":b"II*\0\0\0\0\0",
    }


def main():
    ROOT.mkdir(parents=True,exist_ok=True)
    for name,data in fixtures().items():(ROOT/name).write_bytes(data)
    (ROOT/"provenance.json").write_text(json.dumps({"license":"CC0-1.0","generator":"scripts/generate_tiff_api_fixtures.py",
        "files":{name:{"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()} for name,data in fixtures().items()}},indent=2)+"\n")


if __name__=="__main__":main()
