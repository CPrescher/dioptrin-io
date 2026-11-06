"""Unchanged low-level TiffIO reads, original IFD metadata and explicit views.

Native stored samples are compared separately with an independent TIFF reader.
The reference's palette/mono output is an explicit view, not a stored array.
Upstream writers run only as the unchanged three-test suite's fixture creation;
no native writer/object/cache/refresh compatibility is claimed.
"""
import argparse
import contextlib
import hashlib
import importlib
import inspect
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import numpy as np
import tifffile
from metadata_parity import encode, _equal
from header_reference import tag_value
from generate_tiff_api_fixtures import fixtures

PIN="9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece"
ROOT=pathlib.Path(__file__).resolve().parents[1]


def fingerprint(path):
    path=pathlib.Path(path)
    return {"path":str(path.resolve()),"bytes":path.stat().st_size,"sha256":hashlib.sha256(path.read_bytes()).hexdigest()}


def signature(data):
    array=np.asarray(data)
    canonical=np.ascontiguousarray(array.astype(array.dtype.newbyteorder("<"),copy=False))
    return {"pixels":{"dtype":array.dtype.name,"sample_sha256":hashlib.sha256(canonical.tobytes()).hexdigest()},"shape":list(array.shape)}


def delta(actual,expected):
    return [{"field":key,"reference":value,"native":actual.get(key)} for key,value in expected.items()
            if key not in actual or not _equal(actual[key],value)]


def project_info(info,index,images,mono=False):
    """Reference dictionary presentation from native source tags only."""
    tags=info["tags"]
    def scalar(number,default=None):
        value=tags.get(str(number),default)
        return value
    def sequence(number,default):
        value=tags.get(str(number),default)
        return value if isinstance(value,list) else [value]
    def text(number,default):
        value=tags.get(str(number))
        if value is None:
            opaque=info.get("opaque_tags",{}).get(str(number))
            value=bytes.fromhex(opaque["value_hex"]).split(b"\0",1)[0].decode("utf8","replace").replace("\ufffd","?") if opaque else default
        return value.split("\0",1)[0] if isinstance(value,str) else value
    rows,columns=scalar(257),scalar(256)
    bits=scalar(258)
    description=text(270,f"{index+1}/{images}")
    software=text(305,"Unknown Software")
    if software=="Unknown Software" and description.upper().startswith("IMAGEJ"):software=description.split("=")[0]
    photo=scalar(262,1);sample=scalar(339,4);colormap=None
    if "320" in tags:
        values=np.asarray(tags["320"],dtype="uint16")
        if values.max()>255:values=values//256
        colormap=values.astype("uint8").reshape(3,-1).T.copy()
    if mono and photo>1:bits=32;colormap=None;sample=3;photo=1
    result={"nRows":rows,"nColumns":columns,"nBits":bits,"compression":scalar(259,1)!=1,
            "compression_type":scalar(259,1),"imageDescription":description,"stripOffsets":sequence(273,[]),
            "rowsPerStrip":scalar(278,rows),"stripByteCounts":sequence(279,[]),"software":software,
            "date":text(306,"Unknown Date"),"colormap":colormap,"sampleFormat":sample,
            "photometricInterpretation":photo,"info":{}}
    if "279" not in tags:
        result["stripByteCounts"]=[int(rows*columns*sum(bits if isinstance(bits,list) else [bits])/8)]
    if "272" in tags or "272" in info.get("opaque_tags",{}):result["model"]=[text(272,"")]
    if software.startswith("PyMca"):
        items=description.split("=")
        result["info"]={str(items[i*2]):str(items[i*2+1][:-1]) for i in range(len(items)//2)}
    return encode(result)


def compare_info(reference,native,index,images,mono=False):
    projection=project_info(native,index,images,mono)
    result=delta(projection,reference)
    if set(projection)!=set(reference):result.append({"field":"info_dictionary_keys","reference":sorted(reference),"projection":sorted(projection)})
    return result


class Probe:
    def __init__(self,path):self.path=pathlib.Path(path).resolve()
    def __call__(self,kind,path,commands,options=None):
        with tempfile.TemporaryDirectory(prefix="tiff-api-request-") as directory:
            request=pathlib.Path(directory)/"request.json"
            request.write_text(json.dumps({"path":str(path),"commands":commands,**(options or {})}))
            process=subprocess.run([str(self.path),kind,str(request)],capture_output=True,text=True,timeout=120,check=False)
        if process.returncode:return {"status":"error","error":process.stderr}
        value=json.loads(process.stdout)
        if len(value.get("actions",[]))!=len(commands):return {"status":"error","error":"native action count mismatch","native":value}
        if any(action.get("command")!=command for action,command in zip(value["actions"],commands)):
            return {"status":"error","error":"native command correspondence mismatch","native":value}
        return {"status":"ok","actions":value["actions"],"interpretation_policy":value.get("interpretation_policy")}


def raw_reference(path,index):
    with tifffile.TiffFile(path) as file:
        page=file.pages[index]
        if int(np.prod(page.shape))*page.dtype.itemsize>256*1024*1024:raise ValueError("reference array byte admission")
        data=page.asarray()
        if page.planarconfig==2 and data.ndim==3:data=data.transpose(1,2,0)
        return data.copy()


def expected_view(raw,info,mono):
    """Independent literal display calculation; never changes source samples."""
    tags=info["tags"];image=raw
    if tags.get("262",1)==3:
        palette=np.asarray(tags["320"],dtype="uint16")
        if palette.max()>255:palette=palette//256
        palette=palette.astype("uint8").reshape(3,-1).T
        image=palette[raw].astype(raw.dtype)
    if mono and image.ndim==3:
        image=(image[:,:,0]*0.114+image[:,:,1]*0.587+image[:,:,2]*0.299).astype("float32")
    return image


def interpretation_differences(info,interpretation):
    result=delta(interpretation,{"source_ifd_offset":info["ifd_offset"]})
    tags=info["tags"]
    if {str(entry["tag"]) for entry in interpretation["source_entries"]} != set(tags)|set(info.get("opaque_tags",{})):
        result.append({"field":"interpretation_original_entry_ids"})
    if "UncompressedWindowsFromShape" in interpretation["operations"]:
        if interpretation["kind"]!="Strips":return result+[{"field":"unsupported_window_proof_kind"}]
        h,w=tags["257"],tags["256"];channels=tags.get("277",1);bits=tags["258"]
        if isinstance(bits,list):
            if len(set(bits))!=1:return result+[{"field":"unsupported_mixed_precision_window_proof"}]
            bits=bits[0]
        rps=interpretation["rows_per_strip"];separate=tags.get("284",1)==2
        if type(rps) is not int or rps<=0:return result+[{"field":"invalid_interpretation_rows_per_strip"}]
        row_bytes=(w*(1 if separate else channels)*bits+7)//8
        offsets=tags["273"] if isinstance(tags["273"],list) else [tags["273"]]
        per_plane=(h+rps-1)//rps
        expected=[{"offset":offset,"bytes":min(rps,h-(i%per_plane)*rps)*row_bytes} for i,offset in enumerate(offsets)]
        result+=delta(interpretation,{"windows":expected})
    return result


def validate_metadata(path,record):
    differences=[]
    with tifffile.TiffFile(path) as file,pathlib.Path(path).open("rb") as raw:
        pages=list(file.pages)
        actions=record["actions"]
        if len(actions)!=2+len(pages):return [{"field":"metadata_action_count"}]
        if actions[0]["status"]!="ok":return [{"field":"metadata_count_read"}]
        differences+=delta(actions[0]["result"],{"images":len(pages)})
        if actions[1]["status"]!="ok":return differences+[{"field":"metadata_directory_read"}]
        differences+=delta(actions[1]["result"],{"directories":[page.offset for page in pages]})
        for ordinal,(page,action) in enumerate(zip(pages,actions[2:])):
            if action["status"]!="ok":differences.append({"field":"metadata_info_read","index":ordinal});continue
            result=action["result"]
            differences+=delta(result,{"index":ordinal,"images":len(pages),"ifd_offset":page.offset})
            if set(result["tags"])|set(result.get("opaque_tags",{}))!={str(number) for number in page.tags.keys()}:
                differences.append({"field":"original_tag_ids","index":ordinal})
            for number,tag in page.tags.items():
                key=str(number)
                if key in result.get("opaque_tags",{}):
                    widths={1:1,2:1,3:2,4:4,5:8,6:1,7:1,8:2,9:4,10:8,11:4,12:8,13:4,16:8,17:8,18:8}
                    raw.seek(tag.valueoffset);payload=raw.read(tag.count*widths[int(tag.dtype)])
                    if result["opaque_tags"][key]["value_hex"]!=payload.hex():differences.append({"field":"opaque_source_bytes","tag":number})
                else:
                    expected=tag_value(tag,raw,file.byteorder)
                    if not _equal(result["tags"].get(key),expected):differences.append({"field":"original_tag_value","tag":number,"reference":expected,"native":result["tags"].get(key)})
    return differences


def reference_call(image,operation,index=None,kwargs=None):
    kwargs=kwargs or {}
    before=fingerprint(image.fd.name)
    def observed(value):return {**value,"source_before":before,"source_after":fingerprint(image.fd.name)}
    try:
        if operation=="count":return observed({"status":"ok","result":{"images":image.getNumberOfImages()}})
        if operation=="directories":return observed({"status":"ok","result":{"images":image.getImageFileDirectories(),"directories":list(image._IFD)}})
        if operation=="info":return observed({"status":"ok","result":encode(image.getInfo(index))})
        data=image.getImage(index) if operation=="image" else image.getData(index,**kwargs)
        return observed({"status":"ok","result":signature(data)})
    except Exception as error:return observed({"status":"error","error_type":type(error).__name__,"error":str(error)})


def compare_read(native,reference,raw,command,info,mono=False):
    differences=[];qualification=None
    if native["status"]!="ok":
        if reference["status"]=="error":
            if native.get("pixels_available") is not False:differences.append({"field":"failed_read_visible_pixels"})
            return differences,"both_explicitly_rejected"
        if command["index"]<0:return ([] if native.get("pixels_available") is False else [{"field":"negative_visible_pixels"}]),"native_negative_index_policy"
        return [{"field":"native_read","native":native}],None
    if reference["status"]=="error":
        message=reference.get("error","");kind=reference.get("error_type");tags=info.get("tags",{})
        offsets=tags.get("273",[]);offsets=offsets if isinstance(offsets,list) else [offsets]
        if tags.get("259")==32773 and len(offsets)==1 and kind=="ValueError":return [],"reference_single_strip_packbits_defect"
        if tags.get("262")==3 and len(offsets)==1 and kind=="ValueError" and "broadcast" in message:return [],"reference_single_strip_palette_window_defect"
        if "row_min" in command and kind in ("ValueError","IndexError","NotImplementedError"):return [],"reference_row_selection_contract_defect"
        if "Compressed TIFF images not supported" in message:return [],"reference_codec_unsupported"
        if "Invalid TIFF version" in message:return [],"reference_format_unsupported"
        if "Unsupported number of bits" in message or "Unsupported combination" in message:return [],"reference_scalar_layout_unsupported"
        return [{"field":"unexpected_reference_read_error","reference":reference}],None
    expected=reference["result"]
    differences=delta(native["result"],expected)
    if differences:
        if "row_min" in command:qualification="reference_row_selection_contract_defect"
        elif info.get("tags",{}).get("259")==32773 and len(info["tags"].get("273",[]) if isinstance(info["tags"].get("273"),list) else [info["tags"].get("273")])==1:
            qualification="reference_single_strip_packbits_defect"
    return differences,qualification


class Audit:
    def __init__(self,probe,policies=None):
        from fabio.TiffIO import TiffIO
        self.cls=TiffIO;self.probe=probe;self.policies=policies or {};self.test=None;self.depth=0;self.events=[];self.errors=[];self.internal={}
    def verify(self,path,operation,index,reference,mono=False,kwargs=None,origin="independent"):
        kwargs=kwargs or {};before=reference.get("source_before",fingerprint(path))
        options=self.policies.get(before["sha256"],{})
        with tifffile.TiffFile(path) as file:count=len(file.pages)
        commands=[{"op":"count"},{"op":"directories"}]+[{"op":"info","index":i} for i in range(count)]
        metadata=self.probe("tiff-metadata-api",path,commands)
        record={"test":self.test,"path":str(path),"source":before,"origin":origin,"operation":operation,"index":index,
                "reference":reference,"metadata":metadata,"preselected_policy":options,"differences":[],"qualification":None}
        if before!=fingerprint(path) or reference.get("source_after",before)!=before:record["differences"].append({"field":"reference_source_changed"})
        if metadata["status"]!="ok":record["differences"].append({"field":"metadata_open","native":metadata});return record
        record["differences"]+=validate_metadata(path,metadata)
        if operation in ("count","directories"):
            if reference["status"]=="ok":
                record["differences"]+=delta(metadata["actions"][0]["result"],{"images":reference["result"]["images"]})
                if operation=="directories":record["differences"]+=delta(metadata["actions"][1]["result"],{"directories":reference["result"]["directories"]})
            else:record["qualification"]="reference_closed_file_directory_method_defect"
        elif operation=="info":
            if 0<=index<count:
                if reference["status"]=="ok":record["differences"]+=compare_info(reference["result"],metadata["actions"][2+index]["result"],index,count,mono)
                else:record["qualification"]="reference_info_unsupported"
            elif index<0 and reference["status"]=="ok":record["qualification"]="native_negative_index_policy"
            failed=self.probe("tiff-metadata-api",path,[{"op":"info","index":index}])
            if index<0 or index>=count:
                if failed["status"]!="ok" or failed["actions"][0]["status"]!="error" or failed["actions"][0].get("pixels_available") is not False:record["differences"].append({"field":"metadata_invalid_index_not_rejected"})
        else:
            command={"op":"view","index":index,"mono":mono}
            if "rowMin" in kwargs or "rowMax" in kwargs:
                command.update(row_min=kwargs.get("rowMin",0),row_max=kwargs.get("rowMax",metadata["actions"][2+max(0,index)]["result"]["tags"]["257"]-1))
            stored={**command,"op":"data"};stored.pop("mono",None)
            native=self.probe("tiff-api",path,[stored,command,{"op":"info","index":index}],options);record["native"]=native
            if native["status"]!="ok":
                tags=metadata["actions"][2]["result"]["tags"] if count else {}
                if not options and "279" not in tags and tags.get("259",1)==1 and "StripByteCounts" in native.get("error", ""):
                    record.update(status="qualified_difference",qualification="strict_missing_declared_bytecount_window",source_unchanged=before==fingerprint(path))
                    return record
                record["differences"].append({"field":"native_pixel_open","native":native})
                record["status"]="difference"
                return record
            info=metadata["actions"][2+index]["result"] if 0<=index<count else {}
            if 0<=index<count:
                native_info=native["actions"][2]
                if native_info["status"]!="ok":record["differences"].append({"field":"native_source_info_read"})
                else:
                    record["differences"]+=delta(native_info["result"],{"tags":info["tags"],"opaque_tags":info.get("opaque_tags",{})})
                    interpretation=native_info["result"].get("compatibility")
                    record["source_interpretation"]=interpretation
                    if options and interpretation is None:record["differences"].append({"field":"selected_interpretation_missing_provenance"})
                    if interpretation:
                        record["differences"]+=interpretation_differences(info,interpretation)
                        source_bytes=pathlib.Path(path).read_bytes()
                        for entry in interpretation["source_entries"]:
                            start=entry["entry_offset"];encoded=bytes.fromhex(entry["ifd_entry_hex"])
                            if source_bytes[start:start+len(encoded)]!=encoded:record["differences"].append({"field":"compatibility_original_ifd_descriptor","tag":entry["tag"]})
            raw=None
            if 0<=index<count:
                try:
                    raw=raw_reference(path,index)
                    first=command.get("row_min",0);last=command.get("row_max",raw.shape[0]-1)
                    if 0<=first<=last<raw.shape[0]:
                        raw=raw[first:last+1]
                        record["stored_reference"]=signature(raw)
                        if native["actions"][0]["status"]=="ok":record["differences"]+=delta(native["actions"][0]["result"],record["stored_reference"])
                        else:record["differences"].append({"field":"stored_source_read_failed","native":native["actions"][0]})
                        if native["actions"][1]["status"]=="ok":
                            record["independent_view_reference"]=signature(expected_view(raw,info,mono))
                            record["differences"]+=delta(native["actions"][1]["result"],record["independent_view_reference"])
                            record["differences"]+=delta(native["actions"][1]["result"],{"rows":[first,last+1],"ifd_offset":info["ifd_offset"],
                                "source_orientation":info["tags"].get("274",1),"policy":"FabioTiffIoMono" if mono else "FabioTiffIo"})
                except Exception as error:record["independent_source_error"]={"type":type(error).__name__,"error":str(error)}
            issues,qualification=compare_read(native["actions"][1],reference,raw,command,info,mono)
            record["view_differences"]=issues;record["qualification"]=qualification
            if issues and not qualification:record["differences"]+=issues
            if qualification and "reference_" in qualification and 0<=index<count and "stored_reference" not in record:
                record["differences"].append({"field":"unverified_reference_difference"})
        record["source_unchanged"]=before==fingerprint(path)
        if not record["source_unchanged"]:record["differences"].append({"field":"source_changed"})
        record["status"]="difference" if record["differences"] else "qualified_difference" if record["qualification"] else "match"
        return record

    @contextlib.contextmanager
    def observe(self):
        original_init=self.cls.__init__;original_write=self.cls.writeImage
        def guarded(original):
            def call(instance,*args,**kwargs):
                self.depth+=1
                try:return original(instance,*args,**kwargs)
                finally:self.depth-=1
            return call
        def wrapper(name,original):
            def call(instance,*args,**kwargs):
                if self.depth:
                    self.internal[name]=self.internal.get(name,0)+1
                    return original(instance,*args,**kwargs)
                source_before=None
                try:source_before=fingerprint(instance.fd.name)
                except Exception as error:self.errors.append({"test":self.test,"api":name,"scope":"before_reference_source","error":str(error)})
                self.depth+=1
                reference=None
                try:
                    value=original(instance,*args,**kwargs)
                except Exception as error:
                    reference={"status":"error","error_type":type(error).__name__,"error":str(error)}
                    raise
                else:
                    try:
                        if name=="getNumberOfImages":reference={"status":"ok","result":{"images":value}}
                        elif name=="getImageFileDirectories":reference={"status":"ok","result":{"images":value,"directories":list(instance._IFD)}}
                        else:reference={"status":"ok","result":encode(value) if name=="getInfo" else signature(value)}
                    except Exception as error:self.errors.append({"test":self.test,"api":name,"scope":"return_capture","error":str(error)})
                finally:
                    self.depth-=1
                    try:
                        if reference is not None:
                            if source_before is not None:reference.update(source_before=source_before,source_after=fingerprint(instance.fd.name))
                            op={"getNumberOfImages":"count","getImageFileDirectories":"directories","getInfo":"info","getData":"data","getImage":"image"}[name]
                            self.events.append(self.verify(instance.fd.name,op,args[0] if args else None,reference,instance._forceMonoOutput,kwargs,"unchanged_upstream_test"))
                    except Exception as error:self.errors.append({"test":self.test,"api":name,"type":type(error).__name__,"error":str(error)})
                return value
            return call
        with contextlib.ExitStack() as stack:
            stack.enter_context(mock.patch.object(self.cls,"__init__",guarded(original_init)))
            stack.enter_context(mock.patch.object(self.cls,"writeImage",guarded(original_write)))
            for name in ("getNumberOfImages","getImageFileDirectories","getInfo","getData","getImage"):
                stack.enter_context(mock.patch.object(self.cls,name,wrapper(name,getattr(self.cls,name))))
            yield


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe",type=pathlib.Path,required=True);parser.add_argument("--fabio-source",type=pathlib.Path,required=True)
    parser.add_argument("--corpus",type=pathlib.Path);parser.add_argument("--report",type=pathlib.Path,required=True)
    parser.add_argument("--from-shape",type=pathlib.Path,action="append",default=[],help="explicit source paths selecting existing uncompressed FromShape; captured hash chooses policy before any pixel probe")
    args=parser.parse_args();import fabio
    if fabio.version!="2026.6.0" or subprocess.check_output(["git","-C",str(args.fabio_source),"rev-parse","HEAD"],text=True).strip()!=PIN:parser.error("requires pinned FabIO2026.6.0")
    module=importlib.import_module("fabio.test.test_tiffio");runtime=importlib.import_module("fabio.TiffIO")
    imports=[]
    for imported,relative in ((module,"test/test_tiffio.py"),(runtime,"TiffIO.py")):
        actual=pathlib.Path(inspect.getfile(imported));pinned=args.fabio_source/"src/fabio"/relative
        imports.append({"actual":fingerprint(actual),"pinned":fingerprint(pinned),"matches_pinned":actual.read_bytes()==pinned.read_bytes()})
    tracked=[pathlib.Path(item[key]["path"]) for item in imports for key in ("actual","pinned")]
    tracked += [pathlib.Path(__file__),ROOT/"scripts/test_tiff_api_reference.py",ROOT/"scripts/generate_tiff_api_fixtures.py",ROOT/"scripts/header_reference.py",ROOT/"scripts/metadata_parity.py",ROOT/"tests/tiff_api.rs",ROOT/"Cargo.toml",ROOT/"Cargo.lock",ROOT/"examples/api_probe.rs"]
    tracked += sorted((ROOT/"src").rglob("*.rs"))+sorted((ROOT/"tests/data/tiff-api").glob("*"))
    before={str(path):fingerprint(path) for path in tracked};probe_before=fingerprint(args.probe)
    policy_records=[{"source":fingerprint(path),"options":{"uncompressed_windows":"from_shape"},"selection":"explicit caller option, matched by immutable source SHA256 before decode; never a failed-read retry"} for path in args.from_shape]
    selected={row["source"]["sha256"]:row["options"] for row in policy_records}
    audit=Audit(Probe(args.probe),selected);outcomes=[]
    class Result(unittest.TextTestResult):
        def startTest(self,test):audit.test=test.id();outcomes.append(test.id());super().startTest(test)
    with audit.observe():result=unittest.TextTestRunner(verbosity=1,resultclass=Result).run(module.suite())
    observed=list(audit.events);audit.events=[];audit.test=None
    supplementary=[]
    paths=[ROOT/"tests/data/tiff-api"/name for name in fixtures()]
    for name,payload in fixtures().items():
        if (ROOT/"tests/data/tiff-api"/name).read_bytes()!=payload:raise ValueError("independent TIFF API fixture differs from generator")
    paths+=[ROOT/"tests/data/tiff/le-bigtiff.tif",ROOT/"tests/data/tiff/float64.tif"]
    if args.corpus:paths += [args.corpus/name for name in ("a0009.tif","fit2d.tif","frame_00017.tif","indexed_color.tif") if (args.corpus/name).is_file()]
    for path in paths:
        try:
            with audit.cls(str(path),cache_length=0) as source:
                count=source.getNumberOfImages()
        except Exception as error:
            reference_error={"status":"error","error_type":type(error).__name__,"error":str(error)}
            with tifffile.TiffFile(path) as source:count=len(source.pages)
            metadata=Probe(args.probe)("tiff-metadata-api",path,[{"op":"count"},{"op":"directories"}]+[{"op":"info","index":i} for i in range(count)])
            issues=validate_metadata(path,metadata) if metadata["status"]=="ok" else [{"field":"native_metadata_open"}]
            supplementary.append({"path":str(path),"source":fingerprint(path),"reference_open_error":reference_error,"metadata":metadata,"differences":issues,
                                  "status":"qualified_difference" if not issues else "difference","qualification":"reference_format_unsupported"})
            for index in range(count):supplementary.append(audit.verify(path,"data",index,reference_error))
            continue
        calls=[("count",None,False,{}),("directories",None,False,{})]
        for index in range(count):calls += [("info",index,False,{}),("data",index,False,{}),("image",index,False,{}),("data",index,True,{})]
        if count:calls += [("info",-1,False,{}),("data",-1,False,{}),("data",0,False,{"rowMin":0,"rowMax":0}),("data",0,False,{"rowMin":-1,"rowMax":0}),("data",count,False,{})]
        for operation,index,mono,kwargs in calls:
            try:
                with audit.cls(str(path),cache_length=0,mono_output=mono) as source:reference=reference_call(source,operation,index,kwargs)
                supplementary.append(audit.verify(path,operation,index,reference,mono,kwargs))
            except Exception as error:
                supplementary.append({"path":str(path),"operation":operation,"index":index,"status":"difference","audit_error":{"type":type(error).__name__,"error":str(error)}})
        with audit.cls(str(path),cache_length=0) as source:
            source.getNumberOfImages()
            reference=reference_call(source,"directories")
        supplementary.append(audit.verify(path,"directories",None,reference))
    all_records=observed+supplementary
    report={"schema":1,"contract":__doc__,"reference_commit":PIN,"reference_tests":{"run":result.testsRun,"failures":len(result.failures),"errors":len(result.errors),"skips":len(result.skipped),"ids":outcomes},
            "observed":observed,"internal_fixture_or_nested_calls_excluded":audit.internal,"supplementary":supplementary,"observer_errors":audit.errors,
            "explicit_source_policies":policy_records,"policy_sources_unchanged":all(fingerprint(row["source"]["path"])==row["source"] for row in policy_records),
            "reference_imports":imports,"environment":{"fabio":fabio.version,"numpy":np.__version__,"tifffile":tifffile.__version__},
            "sources":before,"probe":probe_before,"sources_unchanged":all(fingerprint(path)==value for path,value in before.items()),"probe_unchanged":fingerprint(args.probe)==probe_before}
    report["all_qualified"]=(result.wasSuccessful() and result.testsRun==3 and len(observed)==5 and not audit.errors and all(item["matches_pinned"] for item in imports)
                             and report["sources_unchanged"] and report["probe_unchanged"] and report["policy_sources_unchanged"] and all(row.get("status") in ("match","qualified_difference") for row in all_records))
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+"\n")
    print(json.dumps({"qualified":report["all_qualified"],"reference_tests":result.testsRun,"observed_reads":len(observed),"supplementary_reads":len(supplementary),"report":str(args.report)}))
    return 0 if report["all_qualified"] else 1


if __name__=="__main__":raise SystemExit(main())
