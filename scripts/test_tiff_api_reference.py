"""Controls for low-level metadata, source arrays, explicit views and observers."""
import copy
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock
import numpy as np
import tifffile
from tiff_api_reference import (Audit, Probe, compare_info, compare_read, delta,
                                expected_view, interpretation_differences, project_info, signature)
from generate_tiff_api_fixtures import fixtures


def info():
    return {"ifd_offset":8,"tags":{"256":2,"257":3,"258":8,"259":1,"262":2,"273":[100,106,112],"277":3,"278":1,"279":[6,6,6],
            "274":6,"270":"Title=original\n","305":"PyMca source","306":"original date"},"opaque_tags":{}}


class TiffControls(unittest.TestCase):
    def test_original_info_fields_defaults_and_source_values_are_all_compared(self):
        native=info();before=copy.deepcopy(native)
        wanted=project_info(native,0,1)
        self.assertEqual(native,before)
        self.assertFalse(compare_info(wanted,native,0,1))
        for field in wanted:
            altered=copy.deepcopy(wanted);del altered[field]
            # Every reference field matters; compare both directions so missing
            # observed coverage cannot be mistaken for a partial metadata match.
            self.assertTrue(delta(altered,wanted),field)
            self.assertTrue(compare_info(altered,native,0,1),field)
        changed=copy.deepcopy(native);changed["tags"]["273"][1]=999
        self.assertTrue(compare_info(wanted,changed,0,1))

    def test_stored_u64_bits_and_float64_reserved_words_are_not_numeric_approximations(self):
        first=signature(np.asarray([0,(1<<53)+1,(1<<64)-1],dtype=">u8"))
        second=signature(np.asarray([0,(1<<53)+1,(1<<64)-1],dtype="<u8"))
        self.assertEqual(first,second)
        floats=np.asarray([0x8000000000000000,0x7ff8000000000042],dtype="<u8").view("<f8")
        before=signature(floats);floats[0]=0
        self.assertTrue(delta(signature(floats),before))

    def test_palette_source_indices_are_not_replaced_by_the_explicit_view(self):
        source=np.asarray([[0,1]],dtype="uint16")
        metadata={"tags":{"262":3,"320":[0,65535,65535,0,0,32768]}}
        result=expected_view(source,metadata,False)
        self.assertEqual(result.dtype,np.dtype("uint16"));self.assertEqual(result.tolist(),[[[0,255,0],[255,0,128]]])
        self.assertEqual(source.tolist(),[[0,1]])

    def test_literal_bgr_weights_and_float32_promotion_remain_explicit(self):
        source=np.asarray([[[255,0,0],[0,0,255]]],dtype="uint8")
        view=expected_view(source,info(),True)
        self.assertEqual(view.dtype,np.dtype("float32"));self.assertEqual(view.tolist(),[[np.float32(255*.114).item(),np.float32(255*.299).item()]])
        source=np.asarray([[[1e-20,1e10,-1e5]]],dtype="float32")
        self.assertEqual(expected_view(source,info(),True).dtype,np.dtype("float32"))

    def test_errors_must_not_expose_a_stale_data_buffer(self):
        reference={"status":"error","error":"out of bounds"}
        good={"status":"error","error":"out of bounds","pixels_available":False}
        self.assertFalse(compare_read(good,reference,None,{"index":-1},{},False)[0])
        changed={**good,"pixels_available":True}
        self.assertTrue(compare_read(changed,reference,None,{"index":-1},{},False)[0])

    def test_probe_checks_command_correspondence_and_missing_action_results(self):
        probe=Probe("/unused/probe")
        for response in ["{\"actions\":[]}","{\"actions\":[{\"command\":{\"op\":\"wrong\"}}]}"]:
            with mock.patch("tiff_api_reference.subprocess.run",return_value=subprocess.CompletedProcess([],0,response,"")):
                self.assertEqual(probe("tiff-api","/unused/file",[{"op":"count"}])["status"],"error")

    def test_observer_restores_reference_methods_even_on_failure(self):
        audit=Audit(None);original=audit.cls.getData
        with self.assertRaises(RuntimeError):
            with audit.observe():raise RuntimeError("control")
        self.assertIs(audit.cls.getData,original)

    def test_observer_capture_failure_does_not_change_reference_return_or_cache(self):
        path=pathlib.Path(__file__).resolve().parents[1]/"tests/data/tiff-api/white.tif"
        audit=Audit(lambda *_:self.fail("capture failure must not invent native evidence"))
        with audit.observe(),mock.patch("tiff_api_reference.signature",side_effect=RuntimeError("capture failure")):
            with audit.cls(str(path)) as reader:data=reader.getData(0)
        self.assertEqual(data.tolist(),[[0,1],[255,128],[17,254]])
        self.assertTrue(audit.errors)
        self.assertEqual(audit.errors[0]["scope"],"return_capture")

    def test_unknown_reference_failures_are_not_relabelled_unsupported_formats(self):
        native={"status":"ok","result":signature(np.zeros((1,2),dtype="uint8"))}
        reference={"status":"error","error_type":"MemoryError","error":"resource failure"}
        issues,qualification=compare_read(native,reference,None,{"index":0},info(),False)
        self.assertTrue(issues);self.assertIsNone(qualification)

    def test_palette_window_defect_is_narrowly_classified_without_hiding_native_view_checks(self):
        native={"status":"ok","result":signature(np.zeros((2,2,3),dtype="uint8"))}
        reference={"status":"error","error_type":"ValueError","error":"could not broadcast input array from shape (6,2,3) into shape (2,2,3)"}
        source={"tags":{"262":3,"273":174}}
        issues,qualification=compare_read(native,reference,None,{"index":0},source)
        self.assertFalse(issues);self.assertEqual(qualification,"reference_single_strip_palette_window_defect")
        source["tags"]["262"]=1
        self.assertTrue(compare_read(native,reference,None,{"index":0},source)[0])

    def test_explicit_window_provenance_cannot_invent_original_tags_or_wrong_sizes(self):
        source=info();source["tags"]["262"]=1;source["tags"]["277"]=1
        interpretation={"source_ifd_offset":8,"kind":"Strips","rows_per_strip":1,
                        "operations":["UncompressedWindowsFromShape"],
                        "source_entries":[{"tag":int(key)} for key in source["tags"]],
                        "windows":[{"offset":offset,"bytes":2} for offset in [100,106,112]]}
        self.assertFalse(interpretation_differences(source,interpretation))
        for field in ("source_ifd_offset","rows_per_strip","source_entries","windows"):
            changed=copy.deepcopy(interpretation)
            changed[field]=0 if field in ("source_ifd_offset","rows_per_strip") else []
            self.assertTrue(interpretation_differences(source,changed),field)

    def test_independent_files_match_generator_and_nonmonotonic_chain(self):
        root=pathlib.Path(__file__).resolve().parents[1]/"tests/data/tiff-api"
        for name,contents in fixtures().items():self.assertEqual((root/name).read_bytes(),contents)
        with tifffile.TiffFile(root/"stack-nonmonotonic.tif") as source:
            self.assertEqual([page.offset for page in source.pages],[512,8]);self.assertEqual(source.pages[0].asarray().tolist(),[[0,65535],[32769,1],[2,3]])


if __name__=="__main__":unittest.main()
