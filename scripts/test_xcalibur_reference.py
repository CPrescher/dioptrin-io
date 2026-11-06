"""Controls for explicit mask/API comparisons and pinned reference defects."""
import dataclasses
import pathlib
import struct
import unittest

import numpy as np
from xcalibur_reference import binary_mask, field_comparison, from_native_regions, normalize


class ComparisonControls(unittest.TestCase):
    def test_named_scalar_mapping_does_not_coerce_arbitrary_lists(self):
        self.assertEqual(normalize({'itreatment': (7,), 'points': (7,)}), {'itreatment': 7, 'points': [7]})
        self.assertEqual(normalize({'itreatment': [7]}), {'itreatment': [7]})

    def test_field_differences_cannot_pass_and_absence_is_explicit(self):
        from fabio.xcaliburimage import CcdCharacteristiscs
        ref = CcdCharacteristiscs(ddarkcurrentinADUpersec=5)
        self.assertEqual(field_comparison(ref, {})['status'], 'difference')
        result = field_comparison(ref, {'ddarkcurrentinADUpersec':5})
        self.assertEqual(result['status'], 'match')
        self.assertIn('pschipbadpolygon', result['absent_sections_with_reference_defaults'])
        fields = normalize(dataclasses.asdict(ref))
        fields['extra'] = 3
        self.assertEqual(field_comparison(ref, fields)['status'], 'difference')

    def test_only_binary_mask_conversion_is_permitted(self):
        self.assertEqual(binary_mask(np.array([[0,1]], dtype='int8')).dtype, np.dtype('uint8'))
        for data in [[[0,-1]], [[0,2]], [0,1]]:
            with self.assertRaises(ValueError): binary_mask(data)

    def test_native_geometry_reconstructs_with_reference_renderer(self):
        native = {'polygons':[{'itype':1,'ipoints':4,'iax':[0,1,99,0,0,0],'iay':[0,0,99,0,0,0]}],
                  'points':[{'spt':{'ix':2,'iy':1},'sptreplace1':{'ix':0,'iy':0},'sptreplace2':{'ix':0,'iy':0},'itreatment':12}]}
        self.assertEqual(from_native_regions(native).build_mask((2,3)).tolist(), [[1,1,0],[0,0,1]])

    def test_independent_fixture_exposes_reference_row_count_and_scalar_defects(self):
        from fabio.xcaliburimage import CcdCharacteristiscs, ChipBadPoint
        from generate_xcalibur_fixtures import fixture
        source = fixture()
        path = pathlib.Path(__file__).resolve().parents[1]/'tests/data/xcalibur/records.ccd'
        self.assertEqual(source, path.read_bytes())
        ref = CcdCharacteristiscs.loads(source)
        self.assertEqual(ref.inumofcornermasks, (1,))
        self.assertEqual(ref.pschipbadpoint[0].itreatment, (0xabcd,))
        self.assertEqual(ref.ibadrows1x1, 1)
        self.assertEqual(len(ref.pschipbadrow1x1), 0)  # loop uses column count 0
        self.assertEqual(struct.unpack_from('<H', source, 2008)[0], 1)
        with self.assertRaises((struct.error, TypeError)):
            ChipBadPoint.loads(source[1886:1900]).dumps()


if __name__ == '__main__': unittest.main()
