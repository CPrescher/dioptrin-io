"""Controls for raw-byte integrity and rejection classification."""
import copy
import os
import pathlib
import subprocess
import tempfile
import unittest
from cbf_encoded_reference import DATA, compare_native, oracle


class EncodedControls(unittest.TestCase):
    def test_marker_trailer_and_declared_bounds_are_not_encoded_data(self):
        raw=(DATA/'valid.cbf').read_bytes()
        parsed=oracle(raw)
        self.assertEqual(parsed['bytes'],2)
        self.assertEqual(parsed['prefix_hex'],'0102')
        self.assertTrue(parsed['checksum_matches'])
        self.assertTrue(parsed['available_complete'])
        self.assertFalse(oracle((DATA/'truncated.cbf').read_bytes())['available_complete'])

    def test_integrity_and_encoding_identity_cannot_be_masked(self):
        expected=oracle((DATA/'valid.cbf').read_bytes())
        native={k:expected[k] for k in ('bytes','sha256','prefix_hex','computed_md5','declared_md5','checksum_matches','header_sha256','binary_fields','shape','source_pixel_type')}
        native.update(kind='cbf_encoded',representation='OriginalEncodedPayload')
        self.assertFalse(compare_native(native,expected))
        for key in native:
            changed=copy.deepcopy(native);changed[key]='changed'
            self.assertTrue(compare_native(changed,expected),key)
        self.assertFalse(oracle((DATA/'wrong-checksum.cbf').read_bytes())['checksum_matches'])
        self.assertIsNone(oracle((DATA/'no-checksum.cbf').read_bytes())['checksum_matches'])

    @unittest.skipUnless(hasattr(os,'mkfifo'),'POSIX FIFO control')
    def test_encoded_path_rejects_fifo_before_open(self):
        root=pathlib.Path(__file__).resolve().parents[1]
        probe=pathlib.Path(os.environ.get('DIOPTRIN_API_PROBE',root/'target/release/examples/api_probe')).resolve()
        if not probe.is_file():self.skipTest('API probe required')
        with tempfile.TemporaryDirectory() as directory:
            path=pathlib.Path(directory)/'source.fifo';os.mkfifo(path)
            result=subprocess.run([str(probe),'cbf-encoded',str(path)],capture_output=True,text=True,timeout=3)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('path snapshots require regular files',result.stderr)


if __name__=='__main__':unittest.main()
