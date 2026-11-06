"""NPY count/type/layout and reference-failure qualification controls."""
import copy
import json
import unittest
import numpy as np
from npy_type_reference import DATA,array_frames,canonical,compare_native,frame_projection,qualify_reference

class NpyControls(unittest.TestCase):
    def test_manual_headers_load_all_versions_endians_orders_and_source_words(self):
        manifest=json.loads((DATA/'provenance.json').read_text());self.assertEqual(manifest['license'],'CC0-1.0')
        for name,case in manifest['files'].items():
            data=np.load(DATA/name,allow_pickle=False)
            self.assertEqual(list(data.shape),case['source_shape'],name)
            self.assertEqual(data.dtype.str,case['dtype'],name)
            self.assertEqual([canonical(frame).hex() for frame in array_frames(data)],case['frame_hex'],name)

    def test_source_dtype_words_shape_and_header_mutations_cannot_pass(self):
        manifest=json.loads((DATA/'provenance.json').read_text());case=manifest['files']['half-v1-be-f-r3.npy'];data=np.load(DATA/case['file'],allow_pickle=False)
        expected=[frame_projection(i,frame) for i,frame in enumerate(array_frames(data))]
        native={'frames':case['frames'],'metadata':{key:case[key] for key in ['source_shape','fortran_order','pixel_type','little_endian','header_hex']},'images':[dict(frame,pixels={'sample_sha256':frame['sample_sha256']}) for frame in expected]}
        native['metadata'].update(shape=case['frame_shape'],fields={'descr':case['dtype']})
        self.assertFalse(compare_native(native,case,expected))
        for field in ['word','type','shape','order','header']:
            changed=copy.deepcopy(native)
            if field=='word':changed['images'][0]['value_hex']='00'*12
            elif field=='type':changed['metadata']['pixel_type']='U16'
            elif field=='shape':changed['metadata']['source_shape']=[2,6]
            elif field=='order':changed['metadata']['fortran_order']=False
            else:changed['metadata']['header_hex']='00'
            self.assertTrue(compare_native(changed,case,expected),field)

    def test_boolean_noncanonical_words_and_empty_shape_are_not_normalized(self):
        raw=np.frombuffer(bytes([0,1,2,127,128,255]),dtype='?')
        self.assertEqual(canonical(raw),bytes([0,1,2,127,128,255]))
        for shape in [(0,),(0,2,3),(2,0,3),(2,2,0,3)]:
            data=np.empty(shape,dtype='<f2');frames=array_frames(data)
            self.assertEqual(list(data.shape),list(shape))
            self.assertEqual(frames.size,0)
            self.assertTrue(all(frame.size==0 for frame in frames))

    def test_reference_failure_only_qualifies_exact_valid_empty_rank4_case(self):
        failure={'status':'not_completed','error_type':'ValueError','error':'cannot reshape array of size 0 into shape (0,3)','reading_parity_claimed':False}
        self.assertEqual(qualify_reference(failure,{'source_shape':[2,2,0,3]},[])['status'],'reference_bug_valid_empty_rank4')
        changed=dict(failure,error='unrelated source failure')
        self.assertTrue(qualify_reference(changed,{'source_shape':[2,2,0,3]},[])['differences'])
        for shape in [[2,2,3],[2,2,2,3]]:
            self.assertTrue(qualify_reference(failure,{'source_shape':shape},[])['differences'])

if __name__=='__main__':unittest.main()
