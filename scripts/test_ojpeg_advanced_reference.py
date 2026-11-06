"""Evidence/authority controls for the advanced OJPEG proof."""
import unittest
import copy
from unittest.mock import patch
import numpy as np
from compare_ojpeg_advanced import fabio_observation,qualified_stock_observations
from generate_ojpeg_advanced_fixtures import source_planes
class FakeImage:
 def __init__(self,data):self.data=data;self.lib='PIL'
 def __enter__(self):return self
 def __exit__(self,*args):return False
class EvidenceControls(unittest.TestCase):
 def test_returned_no_pixels_is_a_failed_read_not_a_numeric_signature(self):
  with patch('compare_ojpeg_advanced.fabio.open',return_value=FakeImage(None)):
   self.assertEqual(fabio_observation('source.tif')['status'],'no_pixels')
 def test_unrelated_api_errors_are_not_qualified_as_decoder_rejections(self):
  with patch('compare_ojpeg_advanced.fabio.open',side_effect=AttributeError('unexpected API')):
   with self.assertRaises(AttributeError):fabio_observation('source.tif')
 def test_object_array_is_not_a_pixel_result(self):
  with patch('compare_ojpeg_advanced.fabio.open',return_value=FakeImage(np.array([None],dtype=object))):
   with self.assertRaises(TypeError):fabio_observation('source.tif')
 def test_numeric_display_result_is_recorded_as_display_only(self):
  with patch('compare_ojpeg_advanced.fabio.open',return_value=FakeImage(np.array([[1]],dtype='f4'))):
   result=fabio_observation('source.tif');self.assertEqual(result['status'],'ok');self.assertIn('separate',result['classification'])
 def test_native_jpeg_header_is_admitted_before_large_output_allocation(self):
  class Codec:
   destroyed=False
   def tj3Init(self,kind):return 1
   def tj3DecompressHeader(self,*args):return 0
   def tj3Get(self,handle,key):return {5:16,6:16,7:8,4:2}[key]
   def tj3DecompressToYUV8(self,*args):raise AssertionError('must not decode before dimension admission')
   def tj3Destroy(self,*args):self.destroyed=True
  codec=Codec()
  with self.assertRaisesRegex(ValueError,'header differs'):source_planes(codec,b'fixture',[10**12,10**12],[2,2])
  self.assertTrue(codec.destroyed)
class StockQualificationControls(unittest.TestCase):
 def setUp(self):
  self.stock={'stored-restarts-prefix.tif':{'status':'native_error','error':'incomplete independent source chunk 1: -1 != 3072','diagnostics':'Corrupt JPEG data: premature end of data segment. LibJpeg: Unexpected error.'},'planar420-complete.tif':{'status':'native_error','error':'stock incomplete planar source component1: -1','diagnostics':'jpeg_start_decompress() returned max_h_samp_factor = 1 and max_v_samp_factor = 1, expected 2 and 2'},'table-grid-explicit-reset.tif':{'status':'ok','source_word_differences':[{'field':f'chunk{c}.plane{p}.sample_sha256'}for c in [2,3]for p in range(3)],'diagnostics':'Application transferred too many scanlines'}}
 def test_exact_known_source_layout_failures_are_qualified(self):self.assertTrue(qualified_stock_observations(self.stock))
 def test_missing_codec_unrelated_error_or_changed_output_is_not_qualified(self):
  for name,field,value in [('stored-restarts-prefix.tif','error','missing codec'),('planar420-complete.tif','diagnostics','unrelated oracle failure'),('table-grid-explicit-reset.tif','source_word_differences',[])]:
   stock=copy.deepcopy(self.stock);stock[name][field]=value;self.assertFalse(qualified_stock_observations(stock))
if __name__=='__main__':unittest.main()
