"""CC0 minimal CBF sources, independent of every FabIO writer/decoder."""
import base64
import hashlib
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1] / 'tests/data/cbf-encoded'


def source(payload=b'\x01\x02', checksum=True, wrong=False, conversion='x-CBF_BYTE_OFFSET'):
    digest = hashlib.md5(b'wrong' if wrong else payload).digest()
    header = ('###CBF: VERSION 1.5\ndata_independent\n_array_data.data\n;\n'
              '--CIF-BINARY-FORMAT-SECTION--\n'
              f'Content-Type: application/octet-stream; conversions="{conversion}"\n'
              'Content-Transfer-Encoding: BINARY\n'
              f'X-Binary-Size: {len(payload)}\nX-Binary-ID: 1\n'
              'X-Binary-Element-Type: "signed 32-bit integer"\n'
              'X-Binary-Element-Byte-Order: LITTLE_ENDIAN\n'
              'X-Binary-Number-of-Elements: 2\n'
              'X-Binary-Size-Fastest-Dimension: 2\n'
              'X-Binary-Size-Second-Dimension: 1\nX-Binary-Size-Padding: 0\n')
    if checksum:
        header += f'Content-MD5: {base64.b64encode(digest).decode()}\n'
    return header.encode() + b'\n\x0c\x1a\x04\xd5' + payload + b'\n--CIF-BINARY-FORMAT-SECTION----\n;\n'


def fixtures():
    valid = source()
    return {'valid.cbf': valid, 'no-checksum.cbf': source(checksum=False),
            'wrong-checksum.cbf': source(wrong=True),
            'incomplete-escape.cbf': source(payload=b'\x80'),
            'big-endian-label.cbf': source().replace(b'LITTLE_ENDIAN',b'BIG_ENDIAN'),
            'unsupported-conversion.cbf': source(conversion='x-CBF_PACKED'),
            'truncated.cbf': valid[:valid.index(b'\x0c\x1a\x04\xd5')+5]}


def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    records = []
    for name, data in fixtures().items():
        (ROOT/name).write_bytes(data)
        records.append({'path': name, 'sha256': hashlib.sha256(data).hexdigest(),
                        'condition': 'malformed_payload' if name in {'truncated.cbf', 'incomplete-escape.cbf'} else
                                     'integrity_mismatch' if name == 'wrong-checksum.cbf' else
                                     'opaque_conversion_unvalidated_payload' if name == 'unsupported-conversion.cbf' else
                                     'unsupported_decoded_byte_order' if name == 'big-endian-label.cbf' else 'valid'})
    (ROOT/'provenance.json').write_text(json.dumps({'license': 'CC0-1.0', 'generator': 'scripts/generate_cbf_encoded_fixtures.py',
        'contract': 'Two byte-offset deltas 1,2 encode counts 1,3; incomplete escape deliberately has no complete count array.',
        'files': records}, indent=2)+'\n')


if __name__ == '__main__': main()
