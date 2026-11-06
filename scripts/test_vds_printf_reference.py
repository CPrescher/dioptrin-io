"""Strict word/provenance/owner and cold-reference qualification controls."""
import copy,json,pathlib,tempfile,unittest
import h5py
import numpy as np
from vds_printf_reference import DATA,canonical,compare_native,compare_reference,declaration,expected_frames,gate,parse_pattern,qualify_nested,request,sha


class PrintfControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):cls.manifest=json.loads((DATA/'provenance.json').read_text())

    def fake_native(self,name):
        case=self.manifest['cases'][name];config=request(case);nodes={}
        root={'source_file':config['path'],'dataset':config['dataset'],'mappings':[]}
        nodes[(config['path'],config['dataset'])]=root
        if config['printf_mappings']:
            for plan in config['printf_mappings']:
                owner=config['path'] if plan['source_file'] is None else plan['source_file'];key=(owner,plan['dataset'])
                node=nodes.setdefault(key,{'source_file':owner,'dataset':plan['dataset'],'mappings':[]})
                decl=declaration(owner,plan['dataset'],plan['mapping_index'],plan)
                for i in range(plan['blocks']):node['mappings'].append({'stored_filename':parse_pattern(plan['stored_filename'],i),'declared_dataset':parse_pattern(plan['declared_dataset'],i),'printf_expansion':{'declaration':copy.deepcopy(decl),'block_index':i}})
        else:
            decl=declaration(config['path'],config['dataset'],0,None)
            root['mappings'].append({'stored_filename':parse_pattern(decl['stored_filename'],0),'declared_dataset':parse_pattern(decl['declared_dataset'],0),'printf_expansion':{'declaration':decl,'block_index':0}})
        original=declaration(config['path'],config['dataset'],0,None)['virtual_selection']
        root.update(frozen_shape=case['shape'],declared_shape=original['shape'],master_max_shape=original['maximum_shape'],unmapped_regions='DeclaredFill' if config['unmapped_regions']=='declared_fill' else 'Reject',nested_nodes=[v for v in nodes.values() if v is not root])
        paths={config['path'],*(b['actual_file'] for b in config['vds_filenames'])}
        native={'kind':'hdf5_layout','frames':case['shape'][0],'native_hdf5_version':[2,2,0],'metadata':[{'source_shape':case['shape'],'shape':case['shape'][1:],'frames':case['shape'][0],'first_frame':0,'pixel_type':expected_frames(case)[0]['pixel_type'],'little_endian':np.dtype(case['dtype']).byteorder!='>','virtual_dataset':root}],
                'images':[{'status':'ok',**f} for f in expected_frames(case)],'sources':[{'path':p,'bytes':pathlib.Path(p).stat().st_size} for p in paths],'source_bindings':[{'requested':p,'resolved':p} for p in paths],'links':[]}
        return case,config,native

    def test_tokens_are_expanded_once_not_full_printf_or_axis_guessing(self):
        self.assertEqual(parse_pattern('%%b/%b/%b/%%%%',12),'%b/12/12/%%')
        self.assertEqual(parse_pattern('élément-%b',0),'élément-0')
        for pattern in ['%','%d','%03b','%0b','%s']:
            with self.assertRaises(ValueError):parse_pattern(pattern,0)
        # Confirm these are actual pinned HDF5 producer restrictions, not just
        # our helper's assumptions. Finite source / unlimited virtual block.
        v=h5py.h5s.create_simple((0,2,3),(h5py.h5s.UNLIMITED,2,3));v.select_hyperslab((0,0,0),(h5py.h5s.UNLIMITED,1,1),stride=(2,2,3),block=(2,2,3));s=h5py.h5s.create_simple((2,2,3))
        for token in [b'%0b',b'%03b',b'%d']:
            with self.assertRaises((ValueError,RuntimeError)):h5py.h5p.create(h5py.h5p.DATASET_CREATE).set_virtual(v,b'part'+token,b'/data',s)

    def test_every_fixture_count_word_declaration_and_source_binding_is_required(self):
        for name in self.manifest['cases']:
            case,config,native=self.fake_native(name);self.assertFalse(compare_native(native,case,config),name)
        case,config,native=self.fake_native('files-u64')
        for key in native:
            altered=copy.deepcopy(native);del altered[key];self.assertTrue(compare_native(altered,case,config),key)
        for key in native['metadata'][0]:
            altered=copy.deepcopy(native);del altered['metadata'][0][key];self.assertTrue(compare_native(altered,case,config),key)
        for key in native['images'][0]:
            altered=copy.deepcopy(native);del altered['images'][0][key];self.assertTrue(compare_native(altered,case,config),key)
        for key in native['metadata'][0]['virtual_dataset']['mappings'][0]['printf_expansion']['declaration']:
            altered=copy.deepcopy(native);del altered['metadata'][0]['virtual_dataset']['mappings'][0]['printf_expansion']['declaration'][key];self.assertTrue(compare_native(altered,case,config),key)

    def test_original_declared100_and_exact_owner_node_identity_cannot_be_discarded(self):
        case,config,native=self.fake_native('predeclared-prefix')
        self.assertEqual(native['metadata'][0]['virtual_dataset']['declared_shape'][0],100)
        for field,value in [('declared_shape',[6,2,3]),('frozen_shape',[100,2,3])]:
            altered=copy.deepcopy(native);altered['metadata'][0]['virtual_dataset'][field]=value;self.assertTrue(compare_native(altered,case,config),field)
        case,config,native=self.fake_native('nested-owner');decl=native['metadata'][0]['virtual_dataset']['nested_nodes'][0]['mappings'][0]['printf_expansion']['declaration']
        for key,value in [('source_file',config['path']),('dataset','/other'),('mapping_index',1),('plan',None)]:
            altered=copy.deepcopy(native);altered['metadata'][0]['virtual_dataset']['nested_nodes'][0]['mappings'][0]['printf_expansion']['declaration'][key]=value;self.assertTrue(compare_native(altered,case,config),key)

    def test_duplicate_or_wrong_expanded_block_never_counts_as_complete_coverage(self):
        case,config,native=self.fake_native('files-u64')
        for index in [0,99]:
            altered=copy.deepcopy(native);altered['metadata'][0]['virtual_dataset']['mappings'][1]['printf_expansion']['block_index']=index;self.assertTrue(compare_native(altered,case,config))
        altered=copy.deepcopy(native);altered['metadata'][0]['virtual_dataset']['mappings'][0]['stored_filename']='part-0.h5';self.assertTrue(compare_native(altered,case,config))
        altered=copy.deepcopy(native);altered['sources'].append({'path':'/unapproved/ambient.h5','bytes':1});self.assertTrue(compare_native(altered,case,config))

    def test_dtype_and_bits_are_checked_without_float_or_bool_normalization(self):
        for name in ['files-bool','files-f16','files-c32','files-u64','files-u64-be']:
            case,config,native=self.fake_native(name)
            altered=copy.deepcopy(native);value=bytearray.fromhex(altered['images'][0]['value_hex']);value[2]^=1;altered['images'][0]['value_hex']=value.hex();self.assertTrue(compare_native(altered,case,config),name)
            altered=copy.deepcopy(native);altered['images'][0]['pixels']['dtype']='float64';self.assertTrue(compare_native(altered,case,config),name)
        case=self.manifest['cases']['files-bool'];self.assertIn('027f80ff',case['frame_hex'][0])
        case=self.manifest['cases']['files-f16'];self.assertTrue(case['frame_hex'][0].startswith('0080557e'))

    def test_only_exact_cold_zero_and_separate_warm_source_words_qualify_cache_defect(self):
        case=self.manifest['cases']['nested-owner'];wanted=expected_frames(case);cold={'mode':'unchanged_cold_default','selected_shape':case['shape'],'frames':copy.deepcopy(wanted)}
        for value in cold['frames']:
            raw=bytes(len(bytes.fromhex(value['value_hex'])));value['value_hex']=raw.hex();value['pixels']['sample_sha256']=sha(raw)
        raw=bytes.fromhex(''.join(case['frame_hex']));warm={'mode':'independent_warm_child_control','selected_shape':case['shape'],'frames':wanted,'warm_child':{'shape':case['shape'],'dtype':'uint64','value_hex':raw.hex(),'sample_sha256':sha(raw)}}
        self.assertEqual(qualify_nested(cold,warm,case)['classification'],'qualified_pinned_hdf5_nested_printf_extent_cache_defect')
        altered=copy.deepcopy(cold);altered['frames'][0]['value_hex']='01'+altered['frames'][0]['value_hex'][2:]
        with self.assertRaises(ValueError):qualify_nested(altered,warm,case)
        for key in warm:
            altered=copy.deepcopy(warm);del altered[key]
            with self.assertRaises(ValueError):qualify_nested(cold,altered,case)
        with self.assertRaises(ValueError):qualify_nested({'status':'error','error':'No such file'},warm,case)

    def test_gate_requires_all_native_frames_and_all_independent_classifications(self):
        summary={'cases':14,'frames':60,'native_differences':0,'direct_reference_matches':13,'qualified_nested_cache_defects':1,'errors':0,'guard_cases_passed':2,'imports_unchanged':True,'sources_stable':True,'probe_stable':True}
        self.assertTrue(gate(summary))
        for key in summary:
            altered=copy.deepcopy(summary);del altered[key];self.assertFalse(gate(altered),key)
            altered=copy.deepcopy(summary);altered[key]=not summary[key] if type(summary[key]) is bool else summary[key]+1;self.assertFalse(gate(altered),key)
        for key in ['imports_unchanged','sources_stable','probe_stable']:
            altered=copy.deepcopy(summary);altered[key]=1;self.assertFalse(gate(altered),key)


if __name__=='__main__':unittest.main()
