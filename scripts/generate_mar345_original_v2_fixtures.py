"""CC0 original seven-bit MAR/PCK V2 source words and manual bit packing."""
import hashlib,json,pathlib
from generate_mar345_v2_fixtures import source,pack,prediction
ROOT=pathlib.Path(__file__).resolve().parents[1]/"tests/data/mar345-v2-original"
WIDTHS=[0,*range(3,17),32]


def fixtures():
    result={}
    for width in WIDTHS:
        values=[0]*32 if not width else [i%3 for i in range(32)]
        result[f"original-width{width}.mar345"]=source(values,[4,8],forced=width,dialect="original")
    for power in range(8):
        count=max(4,1<<power);shape=[count//8,8] if count>=16 else [2,count//2]
        result[f"original-zero-run{1<<power}.mar345"]=source([0]*count,shape,run=1<<power,dialect="original")
    values=[(i*31)%32768 for i in range(32)]
    for little,name in [(True,"le"),(False,"be")]:
        counts=values.copy()
        for index,value in [(0,65536),(7,999999),(16,2147483648),(31,4294967295)]:counts[index]=value
        for compact in [False,True]:result[f"original-overflow-{'compact' if compact else 'padded'}-{name}.mar345"]=source(counts,[4,8],little=little,compact=compact,extended=compact,dialect="original")
    values=[32768+i for i in range(9)]+[65535,32768,1,0,65534,17,32767,65535,0,1,65535,32768,32769,2,3]
    result["original-signed-neighborhood.mar345"]=source(values,[4,6],dialect="original")
    result["original-marker-crlf.mar345"]=source([i%3 for i in range(32)],[4,8],crlf=True,dialect="original")
    # Seven-bit zero-run headers leave real unused bits in the final source byte.
    result["original-partial-last-byte.mar345"]=source([0]*4,[2,2],run=4,dialect="original")
    differences=[-(1<<31),(1<<31)-1,-1,0,1,65535,-65536,32768]*4;values=[]
    for index,difference in enumerate(differences):values.append((difference+prediction(values,index,8))%65536)
    result["original-residual32-extrema.mar345"]=source(values,[4,8],encoded=pack(differences,forced=32,dialect="original"),dialect="original")
    return result


def main():
    ROOT.mkdir(parents=True,exist_ok=True)
    manifest={"license":"CC0-1.0","generator":"scripts/generate_mar345_original_v2_fixtures.py","contract":"Original JPA V2 seven-bit block alphabet and signed16 source predictor; independently authored words/packing, separately checked by unchanged original writer/reader","files":{}}
    for name,(data,values,record) in fixtures().items():(ROOT/name).write_bytes(data);manifest["files"][name]=record
    (ROOT/"provenance.json").write_text(json.dumps(manifest,indent=2)+"\n")


if __name__=="__main__":main()
