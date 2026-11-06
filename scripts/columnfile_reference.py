"""Independent column-table audit: original C decoder, never a repaired cf_io.

FabIO MIT C source is compiled unchanged outside the repository. Its unsafe
decoder receives only prevalidated tiny ASCII cases in resource-limited children.
No C runtime/dependency is incorporated into dioptrin-io. Binary cf_read_bin is
an explicit unimplemented stub, not evidence of malformed binary input.
"""
import argparse
import hashlib
import importlib
import inspect
import json
import math
import os
import pathlib
import re
import resource
import struct
import subprocess
import sys
import tempfile

from generate_columnfile_fixtures import fixtures, projection

PIN = "9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece"
ROOT = pathlib.Path(__file__).resolve().parents[1]
WRAPPER = r'''
#include <stdint.h>
#include <inttypes.h>
#include <float.h>
#include <locale.h>
#include <sys/resource.h>
#include <errno.h>
#include "columnfile.h"
int main(int argc, char **argv) {
 if(argc!=3 || sizeof(double)!=8 || sizeof(uint64_t)!=8 || FLT_RADIX!=2 || DBL_MANT_DIG!=53 || DBL_MAX_EXP!=1024) return 2;
 struct rlimit bound={256*1024*1024,256*1024*1024};int memory_result=setrlimit(RLIMIT_AS,&bound);int memory_errno=memory_result?errno:0;
 setlocale(LC_NUMERIC,"C");
 FILE *f=fopen(argv[2],"rb"); if(!f) return 3;
 cf_data *v=argv[1][0]=='b'?cf_read_bin(f,NULL,0):cf_read_ascii(f,NULL,0);
 fclose(f); uint16_t endian=1;
 printf("{\"memory_limit\":{\"attempted_bytes\":268435456,\"applied\":%s,\"errno\":%d},\"abi\":{\"double_bytes\":%zu,\"int_bytes\":%zu,\"pointer_bytes\":%zu,\"endian\":\"%s\",\"ieee754_binary64\":true},",memory_result?"false":"true",memory_errno,sizeof(double),sizeof(int),sizeof(void*),*(unsigned char*)&endian?"little":"big");
 if(!v) { printf("\"status\":\"no_decoder_result\"}\n"); return 0; }
 if(v->ncols<1 || v->ncols>31 || v->nrows<0 || v->nrows>64) return 4;
 printf("\"status\":\"decoded\",\"shape\":[%d,%d],\"labels_hex\":[",v->nrows,v->ncols);
 for(int i=0;i<v->ncols;i++) {if(i)printf(",");printf("\""); for(unsigned char*p=(unsigned char*)v->clabels[i];*p;p++)printf("%02x",*p);printf("\"");}
 printf("],\"value_bits\":[");
 for(int r=0;r<v->nrows;r++)for(int c=0;c<v->ncols;c++){uint64_t bits;memcpy(&bits,&v->data[r][c],8);if(r||c)printf(",");printf("\"%016" PRIx64 "\"",bits);}
 printf("]}\n");cf_free(v);return 0;
}
'''


def fingerprint(path):
    path=pathlib.Path(path)
    return {"path":str(path.resolve()),"bytes":path.stat().st_size,"sha256":hashlib.sha256(path.read_bytes()).hexdigest()}


def admit_c_ascii(data):
    """Avoid every identified unsafe C branch; not general format validation."""
    if not data or len(data)>65536 or any(b<32 and b not in (9,10,13) for b in data): raise ValueError("unsafe C source size/control byte")
    lines=data.splitlines(keepends=True)
    if not lines or not lines[0].startswith(b"#") or any(len(line)>2047 for line in lines): raise ValueError("unsafe C header/line window")
    labels=lines[0][1:].split()
    if not 1<=len(labels)<=31 or any(len(label)>127 for label in labels): raise ValueError("unsafe C label count/length")
    if len(lines)>65: raise ValueError("C oracle row guard")
    for line in lines[1:]:
        tokens=line.split()
        if len(tokens)!=len(labels) or line.lstrip().startswith(b"#"): raise ValueError("unsafe C row/comment")
        for token in tokens:
            token.decode("ascii")
            float(token)
    return {"columns":len(labels),"rows":len(lines)-1,"final_newline":data.endswith(b"\n")}


def child_limits():
    resource.setrlimit(resource.RLIMIT_CPU,(2,2))
    resource.setrlimit(resource.RLIMIT_FSIZE,(1024*1024,1024*1024))
    resource.setrlimit(resource.RLIMIT_CORE,(0,0))


def build_c(source,directory):
    source=pathlib.Path(source);directory=pathlib.Path(directory)
    original=source/"src/fabio/ext/src/columnfile.c"
    header=source/"src/fabio/ext/include/columnfile.h"
    for path in (original,header):
        relative=str(path.relative_to(source))
        pinned=subprocess.check_output(["git","-C",str(source),"show",f"{PIN}:{relative}"])
        if path.read_bytes()!=pinned: raise ValueError("C oracle source differs from pinned git object")
    wrapper=directory/"oracle.c";wrapper.write_text(WRAPPER)
    before=[fingerprint(path) for path in (original,header,source/"copyright")]
    executable=directory/"oracle"
    command=["cc","-std=c99","-O0","-I",str(header.parent),str(wrapper),str(original),"-o",str(executable)]
    compile_result=subprocess.run(command,capture_output=True,text=True,check=True,timeout=30)
    if before != [fingerprint(path) for path in (original,header,source/"copyright")]: raise ValueError("C oracle source changed during compilation")
    return executable,{"sources":before,
                       "wrapper":fingerprint(wrapper),"binary":fingerprint(executable),"command":command,
                       "compiler":subprocess.check_output(["cc","--version"],text=True),"compiler_stderr":compile_result.stderr,
                       "license":"MIT; FabIO authors/ESRF; pinned copyright manifest retained by hash",
                       "safety":"prevalidated <=31 labels, <=127 bytes/label, <=64 exact rows, <=2047 bytes/line, <=64KiB source; admitted C allocation requests total <3MiB (8192 rows, no growth); child CPU2s/fileoutput1MiB/core0/wall5s; AS256MiB attempted after exec and actual OS result recorded, not assumed"}


def c_oracle(executable,path,mode="a"):
    if mode=="a": admit_c_ascii(pathlib.Path(path).read_bytes())
    process=subprocess.run([str(executable),mode,str(path)],capture_output=True,text=True,timeout=5,
                           preexec_fn=child_limits,check=False)
    if process.returncode: raise RuntimeError(f"isolated C oracle failed: {process.returncode}: {process.stderr}")
    return json.loads(process.stdout)


def expected(data):
    result=projection(data)
    exact=[]
    for token_hex,bits in zip(result["lexemes_hex"],result["value_bits"]):
        token=bytes.fromhex(token_hex)
        value=struct.unpack(">d",bytes.fromhex(bits))[0]
        exact.append((math.isfinite(value) and int(value)==int(token)) if re.fullmatch(rb"[+-]?[0-9]+",token) else None)
    return {**result,"kind":"columnfile","representation":"DecodedFloat64View","integer_exactness":exact}


def differences(actual,wanted):
    def same(a,b):
        if type(a) is not type(b):return False
        if isinstance(a,list):return len(a)==len(b) and all(same(x,y) for x,y in zip(a,b))
        if isinstance(a,dict):return a.keys()==b.keys() and all(same(a[key],b[key]) for key in a)
        return a==b
    return [{"field":key,"reference":value,"native":actual.get(key)} for key,value in wanted.items() if key not in actual or not same(actual[key],value)]


def rejection_differences(result,reason):
    if result.get("status")!="error" or reason not in result.get("stderr", ""):
        return [{"field":"expected_explicit_rejection","expected_reason":reason,"native":result}]
    return []


def native(probe,path,kind,payload,binary=False):
    process=subprocess.run([str(probe),kind,str(path),*(["--binary"] if binary else [])],input=payload if kind.endswith("stream") else None,
                           capture_output=True,timeout=15,check=False)
    if process.returncode:return {"status":"error","stderr":process.stderr.decode("utf8","replace")}
    return {"status":"ok","table":json.loads(process.stdout)}


def wrapper_bug(path):
    """Run the original Python wrapper unchanged with a disposable TMPDIR."""
    code='''import io,json,sys,inspect,hashlib
from fabio.ext import cf_io
from fabio.datIO import columnfile
result={"extension":{"path":inspect.getfile(cf_io),"sha256":hashlib.sha256(open(inspect.getfile(cf_io),"rb").read()).hexdigest()},"reads":[]}
for mode in ("a","b"):
 try:
  with open(sys.argv[1],"rb") as stream: value=cf_io.read(stream,mode)
  result["reads"].append({"api":"cf_io.read","mode":mode,"status":"returned","none_result":value==(None,None)})
 except Exception as error: result["reads"].append({"api":"cf_io.read","mode":mode,"status":"error","type":type(error).__name__,"errno":getattr(error,"errno",None),"error":str(error)})
try:
 value=columnfile().read(sys.argv[1]);result["reads"].append({"api":"datIO.columnfile.read","status":"returned"})
except Exception as error:result["reads"].append({"api":"datIO.columnfile.read","status":"error","type":type(error).__name__,"error":str(error)})
print(json.dumps(result))'''
    with tempfile.TemporaryDirectory(prefix="dioptrin-original-cf-io-") as temporary:
        result=subprocess.run([sys.executable,"-c",code,str(path)],capture_output=True,text=True,timeout=15,check=True,
                              env={**os.environ,"TMPDIR":temporary})
    return json.loads(result.stdout)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe",type=pathlib.Path,required=True)
    parser.add_argument("--fabio-source",type=pathlib.Path,required=True)
    parser.add_argument("--report",type=pathlib.Path,required=True)
    args=parser.parse_args();args.probe=args.probe.resolve()
    import fabio
    if fabio.version!="2026.6.0" or subprocess.check_output(["git","-C",str(args.fabio_source),"rev-parse","HEAD"],text=True).strip()!=PIN:parser.error("requires pinned FabIO2026.6.0")
    tracked=[pathlib.Path(__file__),ROOT/"scripts/test_columnfile_reference.py",ROOT/"scripts/generate_columnfile_fixtures.py",
             ROOT/"src/columnfile.rs",ROOT/"tests/columnfile.rs",ROOT/"Cargo.toml",ROOT/"Cargo.lock",ROOT/"examples/api_probe.rs"]
    tracked += sorted((ROOT/"tests/data/columnfile").glob("*"))
    tracked += [args.fabio_source/"src/fabio/ext/cf_io.pyx",args.fabio_source/"src/fabio/datIO.py"]
    tracked += [args.fabio_source/"src/fabio/ext/src/columnfile.c",args.fabio_source/"src/fabio/ext/include/columnfile.h",args.fabio_source/"copyright"]
    tracked += sorted((ROOT/"src").rglob("*.rs"))
    imports=[]
    for name in ("datIO",):
        module=importlib.import_module("fabio."+name);actual=pathlib.Path(inspect.getfile(module));pinned=args.fabio_source/f"src/fabio/{name}.py"
        imports.append({"module":module.__name__,"actual":fingerprint(actual),"pinned":fingerprint(pinned),"matches_pinned":actual.read_bytes()==pinned.read_bytes()})
        tracked.append(actual)
    tracked.append(pathlib.Path(inspect.getfile(importlib.import_module("fabio.ext.cf_io"))))
    before={str(path):fingerprint(path) for path in tracked};probe_before=fingerprint(args.probe)
    manifest=json.loads((ROOT/"tests/data/columnfile/provenance.json").read_text());reads=[]
    with tempfile.TemporaryDirectory(prefix="dioptrin-column-c-oracle-") as temporary:
        executable,c=build_c(args.fabio_source,temporary)
        for name,payload in fixtures().items():
            path=ROOT/"tests/data/columnfile"/name;case=manifest["files"][name]
            if path.read_bytes()!=payload:raise ValueError("fixture differs from independent generator")
            record={"path":str(path),"source":fingerprint(path),"reads":[],"c_oracle_scope":"unsafe malformed/wide/comment cases deliberately excluded"}
            if case["c_oracle_safe"]:
                reference=c_oracle(executable,path);record["c_oracle"]=reference
                source=expected(payload)
                c_expected={key:source[key] for key in ("shape","labels_hex","value_bits")}
                if name=="unterminated-row.dat":c_expected.update(shape=[0,2],value_bits=[]);record["reference_difference"]=case["reference_difference"]
                record["c_differences"]=differences(reference,c_expected)
            for kind,provenance in (("columnfile","PathSnapshot"),("columnfile-bytes","Buffer"),("columnfile-stream","Stream")):
                result=native(args.probe,path,kind,payload)
                delta=[]
                if case["native_reject"]:
                    delta=rejection_differences(result,case["error_contains"])
                elif result["status"]!="ok":delta.append({"field":"native_open","native":result})
                else:delta=differences(result["table"],{**expected(payload),"provenance":provenance})
                record["reads"].append({"api":kind,"native":result,"differences":delta})
            record["source_unchanged"]=fingerprint(path)==record["source"]
            reads.append(record)
        binary=c_oracle(executable,ROOT/"tests/data/columnfile/matrix.dat","b")
    binary_native=[{"api":kind,"result":native(args.probe,ROOT/"tests/data/columnfile/matrix.dat",kind,fixtures()["matrix.dat"],True)}
                   for kind in ("columnfile","columnfile-bytes","columnfile-stream")]
    wrapper=wrapper_bug(ROOT/"tests/data/columnfile/matrix.dat")
    direct=wrapper["reads"][:2]
    report={"schema":1,"reference_commit":PIN,"reference_version":fabio.version,"contract":__doc__,"c_oracle":c,
            "binary_capability":{"classification":"unimplemented_reference_decoder_no_binary_schema","reference":binary,"native":binary_native},
            "original_wrapper":wrapper,"reference_imports":imports,"reads":reads,"sources":before,"probe":probe_before,
            "sources_unchanged":all(fingerprint(path)==value for path,value in before.items()),"probe_unchanged":fingerprint(args.probe)==probe_before}
    report["all_qualified"]=(report["sources_unchanged"] and report["probe_unchanged"] and all(i["matches_pinned"] for i in imports)
                             and binary["status"]=="no_decoder_result" and all(row.get("errno")==9 for row in direct)
                             and all(row["result"]["status"]=="error" and "binary schema" in row["result"].get("stderr", "") for row in binary_native)
                             and wrapper["reads"][2]["status"]=="error" and all(row["source_unchanged"] and not row.get("c_differences")
                             and all(not read["differences"] for read in row["reads"]) for row in reads))
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+"\n")
    print(json.dumps({"qualified":report["all_qualified"],"sources":len(reads),"native_reads":sum(len(row["reads"]) for row in reads),"report":str(args.report)}))
    return 0 if report["all_qualified"] else 1


if __name__=="__main__":raise SystemExit(main())
