"""Independent native-libtiff row oracle for the pinned partial ThunderScan file.

Import-safe test tooling only. No reader/runtime Python bridge and no repaired
failed-row bytes are used. Both original IFDs are compared in stored orientation.
"""
import ctypes
import hashlib
import os
import pathlib
import struct
import numpy as np

SOURCE_SHA256 = '0b9c03248c2e1d4acaf1437e9b16326a1e0da46342c56f38a4a1b62a51d448d7'

def _sha(data):
    return hashlib.sha256(data).hexdigest()

def _ifds(data):
    endian='<' if data[:2]==b'II' else '>' if data[:2]==b'MM' else None
    if endian is None or struct.unpack_from(endian+'H',data,2)[0]!=42:
        raise ValueError('expected pinned classic TIFF')
    offset = struct.unpack_from(endian+'I', data, 4)[0]
    result = []
    seen = set()
    while offset:
        if offset in seen:
            raise ValueError('source IFD cycle')
        seen.add(offset)
        count = struct.unpack_from(endian+'H', data, offset)[0]
        tags = {}
        for position in range(offset + 2, offset + 2 + 12 * count, 12):
            tag, kind, size, value = struct.unpack_from(endian+'HHII', data, position)
            if kind not in (3, 4):
                continue
            unit = 2 if kind == 3 else 4
            start = position + 8 if size * unit <= 4 else value
            raw = data[start:start + size * unit]
            if len(raw) != size * unit:
                raise ValueError('source metadata bounds')
            tags[tag] = list(struct.unpack(endian + ('H' if kind == 3 else 'I') * size, raw))
        result.append((offset, tags))
        offset = struct.unpack_from(endian+'I', data, offset + 2 + 12 * count)[0]
    return result

def _token_rows(encoded, width, requested):
    """Locate row boundaries/counts without reconstructing pixel values."""
    position = 0
    complete = 0
    integrity = 'Complete'
    for _ in range(requested):
        produced = 0
        while position < len(encoded) and produced < width:
            code = encoded[position]
            position += 1
            mode = code & 192
            if mode == 0:
                count = code
                if produced + count > width:
                    integrity = 'InvalidRow'
                    break
            elif mode == 64:
                count = sum(((code >> shift) & 3) != 2 for shift in (4, 2, 0))
            elif mode == 128:
                count = sum(((code >> shift) & 7) != 4 for shift in (3, 0))
            else:
                count = 1
            # The native codec's packed delta fields outside the declared row
            # width are storage padding, not additional logical source samples.
            produced = min(width, produced + count)
        if integrity != 'Complete' or produced != width:
            if integrity == 'Complete':
                integrity = 'InputExhausted'
            break
        complete += 1
    if complete == requested:
        if all(code in (0, 0x6a, 0xa4) for code in encoded[position:]):
            position = len(encoded)
        else:
            integrity = 'TrailingCodedData'
    return complete, position, integrity

def _bind(library):
    lib = ctypes.CDLL(str(library))
    lib.TIFFOpen.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
    lib.TIFFOpen.restype = ctypes.c_void_p
    lib.TIFFClose.argtypes = [ctypes.c_void_p]
    lib.TIFFSetSubDirectory.argtypes = [ctypes.c_void_p, ctypes.c_uint64]
    lib.TIFFSetSubDirectory.restype = ctypes.c_int
    lib.TIFFReadScanline.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint16]
    lib.TIFFReadScanline.restype = ctypes.c_int
    lib.TIFFScanlineSize64.argtypes = [ctypes.c_void_p]
    lib.TIFFScanlineSize64.restype = ctypes.c_uint64
    lib.TIFFRawStripSize64.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
    lib.TIFFRawStripSize64.restype = ctypes.c_uint64
    lib.TIFFGetVersion.restype = ctypes.c_char_p
    return lib

def _source(path, explicit_libtiff_path):
    path = pathlib.Path(path)
    binary = pathlib.Path(explicit_libtiff_path).resolve()
    data = path.read_bytes()
    if _sha(data) != SOURCE_SHA256:
        raise ValueError('ThunderScan oracle requires the exact pinned source hash')
    lib = _bind(binary)
    handle = lib.TIFFOpen(os.fsencode(path), b'r')
    if not handle:
        raise ValueError('independent libtiff could not open source')
    pages = []
    try:
        for index, (offset, tags) in enumerate(_ifds(data)):
            width, height, bits = tags[256][0], tags[257][0], tags[258][0]
            if tags.get(277, [1]) != [1] or bits not in (1, 4):
                raise ValueError('unexpected source sample layout')
            if lib.TIFFSetSubDirectory(handle, offset) != 1:
                raise ValueError('independent IFD selection failed')
            row_bytes = (width * bits + 7) // 8
            if lib.TIFFScanlineSize64(handle) != row_bytes:
                raise ValueError('independent native source scanline size differs')
            buffer = ctypes.create_string_buffer(row_bytes)
            values, spans, missing, strips = [], [], [], []
            rows = tags.get(278, [height])[0]
            total = 0
            for strip, (start, length) in enumerate(zip(tags[273], tags[279])):
                first = strip * rows
                requested = min(rows, height - first)
                if lib.TIFFRawStripSize64(handle, strip) != length:
                    raise ValueError('independent libtiff changed declared strip bytecount')
                native_rows = 0
                for row in range(first, first + requested):
                    if lib.TIFFReadScanline(handle, buffer, row, 0) != 1:
                        break  # Discard every byte of the failed native row.
                    packed = np.frombuffer(buffer.raw, dtype='u1')
                    if bits == 1:
                        logical = np.unpackbits(packed)[:width]
                    else:
                        logical = np.stack((packed >> 4, packed & 15), axis=1).ravel()[:width]
                    values.append(logical.copy())
                    native_rows += 1
                if bits == 4:
                    decoded, consumed, integrity = _token_rows(data[start:start+length], width, requested)
                    if decoded != native_rows:
                        raise ValueError('independent native rows differ from source token boundaries')
                else:
                    decoded, consumed, integrity = requested, length, 'Complete'
                    if native_rows != requested:
                        raise ValueError('complete second IFD unexpectedly failed')
                if native_rows:
                    if spans and spans[-1]['start_row'] + spans[-1]['rows'] == first:
                        spans[-1]['rows'] += native_rows
                    else:
                        spans.append({'start_row':first, 'rows':native_rows, 'sample_offset':total})
                    total += native_rows * width
                if native_rows < requested:
                    missing.append({'start_row':first+native_rows, 'rows':requested-native_rows, 'strip':strip})
                strips.append({'offset':start, 'declared_bytes':length, 'consumed_bytes':consumed, 'requested_rows':requested, 'complete_rows':native_rows, 'integrity':integrity})
            flattened = np.concatenate(values).astype('u1') if values else np.array([], dtype='u1')
            pages.append({'frame':index, 'declared_shape':[height,width], 'source_bits':bits, 'dtype':'uint8', 'shape':[len(flattened)] if missing else [height,width], 'sample_sha256':_sha(flattened.tobytes()), 'sample_count':len(flattened), 'available_rows':spans, 'missing_rows':missing, 'strips':strips, 'complete':not missing and all(s['integrity']=='Complete' for s in strips)})
    finally:
        lib.TIFFClose(handle)
    if _sha(path.read_bytes()) != SOURCE_SHA256:
        raise ValueError('source changed during independent read')
    if len(pages) != 2 or pages[0]['sample_count'] != 356*1512 or pages[0]['missing_rows'] != [{'start_row':356,'rows':3,'strip':5}] or not pages[1]['complete']:
        raise ValueError('pinned ThunderScan availability evidence changed')
    return {'source_sha256':SOURCE_SHA256, 'source_unchanged':True, 'library':str(binary), 'library_sha256':_sha(binary.read_bytes()), 'library_version':lib.TIFFGetVersion().decode(errors='replace').strip(), 'method':'native TIFFReadScanline; exact source tags/windows; failed native row discarded; independent token-only boundary checks; raw nibble/bit samples without photometric inversion', 'pages':pages}

def compare(path, frames, explicit_libtiff_path):
    """Compare full-probe raw frames, including explicit available-row metadata."""
    reference = _source(path, explicit_libtiff_path)
    differences = []
    def check(frame, field, expected, actual):
        if expected != actual:
            differences.append({'frame':frame,'field':field,'expected':expected,'actual':actual})
    check(None, 'frames', 2, len(frames))
    for index, (expected, actual) in enumerate(zip(reference['pages'], frames)):
        check(index, 'dtype', expected['dtype'], {'U8':'uint8'}.get(actual.get('dtype'),actual.get('dtype')))
        check(index, 'shape', expected['shape'], actual.get('shape'))
        check(index, 'sample_sha256', expected['sample_sha256'], actual.get('sample_sha256',actual.get('hash')))
        if index == 0:
            recovered = actual.get('source_recovery',{})
            check(index,'format','tiff-thunderscan',recovered.get('format'))
            for field in ['declared_shape','available_rows','complete']:
                check(index,field,expected[field],recovered.get(field))
            missing = [{key:row.get(key) for key in ['start_row','rows','strip']} for row in recovered.get('missing_rows',[])]
            check(index,'missing_rows',expected['missing_rows'],missing)
            actual_strips = [{key:strip.get(key) for key in expected['strips'][0]} for strip in recovered.get('strips',[])]
            check(index,'strips',expected['strips'],actual_strips)
    return {'differences':differences,'reference':reference}
