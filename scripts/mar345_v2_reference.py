"""MAR V2 original seven-bit default and explicit FabIO eight-bit compatibility proof.

The original JPA encoder/decoder and pinned legacy C decoder are compiled outside Git and used only in guarded
subprocesses on admitted small streams. Production reuses its prior MIT decoder;
no LGPL expression translation, linked decoder or Python dependency is added.
"""
import argparse
import hashlib
import importlib
import inspect
import json
import pathlib
import re
import resource
import struct
import subprocess
import sys
import tempfile

from generate_mar345_v2_fixtures import fixtures, prediction, source
from generate_mar345_original_v2_fixtures import fixtures as original_fixtures

ROOT=pathlib.Path(__file__).resolve().parents[1]
PIN="9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece"
WIDTHS=[0,*range(4,17),32]
C_WRAPPER=r'''
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <limits.h>
#include <sys/resource.h>
#include "ccp4_pack.h"
int main(int argc,char**argv){
 if(argc!=5 || sizeof(unsigned int)!=4 || sizeof(int)!=4 || CHAR_BIT!=8)return 2;
 struct rlimit mem={268435456,268435456};int applied=setrlimit(RLIMIT_AS,&mem);int memory_errno=applied?errno:0;
 size_t offset=strtoul(argv[2],NULL,10),width=strtoul(argv[3],NULL,10),height=strtoul(argv[4],NULL,10),count=width*height;
 if(width<2||height<2||width>8192||height>8192||count>32768)return 3;
 FILE*f=fopen(argv[1],"rb");if(!f)return 4;fseek(f,0,SEEK_END);long n=ftell(f);
 if(n<0||n>1048576||offset>=(size_t)n)return 5;
 size_t bytes=(size_t)n-offset;unsigned char*input=calloc(bytes+1,1);unsigned int*output=calloc(count+16,sizeof(unsigned int));
 if(!input||!output)return 6;fseek(f,(long)offset,SEEK_SET);if(fread(input,1,bytes,f)!=bytes)return 7;fclose(f);
 for(size_t i=count;i<count+16;i++)output[i]=0x7badcafe;
 ccp4_unpack_v2_string(output,input,width,height,count);
 for(size_t i=count;i<count+16;i++)if(output[i]!=0x7badcafe)return 8;
 printf("{\"status\":\"decoded\",\"abi\":{\"int_bytes\":%zu,\"uint_bytes\":%zu,\"char_bits\":%d},\"memory_limit\":{\"attempted_bytes\":268435456,\"applied\":%s,\"errno\":%d},\"output_guard_intact\":true,\"values\":[",sizeof(int),sizeof(unsigned int),CHAR_BIT,applied?"false":"true",memory_errno);
 for(size_t i=0;i<count;i++){if(i)putchar(',');printf("%u",output[i]);}puts("]}");free(input);free(output);return 0;
}
'''


def sha(data):return hashlib.sha256(data).hexdigest()


def fingerprint(path):
    path=pathlib.Path(path);return {"path":str(path.resolve()),"bytes":path.stat().st_size,"sha256":sha(path.read_bytes())}


def canonical(values):return struct.pack("<"+str(len(values))+"I",*values)


def delta(actual,expected):
    def same(a,b):
        if type(a) is not type(b):return False
        if isinstance(a,list):return len(a)==len(b) and all(same(x,y) for x,y in zip(a,b))
        if isinstance(a,dict):return a.keys()==b.keys() and all(same(a[k],b[k]) for k in a)
        return a==b
    return [{"field":k,"reference":v,"native":actual.get(k)} for k,v in expected.items() if k not in actual or not same(actual[k],v)]


def parse(data,dialect="fabio8"):
    """Independent admission for known small complete streams, before unsafe C."""
    if not 4096<len(data)<=1_048_576:raise ValueError("oracle source bound")
    if data[:4]==struct.pack("<i",1234):order="<"
    elif data[:4]==struct.pack(">i",1234):order=">"
    else:raise ValueError("invalid source endian marker")
    widths=WIDTHS if dialect=="fabio8" else [0,*range(3,17),32]
    run_bits=4 if dialect=="fabio8" else 3
    words=list(struct.unpack_from(order+"16i",data));width,high,count=words[1],words[2],words[5]
    if not 2<=width<=8192 or not 0<=high<=count or count%width or not 2<=count//width<=8192 or not 4<=count<=32768:raise ValueError("oracle image bound")
    if words[3]!=1 or words[4] not in (0,1):raise ValueError("oracle source storage/mode")
    compact=high*8;padded=((high+7)//8)*64;candidates=[]
    for size in sorted({compact,padded}):
        offset=4096+size
        match=re.match(rb"\r?\nCCP4 packed image V2, X: ([0-9]+), Y: ([0-9]+)\r?\n",data[offset:])
        if match:candidates.append((size,match))
    if len(candidates)!=1:raise ValueError("oracle marker/table ambiguity")
    table_bytes,match=candidates[0]
    if [int(match[2]),int(match[1])]!=[count//width,width]:raise ValueError("oracle shape contradiction")
    overflow=[];seen=set()
    for at in range(4096,4096+compact,8):
        index,value=struct.unpack_from(order+"II",data,at)
        if not 1<=index<=count or index in seen:raise ValueError("oracle invalid/duplicate overflow")
        seen.add(index);overflow.append({"one_based_index":index,"value":value})
    if any(data[4096+compact:4096+table_bytes]):raise ValueError("oracle nonzero table padding")
    offset=4096+table_bytes+match.end();encoded=data[offset:];bits=0;position=0;integer=int.from_bytes(encoded,"little");signed=[];selectors=[];runs=[]
    def take(n):
        nonlocal bits
        if bits+n>len(encoded)*8:raise ValueError("oracle truncated block")
        value=(integer>>bits)&((1<<n)-1);bits+=n;return value
    while position<count:
        exponent=take(run_bits);selector=take(4)
        if selector>=len(widths):raise ValueError("oracle reserved selector15")
        run=1<<exponent;size=widths[selector]
        if run>count-position:raise ValueError("oracle block overrun")
        selectors.append(selector);runs.append(exponent)
        for _ in range(run):
            raw=take(size);signed.append(raw-(1<<size) if size and raw&(1<<(size-1)) else raw)
        position+=run
    if (bits+7)//8!=len(encoded) or integer>>bits:raise ValueError("oracle unexplained tail/padding")
    start=data.find(b"mar research",64,256)
    if start<0 or b"END OF HEADER" not in data[start:4096]:raise ValueError("oracle source text header")
    entries=[];fields={}
    for line in data[start:4096].splitlines():
        line=line.strip(b" \t\r\n\0")
        if line==b"END OF HEADER":break
        if not line or line==b"mar research":continue
        pair=line.decode("latin1").split(None,1);key=pair[0];value=pair[1].strip() if len(pair)>1 else ""
        entries.append([key,value]);fields[key]=value
    metadata={"shape":[count//width,width],"pixel_type":"U32","base_pixel_type":"U16","little_endian":order=="<","pck_version":2,"pck_v2_dialect":"FabioLegacyEightBit" if dialect=="fabio8" else "Original","overflow_layout":"PaddedEight" if table_bytes==padded else "Compact","header_hex":data[:4096].hex(),"binary_words":words,"text_start":start,"fields":fields,"entries":entries,"overflow_records":overflow,"overflow_bytes_hex":data[4096:4096+table_bytes].hex(),"marker_hex":data[4096+table_bytes:offset].hex(),"encoded_bytes":len(encoded)}
    return {"metadata":metadata,"offset":offset,"encoded":encoded,"differences":signed,"selectors":selectors,"run_exponents":runs,"bits":bits}


def default_safe(record):
    """Validate every V1 memory access before calling the unchanged default.

    This is an unsafe-oracle admission gate, never native format behavior.
    The reference default trims binary whitespace and then uses V1 framing.
    """
    data=record["encoded"].lstrip();integer=int.from_bytes(data,"little");bit=0;count=record["metadata"]["binary_words"][5];position=0
    while bit//8<len(data) and position<count:
        if bit+6>len(data)*8:return False,"V1 default would read an incomplete header"
        header=(integer>>bit)&63;bit+=6;run=1<<(header&7);width=[0,4,5,6,7,8,16,32][header>>3]
        if width and run>count-position:return False,"V1 default would write beyond declared output"
        if bit+run*width>len(data)*8:return False,"V1 default would read beyond stored payload"
        bit+=run*width;position+=run
    return True,"all V1 default memory accesses prevalidated; returned counts may still be wrong/incomplete"


def child_limits():
    resource.setrlimit(resource.RLIMIT_CPU,(8,8));resource.setrlimit(resource.RLIMIT_FSIZE,(2*1024*1024,2*1024*1024));resource.setrlimit(resource.RLIMIT_CORE,(0,0))


def build_oracle(source,directory):
    files=[source/"src/fabio/ext/src/ccp4_pack.c",source/"src/fabio/ext/include/ccp4_pack.h"]
    for path in files:
        pinned=subprocess.check_output(["git","-C",str(source),"show",f"{PIN}:{path.relative_to(source)}"])
        if path.read_bytes()!=pinned:raise ValueError("LGPL test oracle differs from pinned source")
    wrapper=directory/"oracle.c";wrapper.write_text(C_WRAPPER);binary=directory/"oracle"
    command=["cc","-std=c99","-O0","-I",str(files[1].parent),str(wrapper),str(files[0]),"-o",str(binary)]
    result=subprocess.run(command,capture_output=True,text=True,timeout=30,check=True)
    return binary,{"sources":[fingerprint(p) for p in files],"wrapper":fingerprint(wrapper),"binary":fingerprint(binary),"command":command,"compiler":subprocess.check_output(["cc","--version"],text=True),"stderr":result.stderr,"license":"LGPL-3.0-or-later; untouched pinned ESRF/Henning Sorensen/Erik Knudsen/CCP4 source, external test-only executable; no source expressions or runtime code incorporated","bounds":"strict source/table/shape/block/selector/tail prevalidation; <=1MiB file, <=32768 samples; CPU8s/wall20s/fileoutput2MiB/core0; AS256MiB attempted after exec with actual OS outcome recorded; one zero input prefetch sentinel outside physical stream and16 output guard words; sentinel is never interpreted as a source field"}


def c_oracle(binary,path,record):
    width=record["metadata"]["shape"][1];height=record["metadata"]["shape"][0]
    # Caller must always provide fresh admission; never trust supplied offsets.
    if parse(path.read_bytes())!=record:raise ValueError("source/admission changed before C oracle")
    result=subprocess.run([str(binary),str(path),str(record["offset"]),str(width),str(height)],capture_output=True,text=True,timeout=20,preexec_fn=child_limits)
    if result.returncode:raise ValueError(f"isolated LGPL oracle failed:{result.returncode}:{result.stderr}")
    value=json.loads(result.stdout)
    if len(value["values"])!=width*height or not value["output_guard_intact"]:raise ValueError("oracle output bound/guard failure")
    values=value.pop("values")
    for overflow in record["metadata"]["overflow_records"]:values[overflow["one_based_index"]-1]=overflow["value"]
    value.update(dtype="uint32",shape=[height,width],sample_sha256=sha(canonical(values)))
    return value,values


def native(probe,path,dialect="fabio8"):
    result=subprocess.run([str(probe),"mar345-layout",str(path),dialect],capture_output=True,text=True,timeout=30)
    if result.returncode:raise ValueError(f"native failed:{result.returncode}:{result.stderr}")
    return json.loads(result.stdout)


def compare_native(actual,record,values):
    raw=canonical(values);metadata=record["metadata"]
    return delta(actual,{"frames":1,"shape":metadata["shape"],"pixel_type":"U32","pixels":{"dtype":"uint32","sample_sha256":sha(raw)},"value_hex":raw.hex(),"metadata":metadata})


def qualify_oracle(record,values,reference):
    if values==reference:return {"classification":"exact_original_words","differences":[]}
    # The old C decoder uses unsigned four-neighbor accumulation/division.
    # A negative signed sum with remainder floors instead of C signed truncation.
    # Verify that exact known arithmetic defect accounts for every differing word;
    # no unexplained oracle disagreement is accepted as success.
    width=record["metadata"]["shape"][1];legacy=[]
    for index,difference in enumerate(record["differences"]):
        if index==0:prediction=0
        elif index<=width:prediction=legacy[index-1]
        else:
            neighbors=[legacy[i] for i in [index-1,index-width-1,index-width,index-width+1]]
            signed=[(v+32768)%65536-32768 for v in neighbors]
            prediction=((sum(signed)+2)&0xffffffff)//4
        legacy.append((difference+prediction)%65536)
    for overflow in record["metadata"]["overflow_records"]:legacy[overflow["one_based_index"]-1]=overflow["value"]
    if legacy!=reference:raise ValueError("LGPL oracle has an unexplained count disagreement")
    return {"classification":"qualified_legacy_unsigned_predictor_division","differing_words":sum(a!=b for a,b in zip(values,reference)),"evidence":"legacy C decoder stores signed16 neighbors into unsigned int then divides unsigned sum; this floors negative nonmultiples, whereas existing MIT source predictor uses signed division toward zero; independently specified source words and encoded residuals match native exactly"}


def default_child(path):
    import fabio
    reader=fabio.open(str(path))
    try:
        array=reader.data
        raw=array.astype("<u4").tobytes()
        return {"status":"decoded","shape":list(array.shape),"dtype":array.dtype.name,"sample_sha256":sha(raw)}
    finally:reader.close()


def original_default(path,record):
    safe,reason=default_safe(record)
    if not safe:return {"status":"not_called_unsafe_v1_interpretation","reason":reason}
    result=subprocess.run([sys.executable,str(pathlib.Path(__file__).resolve()),"--default-child",str(path)],capture_output=True,text=True,timeout=20,preexec_fn=child_limits)
    if result.returncode:return {"status":"error" if result.returncode>0 else "isolated_crash","returncode":result.returncode,"stderr":result.stderr}
    return json.loads(result.stdout)


def qualify_default(value,record,words,path):
    safe,reason=default_safe(record)
    if value.get("status")=="not_called_unsafe_v1_interpretation":
        if safe:raise ValueError("unchanged default skipped without unsafe-access evidence")
        return "not_called_unsafe_v1_interpretation"
    if not safe:raise ValueError("unsafe unchanged default was called")
    if value.get("status")=="decoded":
        if value.get("shape")!=record["metadata"]["shape"] or value.get("dtype")!="uint32":raise ValueError("unexplained default source shape/dtype difference")
        if value.get("sample_sha256")==sha(canonical(words)):
            if any(words):raise ValueError("nonzero V2 default equality requires independent review")
            return "coincidental_all_zero_subset_match"
        return "default_V1_decoder_misreads_V2_stream"
    if value.get("status")=="error" and not record["metadata"]["little_endian"] and value.get("type")=="OSError" and value.get("error")==f"Fabio could not identify {path}":
        return "pinned_big_endian_detection_error"
    raise ValueError("unexplained default error/crash is not a qualified format defect")


def compact(value):
    if isinstance(value,list):return [compact(v) for v in value]
    if isinstance(value,dict):
        out={k:compact(v) for k,v in value.items() if k!="value_hex"}
        if "value_hex" in value:out.update(value_bytes=len(bytes.fromhex(value["value_hex"])),value_sha256=sha(bytes.fromhex(value["value_hex"])))
        return out
    return value


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--probe",type=pathlib.Path,required=True);parser.add_argument("--fabio-source",type=pathlib.Path,required=True);parser.add_argument("--corpus",type=pathlib.Path);parser.add_argument("--ccp4-source",type=pathlib.Path,required=True);parser.add_argument("--report",type=pathlib.Path,required=True);args=parser.parse_args()
    import fabio
    if fabio.version!="2026.6.0" or subprocess.check_output(["git","-C",str(args.fabio_source),"rev-parse","HEAD"],text=True).strip()!=PIN:parser.error("requires pinned source/environment")
    module=importlib.import_module("fabio.mar345image");extension=importlib.import_module("fabio.ext.mar345_IO");imports=[]
    for imported,relative in [(module,"mar345image.py")]:
        actual=pathlib.Path(inspect.getfile(imported));pinned=args.fabio_source/"src/fabio"/relative;original=subprocess.check_output(["git","-C",str(args.fabio_source),"show",f"{PIN}:src/fabio/{relative}"])
        imports.append({"actual":fingerprint(actual),"pinned":fingerprint(pinned),"matches_pinned":actual.read_bytes()==original==pinned.read_bytes()})
    tracked=[ROOT/"Cargo.toml",ROOT/"Cargo.lock",ROOT/"build.rs",ROOT/"examples/api_probe.rs",ROOT/"scripts/mar345_v2_reference.py",ROOT/"scripts/test_mar345_v2_reference.py",ROOT/"scripts/generate_mar345_v2_fixtures.py",ROOT/"tests/mar345.rs",ROOT/"tests/mar345_v2.rs",pathlib.Path(extension.__file__)]
    tracked+=sorted((ROOT/"src").rglob("*.rs"))+sorted((ROOT/"tests/data/mar345-v2").glob("*"))
    tracked += [args.fabio_source/"src/fabio/ext/mar345_IO.pyx",args.fabio_source/"src/fabio/ext/src/ccp4_pack.c",args.fabio_source/"src/fabio/ext/include/ccp4_pack.h"]
    tracked += [pathlib.Path(i[k]["path"]) for i in imports for k in ("actual","pinned")]
    primary=[]
    for relative in ["src/fabio/ext/mar345_IO.pyx","src/fabio/ext/src/ccp4_pack.c","src/fabio/ext/include/ccp4_pack.h"]:
        path=args.fabio_source/relative;pinned=subprocess.check_output(["git","-C",str(args.fabio_source),"show",f"{PIN}:{relative}"])
        primary.append({**fingerprint(path),"unchanged_git":path.read_bytes()==pinned})
    tracked += [ROOT/"scripts/generate_mar345_original_v2_fixtures.py",ROOT/"scripts/mar345_original_v2_oracle.py"]+sorted((ROOT/"tests/data/mar345-v2-original").glob("*"))
    tracked += [args.ccp4_source/name for name in ("pack_c.c","pack_c.h","libccp4-8.0.0.tar.gz")]
    before={str(p):fingerprint(p) for p in tracked};probe_before=fingerprint(args.probe);cases=[];errors=[]
    from mar345_original_v2_oracle import build as build_original,decode as original_decode,authored_cases,roundtrip
    originals=[];writer_roundtrips=[]
    with tempfile.TemporaryDirectory(prefix="dioptrin-marv2-oracle-") as directory:
        original_binary,original_oracle=build_original(args.ccp4_source,pathlib.Path(directory))
        for name,(payload,values,provenance) in original_fixtures().items():
            path=ROOT/"tests/data/mar345-v2-original"/name;snapshot=fingerprint(path)
            try:
                if path.read_bytes()!=payload:raise ValueError("independent original source differs")
                record=parse(payload,"original");actual=native(args.probe,path,"original");diff=compare_native(actual,record,values)
                oracle_result,oracle_values=original_decode(original_binary,path,record)
                if oracle_values!=values:raise ValueError("original V2 decoder differs from independent authored words")
                default=original_default(path,record);qualification=qualify_default(default,record,values,path)
                originals.append({"source":snapshot,"native":compact(actual),"reference":{"shape":record["metadata"]["shape"],"dtype":"uint32","sample_sha256":sha(canonical(values))},"original_oracle":oracle_result,"unchanged_fabio_default":default,"default_qualification":qualification,"selectors":sorted(set(record["selectors"])),"run_exponents":sorted(set(record["run_exponents"])),"differences":diff})
                if snapshot!=fingerprint(path):raise ValueError("original source changed")
            except Exception as error:errors.append({"path":str(path),"type":type(error).__name__,"error":str(error)})
        for name,shape,values in authored_cases():
            try:
                header=source(values,shape,dialect="original")[0][:4096]
                path,record,receipt=roundtrip(original_binary,pathlib.Path(directory),name,shape,values,header)
                actual=native(args.probe,path,"original");diff=compare_native(actual,record,values)
                writer_roundtrips.append({"name":name,"receipt":receipt,"native":compact(actual),"reference":{"shape":shape,"dtype":"uint32","sample_sha256":sha(canonical(values))},"differences":diff})
            except Exception as error:errors.append({"scope":"original_writer_roundtrip","case":name,"type":type(error).__name__,"error":str(error)})
        original_oracle["binary_after"]=fingerprint(original_binary)
        if original_oracle["binary_after"]!=original_oracle["binary"]:errors.append({"scope":"original_oracle","error":"compiled binary changed"})
        binary,oracle=build_oracle(args.fabio_source,pathlib.Path(directory))
        for name,(payload,values,provenance) in fixtures().items():
            path=ROOT/"tests/data/mar345-v2"/name;snapshot=fingerprint(path)
            try:
                if path.read_bytes()!=payload:raise ValueError("independent generated source differs")
                record=parse(payload);actual=native(args.probe,path);differences=compare_native(actual,record,values)
                original,oldvalues=c_oracle(binary,path,record);qualification=qualify_oracle(record,values,oldvalues)
                default=original_default(path,record)
                default_qualification=qualify_default(default,record,values,path)
                cases.append({"source":snapshot,"native":compact(actual),"reference":{"shape":record["metadata"]["shape"],"dtype":"uint32","sample_sha256":sha(canonical(values))},"oracle":original,"oracle_qualification":qualification,"unchanged_fabio_default":default,"default_qualification":default_qualification,"default_equal":default.get("sample_sha256")==sha(canonical(values)),"selectors":sorted(set(record["selectors"])),"run_exponents":sorted(set(record["run_exponents"])),"differences":differences})
                if snapshot!=fingerprint(path):raise ValueError("source changed")
            except Exception as error:errors.append({"path":str(path),"type":type(error).__name__,"error":str(error)})
    actual_v1=[]
    if args.corpus:
        for name in ["example.mar2300","Fe3O4_023_101.mar2560"]:
            path=args.corpus/name;snapshot=fingerprint(path)
            try:
                actual=native(args.probe,path,"original");reader=fabio.open(str(path))
                try:array=reader.data;reference={"frames":reader.nframes,"shape":list(array.shape),"pixel_type":"U32","pixels":{"dtype":array.dtype.name,"sample_sha256":sha(array.astype("<u4").tobytes())}}
                finally:reader.close()
                if actual["metadata"]["pck_version"]!=1:raise ValueError("existing corpus source is not V1")
                actual_v1.append({"source":snapshot,"native":compact(actual),"reference":reference,"differences":delta(actual,reference)})
                if snapshot!=fingerprint(path):raise ValueError("source changed")
            except Exception as error:errors.append({"path":str(path),"type":type(error).__name__,"error":str(error)})
    after={str(p):fingerprint(p) for p in tracked};probe_after=fingerprint(args.probe)
    summary={"original_v2_sources":len(originals),"original_v2_differences":sum(len(c["differences"]) for c in originals),"original_selectors":sorted({v for c in originals for v in c["selectors"]}),"original_run_exponents":sorted({v for c in originals for v in c["run_exponents"]}),"original_writer_roundtrips":len(writer_roundtrips),"original_writer_differences":sum(len(c["differences"]) for c in writer_roundtrips),"original_default_equal":sum(c["default_qualification"]=="coincidental_all_zero_subset_match" for c in originals),"original_default_wrong_words":sum(c["default_qualification"]=="default_V1_decoder_misreads_V2_stream" for c in originals),"original_default_unsafe_not_called":sum(c["default_qualification"]=="not_called_unsafe_v1_interpretation" for c in originals),"independent_v2_sources":len(cases),"v2_native_differences":sum(len(c["differences"]) for c in cases),"oracle_exact":sum(c["oracle_qualification"]["classification"]=="exact_original_words" for c in cases),"oracle_qualified_unsigned_predictor":sum(c["oracle_qualification"]["classification"]=="qualified_legacy_unsigned_predictor_division" for c in cases),"default_called":sum(c["unchanged_fabio_default"]["status"]!="not_called_unsafe_v1_interpretation" for c in cases),"default_equal":sum(c["default_equal"] for c in cases),"default_wrong_words":sum(c["default_qualification"]=="default_V1_decoder_misreads_V2_stream" for c in cases),"default_big_endian_errors":sum(c["default_qualification"]=="pinned_big_endian_detection_error" for c in cases),"default_unsafe_not_called":sum(c["default_qualification"]=="not_called_unsafe_v1_interpretation" for c in cases),"selectors":sorted({v for c in cases for v in c["selectors"]}),"run_exponents":sorted({v for c in cases for v in c["run_exponents"]}),"actual_v1_sources":len(actual_v1),"actual_v1_differences":sum(len(c["differences"]) for c in actual_v1),"errors":len(errors),"sources_stable":before==after,"probe_stable":probe_before==probe_after,"imports_unchanged":all(i["matches_pinned"] for i in imports),"primary_unchanged":all(p["unchanged_git"] for p in primary)}
    passed=gate(summary,bool(args.corpus))
    report={"schema":1,"passed":passed,"scope":"Original JPA MAR V2 seven-bit default, independently proven by unchanged original encoder/decoder; explicitly selected incompatible FabIO legacy eight-bit framing; source U32 words and metadata verified separately for each contract","pin":PIN,"summary":summary,"imports":imports,"primary_sources":primary,"original_oracle":original_oracle,"oracle":oracle,"probe_before":probe_before,"probe_after":probe_after,"sources_before":before,"sources_after":after,"original_v2_cases":originals,"original_writer_roundtrips":writer_roundtrips,"fabio_legacy8_cases":cases,"actual_v1":actual_v1,"errors":errors,"limitations":["No genuine acquired V2 sample exists in the pinned corpus. Original V2 is validated with independently authored words, unchanged official original decoder, and unchanged original encoder/decoder roundtrips; FabIO eight-bit framing requires an explicit separate caller policy.","Spiral/raw/old IMAGE and predictor dimensions below2 remain unsupported.","Pinned default selects V1 decoder even for V2 markers; safe unchanged calls are recorded, unsafe V1 reinterpretations are refused by test-only admission rather than risking a known out-of-bounds read/write.","LGPL decoder is external test-only source unchanged; production contains no LGPL code or decoder dependency."]}
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2)+"\n");print(json.dumps({"passed":passed,"summary":summary,"report":str(args.report)}));return 0 if passed else 1


def gate(summary,corpus):
    wanted={"original_v2_sources":32,"original_v2_differences":0,"original_selectors":list(range(16)),"original_run_exponents":list(range(8)),"original_writer_roundtrips":5,"original_writer_differences":0,"original_default_equal":10,"original_default_wrong_words":8,"original_default_unsafe_not_called":14,"independent_v2_sources":39,"v2_native_differences":0,"selectors":list(range(15)),"run_exponents":list(range(16)),"actual_v1_sources":2 if corpus else 0,"actual_v1_differences":0,"errors":0,"sources_stable":True,"probe_stable":True,"imports_unchanged":True,"primary_unchanged":True,"oracle_exact":37,"oracle_qualified_unsigned_predictor":2,"default_called":21,"default_equal":14,"default_wrong_words":5,"default_big_endian_errors":2,"default_unsafe_not_called":18}
    return not delta(summary,wanted)



if __name__=="__main__":
    if len(sys.argv)==3 and sys.argv[1]=="--default-child":
        try:print(json.dumps(default_child(pathlib.Path(sys.argv[2]))))
        except Exception as error:print(json.dumps({"status":"error","type":type(error).__name__,"error":str(error)}))
    else:raise SystemExit(main())
