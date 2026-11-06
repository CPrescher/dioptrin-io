"""Controls for extended source metadata and independent header-only proof."""
import copy
import enum
import json
import pathlib
import subprocess
import unittest
from types import SimpleNamespace

import numpy as np
from metadata_attributes import inventory,compare_attributes,value
from metadata_parity import encode
from header_reference import compare as compare_headers


class AttributeControls(unittest.TestCase):
    def read(self,codec,values,status='counts_match'):
        return {'codec':codec,'status':status,'reference_attributes':{k:{'category':'source_metadata','value':v} for k,v in values.items()}}

    def test_byte_order_enum_and_unknown_attributes_are_explicit(self):
        class ByteOrder(enum.StrEnum): LITTLE='<'
        self.assertEqual(value(ByteOrder.LITTLE),'<')
        image=SimpleNamespace(header={},data=None,filename='name',calibration_secret=3)
        read={'codec':'unknown','reference_attributes':{key:item for key,item in inventory('unknown',image).items() if key not in {'shape','dtype','nframes'}},'status':'counts_match'}
        result=compare_attributes(read,[])
        self.assertEqual(result['unresolved_attributes'],1)
        self.assertEqual(result['comparisons'][0]['attribute'],'calibration_secret')

    def test_cbf_pilatus_numbers_are_not_replaced_with_reference_values(self):
        read=self.read('cbfimage',{'cif':{'_array_data.data':'CIF Binary Section'},'pilatus_headers':{'Detector_distance':0.25}})
        frame={'fields':{},'metadata_details':{'cif_fields':{},'pilatus':{'Detector_distance':0.25}}}
        self.assertEqual(compare_attributes(read,[frame])['status'],'accounted')
        frame['metadata_details']['pilatus']['Detector_distance']=0.5
        self.assertEqual(compare_attributes(read,[frame])['unresolved_attributes'],1)

    def test_numpy_complete_array_hash_and_reference_reshape_are_measured(self):
        reference=encode(np.arange(24,dtype='int32').reshape(4,2,3))
        read=self.read('numpyimage',{'dataset':reference})
        frames=[{'dtype':'int32','metadata_details':{'source_shape':[2,2,2,3]}},
                {'metadata_details':{'dataset_sha256':reference['sha256']}}]
        self.assertEqual(compare_attributes(read,frames)['status'],'accounted')
        frames[-1]['metadata_details']['dataset_sha256']='0'*64
        self.assertEqual(compare_attributes(read,frames)['unresolved_attributes'],1)

    def test_hdf5_dataset_identity_shape_and_dtype_are_all_checked(self):
        path=str(pathlib.Path('/tmp/source.h5').resolve())
        descriptor={'kind':'hdf5_object','path':'/entry/data','source_file':path,'shape':[2,3,4],'dtype':'uint16'}
        read=self.read('hdf5image',{'dataset':descriptor})
        frame={'dtype':'uint16','metadata_details':{'dataset':'/entry/data','source_file':path,'source_shape':[2,3,4]}}
        self.assertEqual(compare_attributes(read,[frame])['status'],'accounted')
        for key,wrong in [('dataset','/wrong'),('source_file','/tmp/another.h5'),('source_shape',[1,3,4])]:
            changed=copy.deepcopy(frame);changed['metadata_details'][key]=wrong
            self.assertEqual(compare_attributes(read,[changed])['unresolved_attributes'],1)

    def test_edf_general_fields_and_frame_availability_are_separate(self):
        read=self.read('edfimage',{'generalframe':{'kind':'frame_metadata','header':{'Title':'source'}},'incomplete_data':False})
        frame={'metadata_details':{'general_headers':[{'TITLE':'source'}],'incomplete_data':False}}
        self.assertEqual(compare_attributes(read,[frame])['status'],'accounted')
        frame['metadata_details']['general_headers'][0]['TITLE']='changed'
        self.assertEqual(compare_attributes(read,[frame])['unresolved_attributes'],1)
        # A truncated following header does not alter the selected complete frame.
        read=self.read('edfimage',{'incomplete_data':False})
        frame={'source_recovery':{'stop':{'kind':'UnparsedRecord'},'images':[{'availability':{'present_samples':[0,6],'declared_samples':6}}]}}
        self.assertEqual(compare_attributes(read,[frame])['status'],'accounted')
        frame['source_recovery']['images'][0]['availability']['present_samples']=[0,5]
        self.assertEqual(compare_attributes(read,[frame])['unresolved_attributes'],1)

    def test_tiff_probe_qualification_requires_independently_correct_source_bits(self):
        raw=np.zeros(32,dtype=np.uint16);raw[21]=12345
        read=self.read('tifimage',{'nbits':12345})
        frame={'fields':{'tags':{'258':16}}}
        result=compare_attributes(read,[frame],raw.tobytes().hex(),16)
        self.assertEqual(result['comparisons'][0]['status'],'qualified_reference_difference')
        for wrong in [8,32]:
            frame['fields']['tags']['258']=wrong
            self.assertEqual(compare_attributes(read,[frame],raw.tobytes().hex(),16)['unresolved_attributes'],1)
        frame['fields']['tags']['258']=16
        raw[21]=12346
        self.assertEqual(compare_attributes(read,[frame],raw.tobytes().hex(),16)['unresolved_attributes'],1)

    def test_independent_header_oracle_detects_wrong_tags_offsets_and_pixel_claims(self):
        root=pathlib.Path(__file__).resolve().parents[1]
        source=root/'tests/data/tiff/uint16.tif'
        probe=root/'target/release/examples/parity_probe'
        frames=[json.loads(line) for line in subprocess.check_output([str(probe),'tiff-header',str(source)],text=True).splitlines()]
        self.assertEqual(compare_headers(source,frames)['status'],'match')
        for mutation in ['value','missing','offset','prefix','pixels']:
            changed=copy.deepcopy(frames)
            if mutation=='value':changed[0]['fields']['tags']['256']=999
            elif mutation=='missing':changed[0]['fields']['tags'].pop('256')
            elif mutation=='offset':changed[0]['ifd_offset']+=1
            elif mutation=='prefix':changed[0]['source_prefix_hex']='00'*64
            else:changed[0]['pixels_available']=True
            self.assertEqual(compare_headers(source,changed)['status'],'difference')

    def test_spe_frame_count_qualification_rejects_wrong_native_totals(self):
        read=self.read('speimage',{'nframes':1})
        frame={'frames':2,'fields':{'num_frames':'2'},'metadata_details':{'frames':2}}
        result=compare_attributes(read,[frame,{}])
        self.assertEqual(result['comparisons'][0]['status'],'qualified_reference_difference')
        frame['frames']=3
        self.assertEqual(compare_attributes(read,[frame,{}])['unresolved_attributes'],1)
        frame['frames']=2
        read['reference_attributes']['nframes']['value']=True
        self.assertEqual(compare_attributes(read,[frame,{}])['unresolved_attributes'],1)

    def test_edf_padding_diagnostic_requires_bounded_verified_padding(self):
        read=self.read('edfimage',{'incomplete_file':True})
        read['metadata_integrity_evidence']={'tail_hex':bytes(415).hex()}
        frame={'metadata_details':{'incomplete_data':False}}
        self.assertEqual(compare_attributes(read,[frame])['comparisons'][0]['status'],'qualified_reference_difference')
        for tail in [b'',bytes(512),b'not padding']:
            read['metadata_integrity_evidence']['tail_hex']=tail.hex()
            self.assertEqual(compare_attributes(read,[frame])['unresolved_attributes'],1)


if __name__=='__main__':unittest.main()
