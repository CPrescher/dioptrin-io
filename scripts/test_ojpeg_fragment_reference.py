"""Controls for component proof comparison; no runtime/Python bridge."""
import copy
import pathlib
import unittest
import tempfile
from compare_ojpeg_fragments import compare_components,verify_package_files
ROOT=pathlib.Path(__file__).resolve().parents[1]
class ComponentControls(unittest.TestCase):
    def setUp(self):
        plane={'component':'Y','shape':[16,32],'valid_shape':[13,19],'sampling':[1,1],'sample_sha256':'source-words'}
        planes=[dict(plane,component=c)for c in ['Y','Cb','Cr']]
        self.reference={'shape':[29,51],'subsampling':[2,2],'chunks':[{'origin':[16,32],'image_shape':[13,19],'planes':planes}]}
    def test_exact_component_grids(self):self.assertEqual(compare_components(copy.deepcopy(self.reference),self.reference),[])
    def test_correct_hash_does_not_hide_wrong_spatial_origin(self):
        actual=copy.deepcopy(self.reference);actual['chunks'][0]['origin']=[32,0]
        self.assertTrue(any(d['field'].endswith('.origin')for d in compare_components(actual,self.reference)))
    def test_plane_order_padding_or_reserved_word_difference_is_not_display_equivalence(self):
        for key,value in [('component','Cr'),('shape',[13,19]),('sample_sha256','converted-display')]:
            actual=copy.deepcopy(self.reference);actual['chunks'][0]['planes'][0][key]=value
            self.assertTrue(compare_components(actual,self.reference))
    def test_missing_plane_or_chunk_is_a_failure(self):
        actual=copy.deepcopy(self.reference);actual['chunks'][0]['planes'].pop();self.assertTrue(compare_components(actual,self.reference))
        actual=copy.deepcopy(self.reference);actual['chunks'].clear();self.assertTrue(compare_components(actual,self.reference))
class AuthorityControls(unittest.TestCase):
    def test_installed_reference_source_must_match_pinned_checkout(self):
        with tempfile.TemporaryDirectory()as folder:
            root=pathlib.Path(folder);expected=root/'expected';actual=root/'actual';expected.mkdir();actual.mkdir()
            (expected/'reader.py').write_bytes(b'unchanged pinned source')
            (actual/'reader.py').write_bytes(b'unchanged pinned source')
            self.assertEqual(len(verify_package_files(expected,actual,['reader.py'])),1)
            (actual/'reader.py').write_bytes(b'local compatibility patch')
            with self.assertRaisesRegex(ValueError,'differs from pinned'):verify_package_files(expected,actual,['reader.py'])
    def test_missing_reference_implementation_is_not_a_decoder_rejection(self):
        with tempfile.TemporaryDirectory()as folder:
            root=pathlib.Path(folder)
            with self.assertRaises(FileNotFoundError):verify_package_files(root,root,['absent.py'])
if __name__=='__main__':unittest.main()
