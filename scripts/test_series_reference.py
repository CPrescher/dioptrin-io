"""Negative controls for real series observations and native cursor comparisons."""
import copy
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

import numpy as np

from series_reference import (PINNED_COVERAGE, Observer, Probe, compare_action, compare_frame,
                              control_trace, coverage_differences, expected_move, signature)


class Frame:
    data = np.asarray([[0, 65535, 32769]], dtype=">u2")
    header = {"frame_id": "2", "filename": "/data/source.edf"}


class SeriesControls(unittest.TestCase):
    def test_exact_samples_dtype_shape_and_all_three_frame_indices_are_required(self):
        reference = signature(Frame())
        location = {"index": 7, "file_index": 2, "local_frame": 1, "path": "/data/source.edf"}
        native = {**reference, **location}
        self.assertFalse(compare_frame(native, reference, location)[0])
        for field, wrong in [("dtype", "int32"), ("shape", [3, 1]), ("sample_sha256", "0" * 64),
                             ("index", 1), ("file_index", 1), ("local_frame", 2), ("path", "/other.edf")]:
            changed = {**native, field: wrong}
            self.assertTrue(compare_frame(changed, reference, location)[0], field)

    def test_header_fields_are_compared_without_losing_case_presentation(self):
        reference = signature(Frame())
        location = {"index": 0, "file_index": 0, "local_frame": 0, "path": "/data/source.edf"}
        native = {**reference, **location, "fields": {"FRAME_ID": "2", "FILENAME": "/data/source.edf"}}
        self.assertFalse(compare_frame(native, reference, location)[0])
        changed = copy.deepcopy(native)
        changed["fields"]["FRAME_ID"] = "1"
        self.assertTrue(compare_frame(changed, reference, location)[0])
        changed = copy.deepcopy(native)
        del changed["fields"]["FILENAME"]
        self.assertTrue(compare_frame(changed, reference, location)[0])

    def test_failed_requests_must_clear_current_pixels_and_frame_record(self):
        _, expected = expected_move(1, 3, {"op": "read", "index": -1})
        valid = {**expected, "error": "negative index"}
        self.assertFalse(compare_action(valid, expected))
        for key, wrong in [("status", "ok"), ("current", 1), ("pixels_available", True),
                           ("position", {"kind": "Frame", "index": 1}), ("frame", {"index": 1}),
                           ("error", None)]:
            self.assertTrue(compare_action({**valid, key: wrong}, expected), key)

    def test_boundaries_empty_sources_and_extreme_offsets_never_wrap(self):
        cases = [(-1, 3, {"op": "next"}, 0), (0, 3, {"op": "previous"}, -1),
                 (3, 3, {"op": "previous"}, 2), (2, 3, {"op": "next"}, 3),
                 (2, 3, {"op": "read", "index": -1}, -1),
                 (2, 3, {"op": "relative", "offset": -(1 << 63)}, -1),
                 (-1, 3, {"op": "relative", "offset": (1 << 63) - 1}, 3),
                 (-1, 0, {"op": "first"}, 0), (0, 0, {"op": "last"}, -1)]
        for anchor, count, command, target in cases:
            self.assertEqual(expected_move(anchor, count, command)[0], target)
        self.assertGreater(len(control_trace(0)), 15)

    def test_canonical_bytes_keep_u64_extrema_negative_zero_and_nan_payload(self):
        class Values:
            header = {}
        image = Values()
        image.data = np.asarray([0, (1 << 53) + 1, (1 << 64) - 1], dtype=">u8")
        first = signature(image)
        image.data = image.data.astype("<u8")
        self.assertEqual(signature(image), first)
        image.data = np.asarray([0x8000000000000000, 0x7ff8000000000042], dtype="<u8").view("<f8")
        first = signature(image)
        image.data = np.asarray([0, 0x7ff8000000000042], dtype="<u8").view("<f8")
        self.assertNotEqual(signature(image)["sample_sha256"], first["sample_sha256"])

    def test_observer_refuses_to_consume_a_live_filename_generator_at_close(self):
        import fabio.test.test_filenames as filename_tests
        count = []

        def filenames():
            count.append("consumed")
            yield "/never-open.edf"

        def forbidden_probe(*_):
            self.fail("native probe must not run with a partial manifest")

        observer = Observer(forbidden_probe)
        with observer.installed(filename_tests.TestFilenameObjects):
            series = observer.module.FileSeries(filenames=filenames())
            self.assertEqual(count, [])
            series.close()
            self.assertEqual(count, [])
        self.assertEqual(observer.errors[0]["scope"], "unconsumed_filename_iterator")

    def test_iterator_observer_captures_only_the_frames_actually_yielded(self):
        import fabio
        import fabio.test.test_filenames as filename_tests
        with tempfile.TemporaryDirectory() as directory:
            paths = [str(pathlib.Path(directory) / f"frame{i}.edf") for i in range(2)]
            for path in paths:
                fabio.edfimage.EdfImage(data=np.asarray([[0, 65535]], dtype="uint16")).write(path)
            consumed = []

            def filenames():
                for path in paths:
                    consumed.append(path)
                    yield path

            observer = Observer(lambda *_: self.fail("no eager audit"))
            with observer.installed(filename_tests.TestFilenameObjects):
                series = observer.module.FileSeries(filenames=filenames())
                iterator = series.frames()
                self.assertEqual(consumed, [])
                next(iterator)
                self.assertEqual(consumed, paths[:1])
                self.assertEqual(observer.states[id(series)]["yields"], 1)
                next(iterator)
                self.assertEqual(consumed, paths)
                self.assertEqual(observer.states[id(series)]["yields"], 2)
                with self.assertRaises(StopIteration): next(iterator)
                with observer.independent(): series.close()

    def test_missing_or_extra_pinned_observer_coverage_cannot_pass(self):
        self.assertFalse(coverage_differences(PINNED_COVERAGE))
        for key in PINNED_COVERAGE:
            changed = dict(PINNED_COVERAGE)
            changed[key] += 1
            self.assertTrue(coverage_differences(changed), key)

    def test_unrelated_native_errors_cannot_qualify_nonnumeric_seed_policy(self):
        for native in [{"status": "error", "stderr": "wrong flag", "returncode": 1},
                       {"status": "probe_error", "error": "timeout"}]:
            observer = Observer(lambda *_: native)
            observer.filename_calls.append({"test": "control", "api": "jump_filename",
                                            "seed": "mask.msk", "number": None, "reference": "mask.msk"})
            self.assertEqual(observer.filename_results()[0]["status"], "difference")

    def test_probe_failures_are_retained_as_failed_evidence(self):
        probe = Probe("/unused/probe")
        with mock.patch("series_reference.subprocess.run", side_effect=subprocess.TimeoutExpired("probe", 1)):
            self.assertEqual(probe("filename", {})["status"], "probe_error")
        for stdout in ["not json", "[]"]:
            with mock.patch("series_reference.subprocess.run", return_value=subprocess.CompletedProcess([], 0, stdout, "")):
                self.assertEqual(probe("filename", {})["status"], "probe_error")

    def test_installation_restores_reference_callables_even_when_a_test_raises(self):
        import fabio
        import fabio.test.test_filenames as filename_tests
        observer = Observer(lambda *_: None)
        original_open = fabio.open
        original_get = observer.module.FileSeries._get_frame
        with self.assertRaisesRegex(RuntimeError, "stop"):
            with observer.installed(filename_tests.TestFilenameObjects):
                raise RuntimeError("stop")
        self.assertIs(fabio.open, original_open)
        self.assertIs(observer.module.FileSeries._get_frame, original_get)


if __name__ == "__main__":
    unittest.main()
