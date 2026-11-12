"""Keep CI rankings paired and reject incomplete correctness evidence."""
import copy
import unittest
from report_codec_ci import summarize, markdown


class CodecSummaryTests(unittest.TestCase):
    def setUp(self):
        # Deliberately variable host speed: paired medians differ from a ratio
        # of median rates. Pairing must survive shuffling the sample order.
        self.record = {
            "arch": "test", "os": "test", "cpu": "test", "allocated_cpus": 1,
            "commit": None, "worker_counts": [1], "repeats": 3,
            "sources": [{"case": "case", "dtype": "u16", "shape": [1, 8, 8],
                         "automatic_unshuffle": "Scalar"}],
            "validation": [{"case": "case", "workers": 1, "unshuffle": "Scalar",
                            "lz4": lz4, "frames": 1, "all_counts_match": True}
                           for lz4 in ["Rust", "Native"]],
            "samples": [],
        }
        for repeat, (rust, native) in enumerate([(1., .5), (10., 5.), (100., 100.)]):
            for lz4, seconds in [("Rust", rust), ("Native", native)]:
                self.record["samples"].append({
                    "case": "case", "workers": 1, "unshuffle": "Scalar", "lz4": lz4,
                    "repeat": repeat, "frames": 1, "seconds": seconds,
                    "fps": 1 / seconds, "decoded_mib_s": 1 / seconds,
                })

    def test_pairing_is_by_repeat_and_configuration(self):
        self.record["samples"].reverse()
        result = summarize(self.record)
        self.assertEqual(result["lz4_comparisons"][0]["native_over_rust_median"], 2.)
        self.assertIn("Scalar + Native", markdown(result))

    def test_missing_sample_or_count_validation_is_rejected(self):
        for field in ["samples", "validation"]:
            incomplete = copy.deepcopy(self.record)
            incomplete[field].pop()
            with self.assertRaises(AssertionError):
                summarize(incomplete)

    def test_unmatched_frame_counts_are_rejected(self):
        self.record["samples"][1]["frames"] = 2
        with self.assertRaises(AssertionError):
            summarize(self.record)

    def test_count_failure_is_rejected(self):
        self.record["validation"][0]["all_counts_match"] = False
        with self.assertRaises(AssertionError):
            summarize(self.record)


if __name__ == "__main__":
    unittest.main()
