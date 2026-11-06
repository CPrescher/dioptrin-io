"""Source/composite/semantic-view controls independent of native parser output."""
import copy
import json
import unittest

from cif2_reference import DATA, compare_record, expected_record, semantic_projection


class Cif2Controls(unittest.TestCase):
    def setUp(self):
        self.fixtures = json.loads((DATA/"provenance.json").read_text())["fixtures"]

    def spec(self, name):
        return next(x for x in self.fixtures if x["file"] == name)

    def native(self, name):
        spec = self.spec(name)
        payload = (DATA/name).read_bytes()
        return expected_record(spec, payload, "Buffer"), spec, payload

    def test_all_handwritten_positive_trees_have_exact_ordered_source_anchors(self):
        for spec in self.fixtures:
            if spec["reader_action"] != "read":
                continue
            payload = (DATA/spec["file"]).read_bytes()
            record = expected_record(spec, payload, "Buffer")
            self.assertFalse(compare_record(record, spec, payload, "Buffer"))
            for block in record["blocks"]:
                token = block["name"]
                self.assertEqual(bytes.fromhex(token["raw_hex"]), payload[slice(*token["span"])])

    def test_nested_type_null_raw_number_source_spans_and_flags_cannot_change(self):
        native, spec, payload = self.native("nested-unicode.cif")
        changes = []
        changed = copy.deepcopy(native); changed["source_sha256"] = "0"*64; changes.append(changed)
        changed = copy.deepcopy(native); changed["version"] = "Cif11"; changes.append(changed)
        changed = copy.deepcopy(native); changed["blocks"][0]["items"][0]["value"]["token"]["text"] = "9007199254740992"; changes.append(changed)
        changed = copy.deepcopy(native); changed["blocks"][0]["name"]["content"][0] += 1; changes.append(changed)
        changed = copy.deepcopy(native); changed["blocks"][0]["items"][1]["value"]["composite"]["values"].reverse(); changes.append(changed)
        changed = copy.deepcopy(native); changed["blocks"][0]["items"][1]["value"]["composite"]["values"][2]["composite"]["entries"][0]["value"]["composite"]["values"][1]["null"] = None; changes.append(changed)
        for changed in changes:
            self.assertTrue(compare_record(changed, spec, payload, "Buffer"))

    def test_raw_and_logical_protocol_outputs_remain_separate(self):
        native, spec, payload = self.native("protocol-crlf.cif")
        fields = {i["name"]["text"]: i["value"] for i in native["blocks"][0]["items"]}
        self.assertIn("\r\n", fields["_both"]["token"]["text"])
        self.assertEqual(fields["_both"]["logical_text"], {"text":"first second","prefix":"P>","folded":True,"normalized_eol":True})
        self.assertEqual(fields["_single_prefix"]["logical_text"]["text"], "first\\\n second")
        for key, value in [("folded", False), ("prefix", None), ("text", "first second\n"), ("normalized_eol", False)]:
            changed = copy.deepcopy(native)
            changed["blocks"][0]["items"][2]["value"]["logical_text"][key] = value
            self.assertTrue(compare_record(changed, spec, payload, "Buffer"))

    def test_duplicate_map_projection_never_replaces_complete_source_tree(self):
        native, spec, payload = self.native("duplicate-table.cif")
        entries = native["blocks"][0]["items"][0]["value"]["composite"]["entries"]
        self.assertEqual(len(entries), 3)
        projected = semantic_projection(native)[0]["fields"]["_table"]
        self.assertEqual(projected, {"same":"2", "Other":"3"})
        changed = copy.deepcopy(native)
        changed["blocks"][0]["items"][0]["value"]["composite"]["entries"].pop(0)
        self.assertEqual(semantic_projection(changed)[0]["fields"]["_table"], projected)
        self.assertTrue(compare_record(changed, spec, payload, "Buffer"))

    def test_prior_unsupported_source_remains_same_bytes_and_is_now_positive(self):
        native, spec, payload = self.native("../cif-syntax/cif2.cif")
        self.assertEqual(spec["input_condition"], "valid_cif2")
        self.assertEqual(native["blocks"][0]["items"][0]["value"]["logical_text"]["text"], "αβ")
        self.assertFalse(compare_record(native, spec, payload, "Buffer"))

    def test_malformed_resource_limited_and_unknown_version_are_distinct(self):
        classes = {spec["input_condition"] for spec in self.fixtures}
        self.assertTrue({"valid_cif2", "malformed_cif2", "valid_cif2_resource_limit", "unknown_version"}.issubset(classes))
        for spec in self.fixtures:
            if spec["reader_action"] == "reject":
                self.assertTrue(spec["expected_error"])


if __name__ == "__main__":
    unittest.main()
