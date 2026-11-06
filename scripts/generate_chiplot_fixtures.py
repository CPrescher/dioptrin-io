"""Independent CC0 CHIPLOT examples; no reference serialization or data used."""
import hashlib
import json
import pathlib
ROOT=pathlib.Path(__file__).resolve().parents[1]
DEST=ROOT/'tests/data/chiplot'


def fixtures():
    return {
        'ascending.chi':b'Independent ascending\n2theta\nintensity\n4\n0 00012.3400\n1 2\n2 3\n3 4\n',
        'descending.chi':b'Descending source\nx\ny\n3\n3 11\n2 22\n1 33\n',
        'nonuniform.chi':b'Irregular source\nx\ny\n3\n0 11\n0.5 22\n2 33\n',
        'multi-curve.chi':b'Two stored curves\nx\nY curves\n3,2\n0,1,9007199254740993\n1 2 18446744073709551615 # exactU64\n2,3,18446744073709551616\n',
        'zero.chi':b'Zero point profile\nx\ny\n0\n# no stored samples\n',
        'one.chi':b'One point profile\nx\ny\n1\n-0 +0009007199254740993\n',
        'nonfinite.chi':b'Explicit nonfinite extension\nx\ny\n3\n0 nan\n1 -inf\n2 +Infinity\n',
        'precision.chi':b'Exact text and extremes\nx\ny\n4\n-0 9223372036854775807\n1 18446744073709551615\n2 1.23456789012345678901234567890123456789\n3 1e-999\n',
        'comments-crlf.chi':b' raw title \r\nX\r\nY\r\n2 # declared points\r\n# full comment\r\n0, 1 # first\r\n\r\n1 2 # second\r\n# tail\r\n',
    }


def main():
    DEST.mkdir(parents=True,exist_ok=True)
    files=[]
    for name,payload in fixtures().items():
        (DEST/name).write_bytes(payload)
        files.append({'path':name,'bytes':len(payload),'sha256':hashlib.sha256(payload).hexdigest()})
    (DEST/'provenance.json').write_text(json.dumps({'schema':1,'license':'CC0-1.0','origin':'independently authored from the FIT2D documented minimum CHIPLOT header/column grammar; nonfinite/comment cases are explicitly labelled reader extensions',
        'primary_format':{'url':'https://www.esrf.fr/computing/scientific/FIT2D/FIT2D_REF/node115.html','sha256':'e3b012c52ad4134f32cbd0fb72373931c5c1e1f4c53a73701260db9ac7ba59b7','bytes':37018,'local_audit_copy':'/tmp/dioptrin-chiplot-format-node115.html'},
        'generator':'scripts/generate_chiplot_fixtures.py','files':files},indent=2)+'\n')


if __name__=='__main__':main()
