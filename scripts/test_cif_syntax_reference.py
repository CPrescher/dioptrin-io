"""Controls for independent trees, token/source fidelity and rejection checks."""
import copy
import json
import os
import pathlib
import subprocess
import tempfile
import unittest

from cif_syntax_reference import DATA, compare_record, expected_record, flat_projection, reference_reads, rejection_matches


class SyntaxControls(unittest.TestCase):
    def setUp(self):
        self.fixtures = json.loads((DATA / "provenance.json").read_text())["fixtures"]
        self.spec = next(r for r in self.fixtures if r["file"] == "scopes.cif")
        self.payload = (DATA / self.spec["file"]).read_bytes()

    def test_all_positive_expected_trees_validate_exact_source_anchors(self):
        for spec in self.fixtures:
            if spec["input_condition"] != "valid_cif11":
                continue
            payload = (DATA / spec["file"]).read_bytes()
            expected = expected_record(spec, payload, "Buffer")
            self.assertFalse(compare_record(expected, spec, payload, "Buffer"))
            self.assertEqual(expected["source_bytes"], len(payload))
            for block in expected["blocks"]:
                token = block["name"]
                self.assertEqual(bytes.fromhex(token["raw_hex"]), payload[slice(*token["span"])])

    def test_scope_style_null_numeric_source_and_token_span_changes_fail(self):
        expected = expected_record(self.spec, self.payload, "Buffer")
        changes = []
        for key, value in [("source_sha256", "0"*64), ("provenance", "PathSnapshot"), ("version", "Cif20")]:
            changed = copy.deepcopy(expected); changed[key] = value; changes.append(changed)
        changed = copy.deepcopy(expected); changed["blocks"].reverse(); changes.append(changed)
        changed = copy.deepcopy(expected); changed["blocks"][0]["name"]["span"][0] += 1; changes.append(changed)
        changed = copy.deepcopy(expected); changed["blocks"][0]["items"][0]["value"]["token"]["style"] = "Bare"; changes.append(changed)
        changed = copy.deepcopy(expected); changed["blocks"][0]["items"][1]["value"]["null"] = None; changes.append(changed)
        changed = copy.deepcopy(expected); changed["blocks"][0]["items"][3]["values"][1]["token"]["text"] = "9007199254740992"; changes.append(changed)
        changed = copy.deepcopy(expected); changed["blocks"][0]["items"][4]["items"] = []; changes.append(changed)
        for changed in changes:
            self.assertTrue(compare_record(changed, self.spec, self.payload, "Buffer"))

    def test_raw_crlf_is_preserved_in_expected_content(self):
        spec = next(r for r in self.fixtures if r["file"] == "lexical-crlf.cif")
        payload = (DATA / spec["file"]).read_bytes()
        expected = expected_record(spec, payload, "Buffer")
        field = next(i for i in expected["blocks"][0]["items"] if i["name"]["text"] == "_text")
        self.assertEqual(field["value"]["token"]["text"], "first\r\n second")
        changed = copy.deepcopy(expected)
        changed["blocks"][0]["items"][5]["value"]["token"]["text"] = "first\n second"
        self.assertTrue(compare_record(changed, spec, payload, "Buffer"))

    def test_unchanged_reference_simple_loop_is_observed_and_scopes_are_never_invented(self):
        spec = next(r for r in self.fixtures if r["file"] == "simple-loop.cif")
        payload = (DATA / spec["file"]).read_bytes()
        expected = flat_projection(expected_record(spec, payload, "Buffer"))
        records = reference_reads(DATA / spec["file"])
        self.assertEqual(len(records), 4)
        for record in records:
            self.assertEqual(record["status"], "read_completed")
            self.assertEqual(record["projection"], expected)
            self.assertNotIn("blocks", record["projection"])

    def test_malformed_and_unsupported_fixtures_require_specific_error_evidence(self):
        for spec in self.fixtures:
            if spec["input_condition"] != "valid_cif11":
                self.assertTrue(spec.get("expected_error"))
                self.assertTrue(rejection_matches(spec,spec["expected_error"]))
                self.assertFalse(rejection_matches(spec,"unknown CLI command"))
                self.assertFalse(rejection_matches(spec,"source exceeds resource limit"))
        cif2 = next(r for r in self.fixtures if r["file"] == "cif2.cif")
        self.assertEqual(cif2["input_condition"], "unsupported_cif2")
        self.assertIn("unsupported", cif2["expected_error"])

    @unittest.skipUnless(hasattr(os,"mkfifo"),"POSIX FIFO control")
    def test_path_readers_reject_nonregular_inputs_without_waiting_for_a_writer(self):
        root=pathlib.Path(__file__).resolve().parents[1]
        probe=pathlib.Path(os.environ.get("DIOPTRIN_API_PROBE",root/'target/release/examples/api_probe')).resolve()
        if not probe.is_file():self.skipTest("release API probe is required for bounded FIFO controls")
        with tempfile.TemporaryDirectory(prefix='dioptrin-source-admission-') as directory:
            fifo=pathlib.Path(directory)/'source.fifo';os.mkfifo(fifo)
            for command in ('cif-syntax','chiplot'):
                for path in (fifo,pathlib.Path(directory)):
                    result=subprocess.run([str(probe),command,str(path)],stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=3)
                    self.assertNotEqual(result.returncode,0,command)
                    self.assertIn('path snapshots require regular files',result.stderr,command)


if __name__ == "__main__":
    unittest.main()
