"""Render the pinned reference catalog and classified native reading inventory.

python scripts/inventory_fabio.py [PINNED_FABIO_CHECKOUT] [--check]
Without a checkout, retain the committed reference descriptors and hashes.
Native scope, boundaries and validation priorities have one authored source:
docs/reading-inventory.json. No reference module is imported or executed.
"""
import argparse
import ast
import collections
import hashlib
import json
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
PIN = '9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'
ALIASES = {'adscimage': 'dtrekimage'}
CONFIG = 'docs/reading-inventory.json'
CATALOG = 'docs/fabio-2026.6.0-inventory.json'
CATEGORIES = {
    'confirmed_compatibility_gap': 'Confirmed supplemental compatibility gaps',
    'optional_variant': 'Optional variants: implement when an acquired file requires them',
    'intentional_difference': 'Intentional source and interpretation policies',
    'reference_limitation': 'Reference limitations, not native implementation tasks',
    'out_of_scope': 'Outside native detector input',
}
QUALIFIED = {
    'counts_match', 'reference_view_match', 'reference_no_pixels',
    'partial_source_read', 'reconstruction_match', 'component_view_match',
    'reference_decoder_defect',
}
OUTPUTS = (CATALOG, 'docs/FORMAT_COVERAGE.md', 'docs/READING_GAPS.md')


def load(path):
    return json.loads(pathlib.Path(path).read_text())


def sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def reference_catalog(checkout, stored):
    """Preserve only reference facts; native annotations always come from CONFIG."""
    keys = ('module', 'class', 'extensions', 'source', 'source_sha256')
    if checkout is None:
        if stored['reference']['commit'] != PIN:
            raise ValueError('stored reference is not the pinned FabIO revision')
        return stored['reference'], [{k: row[k] for k in keys} for row in stored['formats']]
    if subprocess.check_output(['git', '-C', str(checkout), 'rev-parse', 'HEAD'], text=True).strip() != PIN:
        raise ValueError('requires the pinned FabIO checkout')
    src = checkout / 'src/fabio'
    registry = ast.parse((src / 'fabioformats.py').read_text())
    codecs = next(ast.literal_eval(n.value) for n in registry.body if isinstance(n, ast.Assign)
                  and any(isinstance(a, ast.Name) and a.id == '_default_codecs' for a in n.targets))
    rows = []
    for module, cls in codecs:
        path = src / (module + '.py')
        # A dirty reference must not silently acquire the pinned commit's identity.
        expected = subprocess.check_output(['git', '-C', str(checkout), 'show', f'{PIN}:src/fabio/{module}.py'])
        if path.read_bytes() != expected:
            raise ValueError(f'reference module differs from pinned source: {module}')
        tree = ast.parse(expected)
        c = next((n for n in tree.body if isinstance(n, ast.ClassDef) and n.name == cls), None)
        extensions = [] if c is None else next((ast.literal_eval(n.value) for n in c.body
            if isinstance(n, ast.Assign) and any(isinstance(a, ast.Name) and a.id == 'DEFAULT_EXTENSIONS' for a in n.targets)), [])
        rows.append(dict(module=module, **{'class': cls}, extensions=extensions,
                         source=f'https://github.com/silx-kit/fabio/blob/{PIN}/src/fabio/{module}.py',
                         source_sha256=hashlib.sha256(expected).hexdigest()))
    reference = dict(stored['reference'])
    reference.update(registry_entries=len(rows), unique_registered_codecs=len({ALIASES.get(r['module'], r['module']) for r in rows}),
                     registry_sha256=sha(src / 'fabioformats.py'))
    expected_registry = subprocess.check_output(['git', '-C', str(checkout), 'show', f'{PIN}:src/fabio/fabioformats.py'])
    if (src / 'fabioformats.py').read_bytes() != expected_registry:
        raise ValueError('reference registry differs from pinned source')
    return reference, rows


def snapshot(config, root=ROOT):
    report = load(root / config['evidence_snapshot']['main_report'])
    if report['reference']['commit'] != PIN:
        raise ValueError('evidence report reference differs from inventory')
    unknown = set(report['summary']) - QUALIFIED
    if unknown or not report['input_api_summary']['all_events_qualified']:
        raise ValueError('unclassified detector-reading outcome: reconcile the evidence first')
    if report['reference_tests']['failures'] or report['reference_tests']['errors']:
        raise ValueError('reference test failures require inventory review')
    unresolved = sum(v['unresolved_fields'] for v in report['metadata_summary'].values())
    attr = sum(len(v.get('unresolved', {})) for v in report['attribute_summary'].values())
    if unresolved or attr:
        raise ValueError('unresolved compared metadata requires inventory review')
    return dict(input_outcomes=len(report['reads']), input_status=report['input_api_summary']['status'],
                reading_outcomes=report['summary'], reference_tests=report['reference_tests']['run'],
                reference_test_skips=len(report['reference_tests']['skipped']),
                unresolved_compared_header_fields=unresolved, unresolved_compared_attributes=attr,
                strict_full_reading_parity=report['full_reading_parity'])


def confirmed_cases(entry, root=ROOT):
    """Count only named native rejections of completed unchanged reference reads."""
    report = load(root / entry['evidence'][0])
    rows = report.get('unsupported', report.get('rejections', []))
    index = {}
    for row in rows:
        name = row.get('dataset', row.get('case', {}).get('kind'))
        if name not in entry['case_names']:
            continue
        if name in index:
            raise ValueError('duplicate supplemental case name')
        index[name] = row
    if len(set(entry['case_names'])) != entry['case_count']:
        raise ValueError('confirmed case count must name distinct fixtures')
    for name in entry['case_names']:
        row = index.get(name, {})
        if (row.get('input_condition') != 'well_formed_variant'
                or row.get('reader_action') != 'unsupported'
                or row.get('reference', {}).get('status') != 'completed'
                or row.get('native', {}).get('status') not in {'error', 'not_completed'}):
            raise ValueError(f'not a demonstrated completed-reference/native-rejection case: {name}')
    return entry['case_count']


def validate(config, reference, rows, root=ROOT):
    if config['schema'] != 1 or config['reference_commit'] != PIN:
        raise ValueError('unsupported inventory schema/reference')
    modules = [r['module'] for r in rows]
    if len(modules) != len(set(modules)) or set(modules) != set(config['formats']):
        raise ValueError('every registered reader/alias must have exactly one scope record')
    if len(rows) != reference['registry_entries'] or len({ALIASES.get(r['module'], r['module']) for r in rows}) != reference['unique_registered_codecs']:
        raise ValueError('reference registry cardinality changed')
    entries = config['entries']
    ids = [e['id'] for e in entries]
    if len(ids) != len(set(ids)):
        raise ValueError('duplicate inventory identifier')
    for e in entries:
        if e['category'] not in CATEGORIES:
            raise ValueError('unknown inventory category')
        if not all(e.get(k) for k in ['id', 'title', 'detail', 'evidence']):
            raise ValueError('each boundary needs a description and evidence')
        if e['category'] in {'confirmed_compatibility_gap', 'optional_variant'} and e.get('priority') != 'file_triggered':
            raise ValueError('optional capability is not an automatic detector blocker')
        for p in e['evidence']:
            if not (root / p).is_file():
                raise ValueError(f'missing evidence: {p}')
        if e['category'] == 'confirmed_compatibility_gap':
            confirmed_cases(e, root)
    for module, data in config['formats'].items():
        if set(data['boundary_ids']) - set(ids):
            raise ValueError(f'unknown boundary for {module}')
        for p in data['validation']:
            if p not in OUTPUTS and not list(root.glob(p)):
                raise ValueError(f'missing validation path: {p}')
    for e in config['closed'] + config['validation_batches']:
        for p in e.get('evidence', e.get('existing_evidence', [])):
            if not (root / p).is_file():
                raise ValueError(f'missing closure/validation evidence: {p}')
    # New acquired-file incidents must carry their own evidence; never manufacture
    # a detector blocker from a broad optional-family count.
    for incident in config['confirmed_detector_gaps']:
        if not incident.get('reproducer') or not (root / incident['reproducer']).is_file():
            raise ValueError('confirmed detector gap needs a retained reproducer/manifest')
    return snapshot(config, root)


def link(path):
    target = path.removeprefix('docs/') if path.startswith('docs/') else '../' + path
    return f'[{pathlib.Path(path).stem}]({target})'


def render(config, reference, rows, stats):
    counts = collections.Counter(e['category'] for e in config['entries'])
    cases = sum(e['case_count'] for e in config['entries'] if e['category'] == 'confirmed_compatibility_gap')
    summary = dict(confirmed_detector_issues=len(config['confirmed_detector_gaps']),
                   confirmed_compatibility_areas=counts['confirmed_compatibility_gap'],
                   confirmed_compatibility_fixture_cases=cases,
                   optional_variant_families=counts['optional_variant'],
                   real_file_validation_batches=len(config['validation_batches']))
    formats = []
    for row in rows:
        native = config['formats'][row['module']]
        formats.append(dict(row, **{k: native[k] for k in ['subformats_and_types', 'compression', 'stack', 'implementation', 'validation', 'boundary_ids']}, parity_verified=False))
    catalog = dict(schema=2, reference=reference,
        scope='All pinned default readers/aliases remain visible. Qualified reading scope is separate from universal reference equality, optional variants and intentional policies. Write/object-mutation APIs are outside this crate.',
        native_inventory=dict(source=CONFIG, backlog='docs/READING_GAPS.md', summary=summary,
                              evidence_snapshot=config['evidence_snapshot'], measured_scope=stats),
        cross_cutting_targets=config['cross_cutting_targets'], formats=formats)
    overview = [
        '# FabIO reader coverage and current native scope', '',
        f'Reference: FabIO 2026.6.0, commit `{PIN}`; {reference["registry_entries"]} registry entries / {reference["unique_registered_codecs"]} distinct codecs (ADSC aliases d*TREK).', '',
        '**Current backlog and priorities: [READING_GAPS.md](READING_GAPS.md).**', '',
        'This catalog keeps every pinned reader visible. A restricted scope is not automatically a detector-reading bug. Boundary IDs distinguish demonstrated compatibility cases, optional variants, source policies and reference limitations. `parity_verified=false` means universal reference equality has not been certified; it is not a task count.', '',
        'Authored native scope and classifications have one source, [reading-inventory.json](reading-inventory.json). The [generated catalog](fabio-2026.6.0-inventory.json) preserves pinned reference descriptors/hashes. Dated proofs remain historical evidence.', '',
        '```sh', 'python scripts/inventory_fabio.py /path/to/pinned/fabio',
        'python scripts/inventory_fabio.py --check', '```', '',
        '| Reader | Reference subformats/types | Compression | Stack | Qualified native scope | Boundary IDs |',
        '|---|---|---|---|---|---|',
    ]
    for row in formats:
        boundaries = ', '.join(f'[{id}](READING_GAPS.md#{id.lower()})' for id in row['boundary_ids'])
        overview.append('| ' + ' | '.join([f'[{row["module"]}]({row["source"]})', row['subformats_and_types'], row['compression'], row['stack'], row['implementation'], boundaries]) + ' |')
    overview += ['', 'Read-only CIF1.1/2.0, ASCII table/profile companions, series/navigation, TIFF/Pilatus/NeXus helpers and encoded CBF access have separate qualified contracts linked from the [backlog](READING_GAPS.md).', '',
        'Captured/nonseekable input, conservative content discovery, optional HDF5/sparse input and explicit frozen series are implemented. No implicit neighboring-file lookup, unavailable-count padding or consumer orientation transform is added by this catalog.', '',
        'Further implementation is file-triggered: preserve a reproducible acquired source and independent expected counts/metadata before promoting an optional variant to a detector-reading blocker. Real-file validation is the next priority. Dioptas/PhaseSmith migration remains separate.', '']
    report = config['evidence_snapshot']['main_report']
    gaps = ['# Current detector-reading gap inventory', '',
        f'Reviewed {config["review_date"]}. This is the current backlog; dated audits and JSON proof reports retain the scope tested at their revision. Generated from [reading-inventory.json](reading-inventory.json).', '',
        '## What is actually outstanding', '',
        '| Measure | Total | Meaning |', '|---|---:|---|',
        f'| Confirmed acquired-detector incidents in this inventory | {summary["confirmed_detector_issues"]} | No retained unresolved incident; not a universal support claim |',
        f'| Unclassified outcomes in the acceptance snapshot | 0 / {stats["input_outcomes"]} | Complete reads, explicit views, partial sources and metadata-only events remain distinct |',
        f'| Confirmed supplemental compatibility areas | {summary["confirmed_compatibility_areas"]} | HDF5 image dtype representations and nonnumeric/structured NPY arrays |',
        f'| Named unsupported, FabIO-readable fixture cases | {cases} | Four HDF5 and two NPY cases; not six unread acquired detector files |',
        f'| Optional capability families | {summary["optional_variant_families"]} | Grouped boundaries, not a count of variants or required implementation tickets |',
        f'| Real-file validation batches | {summary["real_file_validation_batches"]} | Requirements and acquired-source qualification, not known decoder failures |', '',
        f'The [acceptance snapshot]({report.removeprefix("docs/")}) ran {stats["reference_tests"]} unchanged reference tests with {stats["reference_test_skips"]} existing slow-test skip; compared header fields and source attributes have zero unresolved differences. The [native checks]({config["evidence_snapshot"]["native_checks"].removeprefix("docs/")}) qualify the reader revision. This inventory cleanup does not rerun or rewrite those historical corpus proofs.', '',
        'The old `full_reading_parity` boolean is a strict reference-equality/instrumentation gate, not a backlog counter. It also requires no upstream skip, excludes independently qualified decoder defects, and retains exact metadata/presentation differences and broad harness annotations. Its false value does not mean that all those conditions need native decoding work.', '',
        '## Classification and promotion rules', '',
        '- A confirmed detector-reading gap needs an acquired-source reproducer, the required reading operation, observed failure and independent expected counts/metadata.',
        '- A confirmed compatibility case is narrower: the retained supplemental fixture completes in unchanged FabIO and the native reader rejects it. Acquired-detector relevance must still be established.',
        '- Optional variants remain file-triggered. A format name or an old `partial` label alone does not make them release blockers.',
        '- Intentional policies, failed/undefined reference branches and APIs outside native input are not implementation tickets.',
        '- Validation needs are tracked separately from code defects. Preserve original counts/dtypes/axes, raw metadata, source manifests and evidence provenance.', '',
    ]
    for category, title in CATEGORIES.items():
        gaps += ['## ' + title, '']
        for e in config['entries']:
            if e['category'] != category:
                continue
            gaps += [f'<a id="{e["id"].lower()}"></a>', f'### {e["id"]}: {e["title"]}', '', e['detail'], '',
                     'Evidence: ' + ', '.join(link(p) for p in e['evidence']) + '.', '']
    gaps += ['## Closed entries removed from the active backlog', '',
             '| Previously listed as pending | Qualified resolution | Evidence |', '|---|---|---|']
    for e in config['closed']:
        gaps.append('| ' + ' | '.join([e['title'], e['resolution'], ', '.join(link(p) for p in e['evidence'])]) + ' |')
    gaps += ['', 'These closures cover their documented contracts. They do not promote all possible datatype/layout combinations to universal support.', '',
             '## Next: acquired detector files', '',
             'Store large originals outside Git. For each acquisition, record detector/software origin, size/hash, source manifests, required reader options, dtype/shape/frame counts, complete count hashes, reserved values and consumer-required metadata with units. Distinguish acquired originals from converted or writer-generated files; use independent source evidence when FabIO fails.', '',
             '| Batch | Validation target | Existing evidence |', '|---|---|---|']
    for e in config['validation_batches']:
        gaps.append('| ' + ' | '.join([e['id'] + ': ' + e['title'], e['check'], ', '.join(link(p) for p in e['existing_evidence'])]) + ' |')
    gaps += ['', 'Promote a reproducible failure found during these batches into a confirmed detector incident, then close it against the same source. Existing fixture/reference qualifications remain visible; there is no automatic expansion through every optional family.', '']
    return {CATALOG: json.dumps(catalog, indent=2) + '\n', 'docs/FORMAT_COVERAGE.md': '\n'.join(overview), 'docs/READING_GAPS.md': '\n'.join(gaps)}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('fabio_source', nargs='?', type=pathlib.Path)
    parser.add_argument('--check', action='store_true', help='verify generated files without changing them')
    args = parser.parse_args(argv)
    config = load(ROOT / CONFIG)
    reference, rows = reference_catalog(args.fabio_source, load(ROOT / CATALOG))
    stats = validate(config, reference, rows)
    rendered = render(config, reference, rows, stats)
    stale = []
    for name, text in rendered.items():
        path = ROOT / name
        if args.check:
            if not path.is_file() or path.read_text() != text:
                stale.append(name)
        else:
            path.write_text(text)
    if stale:
        parser.error('stale generated inventory: ' + ', '.join(stale))
    print(json.dumps({'checked' if args.check else 'generated': list(rendered),
                      'registry_entries': len(rows), 'classified_boundaries': len(config['entries']),
                      'evidence': config['evidence_snapshot']['main_report']}))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
