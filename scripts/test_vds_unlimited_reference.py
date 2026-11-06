"""Omission, ordering, bit-exactness and real HDF5 producer controls."""
import copy,itertools,json,pathlib,unittest
import h5py
import vds_printf_reference as common
from vds_unlimited_reference import DATA,compare_native,desired_target,gate,points,producer_controls
class UnlimitedControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):cls.manifest=json.loads((DATA/'provenance.json').read_text())
    def fake(self,name):
        case=self.manifest['cases'][name];config=common.request(case,DATA);mappings=[]
        with h5py.File(config['path']) as file:
            prop=file['data'].id.get_create_plist()
            for index,record in enumerate(case['mappings']):
                def selected(coordinates,shape):return {'kind':'Irregular','shape':shape,'maximum_shape':shape,'blocks':[[list(c),list(c)] for c in coordinates]}
                mappings.append({'stored_filename':record['file'],'declared_dataset':'/data','source_file':str((DATA/record['file']).resolve()),'resolved_dataset':'/data','source_shape':record['source_shape'],'source_pixel_type':'U64','source_little_endian':True,'storage_allocated':True,'nested_node':None,'printf_expansion':None,'selected_elements':len(record['selected_source_coordinates']),'source_selection':common.selection(prop.get_virtual_srcspace(index)),'virtual_selection':common.selection(prop.get_virtual_vspace(index)),'frozen_source_selection':selected(record['selected_source_coordinates'],record['source_shape']),'frozen_virtual_selection':selected(desired_target(record,case['shape']),case['shape'])})
        paths={config['path'],*(b['actual_file'] for b in config['vds_filenames'])}
        graph={'frozen_shape':case['shape'],'declared_shape':[0,*case['shape'][1:]],'master_max_shape':[2**64-1,*case['shape'][1:]],'source_file':config['path'],'dataset':'/data','unmapped_regions':'DeclaredFill' if case['unmapped_regions']=='declared_fill' else 'Reject','nested_nodes':[],'node_id':0,'fill_value':{'dtype':'uint64','value_hex':'ffffffffffffffff'},'effective_fill_regions':[],'unmapped_pixels_by_frame':[],'unmapped_pixels_per_frame':0,'mappings':mappings}
        native={'kind':'hdf5_layout','frames':case['shape'][0],'native_hdf5_version':[2,2,0],'metadata':[{'source_shape':case['shape'],'shape':case['shape'][1:],'frames':case['shape'][0],'first_frame':0,'pixel_type':'U64','little_endian':True,'virtual_dataset':graph}],'images':[{'status':'ok',**f} for f in common.expected_frames(case)],'sources':[{'path':p,'bytes':pathlib.Path(p).stat().st_size} for p in paths],'source_bindings':[{'requested':p,'resolved':p} for p in paths],'links':[]}
        return case,config,native
    def test_all_authored_words_and_selection_coordinate_cases_pass(self):
        for name,case in self.manifest['cases'].items():
            if case['expect']=='reject':continue
            case,config,native=self.fake(name);self.assertFalse(compare_native(native,case,config),name)
    def test_all_output_and_original_effective_metadata_required(self):
        case,config,native=self.fake('offset-stride')
        for target in [[],['metadata',0],['metadata',0,'virtual_dataset'],['metadata',0,'virtual_dataset','mappings',0],['images',0]]:
            original=native
            for key in target:original=original[key]
            for key in original:
                altered=copy.deepcopy(native);obj=altered
                for part in target:obj=obj[part]
                del obj[key]
                self.assertTrue(compare_native(altered,case,config),(target,key))
        for field in ['shape','maximum_shape','kind','blocks']:
            altered=copy.deepcopy(native);del altered['metadata'][0]['virtual_dataset']['mappings'][0]['frozen_source_selection'][field]
            self.assertTrue(compare_native(altered,case,config),field)
    def test_order_bit_pattern_and_unlimited_selection_cannot_be_faked(self):
        case,config,native=self.fake('spatial-stride')
        altered=copy.deepcopy(native);altered['images'][0]['value_hex']='00'+altered['images'][0]['value_hex'][2:];self.assertTrue(compare_native(altered,case,config))
        altered=copy.deepcopy(native);altered['metadata'][0]['virtual_dataset']['mappings'][0]['frozen_source_selection']['blocks'][0][0][0]=2;self.assertTrue(compare_native(altered,case,config))
        altered=copy.deepcopy(native);altered['metadata'][0]['virtual_dataset']['mappings'][0]['source_selection']['count'][0]=3;self.assertTrue(compare_native(altered,case,config))
        with self.assertRaises(ValueError):points({'kind':'Regular','shape':[2,2,3],'start':[0,0,0],'stride':[1,2,3],'count':[2**64-1,1,1],'block':[1,2,3]})
    def test_exact_source_graph_and_missing_pixels_not_filled_by_helper(self):
        case,config,native=self.fake('interleaved')
        for key in ['sources','source_bindings','images']:
            altered=copy.deepcopy(native);altered[key].pop();self.assertTrue(compare_native(altered,case,config),key)
        rejected=self.manifest['cases']['exhausted-maps'];self.assertEqual(rejected['expect'],'reject')
        self.assertEqual(len(rejected['mappings'][0]['selected_source_coordinates']),18)
        self.assertEqual(len(rejected['mappings'][1]['selected_source_coordinates']),12)
        self.assertEqual(len(desired_target(rejected['mappings'][1],rejected['shape'])),18)
    def test_real_hdf5_unlimited_union_restrictions_and_spatial_cardinality(self):
        result=producer_controls();self.assertEqual([v['result'] for v in result],['producer_rejected']*3+['finite_result','producer_rejected'])
        self.assertEqual(result[3]['selected_points'],0)
        self.assertEqual([v['name'] for v in self.manifest['producer_controls']],['reshape','partial-plane'])
        self.assertTrue(all(v['result']=='producer_rejected' for v in self.manifest['producer_controls']))
    def test_gate_requires_every_count_stability_and_no_boolean_coercion(self):
        wanted={'exact_cases':6,'exact_frames':27,'strict_missing_mapped_samples_cases':1,'differences':0,'errors':0,'imports_unchanged':True,'sources_stable':True,'probe_stable':True}
        self.assertTrue(gate(wanted))
        for key in wanted:
            altered=copy.deepcopy(wanted);del altered[key];self.assertFalse(gate(altered),key)
            altered=copy.deepcopy(wanted);altered[key]=0 if type(wanted[key]) is bool else wanted[key]+1;self.assertFalse(gate(altered),key)
if __name__=='__main__':unittest.main()
