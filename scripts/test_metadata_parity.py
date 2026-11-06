"""Test-only comparison controls: mismatches must remain visible."""
import copy
import struct
import unittest

import numpy as np
from metadata_parity import compare, encode, project, reference_qualifications, summarize, compact


class MetadataControls(unittest.TestCase):
    def test_types_are_exact_except_named_format_projections(self):
        for wrong in (True, 1.0, "1"):
            row = compare({"answer":1}, {"fields":{"answer":wrong}}, "unknown")
            self.assertEqual(row["unresolved_fields"],1)
        row = compare({"HDRBLKS":5}, {"fields":{"HDRBLKS":"5"}}, "bruker100image")
        self.assertEqual(row["status"],"match")
        self.assertEqual(row["field_mappings"]["HDRBLKS"]["kind"],"representation")
        self.assertEqual(compare({}, {"fields":{"extra":1}}, "unknown")["status"],"reference_empty")

    def test_array_type_shape_and_all_sample_bits_gate_comparison(self):
        array = encode(np.array([[0.5,1.5]],dtype="float32"))
        native = copy.deepcopy(array)
        del native["values"]  # Optional diagnostic preview, not array identity.
        frame = {"fields":{"axis":native}}
        self.assertEqual(compare({"axis":array},frame,"unknown")["status"],"match")
        for key, wrong in [("dtype","float64"),("shape",[2,1]),("sha256","0"*64)]:
            changed = copy.deepcopy(frame)
            changed["fields"]["axis"][key] = wrong
            self.assertEqual(compare({"axis":array},changed,"unknown")["unresolved_fields"],1)

    def test_edf_case_mapping_does_not_hide_missing_or_changed_fields(self):
        frame = {"fields":{"DIM_1":"2","IMAGE":"1"}}
        self.assertEqual(compare({"Dim_1":"2","Image":"1"},frame,"edfimage")["status"],"match")
        self.assertEqual(compare({"Dim_1":"3"},frame,"edfimage")["unresolved_fields"],1)
        self.assertEqual(compare({"Dim_2":"3"},frame,"edfimage")["unresolved_fields"],1)

    def test_tiff_defaults_and_palette_view_are_explicit(self):
        frame = {"frame":0,"frames":1,"fields":{"shape":[1,2],"tags":{"256":2,"257":1,"258":8,"273":100,"320":[0,65535,0,32768,0,256]}}}
        view,mappings = project("tifimage",frame,"TiffIO")
        self.assertEqual(mappings["software"]["kind"],"synthetic_default")
        self.assertEqual(view["colormap"], encode(np.array([[0,0,0],[255,128,1]],dtype="uint8")))
        expected = {"colormap":view["colormap"]}
        frame["fields"]["tags"]["320"][1]=32768
        self.assertEqual(compare(expected,frame,"tifimage","TiffIO")["unresolved_fields"],1)

    def test_fit2d_reference_bug_requires_exact_source_and_reference_models(self):
        record = (b"\\beam:00000000r40200000").ljust(512,b" ")
        constant = struct.unpack("<f", bytes.fromhex("17b7d138"))[0]
        frame = {"fields":{"beam":2.5},"metadata_details":{"header_hex":record.hex(),"block_bytes":512}}
        reference = {"beam":constant}
        self.assertEqual(compare(reference,frame,"fit2dimage")["status"],"qualified_reference_difference")
        for wrong in [2.0, constant]:
            changed = copy.deepcopy(frame)
            changed["fields"]["beam"]=wrong
            self.assertEqual(reference_qualifications("fit2dimage",reference,changed,changed["fields"]),{})
        self.assertEqual(compare({"beam":0.1},frame,"fit2dimage")["unresolved_fields"],1)

    def test_mrc_reference_bug_requires_correct_native_semantics(self):
        raw = bytearray(1024)
        struct.pack_into("<f",raw,40,2.5)
        struct.pack_into("<3f",raw,196,1.25,2.5,3.75)
        raw[208:212]=b"MAP "
        struct.pack_into("<f",raw,216,4.5)
        frame = {"fields":{"CELL_A":2.5,"ORIGIN":[1.25,2.5,3.75],"MAP":"MAP ","RMS":4.5},
                 "metadata_details":{"header_hex":raw.hex(),"little_endian":True}}
        reference = {"CELL_A":struct.unpack_from("<i",raw,40)[0],"ORIGIN":0,"MAP":"\0\0\0\0","RMS":0}
        row = compare(reference,frame,"mrcimage")
        self.assertEqual(row["status"],"qualified_reference_difference")
        self.assertEqual(row["qualified_fields"],4)
        changed = copy.deepcopy(frame)
        changed["fields"]["CELL_A"]=2.0
        self.assertEqual(compare(reference,changed,"mrcimage")["unresolved_fields"],1)
        reference["CELL_A"]+=1
        self.assertEqual(compare(reference,frame,"mrcimage")["unresolved_fields"],1)

    def test_ge_fallback_view_never_asserts_fake_native_acquisition_fields(self):
        raw = bytes(8192)
        frame = {"fields":{},"metadata_details":{"kind":"ApsBlank8192","header_hex":raw.hex(),"shape":[2048,2048],"frames":2}}
        reference = {"NumberOfFrames":2,"NumberOfRowsInFrame":2048,"AcquisitionTime":encode(bytes(20))}
        row = compare(reference,frame,"GEimage")
        self.assertEqual(row["status"],"qualified_reference_difference")
        self.assertEqual(row["matched_fields"],0)
        changed = copy.deepcopy(frame)
        changed["metadata_details"]["header_hex"] = (b"x"+raw[1:]).hex()
        self.assertEqual(compare(reference,changed,"GEimage")["unresolved_fields"],3)
        changed = copy.deepcopy(frame)
        changed["metadata_details"]["kind"]="Standard8001"
        self.assertEqual(compare(reference,changed,"GEimage")["unresolved_fields"],3)

    def test_compact_report_preserves_named_field_schemas_and_failed_checks(self):
        read = {"codec":"edfimage","reference_header":{"Image":"1"},
            "metadata":[{"frame":0,**compare({"Image":"2"},{"fields":{"IMAGE":"1"}},"edfimage")}]}
        rows = [read]
        summary = summarize(rows)
        self.assertEqual(summary["edfimage"]["field_names"],["IMAGE"])
        self.assertEqual(summary["edfimage"]["unresolved_fields"],1)
        differences = copy.deepcopy(read["metadata"][0]["differences"])
        schemas = compact(rows)
        schema = schemas[read["metadata"][0]["mapping_schema_sha256"]]
        self.assertEqual(schema["reference_keys"],["IMAGE"])
        self.assertEqual(schema["field_mappings"]["IMAGE"]["kind"],"case_normalization")
        self.assertEqual(read["metadata"][0]["differences"],differences)


if __name__ == "__main__":
    unittest.main()
