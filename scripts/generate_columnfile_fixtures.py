"""CC0 independent legacy column-table fixtures; no upstream writer used."""
import hashlib
import json
import pathlib
import struct

ROOT = pathlib.Path(__file__).resolve().parents[1] / "tests/data/columnfile"


def fixtures():
    return {
        "matrix.dat": b"# angle intensity\n0 -0\n9007199254740993 1.25e-2\ninf -inf\n",
        "opaque-comments.dat": b"# \xff counts counts\r\n  # exposure \xfe\r\n0\t-0\t9007199254740993\r\n",
        "header-only.dat": b"# x y\n",
        "integers.dat": b"# integer\n9223372036854775807\n18446744073709551615\n-9223372036854775808\n18446744073709551616\n+0001\n-0000\n1000000000000000000000000\n",
        "unterminated-row.dat": b"# x y\n1 -0",
        "wide.dat": b"# " + b" ".join(f"c{i}".encode() for i in range(33)) + b"\n" + b" ".join(str(i).encode() for i in range(33)) + b"\n",
        "missing-value.dat": b"# x y\n1\n",
        "invalid-number.dat": b"# x y\n1 mistake\n",
    }


def projection(data):
    lines = data.splitlines(keepends=True)
    labels = lines[0][1:].split()
    comments = [line for line in lines if line.lstrip().startswith(b"#")]
    lexemes = [word for line in lines[1:] if not line.lstrip().startswith(b"#") for word in line.split()]
    values = [float(word) for word in lexemes]
    payload = b"".join(struct.pack("<d", value) for value in values)
    return {"shape": [len(values) // len(labels), len(labels)], "labels_hex": [v.hex() for v in labels],
            "comments_hex": [v.hex() for v in comments], "lexemes_hex": [v.hex() for v in lexemes],
            "value_bits": [struct.pack(">d", value).hex() for value in values],
            "value_sha256": hashlib.sha256(payload).hexdigest(), "source_sha256": hashlib.sha256(data).hexdigest(),
            "source_bytes": len(data)}


def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    records = {}
    for name, data in fixtures().items():
        (ROOT / name).write_bytes(data)
        invalid = name in {"missing-value.dat", "invalid-number.dat"}
        records[name] = {"license": "CC0-1.0", "source_sha256": hashlib.sha256(data).hexdigest(),
                         "native_reject": invalid, "c_oracle_safe": name in {"matrix.dat", "integers.dat", "header-only.dat", "unterminated-row.dat"}}
        if invalid:
            records[name]["error_contains"] = "columnfile row has missing values" if name == "missing-value.dat" else "columnfile invalid float64 token"
        if not invalid:
            records[name]["expected"] = projection(data)
        if name == "unterminated-row.dat":
            records[name]["reference_difference"] = "Pinned C discards a complete final row without a newline because feof is checked after fgets"
    (ROOT / "provenance.json").write_text(json.dumps({"license": "CC0-1.0", "generator": "scripts/generate_columnfile_fixtures.py",
                                                    "contract": "Independent ASCII tokens; decoded values are a float64 interpretation, not stored binary samples",
                                                    "files": records}, indent=2) + "\n")


if __name__ == "__main__": main()
