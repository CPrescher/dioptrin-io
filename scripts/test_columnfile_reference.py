"""Independent controls for unsafe-oracle admission and typed source fidelity."""
import copy
import pathlib
import tempfile
import unittest
from columnfile_reference import admit_c_ascii, build_c, c_oracle, differences, expected, rejection_differences, wrapper_bug
from generate_columnfile_fixtures import fixtures


class ColumnControls(unittest.TestCase):
    def test_unsafe_C_paths_never_pass_prevalidation(self):
        bad=[b"",b"x y\n1 2\n",b"#\n",b"# "+b"x"*128+b"\n1\n",
             b"# x y\n1\n",b"# x y\n1 2 3\n",b"# x y\n\n",b"# x\ninvalid\n",
             b"# x\n1\0\n",b"# x\n1\v2\n",b"# x\n"+b"1\n"*65,
             fixtures()["wide.dat"],fixtures()["opaque-comments.dat"]]
        for source in bad:
            with self.assertRaises((ValueError,UnicodeDecodeError),msg=repr(source[:128])):admit_c_ascii(source)
        for name in ("matrix.dat","integers.dat","header-only.dat","unterminated-row.dat"):
            self.assertGreaterEqual(admit_c_ascii(fixtures()[name])["columns"],1)

    def test_exactness_checks_mathematical_integer_not_saturating_casts(self):
        record=expected(fixtures()["integers.dat"])
        self.assertEqual(record["integer_exactness"],[False,False,True,True,True,True,False])
        self.assertEqual(bytes.fromhex(record["lexemes_hex"][1]),b"18446744073709551615")
        self.assertEqual(record["value_bits"][1],"43f0000000000000")

    def test_all_source_and_view_fields_are_required(self):
        wanted=expected(fixtures()["matrix.dat"])
        self.assertFalse(differences(wanted,wanted))
        for key in wanted:
            changed=copy.deepcopy(wanted);del changed[key]
            self.assertTrue(differences(changed,wanted),key)
        changed=copy.deepcopy(wanted);changed["value_bits"][1]="0000000000000000"
        self.assertTrue(differences(changed,wanted))
        changed=copy.deepcopy(wanted);changed["integer_exactness"][2]=True
        self.assertTrue(differences(changed,wanted))
        changed=copy.deepcopy(wanted);changed["integer_exactness"][0]=1
        self.assertTrue(differences(changed,wanted))

    def test_generated_fixtures_match_original_bytes(self):
        directory=pathlib.Path(__file__).resolve().parents[1]/"tests/data/columnfile"
        for name,payload in fixtures().items():self.assertEqual((directory/name).read_bytes(),payload)

    def test_unrelated_cli_or_filesystem_errors_cannot_qualify_strict_rejection(self):
        reason="columnfile row has missing values"
        self.assertFalse(rejection_differences({"status":"error","stderr":reason},reason))
        for value in [{"status":"ok"},{"status":"error","stderr":"unknown command"},
                      {"status":"error","stderr":"No such file"}]:
            self.assertTrue(rejection_differences(value,reason))

    def test_unmodified_pinned_C_decoder_and_binary_stub_are_separate_from_wrapper(self):
        source=pathlib.Path("/tmp/dioptrin-io-fabio-2026.6.0")
        if not source.exists():self.skipTest("pinned source explicitly required for original C oracle")
        with tempfile.TemporaryDirectory() as directory:
            executable,record=build_c(source,directory)
            path=pathlib.Path(directory)/"values.dat";path.write_bytes(fixtures()["matrix.dat"])
            native=c_oracle(executable,path)
            self.assertEqual(native["shape"],[3,2])
            self.assertFalse(differences(native,{key:expected(path.read_bytes())[key] for key in ("shape","labels_hex","value_bits")}))
            self.assertEqual(native["abi"]["double_bytes"],8)
            self.assertEqual(c_oracle(executable,path,"b")["status"],"no_decoder_result")
            original=wrapper_bug(path)
            self.assertEqual([read["errno"] for read in original["reads"][:2]],[9,9])
            self.assertEqual(original["reads"][2]["status"],"error")


if __name__=="__main__":unittest.main()
