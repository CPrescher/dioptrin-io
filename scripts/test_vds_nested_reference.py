"""Negative controls for the nested graph qualification, independent of Rust."""
import copy
import json
import pathlib
import unittest

import vds_nested_reference as proof
import vds_printf_reference as common


class NestedProofControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.case=json.loads((proof.DATA/'provenance.json').read_text())['cases']['grand-u64']
        cls.config=proof.request(cls.case)
        result=common.run_probe(proof.ROOT/'target/release/examples/api_probe',cls.config)
        if result['status']!='ok':raise AssertionError(result)
        cls.native=result['value']

    def test_source_words_dtype_and_shape_cannot_be_changed(self):
        self.assertEqual(proof.compare_native(self.native,self.case,self.config),[])
        for key,value in [('value_hex','00'),('pixel_type','F64'),('shape',[3,3])]:
            changed=copy.deepcopy(self.native);changed['images'][0][key]=value
            self.assertTrue(proof.compare_native(changed,self.case,self.config))

    def test_original_fill_node_word_and_selection_cannot_be_changed(self):
        for key,value in [('origin_node',999),('value',{'value_hex':'00'}),('origin_selection',{'kind':'All','shape':[3,4,5],'maximum_shape':[3,4,5]})]:
            changed=copy.deepcopy(self.native);changed['metadata'][0]['virtual_dataset']['effective_fill_regions'][0][key]=value
            self.assertTrue(proof.compare_native(changed,self.case,self.config))

    def test_original_declarations_and_coverage_cannot_be_changed(self):
        for key,value in [('stored_filename','wrong.h5'),('selected_elements',999)]:
            changed=copy.deepcopy(self.native);changed['metadata'][0]['virtual_dataset']['mappings'][0][key]=value
            self.assertTrue(proof.compare_native(changed,self.case,self.config))
        changed=copy.deepcopy(self.native);changed['metadata'][0]['virtual_dataset']['unmapped_pixels_by_frame']=[0,0]
        self.assertTrue(proof.compare_native(changed,self.case,self.config))

    def test_summary_gate_rejects_missing_reference_or_source_change(self):
        valid={'exact_cases':10,'exact_frames':24,'reference_matches':20,'guard_cases_passed':6,'differences':0,'errors':0,'imports_unchanged':True,'sources_stable':True,'probe_stable':True}
        self.assertTrue(proof.gate(valid))
        for key,value in [('reference_matches',19),('sources_stable',False),('differences',1),('guard_cases_passed',5)]:
            self.assertFalse(proof.gate({**valid,key:value}))

if __name__=='__main__':unittest.main()
