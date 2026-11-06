"""Small independent reference-helper controls; no corpus writers."""
import pathlib
import unittest
import numpy as np
import tifffile
import tiff_layout_reference as oracle

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURES = ROOT / 'tests/data/tiff-layouts'
LIBTIFF = '/opt/homebrew/opt/libtiff/lib/libtiff.6.dylib'

class ReferenceControls(unittest.TestCase):
    def test_canonical_scalar_words_keep_bits_and_endian(self):
        little=np.array([0x80000000,0x7fc01234,0x3f000000],dtype='<u4').view('<f4')
        big=little.astype('>f4')
        self.assertEqual(oracle.signature(little),oracle.signature(big))
        self.assertEqual(oracle.signature(little)['sample_sha256'],oracle.sha(little.tobytes()))

    def test_primary_catalog_does_not_flatten_children(self):
        for name in ['subifds-classic.tif','subifds-big-be.tif']:
            with tifffile.TiffFile(FIXTURES/name) as file:
                primary,graph=oracle.pages(file)
                self.assertEqual(len(primary),2)
                self.assertEqual(len(graph),4)
                self.assertEqual(sum(p in primary for p in graph),2)

    @unittest.skipUnless(pathlib.Path(LIBTIFF).exists(),'explicit native libtiff unavailable')
    def test_raw_ccitt_rows_match_original_independent_arrays(self):
        reference=oracle.Reference(LIBTIFF)
        for path in sorted(FIXTURES.glob('fax*.tif')):
            original=path.read_bytes()
            with tifffile.TiffFile(path) as file:
                actual=reference.read(path,file.pages[0])
            expected=np.frombuffer(path.with_name(path.name+'.raw').read_bytes(),dtype='u1').reshape(17,93)
            np.testing.assert_array_equal(actual,expected)
            self.assertEqual(path.read_bytes(),original)

    @unittest.skipUnless(pathlib.Path(LIBTIFF).exists(),'explicit native libtiff unavailable')
    def test_explicit_child_selection_retains_native_words(self):
        reference=oracle.Reference(LIBTIFF)
        for name in ['subifds-classic.tif','subifds-big-be.tif']:
            with tifffile.TiffFile(FIXTURES/name) as file:
                child=file.pages[0].pages[0]
                actual=reference.read(FIXTURES/name,child)
                np.testing.assert_array_equal(actual,np.array([[0,10001,20002],[30003,40004,50005]],dtype='u2'))
                self.assertEqual(actual.dtype,np.dtype('u2'))

if __name__=='__main__': unittest.main()
