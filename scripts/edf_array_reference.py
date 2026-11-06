"""EDF source-array/plane proof using unchanged FabIO and independent byte parsing.

This is test tooling only. The strict parser here handles the bounded inline
fixture dialect; it never turns incomplete samples into zero-padded arrays.
"""
import argparse
import bz2
import gzip
import hashlib
import importlib
import importlib.util
import inspect
import itertools
import json
import math
import pathlib
import re
import struct
import subprocess
import tempfile
import unittest
import zlib

import numpy as np
from generate_edf_array_fixtures import fixtures

ROOT = pathlib.Path(__file__).resolve().parents[1]
PIN = "9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece"
TYPES = {"UnsignedByte":"u1", "UnsignedShort":"u2", "SignedShort":"i2", "UnsignedInteger":"u4", "SignedInteger":"i4", "Unsigned64":"u8", "Signed64":"i8", "DoubleValue":"f8", "FloatValue":"f4"}
CODECS = {"NONE":"NONE","GZIP":"GZIP","GZIPCOMPRESSION":"GZIP","ZLIB":"ZLIB","Z":"ZLIB","ZCOMPRESSION":"ZLIB","BZ2":"BZ2","BZIP2":"BZ2","BZCOMPRESSION":"BZ2","BZ2COMPRESSION":"BZ2"}
PIXEL_TYPES = {"u1":"U8", "u2":"U16", "i2":"I16", "u4":"U32", "i4":"I32", "u8":"U64", "i8":"I64", "f8":"F64", "f4":"F32"}


def sha(data): return hashlib.sha256(data).hexdigest()


def fingerprint(path):
    path = pathlib.Path(path)
    return {"path": str(path.resolve()), "bytes": path.stat().st_size, "sha256": sha(path.read_bytes())}


def equal(a,b):
    if type(a) is not type(b): return False
    if isinstance(a,dict): return a.keys()==b.keys() and all(equal(a[k],b[k]) for k in a)
    if isinstance(a,list): return len(a)==len(b) and all(equal(x,y) for x,y in zip(a,b))
    return a==b


def delta(actual, expected):
    return [{"field":key,"reference":value,"native":actual.get(key)} for key,value in expected.items() if key not in actual or not equal(actual[key],value)]


def signature(data):
    array=np.asarray(data)
    canonical=np.ascontiguousarray(array.astype(array.dtype.newbyteorder("<"),copy=False)).tobytes()
    return {"shape":list(array.shape),"pixel_type":PIXEL_TYPES[array.dtype.kind+str(array.dtype.itemsize)],
            "pixels":{"dtype":array.dtype.name,"sample_sha256":sha(canonical)},"value_hex":canonical.hex()}


def geometry(fields):
    keys=[int(k[4:]) for k in fields if k.startswith("DIM_") and re.fullmatch("[0-9]+",k[4:])]
    rank=max(keys,default=0)
    if rank>32: raise ValueError("reference rank bound")
    shape=[int(fields.get(f"DIM_{axis}","0" if axis==1 else "1")) for axis in range(rank,0,-1)]
    if any(n<=0 for n in shape): raise ValueError("empty source axis unsupported")
    if math.prod(shape)>64_000_000: raise ValueError("reference sample bound")
    plane=([1,1] if not shape else [1,shape[0]] if len(shape)==1 else shape[-2:])
    planes=math.prod(shape[:-2])
    return {"source_shape":shape,"source_samples":math.prod(shape),"shape":plane,"plane_count":planes,
            "implicit_dimensions":[axis for axis in range(2,rank+1) if f"DIM_{axis}" not in fields],
            "frame_layout":"Native2D" if rank==2 else "SingletonLeadingAxes" if rank>2 and planes==1 else "ArrayOrPlaneRequired"}


def parse(data):
    """Independently parse complete headers; retain original and effective fields."""
    if data.startswith(b"\x1f\x8b"): data=gzip.decompress(data)
    elif data.startswith(b"BZh"): data=bz2.decompress(data)
    if len(data)>256*1024*1024: raise ValueError("reference encoded bound")
    records=[];offset=0;general={}
    while offset<len(data):
        if not data[offset:].strip(b" \t\r\n\0"):break
        match=re.match(rb"\s*\{(.*?)\}\r?\n",data[offset:],re.S)
        if not match: raise ValueError("reference incomplete header")
        raw=data[offset:offset+match.end()];local={}
        for item in match[1].decode("ascii").split(";"):
            if "=" in item:
                key,value=item.split("=",1);local[key.strip().upper()]=value.strip()
        payload=offset+match.end()
        if "EDF_DATAFORMATVERSION" in local:
            if int(local.get("EDF_BINARYSIZE",local.get("SIZE","0")))!=0:raise ValueError("reference unsupported general binary block")
            general={k:v for k,v in local.items() if not k.startswith("EDF_") and k not in ("SIZE","IMAGE","HEADERID")};offset=payload;continue
        fields={**general,**local};geo=geometry(fields)
        dtype=np.dtype(TYPES[fields["DATATYPE"]]).newbyteorder("<" if fields["BYTEORDER"]=="LowByteFirst" else ">")
        size=int(fields.get("EDF_BINARYSIZE",fields.get("SIZE",str(geo["source_samples"]*dtype.itemsize))))
        encoded=data[payload:payload+size]
        records.append({"metadata":{**geo,"record_index":len(records),"fields":fields,"header_hex":raw.hex(),"pixel_type":PIXEL_TYPES[dtype.kind+str(dtype.itemsize)],"little_endian":dtype.byteorder=="<" or dtype.itemsize==1},
                        "dtype":dtype,"offset":payload,"expected_encoded_bytes":size,"encoded":encoded,"codec":CODECS[fields.get("COMPRESSION","NONE").upper()]})
        offset=payload+size
    return records


def decode(record):
    codec=record["codec"];data=record["encoded"]
    expected=record["metadata"]["source_samples"]*record["dtype"].itemsize
    if len(data)!=record["expected_encoded_bytes"]:raise ValueError("short encoded window")
    if codec=="NONE":
        if len(data)<expected:raise ValueError("incomplete source samples")
        raw=data[:expected]  # Remaining physical slot bytes are preserved by source/window hashes.
    else:
        if codec in ("GZIP","ZLIB"):
            decoder=zlib.decompressobj(31 if codec=="GZIP" else 15)
            raw=decoder.decompress(data)
        elif codec=="BZ2":
            decoder=bz2.BZ2Decompressor();raw=decoder.decompress(data)
        else:raise ValueError("unsupported reference codec")
        if not decoder.eof:raise ValueError("incomplete codec footer")
        tail=decoder.unused_data
        if tail and (len(tail)>=512 or len(data)%512 or tail.strip(b" \t\r\n\v\f\0")):
            raise ValueError("unexplained physical codec tail")
        if len(raw)!=expected:raise ValueError("incomplete source samples")
    return np.frombuffer(raw,dtype=record["dtype"]).reshape(record["metadata"]["source_shape"])



def expected_recovery(record):
    """Verified availability independent of complete-frame geometry or missing data."""
    meta=record["metadata"];encoded=record["encoded"];raw=encoded;integrity="ContainerComplete";members=[]
    if record["codec"]=="GZIP":
        # Independent framing and raw DEFLATE decode: expose payload words even
        # when CRC/ISIZE is invalid or footer is short, never verify those words.
        if encoded[:10] != b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\x03":
            # gzip XFL/OS are transport metadata, not the decoding contract.
            if len(encoded)<10 or encoded[:4]!=b"\x1f\x8b\x08\x00":raise ValueError("reference gzip header outside fixture dialect")
        stream=zlib.decompressobj(-15);raw=stream.decompress(encoded[10:]);footer=stream.unused_data
        if not stream.eof or len(footer)<8:integrity="UnverifiedTruncated"
        else:
            crc,length=struct.unpack("<II",footer[:8])
            integrity="VerifiedMember" if crc==(zlib.crc32(raw)&0xffffffff) and length==len(raw)&0xffffffff else "ChecksumFailure"
        members=[{"encoded_range":[0,len(encoded)],"decoded_range":[0,len(raw)],"integrity":integrity}]
    expected=meta["source_samples"]*record["dtype"].itemsize
    raw=raw[:expected];count=len(raw)//record["dtype"].itemsize;partial=raw[count*record["dtype"].itemsize:]
    typed=np.frombuffer(raw[:count*record["dtype"].itemsize],dtype=record["dtype"])
    complete=count==meta["source_samples"] and not partial and integrity in ("ContainerComplete","VerifiedMember")
    span={"declared_source_shape":meta["source_shape"],"declared_shape":meta["shape"],"record_index":meta["record_index"],
          "declared_samples":meta["source_samples"],"present_samples":[0,count],"missing_samples":None if count==meta["source_samples"] else [count,meta["source_samples"]],
          "verified_samples":[0,count if integrity in ("ContainerComplete","VerifiedMember") else 0],
          "outer_integrity":"ContainerComplete","payload_integrity":integrity,"expected_payload_bytes":record["expected_encoded_bytes"],"available_payload_bytes":len(encoded),
          "availability":"AllSamples" if count==meta["source_samples"] else "HeadersOnly" if not count else "Prefix",
          "complete_verified_frame":complete,"complete_verified_record":complete,"partial_element_bytes":partial.hex()}
    stop=None if len(encoded)==record["expected_encoded_bytes"] else {"kind":"TruncatedPayload","image_record":meta["record_index"],"offset":record["offset"],"expected_bytes":record["expected_encoded_bytes"],"available_bytes":len(encoded)}
    return {"shape":[count],**signature(typed)["pixels"],"frame":meta["record_index"],"fields":meta["fields"],"header_sha256":sha(bytes.fromhex(meta["header_hex"])),"availability":span},members,stop


def compact(value):
    """All bytes are checked before compact report storage; no large Git payloads."""
    if isinstance(value,list):return [compact(v) for v in value]
    if isinstance(value,dict):
        out={key:compact(v) for key,v in value.items() if key!="value_hex"}
        if "value_hex" in value:
            raw=bytes.fromhex(value["value_hex"])
            out.update(value_bytes=len(raw),value_sha256=sha(raw))
        return out
    return value


class Probe:
    def __init__(self,executable,recovery):self.executable=executable;self.recovery=recovery
    def array(self,path,index,leading=None):
        request={"path":str(path),"record":index}
        if leading is not None:request["leading"]=leading
        with tempfile.NamedTemporaryFile("w",suffix=".json") as handle:
            json.dump(request,handle);handle.flush()
            result=subprocess.run([str(self.executable),"edf-array",handle.name],capture_output=True,text=True,timeout=30)
        if result.returncode:raise ValueError(f"native array failed:{result.returncode}: {result.stderr}")
        return json.loads(result.stdout)
    def recovery_read(self,path):
        result=subprocess.run([str(self.recovery),"recovery",str(path)],capture_output=True,text=True,timeout=30)
        if result.returncode:raise ValueError(f"native recovery failed:{result.returncode}: {result.stderr}")
        return json.loads(result.stdout)["source_recovery"]


def compare_array(native,record,reference,count,leading=None):
    expected={"records":count,"metadata":record["metadata"],"array":signature(reference)}
    # Metadata is exact and original bytes are retained; extra output fields
    # outside this projection must be deliberately admitted, never ignored here.
    if leading is not None:
        plane=reference[tuple(leading)] if leading else reference
        plane=plane.reshape(record["metadata"]["shape"])
        expected["plane"]=signature(plane)
    return delta(native,expected)


def compare_recovery(native,record):
    wanted,members,stop=expected_recovery(record);images=native.get("images",[])
    if len(images)!=1:return [{"field":"images","reference_count":1,"native_count":len(images)}]
    image=images[0];diff=delta(image,{key:value for key,value in wanted.items() if key!="availability"})
    diff+= [{**d,"field":"availability."+d["field"]} for d in delta(image.get("availability",{}),wanted["availability"])]
    actual_members=image.get("availability",{}).get("payload_members",[])
    if "payload_members" not in image.get("availability",{}):diff.append({"field":"payload_members","reference":members,"native":"missing"})
    if len(actual_members)!=len(members):diff.append({"field":"payload_members.count","reference":len(members),"native":len(actual_members)})
    else:
        for i,(a,b) in enumerate(zip(actual_members,members)):diff += [{**d,"field":f"payload_members.{i}."+d["field"]} for d in delta(a,b)]
    if not equal(native.get("stop"),stop):diff.append({"field":"stop","reference":stop,"native":native.get("stop")})
    source_bytes=record["offset"]+len(record["encoded"])
    source_expected={"encoded_bytes":source_bytes,"decoded_bytes":source_bytes,"compression":"None","integrity":"ContainerComplete","verified_decoded_bytes":source_bytes,"members":[]}
    diff += [{**d,"field":"source_report."+d["field"]} for d in delta(native.get("source_report",{}),source_expected)]
    return diff


def gate(summary,corpus):
    expected={"complete_sources":14 if corpus else 10,"complete_records":19 if corpus else 12,"explicit_planes":30 if corpus else 23,"recovery_sources":4,"reference_tests":1,"reference_failures":0,"reference_errors":0,"source_failures":0,"array_or_plane_differences":0,"recovery_differences":0,"imports_unchanged":True,"sources_stable":True,"probes_stable":True}
    return all(key in summary and equal(summary[key],value) for key,value in expected.items())


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe",type=pathlib.Path,required=True);parser.add_argument("--recovery-probe",type=pathlib.Path,required=True)
    parser.add_argument("--fabio-source",type=pathlib.Path,required=True);parser.add_argument("--corpus",type=pathlib.Path)
    parser.add_argument("--report",type=pathlib.Path,required=True)
    args=parser.parse_args();import fabio
    runtime=importlib.import_module("fabio.edfimage");tests=importlib.import_module("fabio.test.codecs.test_edfimage")
    if fabio.version!="2026.6.0" or subprocess.check_output(["git","-C",str(args.fabio_source),"rev-parse","HEAD"],text=True).strip()!=PIN:parser.error("requires unchanged pinned FabIO")
    imports=[]
    for module,relative in [(runtime,"edfimage.py"),(tests,"test/codecs/test_edfimage.py"),(importlib.import_module("fabio.fabioutils"),"fabioutils.py"),(importlib.import_module("fabio.compression.compression"),"compression/compression.py")]:
        actual=pathlib.Path(inspect.getfile(module));pinned=args.fabio_source/"src/fabio"/relative
        gitbytes=subprocess.check_output(["git","-C",str(args.fabio_source),"show",f"{PIN}:src/fabio/{relative}"])
        imports.append({"actual":fingerprint(actual),"pinned":fingerprint(pinned),"unchanged_git":gitbytes==pinned.read_bytes(),"matches_pinned":actual.read_bytes()==gitbytes})
    tracked=[ROOT/"Cargo.toml",ROOT/"Cargo.lock",ROOT/"build.rs",ROOT/"examples/api_probe.rs",ROOT/"examples/edf_recovery_probe.rs",ROOT/"scripts/edf_array_reference.py",ROOT/"scripts/test_edf_array_reference.py",ROOT/"scripts/generate_edf_array_fixtures.py",ROOT/"tests/edf_arrays.rs"]
    tracked += sorted((ROOT/"src").rglob("*.rs"))+sorted((ROOT/"tests/data/edf-arrays").glob("*"))
    tracked += [pathlib.Path(item[key]["path"]) for item in imports for key in ("actual","pinned")]
    before={str(p):fingerprint(p) for p in tracked};probes_before=[fingerprint(args.probe),fingerprint(args.recovery_probe)]
    probe=Probe(args.probe,args.recovery_probe);events=[];errors=[]
    complete=[name for name in fixtures() if not any(word in name for word in ("truncated","header-only","corrupt","footer"))]
    paths=[ROOT/"tests/data/edf-arrays"/name for name in complete]
    if args.corpus:paths += [args.corpus/name for name in ("MultiFrame.edf","MultiFrame.edf.gz","MultiFrame.edf.bz2","edfGzip_U16.edf")]
    for path in paths:
        snap=fingerprint(path)
        try:
            records=parse(path.read_bytes());reader=fabio.open(str(path))
            try:
                if reader.nframes!=len(records):raise ValueError("source record count differs")
                for index,record in enumerate(records):
                    reference=reader.get_frame(index).data
                    raw=decode(record)
                    if signature(raw)!=signature(reference):raise ValueError("unchanged FabIO differs from independent source words")
                    native=probe.array(path,index)
                    differences=compare_array(native,record,reference,len(records))
                    planes=[]
                    for coordinates in itertools.product(*(range(n) for n in reference.shape[:-2])):
                        native_plane=probe.array(path,index,list(coordinates))
                        diff=compare_array(native_plane,record,reference,len(records),list(coordinates))
                        planes.append({"leading":list(coordinates),"native":compact(native_plane["plane"]),"differences":diff});differences+=diff
                    events.append({"source":snap,"record":index,"native":compact(native),"independent_source":compact(signature(raw)),"reference":compact(signature(reference)),"planes":planes,"differences":differences})
            finally:reader.close()
        except Exception as error:errors.append({"path":str(path),"type":type(error).__name__,"error":str(error)})
        if snap!=fingerprint(path):errors.append({"path":str(path),"error":"source changed"})
    recovery=[]
    for name in fixtures():
        if name in complete:continue
        path=ROOT/"tests/data/edf-arrays"/name;snap=fingerprint(path)
        try:
            record=parse(path.read_bytes())[0];native=probe.recovery_read(path);wanted,members,stop=expected_recovery(record)
            recovery.append({"source":snap,"reference":wanted,"reference_payload_members":members,"reference_stop":stop,"native":native,"differences":compare_recovery(native,record)})
        except Exception as error:errors.append({"path":str(path),"type":type(error).__name__,"error":str(error)})
        if snap!=fingerprint(path):errors.append({"path":str(path),"error":"source changed"})
    suite=unittest.TestSuite([tests.TestEdfRegression("test_bug_459")]);result=unittest.TestResult();suite.run(result)
    abi={"numpy":np.__version__,"float128_attribute":hasattr(np,"float128"),"longdouble_dtype":np.dtype(np.longdouble).name,"longdouble_bytes":np.dtype(np.longdouble).itemsize,"longdouble_mantissa":np.finfo(np.longdouble).nmant,"byte_offset_importable":importlib.util.find_spec("byte_offset") is not None}
    unsupported=[{"encoding":"FloatIEEE128/DoubleIEEE128/QuadrupleValue","classification":"source_representation_capability_gap","evidence":"pinned edfimage.py DATA_TYPES aliases numpy.float128 conditionally; this is not a platform-independent IEEE binary128 storage contract; no cast/alias to F64"},
                 {"encoding":"Compression containing OFFSET","classification":"unqualified_legacy_codec_gap","evidence":"pinned _unpack imports external top-level byte_offset.analyseCython and converts result to source dtype; no working pinned dialect oracle, no native retry or fabricated fallback"},
                 {"encoding":"empty source arrays","classification":"explicit_unsupported_layout","evidence":"zero-size axes/missing DIM_1 rejected; shape[] is a valid rank0 scalar, not an empty array"},
                 {"encoding":"fractional/exponent DIM_i strings","classification":"explicit_strict_numeric_policy","evidence":"pinned nice_int uses int(float(text)) fallback, including truncation; native dimensions require exact nonnegative integer syntax, never truncated floating-point dimensions"}]
    sources_after={str(p):fingerprint(p) for p in tracked};probes_after=[fingerprint(args.probe),fingerprint(args.recovery_probe)]
    summary={"complete_sources":len(paths),"complete_records":len(events),"explicit_planes":sum(len(e["planes"]) for e in events),"recovery_sources":len(recovery),"reference_tests":result.testsRun,"reference_failures":len(result.failures),"reference_errors":len(result.errors),"source_failures":len(errors),"array_or_plane_differences":sum(len(e["differences"]) for e in events),"recovery_differences":sum(len(e["differences"]) for e in recovery),"imports_unchanged":all(i["unchanged_git"] and i["matches_pinned"] for i in imports),"sources_stable":before==sources_after,"probes_stable":probes_before==probes_after}
    passed=gate(summary,bool(args.corpus))
    report={"schema":1,"scope":"explicit EDF source arrays and selected planes; source image records never flattened; available recovery words independently qualified, never zero-padded", "passed":passed,"summary":summary,"pin":PIN,"imports":imports,"probe_before":probes_before,"probe_after":probes_after,"sources_before":before,"sources_after":sources_after,"arrays":events,"recovery":recovery,"errors":errors,"reference_tests":{"testsRun":result.testsRun,"failures":result.failures,"errors":result.errors},"reference_abi":abi,"remaining":unsupported,"license":"new fixtures/generator CC0-1.0; test-only unchanged FabIO MIT reference; no runtime Python or copied codec"}
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2)+"\n");print(json.dumps({"passed":passed,"summary":summary,"report":str(args.report)}));return 0 if passed else 1


if __name__=="__main__":raise SystemExit(main())
