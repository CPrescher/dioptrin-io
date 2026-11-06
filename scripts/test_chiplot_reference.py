"""Exact lexical/numeric controls; reference bugs remain unsuccessful reads."""
import copy
import unittest
from decimal import Decimal
from chiplot_reference import DATA,compare_record,exact_numeric_identity,number,observe_reference,oracle
from generate_chiplot_fixtures import fixtures

class ProfileControls(unittest.TestCase):
    def test_integer_boundaries_and_decimal_uncertainties_do_not_round(self):
        self.assertEqual(number('9223372036854775807'),{'kind':'I64','value':2**63-1})
        self.assertEqual(number('18446744073709551615'),{'kind':'U64','value':2**64-1})
        self.assertEqual(number('18446744073709551616'),{'kind':'IntegerText'})
        identity=exact_numeric_identity('1.23456789012345678901234567890123456789')
        self.assertEqual(len(identity['digits']),39)
        for value in ['1.2(3)','1D+2','?']:
            with self.assertRaises(ValueError):number(value)
        self.assertNotEqual(exact_numeric_identity('-0'),exact_numeric_identity('0'))
    def test_every_independent_fixture_and_raw_numeric_oracle_matches(self):
        import hashlib
        for name,payload in fixtures().items():
            self.assertEqual((DATA/name).read_bytes(),payload)
            expected=oracle(payload);native={key:copy.deepcopy(value) for key,value in expected.items() if key!='exact_numeric_identities'}
            native.update(kind='chiplot',representation='ExactSourceLexemes',provenance='Buffer',source_bytes=len(payload),source_sha256=hashlib.sha256(payload).hexdigest(),diagnostics=['nonfinite lexical extension'] if name=='nonfinite.chi' else [])
            self.assertFalse(compare_record(native,expected,payload,'Buffer'),name)
            if expected['rows']:
                wrong=copy.deepcopy(native);wrong['rows'][0]['samples'][-1]['lexeme']='0'
                self.assertTrue(compare_record(wrong,expected,payload,'Buffer'),name)
    def test_native_exact_integer_kind_value_and_header_order_are_checked(self):
        import hashlib
        payload=fixtures()['multi-curve.chi'];expected=oracle(payload);native={key:copy.deepcopy(value) for key,value in expected.items() if key!='exact_numeric_identities'}
        native.update(kind='chiplot',representation='ExactSourceLexemes',provenance='Buffer',source_bytes=len(payload),source_sha256=hashlib.sha256(payload).hexdigest())
        native['rows'][0]['samples'][2]['number']['value']=9007199254740992
        self.assertTrue(compare_record(native,expected,payload,'Buffer'))
    def test_unchanged_reference_error_is_never_labelled_parity(self):
        result=observe_reference(DATA/'ascending.chi')
        self.assertEqual(result['status'],'read_not_completed')
        self.assertEqual(result['classification'],'pinned_string_vs_integer_step_bug')
        self.assertFalse(result['reading_parity_claimed'])
        result=observe_reference(DATA/'one.chi')
        self.assertEqual(result['error_type'],'ZeroDivisionError')

if __name__=='__main__':unittest.main()
