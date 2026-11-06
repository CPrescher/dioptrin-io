"""Generate small independent CC0 TIFF layout/CCITT fixtures.

Test-only libtiff/tifffile tooling. Never reads or rewrites the external corpus.
"""
import argparse
import ctypes as ct
import hashlib
import json
import pathlib
import numpy as np
import tifffile


def sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def generate(output, library):
    output = pathlib.Path(output)
    output.mkdir(parents=True, exist_ok=True)
    binary = pathlib.Path(library).resolve(strict=True)
    lib = ct.CDLL(str(binary))
    lib.TIFFGetVersion.restype = ct.c_char_p
    lib.TIFFOpen.argtypes = [ct.c_char_p, ct.c_char_p]
    lib.TIFFOpen.restype = ct.c_void_p
    lib.TIFFClose.argtypes = [ct.c_void_p]
    lib.TIFFSetField.argtypes = [ct.c_void_p, ct.c_uint]
    lib.TIFFSetField.restype = ct.c_int
    lib.TIFFWriteScanline.argtypes = [ct.c_void_p, ct.c_void_p, ct.c_uint32, ct.c_uint16]
    lib.TIFFWriteScanline.restype = ct.c_int
    # Non-byte width, mixed changing elements, makeup and both edge colors.
    pixels = np.array([[int((x // 5 + y) % 3 == 0 or (x + y) % 19 == 0)
                        for x in range(93)] for y in range(17)], dtype='u1')
    for compression, options in [(2, 0), (3, 1), (3, 5)]:
        for fill in [1, 2]:
            path = output / f'fax-{compression}-options{options}-fill{fill}.tif'
            handle = lib.TIFFOpen(bytes(path), b'w')
            if not handle:
                raise ValueError('libtiff writer could not open fixture')
            try:
                tags = [(256, 93), (257, 17), (258, 1), (259, compression),
                        (262, 0), (266, fill), (277, 1), (278, 6), (284, 1), (274, 6)]
                if compression == 3:
                    tags.append((292, options))
                for tag, value in tags:
                    if lib.TIFFSetField(handle, tag, ct.c_uint(value)) != 1:
                        raise ValueError(f'libtiff rejected tag {tag}')
                for row in range(17):
                    packed = np.packbits(pixels[row])
                    if lib.TIFFWriteScanline(handle, packed.ctypes.data, row, 0) != 1:
                        raise ValueError('libtiff rejected complete fixture row')
            finally:
                lib.TIFFClose(handle)
            (output / (path.name + '.raw')).write_bytes(pixels.tobytes())
    for name, options in [('subifds-classic.tif', {}),
                          ('subifds-big-be.tif', {'bigtiff': True, 'byteorder': '>'})]:
        with tifffile.TiffWriter(output / name, **options) as writer:
            writer.write(np.arange(20, dtype='u1').reshape(4, 5), subifds=2, metadata=None)
            writer.write((np.arange(6, dtype='u2') * 10001).reshape(2, 3), subfiletype=1, metadata=None)
            writer.write(np.array([[0.5, -1.5, 7.25, 0.0]], dtype='f4'), subfiletype=1, metadata=None)
            writer.write(np.array([[-3, -2], [-1, 0], [1, 2]], dtype='i2'), metadata=None)
    files = {p.name: {'bytes': p.stat().st_size, 'sha256': sha(p)}
             for p in sorted(output.iterdir()) if p.name.endswith(('.tif', '.raw'))}
    manifest = {'license': 'CC0-1.0', 'origin': 'independent deterministic integer/float arrays; native libtiff CCITT encoder and tifffile IFD writer',
                'numpy_version': np.__version__, 'tifffile_version': tifffile.__version__,
                'libtiff': {'path': str(binary), 'sha256': sha(binary),
                            'version': lib.TIFFGetVersion().decode().strip()},
                'generator_sha256': sha(__file__), 'files': files}
    (output / 'provenance.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', default='tests/data/tiff-layouts')
    parser.add_argument('--libtiff', required=True)
    args = parser.parse_args()
    generate(args.output, args.libtiff)
