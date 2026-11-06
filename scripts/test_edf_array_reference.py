"""Independent controls for source-rank/word/plane and partial-integrity gates."""
import copy
import pathlib
import unittest
import zlib
import numpy as np
from edf_array_reference import compare_array, compare_recovery, decode, expected_recovery, geometry, parse, signature, gate
from generate_edf_array_fixtures import fixtures


class ArrayControls(unittest.TestCase):
    def test_fixture_generation_is_exact_and_source_words_never_float_roundtrip(self):
        directory=pathlib.Path(__file__).resolve().parents[1]/"tests/data/edf-arrays"
        for name,payload in fixtures().items():self.assertEqual((directory/name).read_bytes(),payload,name)
        record=parse(fixtures()["rank4-gzip.edf"])[0];values=decode(record)
        self.assertEqual(values.reshape(-1)[:6].tolist(),[-(1<<63),(1<<63)-1,-1,0,(1<<53)+1,-((1<<53)+1)])
        bits=decode(parse(fixtures()["rank5-zlib.edf"])[0]).view(np.uint64).reshape(-1)
        self.assertEqual(int(bits[1]),0x8000000000000000)
        self.assertEqual(int(bits[2]),0x7ff8000000000042)

    def test_effective_geometry_uses_highest_numeric_key_without_changing_fields(self):
        fields={"DIM_1":"3","DIM_4":"2","DIM_H":"unrelated"};original=copy.deepcopy(fields)
        self.assertEqual(geometry(fields)["source_shape"],[2,1,1,3])
        self.assertEqual(geometry(fields)["implicit_dimensions"],[2,3]);self.assertEqual(fields,original)
        self.assertEqual(geometry({"DIM_1":"3","DIM_003":"17"})["source_shape"],[1,1,3])
        self.assertEqual(geometry({})["source_samples"],1)
        for invalid in [{"DIM_1":"0"},{"DIM_2":"2"},{"DIM_33":"1"},{"DIM_1":"64000001"},{"DIM_1":"2.5"}]:
            with self.assertRaises(ValueError):geometry(invalid)

    def test_all_array_fields_original_header_and_source_record_mapping_are_required(self):
        record=parse(fixtures()["volume-u16-be.edf"])[0];values=decode(record)
        native={"records":1,"metadata":record["metadata"],"array":signature(values),"plane":signature(values[1])}
        self.assertFalse(compare_array(native,record,values,1,[1]))
        for key in native:
            altered=copy.deepcopy(native);del altered[key]
            self.assertTrue(compare_array(altered,record,values,1,[1]),key)
        for key in native["metadata"]:
            altered=copy.deepcopy(native);del altered["metadata"][key]
            self.assertTrue(compare_array(altered,record,values,1,[1]),key)
        for shape in ([6,4],[2,12],[24]):
            altered=copy.deepcopy(native);altered["array"]["shape"]=shape
            self.assertTrue(compare_array(altered,record,values,1,[1]))
        altered=copy.deepcopy(native);altered["records"]=2
        self.assertTrue(compare_array(altered,record,values,1,[1]))
        altered=copy.deepcopy(native);altered["plane"]=signature(values[0])
        self.assertTrue(compare_array(altered,record,values,1,[1]))
        altered=copy.deepcopy(native);altered["array"]["pixels"]["dtype"]="float64"
        self.assertTrue(compare_array(altered,record,values,1,[1]))

    def test_mixed_source_records_do_not_expand_volume_axes_into_records(self):
        records=parse(fixtures()["mixed-records.edf"])
        self.assertEqual([r["metadata"]["record_index"] for r in records],[0,1,2])
        self.assertEqual([decode(r).shape for r in records],[(2,3,4),(2,2),()])
        self.assertEqual([r["metadata"]["plane_count"] for r in records],[2,1,1])

    def recovery_native(self,name):
        record=parse(fixtures()[name])[0];image,members,stop=expected_recovery(record)
        image=copy.deepcopy(image);image["availability"]["payload_members"]=copy.deepcopy(members)
        source_bytes=record["offset"]+len(record["encoded"])
        return record,{"images":[image],"stop":stop,"source_report":{"encoded_bytes":source_bytes,"decoded_bytes":source_bytes,"compression":"None","integrity":"ContainerComplete","verified_decoded_bytes":source_bytes,"members":[]}}

    def test_missing_partial_words_and_geometry_are_separate_from_integrity(self):
        record,native=self.recovery_native("volume-truncated.edf")
        self.assertFalse(compare_recovery(native,record))
        availability=native["images"][0]["availability"]
        self.assertEqual(availability["present_samples"],[0,5]);self.assertEqual(availability["missing_samples"],[5,24])
        self.assertEqual(availability["partial_element_bytes"],"feff00")
        for key in availability:
            altered=copy.deepcopy(native);del altered["images"][0]["availability"][key]
            self.assertTrue(compare_recovery(altered,record),key)
        altered=copy.deepcopy(native);altered["images"][0]["shape"]=[24]
        self.assertTrue(compare_recovery(altered,record))
        altered=copy.deepcopy(native);altered["images"][0]["availability"]["partial_element_bytes"]="feff000000000000"
        self.assertTrue(compare_recovery(altered,record))

    def test_all_words_with_corrupt_or_missing_footer_never_become_verified(self):
        for name,integrity in [("volume-corrupt-member.edf","ChecksumFailure"),("volume-footer-short.edf","UnverifiedTruncated")]:
            record,native=self.recovery_native(name);availability=native["images"][0]["availability"]
            self.assertEqual(availability["present_samples"],[0,12]);self.assertEqual(availability["missing_samples"],None)
            self.assertEqual(availability["payload_integrity"],integrity)
            self.assertFalse(availability["complete_verified_record"])
            self.assertFalse(compare_recovery(native,record))
            for key,value in [("complete_verified_record",True),("verified_samples",[0,12]),("payload_integrity","VerifiedMember")]:
                altered=copy.deepcopy(native);altered["images"][0]["availability"][key]=value
                self.assertTrue(compare_recovery(altered,record),key)
            altered=copy.deepcopy(native);altered["images"][0]["availability"]["payload_members"][0]["encoded_range"][1]-=1
            self.assertTrue(compare_recovery(altered,record))
            with self.assertRaises((ValueError,EOFError,OSError,zlib.error)):decode(record)

    def test_scalar_and_rank1_are_distinct_from_explicit_singleton_planes(self):
        for name,shape,plane in [("scalar.edf",[],[1,1]),("rank1.edf",[6],[1,6])]:
            record=parse(fixtures()[name])[0];source=decode(record)
            native={"records":1,"metadata":record["metadata"],"array":signature(source),"plane":signature(source.reshape(plane))}
            self.assertFalse(compare_array(native,record,source,1,[]))
            self.assertEqual(native["array"]["shape"],shape)
            self.assertEqual(native["plane"]["shape"],plane)

    def test_outer_source_range_and_integrity_cannot_be_omitted(self):
        record,native=self.recovery_native("volume-header-only.edf")
        for key in native["source_report"]:
            altered=copy.deepcopy(native);del altered["source_report"][key]
            self.assertTrue(compare_recovery(altered,record),key)
        altered=copy.deepcopy(native);altered["source_report"]["integrity"]="UnverifiedTruncated"
        self.assertTrue(compare_recovery(altered,record))

    def test_report_gate_requires_every_source_record_plane_control_and_fingerprint(self):
        summary={"complete_sources":14,"complete_records":19,"explicit_planes":30,"recovery_sources":4,"reference_tests":1,"reference_failures":0,"reference_errors":0,"source_failures":0,"array_or_plane_differences":0,"recovery_differences":0,"imports_unchanged":True,"sources_stable":True,"probes_stable":True}
        self.assertTrue(gate(summary,True))
        for key in summary:
            altered=copy.deepcopy(summary);del altered[key];self.assertFalse(gate(altered,True),key)
            altered=copy.deepcopy(summary);altered[key]=not summary[key] if isinstance(summary[key],bool) else summary[key]+1
            self.assertFalse(gate(altered,True),key)
        for key in ["imports_unchanged","sources_stable","probes_stable"]:
            altered=copy.deepcopy(summary);altered[key]=1;self.assertFalse(gate(altered,True),key)


if __name__=="__main__":unittest.main()
