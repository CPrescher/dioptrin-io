"""Independent selection-order and coverage evidence controls."""
import copy,json,unittest
import h5py
from irregular_vds_reference import DATA,canonical,compare_images,compare_coverage,frame

class IrregularControls(unittest.TestCase):
    def test_h5py_matches_every_manual_selected_source_word(self):
        manifest=json.loads((DATA/'provenance.json').read_text());self.assertEqual(manifest['license'],'CC0-1.0')
        for name,case in manifest['cases'].items():
            with h5py.File(DATA/case['master']) as file:self.assertEqual([canonical(row).hex() for row in file['data'][...]],case['frame_hex'],name)
    def test_component_nan_and_noncanonical_boolean_changes_fail(self):
        for name in ['bool','c32','f16']:
            with h5py.File(DATA/(name+'.h5')) as file:expected=[frame(i,a) for i,a in enumerate(file['data'][...])]
            native=[dict(a,status='ok',pixels={'sample_sha256':a['sample_sha256']}) for a in expected]
            self.assertFalse(compare_images(native,expected))
            altered=copy.deepcopy(native);altered[0]['value_hex']='00'*len(bytes.fromhex(altered[0]['value_hex']))
            self.assertTrue(compare_images(altered,expected),name)
            altered=copy.deepcopy(native);altered[0]['pixel_type']='F64';self.assertTrue(compare_images(altered,expected),name)
    def test_variable_coverage_cannot_be_relabelled_constant(self):
        counts=[18,15,17,20];self.assertFalse(compare_coverage({'unmapped_pixels_per_frame':None,'unmapped_pixels_by_frame':counts},counts))
        self.assertTrue(compare_coverage({'unmapped_pixels_per_frame':18,'unmapped_pixels_by_frame':counts},counts))
        self.assertTrue(compare_coverage({'unmapped_pixels_per_frame':None,'unmapped_pixels_by_frame':[18]*4},counts))

if __name__=='__main__':unittest.main()
