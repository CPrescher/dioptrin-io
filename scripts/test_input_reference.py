"""Controls for real stdin, exact whole-record comparison and honest discovery."""
import copy
import io
import json
import pathlib
import subprocess
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from input_reference import (HeaderBridge, discovery_classification, differences, normalize_transport,
                             outer_integrity_evidence, qualify, reference_alignment,
                             routing_decisions, summarize, transport_manifest)


class ComparisonControls(unittest.TestCase):
    def test_only_explicit_exact_roots_are_normalized_without_mutation(self):
        source = {'sources': ['/private/capture', '/other/module.h5'],
                  'fields': {'name': '/private/capture/tail', 'filename': 'capture'},
                  'nested': [{'source_file': '/private/capture'}]}
        before = copy.deepcopy(source)
        normalized, records = normalize_transport(source, ['/private/capture'], '/original/root')
        self.assertEqual(source, before)
        self.assertEqual(normalized['sources'], ['/original/root', '/other/module.h5'])
        self.assertEqual(normalized['fields'], source['fields'])
        self.assertEqual(len(records), 2)

    def test_manifest_is_explicit_and_single(self):
        record = {'captured_root': '/capture', 'captured_roots': ['/encoded', '/capture'], 'original_argument': 'source::dataset'}
        manifest, roots = transport_manifest('native warning\n' + json.dumps({'input_transport': record}))
        self.assertEqual(manifest, record)
        self.assertEqual(roots, ['/capture', '/encoded'])
        for invalid in ['', json.dumps({'input_transport': record}) + '\n' + json.dumps({'input_transport': record}),
                        json.dumps({'input_transport': {'captured_root': 'relative', 'original_argument': 'source'}})]:
            with self.assertRaises(ValueError):
                transport_manifest(invalid)

    def test_all_fields_types_absence_and_plane_hashes_are_significant(self):
        expected = {'dtype': 'uint64', 'shape': [1, 2], 'sample_sha256': 'stored',
                    'components': [{'hash': 'exact', 'shape': [1, 2]}],
                    'view': {'sha256': 'display'}, 'integrity': 'Complete',
                    'headers': {'private': [1, 2]}}
        for field, value in [('dtype', 'float64'), ('shape', [2, 1]), ('sample_sha256', 'lost'),
                             ('components', [{'hash': 'lost', 'shape': [1, 2]}]),
                             ('view', {'sha256': 'wrong'}), ('integrity', 'Unverified'),
                             ('headers', {'private': [1, 3]})]:
            actual = copy.deepcopy(expected)
            actual[field] = value
            self.assertTrue(differences(actual, expected), field)
        self.assertTrue(differences({'a': None}, {}))
        self.assertTrue(differences({'a': True}, {'a': 1}))
        self.assertFalse(differences(expected, copy.deepcopy(expected)))

    def test_explicit_conservative_ambiguities_are_not_discovery_successes(self):
        for codec, expected in [('GEimage', 'Ge'), ('fit2dspreadsheetimage', 'Fit2dSpreadsheet')]:
            detection = {'selected': None, 'candidates': [{'codec': expected, 'evidence': 'AmbiguousHeader'}], 'resolution_error': 'explicit format required'}
            self.assertEqual(discovery_classification(codec, detection), 'conservative_ambiguity')
            detection['candidates'][0]['evidence'] = 'Signature'
            self.assertEqual(discovery_classification(codec, detection), 'difference')

    def test_family_aliases_do_not_hide_wrong_formats(self):
        def detection(codec):
            return {'selected': codec, 'candidates': [{'codec': codec, 'evidence': 'Signature', 'available': True}], 'resolution_error': None}
        self.assertEqual(discovery_classification('pilatusimage', detection('Tiff')), 'match')
        self.assertEqual(discovery_classification('hdf5image', detection('Eiger')), 'container_alias')
        self.assertEqual(discovery_classification('edfimage', detection('Tiff')), 'difference')
        self.assertEqual(discovery_classification('lambdaimage', detection('Lima')), 'difference')
        with self.assertRaises(ValueError):
            discovery_classification('edfimage', {'selected': 'Edf', 'candidates': []})
        self.assertEqual(discovery_classification('binaryimage', {'selected': None, 'candidates': []}), 'explicit_configuration_required')

    def test_ge_edf_container_alias_requires_successful_source_kind(self):
        detection = {'selected': 'Edf', 'candidates': [{'codec': 'Edf', 'evidence': 'StructuredHeader', 'available': True}], 'resolution_error': None}
        self.assertEqual(discovery_classification('GEimage', detection), 'difference')
        self.assertEqual(discovery_classification('GEimage', detection, [{'metadata_details': {'kind': 'ApsBlank8192'}}]), 'difference')
        frames = [{'metadata_details': {'kind': 'ApsEdf8192', 'edf_fields': {'Dim_1': '2048'}}}]
        self.assertEqual(discovery_classification('GEimage', detection, frames), 'source_qualified_container_alias')
        self.assertEqual(discovery_classification('GEimage', detection, [*frames, {'metadata_details': {}}]), 'difference')

    def test_filename_routing_and_stream_limits_remain_visible(self):
        reference = {'path': {'status': 'selected', 'codec': 'speimage'},
                     'bytes_io': {'status': 'rejected', 'error': 'no magic'},
                     'magic_without_filename': {'status': 'rejected'}, 'magic_with_filename': {'status': 'rejected'}}
        result = reference_alignment(reference, 'Spe', 'Spe')
        self.assertTrue(result['path']['filename_fallback'])
        self.assertEqual(result['bytes_io']['status'], 'reference_stream_limitation')
        self.assertEqual(reference['bytes_io']['status'], 'rejected')

    def test_recovery_is_not_complete_count_or_discovery_proof(self):
        source = {'source_recovery': {'source_report': {'compression': 'Gzip', 'integrity': 'UnverifiedTruncated'}}}
        self.assertEqual(len(outer_integrity_evidence([source])), 1)
        source['source_recovery']['source_report']['compression'] = 'None'
        self.assertFalse(outer_integrity_evidence([source]))
        self.assertFalse(summarize([{'status': 'counts_match'}])['all_events_qualified'])
        self.assertTrue(summarize([{'input_apis': {'status': 'qualified'}}])['all_events_qualified'])

    def test_actual_stdin_and_exact_extra_arguments_are_not_materialized(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'data.bin'
            payload = b'original\x00encoded\xffbytes'
            path.write_bytes(payload)
            original = [{'dtype': 'uint16', 'shape': [1, 2], 'sources': [str(path.resolve())], 'sha256': 'hash'}]
            seen = []
            def run(command, **kwargs):
                seen.append(command)
                if '--stream' in command:
                    self.assertNotIn('input', kwargs)
                    self.assertEqual(kwargs['stdin'].read(), payload)
                if command[1] != 'discover':
                    self.assertEqual(command[1:8], ['raw', str(path), '2', '1', '0', 'uint16', '>'])
                    rows = copy.deepcopy(original)
                    rows[0]['sources'] = ['/private/capture']
                    stderr = json.dumps({'input_transport': {'captured_root': '/private/capture', 'original_argument': str(path)}})
                    return subprocess.CompletedProcess(command, 0, json.dumps(rows[0]) + '\n', stderr)
                value = {'candidates': [], 'selected': None, 'resolution_error': 'unknown',
                         'provenance': {'stream': '--stream' in command, 'origin_directory': str(path.resolve().parent) if '--stream' in command else None,
                                        'encoded_bytes': len(payload), 'decoded_bytes': len(payload), 'compression': 'None'}}
                return subprocess.CompletedProcess(command, 0, json.dumps(value), '')
            reference = {'path': {'status': 'rejected'}, 'bytes_io': {'status': 'rejected'}}
            with patch('input_reference.subprocess.run', side_effect=run), patch('input_reference.routing_decisions', return_value=reference):
                result = qualify('/probe', 'binaryimage', 'raw', path, str(path), ['2', '1', '0', 'uint16', '>'], json.dumps(original[0]), 5)
            self.assertEqual(result['status'], 'qualified', result)
            self.assertEqual(len(seen), 3)
            self.assertEqual(result['native_stream']['path_replacements'][0]['field'], '$[0].sources[0]')

    def test_no_retry_on_native_stream_error(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'data'
            path.write_bytes(b'original')
            calls = []
            def run(command, **kwargs):
                calls.append(command)
                return subprocess.CompletedProcess(command, 2, '', 'bad encoded source')
            with patch('input_reference.subprocess.run', side_effect=run), patch('input_reference.routing_decisions', return_value={}):
                result = qualify('/probe', 'edfimage', 'edf', path, str(path), ['--policy', 'explicit'], '{}', 5)
            self.assertEqual(result['status'], 'difference')
            self.assertEqual(sum(command[1] == 'edf' for command in calls), 1)

    def test_header_bridge_never_accesses_pixels_and_errors_fail_qualification(self):
        class HeaderImage:
            _frames = [SimpleNamespace(header={'Dim_1': '2', 'Dim_2': '1'})]
            @property
            def data(self):
                raise AssertionError('header qualification accessed pixels')
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'header.edf'
            path.write_bytes(b'{ independent header bytes }')
            row = {'header_only': True, 'pixels_available': False, 'fields': {'DIM_1': '2', 'DIM_2': '1'}}
            report = {}
            bridge = HeaderBridge('/probe', 5, report, lambda: 'control')
            process = subprocess.CompletedProcess([], 0, json.dumps(row), '')
            record = {}
            with patch('input_reference._run', return_value=process), patch('input_reference.qualify', return_value={'status': 'qualified'}):
                bridge.compare(record, HeaderImage(), str(path), None)
            self.assertEqual(record['status'], 'qualified')
            row['fields']['DIM_2'] = '9'
            process.stdout = json.dumps(row)
            with patch('input_reference._run', return_value=process), patch('input_reference.qualify', return_value={'status': 'qualified'}):
                bridge.compare(record, HeaderImage(), str(path), None)
            self.assertEqual(record['status'], 'difference')

    def test_hdf5_path_metadata_dependency_is_distinct_from_filename_guess(self):
        reference = {'path': {'status': 'selected', 'codec': 'lambdaimage'},
                     'bytes_io': {'status': 'rejected'},
                     'magic_without_filename': {'status': 'rejected'},
                     'magic_with_filename': {'status': 'selected', 'format': 'lambda'}}
        result = reference_alignment(reference, 'Lambda', 'Lambda')
        self.assertTrue(result['path']['path_container_metadata_dependency'])
        self.assertNotIn('filename_fallback', result['path'])

    def test_real_pinned_reference_path_and_bytesio_choose_without_reading(self):
        # Valid NPY magic plus deliberately invalid payload: _openimage selects
        # a class only. Calling read to guess/fallback would fail this control.
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'misleading.edf'
            path.write_bytes(b'\x93NUMPY\x01\x00invalid payload')
            result = routing_decisions(path, str(path))
            self.assertEqual(result['path']['codec'], 'numpyimage')
            self.assertEqual(result['bytes_io']['codec'], 'numpyimage')
            self.assertEqual(path.read_bytes(), b'\x93NUMPY\x01\x00invalid payload')


if __name__ == '__main__':
    unittest.main()
