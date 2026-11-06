"""Controls for companion fields, raw bytes, float bits and reference scope."""
import copy
import pathlib
import tempfile
import unittest

from companion_reference import (compare_record, differences, float_bits,
                                 reference_projection, source_projection)
from generate_xcalibur_run_fixtures import fixtures


class CompanionControls(unittest.TestCase):
    def setUp(self):
        self.payload=fixtures()['records.run']
        self.expected=source_projection(self.payload)
        with tempfile.TemporaryDirectory() as directory:
            path=pathlib.Path(directory)/'records.run';path.write_bytes(self.payload)
            self.reference=reference_projection(path,self.payload)
        self.native={**copy.deepcopy(self.expected),'kind':'xcalibur_run','provenance':'Buffer'}

    def test_every_reference_read_and_loads_field_matches_independent_source(self):
        self.assertEqual(self.reference['status'],'match')
        self.assertFalse(self.reference['read_loads_differences'])
        self.assertEqual(self.reference['fields'],self.expected['fields'])
        self.assertTrue(all(row['roundtrip_source_bytes_preserved'] for row in self.reference['sweeps']))
        self.assertFalse(compare_record(self.native,self.expected,self.reference,'Buffer'))
        self.assertEqual(self.reference['roundtrip']['status'],'fields_preserved')
        self.assertFalse(self.reference['roundtrip']['source_bytes_preserved'])

    def test_unknown_fields_text_padding_trailer_and_float_bits_cannot_be_dropped(self):
        changes=[('fields','inumofsweeps',1),('raw_text_hex','cprefix','00'*256),
                 ('float_bits','dunknown','0000000000000000')]
        for section,field,value in changes:
            altered=copy.deepcopy(self.native);altered[section][field]=value
            self.assertTrue(compare_record(altered,self.expected,self.reference,'Buffer'))
        for field,value in [('trailer_hex',''),('source_sha256','0'*64),('parsed_bytes',0),('provenance','PathSnapshot')]:
            altered=copy.deepcopy(self.native);altered[field]=value
            self.assertTrue(compare_record(altered,self.expected,self.reference,'Buffer'))
        altered=copy.deepcopy(self.native);altered['fields']['pssweep'][0]['dwunknown1']=0
        self.assertTrue(compare_record(altered,self.expected,self.reference,'Buffer'))
        altered=copy.deepcopy(self.native);altered['fields']['pssweep'][0]['iunknown6']=0
        self.assertTrue(compare_record(altered,self.expected,self.reference,'Buffer'))

    def test_raw_text_rejection_is_not_relabelled_reference_run_parity(self):
        payload=fixtures()['raw-text-floats.run']
        expected=source_projection(payload)
        with tempfile.TemporaryDirectory() as directory:
            path=pathlib.Path(directory)/'raw.run';path.write_bytes(payload)
            reference=reference_projection(path,payload)
        self.assertEqual(reference['status'],'reference_text_rejection')
        self.assertIsNone(reference['fields'])
        self.assertIsNone(expected['fields']['cprefix'])
        self.assertEqual(expected['float_bits']['dunknown'],'7ff8000000000042')
        self.assertEqual(reference['sweeps'][0]['float_bits']['dstart'],'7ff0000000000001')
        native={**expected,'kind':'xcalibur_run','provenance':'StreamCapture'}
        self.assertFalse(compare_record(native,expected,reference,'StreamCapture'))

    def test_truncation_and_comparison_types_remain_strict(self):
        for payload in [b'',self.payload[:527],self.payload[:703]]:
            with self.assertRaises(ValueError):source_projection(payload)
        self.assertTrue(differences(True,1))
        self.assertTrue(differences({}, {'reserved':None}))
        self.assertNotEqual(float_bits(-0.0),float_bits(0.0))

    def test_fixtures_match_committed_independent_sources(self):
        root=pathlib.Path(__file__).resolve().parents[1]/'tests/data/xcalibur'
        for name,payload in fixtures().items():
            self.assertEqual((root/name).read_bytes(),payload)


if __name__=='__main__':
    unittest.main()
