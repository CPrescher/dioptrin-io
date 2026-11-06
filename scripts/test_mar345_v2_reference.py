"""Oracle-admission, original-word/provenance and classification controls."""
import copy
import pathlib
import tempfile
import unittest
from generate_mar345_v2_fixtures import fixtures
from generate_mar345_original_v2_fixtures import fixtures as original_fixtures
from mar345_v2_reference import canonical,parse,default_safe,compare_native,qualify_oracle,gate,sha,c_oracle,qualify_default


class V2Controls(unittest.TestCase):
    def test_independent_generator_complete_selector_and_run_alphabet(self):
        directory=pathlib.Path(__file__).resolve().parents[1]/"tests/data/mar345-v2"
        selectors=set();runs=set()
        for name,(payload,values,provenance) in fixtures().items():
            self.assertEqual((directory/name).read_bytes(),payload)
            record=parse(payload);selectors.update(record["selectors"]);runs.update(record["run_exponents"])
            self.assertEqual(sha(canonical(values)),provenance["sample_sha256"])
            self.assertEqual(record["metadata"]["shape"],provenance["metadata"]["shape"])
        self.assertEqual(selectors,set(range(15)));self.assertEqual(runs,set(range(16)))

    def test_no_unsafe_C_call_for_reserved_overrun_truncated_tail_or_invalid_high_table(self):
        original=fixtures()["v2-width4.mar345"][0];record=parse(original);start=record["offset"]
        invalid=[original[:4095],original[:-1],original+b"\0",b"\0"*len(original)]
        for value in (0xf0,0x0f):
            altered=bytearray(original);altered[start]=value;invalid.append(bytes(altered))
        original=fixtures()["v2-partial-last-byte.mar345"][0];altered=bytearray(original);altered[-1]|=128;invalid.append(bytes(altered))
        original=fixtures()["v2-overflow-padded-le.mar345"][0]
        for at,word in [(4096,0),(4096,33),(4104,1),(4128,1)]:
            altered=bytearray(original);altered[at:at+4]=word.to_bytes(4,"little");invalid.append(bytes(altered))
        for payload in invalid:
            with self.assertRaises(ValueError):parse(payload)

    def test_C_oracle_requires_fresh_identical_source_admission(self):
        original=fixtures()["v2-width4.mar345"][0];record=parse(original)
        with tempfile.TemporaryDirectory() as directory:
            path=pathlib.Path(directory)/"source";path.write_bytes(original[:-1])
            with self.assertRaises(ValueError):c_oracle(pathlib.Path("/never-executed"),path,record)

    def test_all_count_words_source_metadata_and_type_fields_are_required(self):
        payload,values,_=fixtures()["v2-overflow-padded-be.mar345"];record=parse(payload);raw=canonical(values)
        good={"frames":1,"shape":record["metadata"]["shape"],"pixel_type":"U32","pixels":{"dtype":"uint32","sample_sha256":sha(raw)},"value_hex":raw.hex(),"metadata":record["metadata"]}
        self.assertFalse(compare_native(good,record,values))
        for key in good:
            bad=copy.deepcopy(good);del bad[key];self.assertTrue(compare_native(bad,record,values),key)
        for key in good["metadata"]:
            bad=copy.deepcopy(good);del bad["metadata"][key];self.assertTrue(compare_native(bad,record,values),key)
        for key,value in [("pck_version",1),("little_endian",True),("overflow_records",[]),("shape",[8,4])]:
            bad=copy.deepcopy(good);bad["metadata"][key]=value;self.assertTrue(compare_native(bad,record,values),key)
        bad=copy.deepcopy(good);bad["value_hex"]="00000000"+bad["value_hex"][8:];self.assertTrue(compare_native(bad,record,values))

    def test_unexplained_oracle_disagreement_never_qualifies_as_reference_bug(self):
        payload,values,_=fixtures()["v2-width4.mar345"];record=parse(payload)
        self.assertEqual(qualify_oracle(record,values,values)["classification"],"exact_original_words")
        altered=values.copy();altered[4]^=1
        with self.assertRaises(ValueError):qualify_oracle(record,values,altered)

    def test_default_unsafe_V1_reinterpretation_is_separate_from_valid_V2(self):
        zero=parse(fixtures()["v2-zero-run1.mar345"][0]);self.assertTrue(default_safe(zero)[0])
        self.assertTrue(any(not default_safe(parse(case[0]))[0] for case in fixtures().values()))
        white=parse(fixtures()["v2-partial-last-byte.mar345"][0]);self.assertEqual(white["encoded"][0],32)
        self.assertNotEqual(white["encoded"],white["encoded"].lstrip())

    def test_proof_gate_requires_full_cardinality_all_guards_and_oracle_qualification(self):
        summary={"original_v2_sources":32,"original_v2_differences":0,"original_selectors":list(range(16)),"original_run_exponents":list(range(8)),"original_writer_roundtrips":5,"original_writer_differences":0,"original_default_equal":10,"original_default_wrong_words":8,"original_default_unsafe_not_called":14,"independent_v2_sources":39,"v2_native_differences":0,"selectors":list(range(15)),"run_exponents":list(range(16)),"actual_v1_sources":2,"actual_v1_differences":0,"errors":0,"sources_stable":True,"probe_stable":True,"imports_unchanged":True,"primary_unchanged":True,"oracle_exact":37,"oracle_qualified_unsigned_predictor":2,"default_called":21,"default_equal":14,"default_wrong_words":5,"default_big_endian_errors":2,"default_unsafe_not_called":18}
        self.assertTrue(gate(summary,True))
        for key in summary:
            bad=copy.deepcopy(summary);del bad[key];self.assertFalse(gate(bad,True),key)
        for key in ["sources_stable","probe_stable","imports_unchanged","primary_unchanged"]:
            bad=copy.deepcopy(summary);bad[key]=1;self.assertFalse(gate(bad,True),key)
        for key,value in [("oracle_exact",40),("default_called",0),("v2_native_differences",1),("errors",1),("selectors",list(range(16)))]:
            bad=copy.deepcopy(summary);bad[key]=value;self.assertFalse(gate(bad,True),key)

    def test_unrelated_reference_error_or_crash_never_qualifies_as_format_gap(self):
        payload,words,_=fixtures()["v2-width4.mar345"];record=parse(payload);path=pathlib.Path("source.mar345")
        for failure in [{"status":"isolated_crash","returncode":-11},{"status":"error","type":"ImportError","error":"missing module"},{"status":"error","type":"OSError","error":"No such file"},{"status":"not_called_unsafe_v1_interpretation"}]:
            with self.assertRaises(ValueError):qualify_default(failure,record,words,path)

    def test_original_seven_bit_selector15_is_valid_and_profiles_are_explicit(self):
        directory=pathlib.Path(__file__).resolve().parents[1]/"tests/data/mar345-v2-original"
        selectors=set();runs=set()
        for name,(payload,values,provenance) in original_fixtures().items():
            self.assertEqual((directory/name).read_bytes(),payload)
            record=parse(payload,"original");selectors.update(record["selectors"]);runs.update(record["run_exponents"])
            self.assertEqual(record["metadata"]["pck_v2_dialect"],"Original")
            self.assertEqual(sha(canonical(values)),provenance["sample_sha256"])
        self.assertEqual(selectors,set(range(16)));self.assertEqual(runs,set(range(8)))
        payload=original_fixtures()["original-width32.mar345"][0];record=parse(payload,"original")
        self.assertEqual(record["selectors"],[15]*32)
        with self.assertRaises(ValueError):parse(payload,"fabio8")
        payload=fixtures()["v2-width4.mar345"][0]
        with self.assertRaises(ValueError):parse(payload,"original")

    def test_original_packer_rejects_malformed_authored_word_inputs_before_execution(self):
        from mar345_original_v2_oracle import roundtrip
        for shape,words in [([2,2],[0,1,2,65536]),([2,2],[0,1,2,-1]),([2,2],[0,1,2,3.0]),([2,2,1],[0,1,2,3]),([2,True],[0,1]),([1,4],[0,1,2,3]),([2,2],[0,1,2])]:
            with self.assertRaises(ValueError):roundtrip(pathlib.Path('/never-executed'),pathlib.Path('/never-written'),'bad',shape,words,bytes(4096))

    def test_original_C_source_pin_is_required_before_compilation(self):
        from mar345_original_v2_oracle import build
        with tempfile.TemporaryDirectory() as directory:
            path=pathlib.Path(directory);(path/'pack_c.c').write_bytes(b'not the original source')
            with self.assertRaises(ValueError):build(path,path)


if __name__=="__main__":unittest.main()
