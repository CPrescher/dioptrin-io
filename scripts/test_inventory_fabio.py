"""Controls preventing stale backlog regeneration or inflated detector blockers."""
import copy
import json
import pathlib
import tempfile
import unittest
from unittest.mock import patch
import inventory_fabio as inv


class InventoryControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.config = inv.load(inv.ROOT / inv.CONFIG)
        cls.stored = inv.load(inv.ROOT / inv.CATALOG)
        cls.reference, cls.rows = inv.reference_catalog(None, cls.stored)
        cls.report = inv.load(inv.ROOT / cls.config['evidence_snapshot']['main_report'])

    def test_all_registered_readers_and_aliases_remain_visible(self):
        self.assertEqual(len(self.rows), 36)
        self.assertEqual(len({inv.ALIASES.get(r['module'], r['module']) for r in self.rows}), 35)
        stats = inv.validate(self.config, self.reference, self.rows)
        rendered = inv.render(self.config, self.reference, self.rows, stats)
        self.assertEqual(len(json.loads(rendered[inv.CATALOG])['formats']), 36)
        for module in self.config['formats']:
            self.assertIn(f'[{module}](', rendered['docs/FORMAT_COVERAGE.md'])
        with self.assertRaisesRegex(ValueError, 'every registered'):
            inv.validate(self.config, self.reference, self.rows[:-1])

    def test_only_six_named_completed_reference_cases_count_as_compatibility_gaps(self):
        entries = [e for e in self.config['entries'] if e['category'] == 'confirmed_compatibility_gap']
        self.assertEqual([e['id'] for e in entries], ['C-HDF5', 'C-NPY'])
        self.assertEqual(sum(inv.confirmed_cases(e) for e in entries), 6)
        case_report = inv.load(inv.ROOT / entries[0]['evidence'][0])
        for change in ['reference_failed', 'native_succeeded', 'malformed', 'duplicate', 'missing']:
            report = copy.deepcopy(case_report)
            row = report['unsupported'][0]
            if change == 'reference_failed':row['reference']['status'] = 'not_completed'
            elif change == 'native_succeeded':row['native']['status'] = 'completed'
            elif change == 'malformed':row['input_condition'] = 'malformed_or_truncated'
            elif change == 'duplicate':report['unsupported'].append(copy.deepcopy(row))
            else:report['unsupported'].pop(0)
            with patch.object(inv, 'load', return_value=report), self.assertRaises(ValueError, msg=change):
                inv.confirmed_cases(entries[0])

    def test_optional_boundaries_do_not_become_acquired_detector_incidents(self):
        self.assertEqual(self.config['confirmed_detector_gaps'], [])
        config = copy.deepcopy(self.config)
        config['confirmed_detector_gaps'].append({'title': 'Unsubstantiated format name'})
        with self.assertRaisesRegex(ValueError, 'reproducer'):
            inv.validate(config, self.reference, self.rows)
        config = copy.deepcopy(self.config)
        next(e for e in config['entries'] if e['category'] == 'optional_variant')['priority'] = 'release_blocker'
        with self.assertRaisesRegex(ValueError, 'automatic detector blocker'):
            inv.validate(config, self.reference, self.rows)

    def test_new_unclassified_reading_or_metadata_failure_requires_review(self):
        for change in ['native_rejected', 'unqualified_input', 'header', 'attribute', 'reference_test']:
            report = copy.deepcopy(self.report)
            if change == 'native_rejected':report['summary']['native_rejected'] = 1
            elif change == 'unqualified_input':report['input_api_summary']['all_events_qualified'] = False
            elif change == 'header':next(iter(report['metadata_summary'].values()))['unresolved_fields'] = 1
            elif change == 'attribute':next(iter(report['attribute_summary'].values()))['unresolved'] = {'new': 1}
            else:report['reference_tests']['errors'] = ['unexpected error']
            with patch.object(inv, 'load', return_value=report), self.assertRaises(ValueError, msg=change):
                inv.snapshot(self.config)

    def test_strict_reference_gate_is_not_relabelled_as_a_reader_gap(self):
        stats = inv.snapshot(self.config)
        self.assertFalse(stats['strict_full_reading_parity'])
        self.assertEqual(stats['reference_test_skips'], 1)
        output = json.loads(inv.render(self.config, self.reference, self.rows, stats)[inv.CATALOG])
        self.assertEqual(output['native_inventory']['summary']['confirmed_detector_issues'], 0)
        self.assertEqual(output['native_inventory']['summary']['confirmed_compatibility_fixture_cases'], 6)
        self.assertTrue(all(r['parity_verified'] is False for r in output['formats']))

    def test_outputs_are_regenerated_from_single_source_without_stale_pending_labels(self):
        outputs = inv.render(self.config, self.reference, self.rows, inv.snapshot(self.config))
        for name, text in outputs.items():
            self.assertEqual((inv.ROOT / name).read_text(), text, name)
        catalog = json.loads(outputs[inv.CATALOG])
        cross = json.dumps(catalog['cross_cutting_targets'])
        self.assertNotIn('pending', cross)
        by_module = {r['module']:r for r in catalog['formats']}
        for module, obsolete in [('mar345image','V2/spiral'),('numpyimage','bool/structured'),('pixiimage','traversal remains explicit gap'),('cbfimage','Semantic CIF/Pilatus parsing and multiple sections pending')]:
            self.assertNotIn(obsolete, by_module[module]['implementation'])
        config = copy.deepcopy(self.config)
        config['formats']['edfimage']['implementation'] += '; explicit new scope'
        changed = inv.render(config, self.reference, self.rows, inv.snapshot(config))
        self.assertNotEqual(changed[inv.CATALOG], outputs[inv.CATALOG])
        self.assertNotEqual(changed['docs/FORMAT_COVERAGE.md'], outputs['docs/FORMAT_COVERAGE.md'])

    def test_unknown_boundary_and_missing_evidence_fail_closed(self):
        config = copy.deepcopy(self.config)
        config['formats']['edfimage']['boundary_ids'].append('UNKNOWN')
        with self.assertRaisesRegex(ValueError, 'unknown boundary'):
            inv.validate(config, self.reference, self.rows)
        config = copy.deepcopy(self.config)
        config['entries'][0]['evidence'] = ['docs/does-not-exist.json']
        with self.assertRaisesRegex(ValueError, 'missing evidence'):
            inv.validate(config, self.reference, self.rows)


if __name__ == '__main__':
    unittest.main()
