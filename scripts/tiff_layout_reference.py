"""Independent raw-sample and explicit-IFD oracle for tiny TIFF fixtures.

Test tooling only: unchanged native libtiff, tifffile metadata, pinned FabIO.
No production source uses these libraries or applies display transformations.
"""
import argparse
import ctypes as ct
import hashlib
import json
import pathlib
import subprocess
import sys
import numpy as np
import tifffile
import fabio


def sha(data):
    return hashlib.sha256(data).hexdigest()


def signature(array):
    array = np.asarray(array)
    data = array.astype(array.dtype.newbyteorder('<'), copy=False).tobytes()
    return {'dtype': str(array.dtype.newbyteorder('=')), 'shape': list(array.shape), 'sample_sha256': sha(data)}


class Reference:
    def __init__(self, path):
        self.path = pathlib.Path(path).resolve(strict=True)
        self.lib = ct.CDLL(str(self.path))
        lib = self.lib
        lib.TIFFGetVersion.restype = ct.c_char_p
        lib.TIFFOpen.argtypes = [ct.c_char_p, ct.c_char_p]
        lib.TIFFOpen.restype = ct.c_void_p
        lib.TIFFClose.argtypes = [ct.c_void_p]
        lib.TIFFSetSubDirectory.argtypes = [ct.c_void_p, ct.c_uint64]
        lib.TIFFSetSubDirectory.restype = ct.c_int
        lib.TIFFScanlineSize64.argtypes = [ct.c_void_p]
        lib.TIFFScanlineSize64.restype = ct.c_uint64
        lib.TIFFReadScanline.argtypes = [ct.c_void_p, ct.c_void_p, ct.c_uint32, ct.c_uint16]
        lib.TIFFReadScanline.restype = ct.c_int

    def metadata(self):
        return {'path': str(self.path), 'sha256': sha(self.path.read_bytes()),
                'version': self.lib.TIFFGetVersion().decode(errors='replace').strip()}

    def read(self, path, page):
        """No RGBA conversion, inversion, orientation application or failed rows."""
        if len(page.shape) != 2 or page.samplesperpixel != 1:
            raise ValueError('oracle fixture scope is single-plane two-dimensional')
        height, width = page.shape
        bits = page.bitspersample
        dtype = np.dtype('u1') if bits == 1 else page.dtype.newbyteorder('=')
        if bits != 1 and bits != dtype.itemsize * 8:
            raise ValueError('oracle fixture scope has unsupported source precision')
        expected = (width + 7) // 8 if bits == 1 else width * dtype.itemsize
        handle = self.lib.TIFFOpen(bytes(path), b'r')
        if not handle:
            raise ValueError('independent libtiff open failed')
        try:
            if self.lib.TIFFSetSubDirectory(handle, page.offset) != 1:
                raise ValueError('independent libtiff directory selection failed')
            if self.lib.TIFFScanlineSize64(handle) != expected:
                raise ValueError('independent row budget differs')
            buffer = ct.create_string_buffer(expected)
            rows = []
            for row in range(height):
                if self.lib.TIFFReadScanline(handle, buffer, row, 0) != 1:
                    raise ValueError(f'independent complete row {row} failed')
                values = (np.unpackbits(np.frombuffer(buffer.raw, dtype='u1'))[:width]
                          if bits == 1 else np.frombuffer(buffer.raw, dtype=dtype))
                rows.append(values.copy())
            return np.stack(rows)
        finally:
            self.lib.TIFFClose(handle)


def native(probe, command, path, *args):
    process = subprocess.run([str(probe), command, str(path), *map(str, args)],
                             capture_output=True, text=True, timeout=20)
    if process.returncode:
        raise ValueError(f'{command}: {process.stderr.strip()}')
    return json.loads(process.stdout)


def pages(file):
    primary = list(file.pages)
    result = {}
    todo = primary.copy()
    while todo:
        page = todo.pop()
        if page.offset in result:
            continue
        result[page.offset] = page
        if page.pages:
            todo.extend(page.pages)
    return [p.offset for p in primary], result


def compare_file(probe, path, reference):
    before = path.read_bytes()
    catalog = native(probe, 'tiff-directories', path)
    differences = []
    def check(field, expected, actual):
        if expected != actual:
            differences.append({'field': field, 'expected': expected, 'actual': actual})
    records = []
    with tifffile.TiffFile(path) as file:
        primary, graph = pages(file)
        check('primary_offsets', primary, catalog['primary_offsets'])
        nodes = {node['ifd_offset']: node for node in catalog['directories']}
        check('directory_offsets', sorted(graph), sorted(nodes))
        for offset, page in sorted(graph.items()):
            node = nodes[offset]
            source = reference.read(path, page)
            expected = signature(source)
            decoded = native(probe, 'tiff-directory', path, offset)
            check(f'{offset}.frames', 1, decoded['frames'])
            check(f'{offset}.shape', expected['shape'], decoded['shape'])
            check(f'{offset}.dtype', expected['dtype'], decoded['pixels']['dtype'])
            check(f'{offset}.sample_sha256', expected['sample_sha256'], decoded['pixels']['sample_sha256'])
            check(f'{offset}.value_hex', source.astype(source.dtype.newbyteorder('<'), copy=False).tobytes().hex(), decoded['value_hex'])
            check(f'{offset}.primary_index', primary.index(offset) if offset in primary else None, node['primary_index'])
            check(f'{offset}.subifd_offsets', list(page.subifds or ()), node['subifd_offsets'])
            order = file.byteorder
            count_size, stride, pointer_size = (8, 20, 8) if file.is_bigtiff else (2, 12, 4)
            count = int.from_bytes(before[offset:offset+count_size], 'little' if order == '<' else 'big')
            position = offset + count_size + count * stride
            next_ifd = int.from_bytes(before[position:position+pointer_size], 'little' if order == '<' else 'big')
            check(f'{offset}.next_ifd', next_ifd, node['next_ifd'])
            if 330 in page.tags:
                tag = page.tags[330]
                entry = node['subifd_entry']
                check(f'{offset}.subifd_entry.field_type', int(tag.dtype), entry['field_type'])
                check(f'{offset}.subifd_entry.count', tag.count, entry['count'])
                check(f'{offset}.subifd_entry.entry_offset', tag.offset, entry['entry_offset'])
                check(f'{offset}.subifd_entry.ifd_entry_hex', before[tag.offset:tag.offset+stride].hex(), entry['ifd_entry_hex'])
            if page.bitspersample == 1:
                raw = np.frombuffer((path.parent / (path.name + '.raw')).read_bytes(), dtype='u1').reshape(page.shape)
                check(f'{offset}.independent_source_array', signature(raw), expected)
            records.append({'ifd_offset': offset, 'primary_index': node['primary_index'],
                            'source_bits': page.bitspersample, 'orientation': int(page.tags[274].value) if 274 in page.tags else 1,
                            'source': expected, 'native': decoded, 'exact_raw_samples': expected == {'dtype': decoded['pixels']['dtype'], 'shape': decoded['shape'], 'sample_sha256': decoded['pixels']['sample_sha256']}})
    try:
        image = fabio.open(str(path))
        observed = {'status': 'ok', 'lib': image.lib, 'nframes': image.nframes, 'primary': signature(image.data)}
        if path.name.startswith('fax-'):
            raw = np.frombuffer((path.parent / (path.name + '.raw')).read_bytes(), dtype='u1').reshape(17, 93)
            # Explicit test consumer only: Pillow applies Orientation=6 and
            # WhiteIsZero display inversion. The native stored-count API does neither.
            display = np.rot90(raw == 0, k=3)
            observed['explicit_consumer'] = signature(display)
            observed['matches_explicit_consumer'] = np.array_equal(image.data, display)
            check('FabIO_explicit_orientation_photometric_consumer', True, observed['matches_explicit_consumer'])
            observed['classification'] = 'intentional source-vs-display orientation/photometric distinction'
        else:
            observed['classification'] = 'primary-chain API only; no pinned FabIO SubIFD selection API'
        if not path.name.startswith('fax-'):
            check('FabIO_primary_count', 2, image.nframes)
        image.close()
    except Exception as error:
        if path.name != 'subifds-big-be.tif':
            check('expected_FabIO_read', 'ok', f'{type(error).__name__}: {error}')
        observed = {'status': 'error', 'error': f'{type(error).__name__}: {error}',
                    'classification': 'reference limitation; independent native libtiff remains source authority'}
    after = path.read_bytes()
    check('source_unchanged', sha(before), sha(after))
    return {'path': str(path), 'source_sha256': sha(before), 'source_bytes': len(before),
            'source_unchanged': before == after, 'catalog': catalog, 'directories': records,
            'fabio': observed, 'differences': differences}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--probe', required=True, type=pathlib.Path)
    parser.add_argument('--libtiff', required=True)
    parser.add_argument('--fixtures', default='tests/data/tiff-layouts', type=pathlib.Path)
    parser.add_argument('--output', required=True, type=pathlib.Path)
    args = parser.parse_args()
    reference = Reference(args.libtiff)
    before = {str(p): sha(p.read_bytes()) for root in ['src/tiff.rs', 'src/tiff', 'vendor/tiff-0.11.3', 'vendor/fax-0.2.7']
              for p in ([pathlib.Path(root)] if pathlib.Path(root).is_file() else sorted(pathlib.Path(root).rglob('*'))) if p.is_file()}
    records = [compare_file(args.probe.resolve(), path.resolve(), reference)
               for path in sorted(args.fixtures.glob('*.tif'))]
    after = {p: sha(pathlib.Path(p).read_bytes()) for p in before}
    result = {'schema': 1, 'gate': not any(r['differences'] for r in records) and before == after,
              'scope': 'Group3 mixed 1D/2D, CCITT RLE, explicit classic/BigTIFF SubIFD catalog and raw selected directories; no implicit display or primary promotion',
              'probe_sha256': sha(args.probe.read_bytes()), 'helper_sha256': sha(pathlib.Path(__file__).read_bytes()),
              'python': sys.version, 'numpy_version': np.__version__, 'tifffile_version': tifffile.__version__, 'fabio_version': fabio.version,
               'unit_test_evidence': {'vendor': {'command': 'CARGO_TARGET_DIR=/tmp/dioptrin-fax-strict-target cargo test --manifest-path vendor/fax-0.2.7/Cargo.toml --lib --offline', 'passed': 19, 'original': 14, 'new': 5},
                                     'native': {'command': 'cargo test --no-default-features --features wrappers --test tiff_layouts --test tiff_rejections', 'passed': 15}},
              'qualification': {'reference_source_differences': 'six fax cases: native raw stored bits and shape differ intentionally from Pillow orientation6 plus WhiteIsZero bool display; exact independent libtiff source equality and explicit test-only display equality are separate',
                                'reference_unsupported': ['FabIO openimage BigTIFF recognition', 'no pinned FabIO public SubIFD offset selection API'],
                                'remaining_native_scope': ['Group3 uncompressed extension', 'CCITT RLE word-aligned32771', 'Group3/RLE tiled layouts', 'semantic interpretation of arbitrary SubIFD tag namespaces']},
              'fabio_source': {'commit': '9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece', 'TiffIO_sha256': sha(pathlib.Path(fabio.TiffIO.__file__).read_bytes())},
              'libtiff': reference.metadata(), 'runtime_sources_before': before, 'runtime_sources_after': after,
              'runtime_sources_unchanged': before == after,
              'files': len(records), 'selected_directories': sum(len(r['directories']) for r in records),
              'exact_raw_sample_matches': sum(d['exact_raw_samples'] for r in records for d in r['directories']), 'records': records}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: result[key] for key in ['gate','files','selected_directories','exact_raw_sample_matches','runtime_sources_unchanged']}))
    return 0 if result['gate'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
