"""Controls for bounded CIF projections and isolated broken-helper observations."""
import copy
import pathlib
import unittest

from cif_document_reference import DATA, EXPECTED, compare_record, reference_reads


class CifDocumentControls(unittest.TestCase):
    def native(self,name='scalars-loops.cif'):
        import hashlib
        payload=(DATA/name).read_bytes()
        result=copy.deepcopy(EXPECTED[name])
        result.update(kind='cif_document',coverage='HeaderProjection',provenance='Buffer',
                      coverage_warning='scalar/loop projection; _array_data.data bodies/full CIF grammar not interpreted',
                      source_bytes=len(payload),source_sha256=hashlib.sha256(payload).hexdigest())
        return result,payload

    def test_all_four_unchanged_reference_entry_points_match_positive_text_profile(self):
        for name in ['scalars-loops.cif','duplicates.cif']:
            native,payload=self.native(name)
            records=reference_reads(DATA/name)
            self.assertEqual(len(records),4)
            self.assertTrue(all(row['status']=='read_completed' for row in records))
            self.assertFalse(compare_record(native,EXPECTED[name],payload,'Buffer',records))
            if name=='scalars-loops.cif':
                self.assertEqual(records[0]['raw_loops'][0]['columns'][0],b'_atom_label'.hex())
                self.assertEqual(records[0]['projection']['fields']['_numeric_text'],'00012.340(5)')

    def test_actual_field_loop_raw_source_and_projection_claim_changes_fail(self):
        native,payload=self.native();reference=reference_reads(DATA/'scalars-loops.cif')
        for key,value in [('source_sha256','0'*64),('coverage','CompleteCif'),('coverage_warning',''),('provenance','PathSnapshot')]:
            changed=copy.deepcopy(native);changed[key]=value
            self.assertTrue(compare_record(changed,EXPECTED['scalars-loops.cif'],payload,'Buffer',reference))
        for field in ['_numeric_text','_unknown','_multiline']:
            changed=copy.deepcopy(native);changed['fields'][field]='changed'
            self.assertTrue(compare_record(changed,EXPECTED['scalars-loops.cif'],payload,'Buffer',reference))
        changed=copy.deepcopy(native);changed['loops'][0]['rows'][0][1]='1.234'
        self.assertTrue(compare_record(changed,EXPECTED['scalars-loops.cif'],payload,'Buffer',reference))
        changed=copy.deepcopy(native);changed['sample_sha256']='invented'
        self.assertTrue(compare_record(changed,EXPECTED['scalars-loops.cif'],payload,'Buffer',reference))

    def test_duplicate_order_and_diagnostic_cannot_be_masked(self):
        native,payload=self.native('duplicates.cif');reference=reference_reads(DATA/'duplicates.cif')
        for field in ['entries','diagnostics']:
            changed=copy.deepcopy(native);changed[field]=[]
            self.assertTrue(compare_record(changed,EXPECTED['duplicates.cif'],payload,'Buffer',reference))

    def test_partial_source_requires_all_native_diagnostics_not_reference_completion(self):
        native,payload=self.native('diagnostics.cif')
        messages=['duplicate CIF item','unsupported CIF token save_','item has no value','incomplete CIF loop row','multiple CIF data blocks']
        native['diagnostics']=messages
        reference=reference_reads(DATA/'diagnostics.cif')
        self.assertFalse(compare_record(native,EXPECTED['diagnostics.cif'],payload,'Buffer',reference))
        for index in range(len(messages)):
            changed=copy.deepcopy(native);changed['diagnostics'].pop(index)
            self.assertTrue(compare_record(changed,EXPECTED['diagnostics.cif'],payload,'Buffer',reference))
        # The pinned reader keeps going into unsupported source syntax. Native
        # raw/diagnostic qualification must not relabel that as full CIF parity.
        self.assertNotEqual(reference[0]['projection']['fields'],native['fields'])


if __name__=='__main__':unittest.main()
