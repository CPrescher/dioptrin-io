"""Independent controlled-header checks for the pinned FIT2D and MRC defects.

Changes only metadata in small licensed fixtures; identical native count hashes
are required. The actual FabIO reader and native executable read each variant.
"""
import argparse
import copy
import hashlib
import json
import pathlib
import struct
import subprocess
import tempfile

import fabio
from metadata_parity import compare, encode

ROOT = pathlib.Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--probe",type=pathlib.Path,required=True)
    parser.add_argument("--report",type=pathlib.Path,required=True)
    args = parser.parse_args()
    if fabio.version != "2026.6.0": raise ValueError("requires pinned FabIO 2026.6.0")
    probe = args.probe.resolve()
    probe_hash = sha(probe)

    def native(kind,path):
        result = subprocess.run([str(probe),kind,str(path)],capture_output=True,text=True,check=True,timeout=60)
        return [json.loads(line) for line in result.stdout.splitlines()]

    cases = []
    sources = {}
    for codec, adapter, fixture, values in [
        ("fit2dimage","fit2d","records/fit2d-512-little.f2d",[0x3e800000,0x3f000000,0x40000000]),
        ("mrcimage","mrc","containers/mrc-mode2-le.mrc",[2.5,5.0]),
    ]:
        source = ROOT/"tests/data"/fixture
        original = source.read_bytes()
        sources[str(source.relative_to(ROOT))] = sha(source)
        baseline_counts = [{key:frame[key] for key in ("dtype","shape","sample_sha256")} for frame in native(adapter,source)]
        for value in values:
            raw = bytearray(original)
            if codec == "fit2dimage":
                offset = raw.index(b"\\gain:")+len(b"\\gain:")+9
                raw[offset:offset+8] = f"{value:08x}".encode("ascii")
                expected = {"gain":struct.unpack("<f",struct.pack("<I",value))[0]}
            else:
                struct.pack_into("<f",raw,40,value)
                struct.pack_into("<3f",raw,196,1.25,value,3.75)
                struct.pack_into("<f",raw,216,4.5)
                struct.pack_into("<i",raw,220,2)
                expected = {"CELL_A":value,"ORIGIN":[1.25,value,3.75],"MAP":"MAP ","RMS":4.5,"NLABL":2}
            with tempfile.TemporaryDirectory(prefix="dioptrin-metadata-control-") as directory:
                path = pathlib.Path(directory)/source.name
                path.write_bytes(raw)
                frames = native(adapter,path)
                image = (fabio.fit2dimage.Fit2dImage if codec == "fit2dimage" else fabio.mrcimage.MrcImage)().read(str(path))
                reference = {key:encode(image.header[key]) for key in expected}
            counts = [{key:frame[key] for key in ("dtype","shape","sample_sha256")} for frame in frames]
            assert counts == baseline_counts
            for key,item in expected.items(): assert frames[0]["fields"][key] == item
            result = compare(reference,frames[0],codec)
            assert result["status"] == "qualified_reference_difference",result
            assert result["qualified_fields"] == len(expected),result
            # Every false native value must lose qualification. A different
            # reference value must lose qualification too; no filename allowlist.
            controls = []
            for key in expected:
                bad_native = copy.deepcopy(frames[0])
                bad_native["fields"][key] = "wrong source metadata"
                native_check = compare(reference,bad_native,codec)
                bad_reference = dict(reference)
                bad_reference[key] = "wrong reference metadata"
                reference_check = compare(bad_reference,frames[0],codec)
                assert native_check["unresolved_fields"] == reference_check["unresolved_fields"] == 1
                controls.append({"field":key,"native_change_rejected":True,"reference_change_rejected":True})
            cases.append({"codec":codec,"fixture":fixture,"modified_file_sha256":hashlib.sha256(raw).hexdigest(),
                "stored_values":expected,"reference_values":reference,"native_values":{key:frames[0]["fields"][key] for key in expected},
                "counts_unchanged":True,"frame_count":len(frames),"count_signatures":counts,"comparison":result,"negative_controls":controls})
    assert sha(probe) == probe_hash
    assert all(sha(ROOT/path)==digest for path,digest in sources.items())
    report = {"qualified":True,"fabio":fabio.version,"probe_sha256":probe_hash,"script_sha256":sha(pathlib.Path(__file__)),
        "native_sources_sha256":{str(p.relative_to(ROOT)):sha(p) for p in sorted((ROOT/"src").rglob("*.rs"))},
        "reference_sources_sha256":{name:sha(pathlib.Path(module.__file__)) for name,module in [("fit2dimage",fabio.fit2dimage),("mrcimage",fabio.mrcimage)]},
        "source_fixtures_sha256":sources,"cases":cases,"scope":"Five controlled metadata variants; unchanged native typed count signatures; real pinned FabIO reads; per-field negative controls. Does not claim general format or API parity."}
    args.report.parent.mkdir(parents=True,exist_ok=True)
    args.report.write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({"qualified":True,"cases":len(cases),"negative_controls":sum(len(c["negative_controls"])*2 for c in cases)}))


if __name__ == "__main__":
    main()
