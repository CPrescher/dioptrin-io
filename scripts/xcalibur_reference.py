"""Compare the unchanged upstream Xcalibur calls against explicit native APIs.

The .ccd API is metadata plus caller-shaped derived masks, never source counts.
The native full decomposition may partition rectangles differently from pyFAI.
"""
import dataclasses
import functools
import hashlib
import json
import pathlib
import subprocess
import tempfile

import numpy as np


def digest(data):
    return hashlib.sha256(data).hexdigest()


def normalize(value, key=None):
    # Named scalar source fields are accidentally returned as one-element tuples
    # by pinned FabIO. Do not apply a general singleton-list coercion.
    if key in {'inumofcornermasks', 'inumofglowingcornermasks', 'itreatment'} and isinstance(value, tuple) and len(value) == 1:
        value = value[0]
    if isinstance(value, dict):
        return {k: normalize(v, k) for k, v in value.items()}
    if isinstance(value, (tuple, list)):
        return [normalize(v) for v in value]
    return value


def field_comparison(reference, native):
    from metadata_parity import _equal
    fields = normalize(dataclasses.asdict(reference))
    defaults = normalize(dataclasses.asdict(type(reference)()))
    differences = []
    absent = []
    for key, value in fields.items():
        if key not in native:
            if _equal(value, defaults[key]):
                absent.append(key)
            else:
                differences.append({'field': key, 'reference': value, 'native': 'absent'})
        elif not _equal(value, native[key]):
            differences.append({'field': key, 'reference': value, 'native': native[key]})
    differences.extend({'field': key, 'native': 'unexpected'} for key in native.keys() - fields.keys())
    return {'status': 'match' if not differences else 'difference', 'fields': len(fields),
            'absent_sections_with_reference_defaults': absent, 'differences': differences,
            'scalar_mapping': ['inumofcornermasks', 'inumofglowingcornermasks', 'itreatment']}


def binary_mask(values):
    values = np.asarray(values)
    if values.ndim != 2 or not np.isin(values, [0, 1]).all():
        raise ValueError('comparison requires a two-dimensional binary mask')
    # int8 -> uint8 is value-preserving only after that binary-value assertion.
    return np.ascontiguousarray(values, dtype=np.uint8)


def from_native_regions(native):
    from fabio.xcaliburimage import CcdCharacteristiscs, ChipBadPolygon, ChipBadPoint, ChipPoint
    points = [ChipBadPoint(ChipPoint(**p['spt']), ChipPoint(**p['sptreplace1']),
                          ChipPoint(**p['sptreplace2']), p['itreatment']) for p in native['points']]
    polygons = [ChipBadPolygon(**p) for p in native['polygons']]
    return CcdCharacteristiscs(pschipbadpoint=points, pschipbadpolygon=polygons)


class Bridge:
    def __init__(self, probe, capabilities, timeout, report, current_test):
        self.probe, self.capabilities, self.timeout = probe, capabilities, timeout
        self.report, self.current_test = report, current_test
        self.path = None
        report['xcalibur_api'] = []

    def probe_json(self, *args):
        result = subprocess.run([str(self.probe), *map(str, args)], capture_output=True, text=True, timeout=self.timeout)
        if result.returncode:
            raise RuntimeError(result.stderr.strip())
        return json.loads(result.stdout)

    def capture(self, operation, compare):
        record = {'test': self.current_test(), 'codec': 'xcaliburimage', 'operation': operation}
        self.report['xcalibur_api'].append(record)
        if 'xcalibur' not in self.capabilities['features']:
            record['status'] = 'feature_disabled'
            return
        try:
            compare(record)
        except Exception as error:
            record.update(status='comparison_error', error=f'{type(error).__name__}: {error}')

    def characteristics(self, source, reference, record):
        with tempfile.TemporaryDirectory(prefix='dioptrin-xcalibur-') as temporary:
            path = pathlib.Path(temporary) / 'buffer.ccd'
            path.write_bytes(source)
            direct = self.path is None
            native = self.probe_json('xcalibur-bytes' if direct else 'xcalibur-ccd', path, '--retain-truncated-tail')
            incomplete = native['incomplete_machine_function']
            strict = subprocess.run([str(self.probe), 'xcalibur-ccd', str(path)], capture_output=True, text=True, timeout=self.timeout)
            if incomplete is not None:
                if strict.returncode == 0:
                    raise ValueError('strict reader accepted incomplete machine function')
                offset = incomplete['offset']
                if incomplete != {'offset':offset, 'expected_bytes':18, 'available_bytes':len(source)-offset} or len(source)-offset >= 18:
                    raise ValueError('incorrect truncation diagnostic')
                if native['trailing_sha256'] != digest(source[offset:]):
                    raise ValueError('truncated bytes were not retained exactly')
            elif strict.returncode:
                raise ValueError('strict reader rejected complete source')
            self.report['source_files'].setdefault(digest(source), {'name':str(self.path) if self.path else 'serialized CCD byte buffer', 'bytes':len(source)})
            record.update(incomplete_machine_function=incomplete, strict_read='rejected' if strict.returncode else 'accepted',
                          read_policy='RetainTruncatedMachineFunction', source_complete=incomplete is None)
            record.update(source=str(self.path) if self.path else 'serialized CCD byte buffer',
                          bytes=len(source), source_sha256=digest(source), native_api='Characteristics::from_bytes' if direct else 'Reader::open',
                          fields=field_comparison(reference, native['fields']), parsed_bytes=native['parsed_bytes'],
                          missing_sections=native['missing_sections'],
                          source_bytes_match=native['source_sha256'] == digest(source),
                          trailer_bytes=len(source)-native['parsed_bytes'])
            # The file has no intrinsic raster shape. Use an explicitly selected
            # enclosing shape for source rectangles/points, retain orientation.
            xs = [int(x) for p in reference.pschipbadpolygon if p.itype == 1 for x in p.iax[:2]]
            ys = [int(y) for p in reference.pschipbadpolygon if p.itype == 1 for y in p.iay[:2]]
            xs += [p.spt.ix for p in reference.pschipbadpoint]
            ys += [p.spt.iy for p in reference.pschipbadpoint]
            shape = [max(ys, default=0)+1, max(xs, default=0)+1]
            expected = binary_mask(reference.build_mask(shape))
            masks = {}
            for projection in ['strict', 'fabio']:
                actual = self.probe_json('xcalibur-ccd', path, *shape, projection, '--retain-truncated-tail')['mask']
                masks[projection] = dict(shape=shape, sha256=actual['sha256'],
                    ignored_records=actual['ignored_records'], match=actual['sha256'] == digest(expected.tobytes()))
            record['derived_masks'] = masks
            record['status'] = 'match' if (record['fields']['status'] == 'match' and record['source_bytes_match'] and all(v['match'] for v in masks.values())) else 'difference'

    def decomposition(self, image, reference, full, original, record):
        values = binary_mask(image.data)
        shape = list(values.shape)
        source_hash = digest(values.tobytes())
        record.update(shape=shape, source_sha256=source_hash, full_requested=full)
        with tempfile.TemporaryDirectory(prefix='dioptrin-xcalibur-mask-') as temporary:
            path = pathlib.Path(temporary) / 'binary.u8'
            path.write_bytes(values.tobytes())
            coarse_reference = binary_mask(original(image, full=False).build_mask(shape))
            native_coarse = self.probe_json('xcalibur-decompose', path, *shape, 'coarse')
            native_full = self.probe_json('xcalibur-decompose', path, *shape, 'full')
            # Reconstruct native geometry using the actual independent reference
            # renderer. Native geometry is derived here, not copied from FabIO.
            cross_hash = digest(binary_mask(from_native_regions(native_full).build_mask(shape)).tobytes())
            reference_hash = digest(binary_mask(reference.build_mask(shape)).tobytes())
            record.update(coarse_mask_match=native_coarse['reconstructed']['sha256'] == digest(coarse_reference.tobytes()),
                native_full_mask_match=native_full['reconstructed']['sha256'] == source_hash,
                reference_renderer_native_geometry_match=cross_hash == source_hash,
                reference_full_reconstructs_source=reference_hash == source_hash,
                reference_mask_sha256=reference_hash,
                native_full_regions={'polygons':len(native_full['polygons']), 'points':len(native_full['points'])},
                native_coarse_regions={'polygons':len(native_coarse['polygons']), 'points':len(native_coarse['points'])},
                native_source_match=native_full['source_sha256'] == source_hash == native_coarse['source_sha256'])
            try:
                import pyFAI.ext.dynamic_rectangle  # noqa: F401
                record['reference_full_dependency'] = 'pyFAI present'
            except ImportError:
                record['reference_full_dependency'] = 'pyFAI absent: upstream full=True falls back to coarse; original equality assertion disabled'
            record['status'] = 'match' if all(record[k] for k in ['coarse_mask_match', 'native_full_mask_match', 'reference_renderer_native_geometry_match', 'native_source_match']) else 'difference'

    def install(self):
        from fabio.xcaliburimage import CcdCharacteristiscs, XcaliburImage
        read, loads, decompose = CcdCharacteristiscs.read.__func__, CcdCharacteristiscs.loads.__func__, XcaliburImage.decompose
        @classmethod
        @functools.wraps(read)
        def read_hook(cls, filename):
            old = self.path
            self.path = filename
            try:
                return read(cls, filename)
            finally:
                self.path = old
        @classmethod
        @functools.wraps(loads)
        def loads_hook(cls, source):
            result = loads(cls, source)
            self.capture('read' if self.path else 'loads', lambda record: self.characteristics(source, result, record))
            return result
        @functools.wraps(decompose)
        def decompose_hook(image, full=False):
            result = decompose(image, full=full)
            self.capture('decompose', lambda record: self.decomposition(image, result, full, decompose, record))
            return result
        CcdCharacteristiscs.read = read_hook
        CcdCharacteristiscs.loads = loads_hook
        XcaliburImage.decompose = decompose_hook
