"""Controls for exact metadata and honest unchanged-helper qualifications."""
import copy
import hashlib
import re
import unittest
import h5py
import numpy as np
from fabio.nexus import Nexus
from lowlevel_reference import (DATA,compare_pilatus,independent_result,inspect_object,
                               object_projection,pilatus_reference,reference_call,reference_alignment,typed_identity)

class LowlevelControls(unittest.TestCase):
    def test_pilatus_constructor_mapping_units_and_raw_repetitions_are_checked(self):
        payload=(DATA/'pilatus.txt').read_bytes();reference=pilatus_reference(payload)
        entries=[]
        for line in payload.decode().splitlines():
            match=re.match(r'# ([^\s,:=]+)[\s,:=]+(.*)',line)
            if match:entries.append([match[1],match[2].lstrip(' \t,:=')])
        native={'kind':'pilatus_document','original_sha256':hashlib.sha256(payload).hexdigest(),'source_bytes':len(payload),'provenance':'Buffer','convention':'PILATUS_1.2','description':payload.decode(),
                'entries':entries,'fields':dict(entries),'typed_fields':copy.deepcopy(reference['constructor']['typed_fields']),'queries':copy.deepcopy(reference['constructor']['queries']),
                'cleaned_view':{'lines':reference['clean_string']['lines'],'diagnostics':[]},'diagnostics':[]}
        self.assertFalse(compare_pilatus(native,payload,'Buffer',reference))
        for mutate in ['typed','units','repetition','raw','cleaned']:
            changed=copy.deepcopy(native)
            if mutate=='typed':changed['typed_fields']['Exposure_time']=0.1
            elif mutate=='units':changed['fields']['Exposure_time']='0.200 ms'
            elif mutate=='repetition':changed['entries'].pop(3)
            elif mutate=='raw':changed['original_sha256']='0'*64
            else:changed['cleaned_view']['lines']=[]
            self.assertTrue(compare_pilatus(changed,payload,'Buffer',reference),mutate)
        self.assertNotEqual(typed_identity(-0.0),typed_identity(0.0))
    def test_pilatus_reference_index_bugs_are_not_relabelled(self):
        value=pilatus_reference((DATA/'pilatus-one-token.txt').read_bytes())
        self.assertEqual(value['constructor']['status'],'not_completed');self.assertEqual(value['constructor']['error_type'],'IndexError')
        self.assertEqual(value['clean_string']['status'],'completed')
        trailing=pilatus_reference((DATA/'pilatus-trailing-spaces.txt').read_bytes())
        self.assertEqual(trailing['clean_string']['status'],'completed')
        dangling=pilatus_reference((DATA/'pilatus-invalid.txt').read_bytes())
        self.assertEqual(dangling['clean_string']['error_type'],'IndexError')
        self.assertEqual(trailing['unknown_convention']['status'],'rejected')
    def test_pilatus_python_line_boundaries_and_cr_continuation_are_positive(self):
        for name,exposure,count in [('pilatus-cr-only.txt',2.0,2),('pilatus-universal-lines.txt',11.0,11),('pilatus-cr-continuation.txt',2.0,3)]:
            value=pilatus_reference((DATA/name).read_bytes())
            self.assertEqual(value['constructor']['status'],'completed',name)
            self.assertEqual(value['constructor']['typed_fields']['Exposure_time'],exposure,name)
            self.assertEqual(value['clean_string']['status'],'completed',name)
            self.assertEqual(len(value['clean_string']['lines']),count,name)
        self.assertEqual(value['constructor']['typed_fields']['Detector'],'PILATUS 100K')
        self.assertEqual(value['clean_string']['lines'][0],'  Detector  PILATUS  100K')
    def test_nexus_original_dtype_endian_shape_and_u64_values_are_checked(self):
        path=DATA/'nexus.h5'
        with h5py.File(path,'r') as file:
            obj=file['entry_new/instrument/detector/counts'];expected=object_projection(obj,path)
            value=expected['attributes']['values']['reserved']
            self.assertFalse(value['little_endian']);self.assertEqual(value['pixels']['dtype'],'uint64')
            self.assertEqual(value['value_hex'],np.array([2**64-1,2**53+1],dtype='<u8').tobytes().hex())
            native=copy.deepcopy(expected);native['requested_path']=obj.name
            self.assertFalse(inspect_object(native,expected))
            for changed_field in ['byte_order','shape','dtype','u64']:
                changed=copy.deepcopy(native)
                if changed_field=='byte_order':changed['byte_order']='LittleEndian'
                elif changed_field=='shape':changed['shape']=[3,1]
                elif changed_field=='dtype':changed['type_descriptor']='Float(U8)'
                else:changed['attributes']['values']['reserved']['value_hex']='00'*16
                self.assertTrue(inspect_object(changed,expected),changed_field)
    def test_nexus_default_get_data_bug_is_distinct_from_native_source_selection(self):
        path=DATA/'nexus.h5';command={'op':'data','path':'/entry_new/instrument/detector'}
        with Nexus(str(path),mode='r') as reference:
            observed=reference_call(reference,command)
            self.assertEqual(observed['status'],'not_completed');self.assertEqual(observed['error_type'],'TypeError');self.assertFalse(observed['reading_parity_claimed'])
            selected=reference_call(reference,{**command,'filter':{'name':'signal','dtype':'int64','value':1}})
            self.assertEqual(selected['status'],'completed')
        with h5py.File(path,'r') as file:
            owned=independent_result(file,command,path);self.assertEqual(len(owned),1)
            entries=independent_result(file,{'op':'entries'},path)
            self.assertEqual([row['object']['path'] for row in entries],['/entry_new','/entry_old'])
            self.assertEqual(entries[0]['start_time'],'2026-01-01T00:00:00+02:00')


class QualificationControls(unittest.TestCase):
    def test_completed_reference_path_differences_cannot_pass(self):
        action={'result':[{'path':'/right'}]};reference={'status':'completed','value':[{'path':'/right'}]};command={'op':'class'}
        self.assertEqual(reference_alignment(action,reference,command)['status'],'match')
        action['result'][0]['path']='/wrong'
        self.assertEqual(reference_alignment(action,reference,command)['status'],'difference')
    def test_unicode_default_positive_and_fixed_ascii_reference_bug_are_separate(self):
        with Nexus(str(DATA/'nexus-unicode.h5'),mode='r') as reference:
            result=reference_call(reference,{'op':'default'})
            self.assertEqual(result['status'],'completed');self.assertEqual(result['value']['path'],'/entry_new/plot')
        with Nexus(str(DATA/'nexus.h5'),mode='r') as reference:
            result=reference_call(reference,{'op':'default'})
            self.assertEqual(result['status'],'not_completed');self.assertEqual(result['error_type'],'TypeError')

if __name__=='__main__':unittest.main()
