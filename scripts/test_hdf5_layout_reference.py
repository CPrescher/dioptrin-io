"""Controls reject count/type/IEEE word differences without normalizing them."""
import copy
import json
import unittest
import h5py
import numpy as np
from hdf5_layout_reference import DATA, canonical, compare_frame, frame_projection

class LayoutControls(unittest.TestCase):
    def test_ieee_component_bits_and_order_are_authoritative(self):
        values=np.array([0x80000000,0x7fc12345],dtype='<u4').view('<c8').reshape(1,1)
        expected=frame_projection(0,values)
        self.assertFalse(compare_frame(expected,expected))
        for key,value in [('value_hex','000000004523c17f'),('value_hex','4523c17f00000080'),('pixel_type','F32'),('shape',[2,1]),('frame',1)]:
            changed=copy.deepcopy(expected);changed[key]=value
            self.assertTrue(compare_frame(changed,expected),(key,value))
        changed=copy.deepcopy(expected);changed['pixels']={'sample_sha256':'0'*64}
        self.assertTrue(compare_frame(changed,expected))

    def test_boolean_noncanonical_bytes_are_not_truth_normalized(self):
        values=np.frombuffer(bytes([0,1,2,255]),dtype='?').reshape(1,4)
        self.assertEqual(canonical(values),bytes([0,1,2,255]))
        expected=frame_projection(0,values);changed=copy.deepcopy(expected);changed['value_hex']='00010101'
        self.assertTrue(compare_frame(changed,expected))

    def test_half_nan_subnormal_and_signed_zero_bits_match_independent_words(self):
        manifest=json.loads((DATA/'provenance.json').read_text())
        self.assertEqual(manifest['license'],'CC0-1.0')
        with h5py.File(DATA/'types.h5') as file:
            for name in ['half-le','half-be','complex64-le','complex64-be','complex128-le','complex128-be','bool']:
                data=file[name][...]
                frames=data if data.ndim==3 else data[None,...]
                self.assertEqual([canonical(a).hex() for a in frames],manifest['datasets'][name]['frame_hex'],name)
            self.assertEqual(canonical(file['bool'][...]).hex(),'0001027f80ff')

if __name__=='__main__':unittest.main()
