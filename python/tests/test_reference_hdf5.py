"""Verify pre-generated licensed HDF5 sources without any h5py dependency."""
import hashlib
import json
from pathlib import Path

import numpy as np

import dioptrin_io as dio


def test_independent_hdf5_corpus_through_numpy():
    directory = Path(__file__).resolve().parents[2] / "tests/data/hdf5-native"
    manifest = json.loads((directory / "provenance.json").read_text())
    for key, case in manifest["datasets"].items():
        filename, dataset = key.split("::")
        with dio.open(directory / filename, dataset=dataset) as scan:
            assert scan.nframes == len(case["frame_sha256"]), key
            for image, expected in zip(scan, case["frame_sha256"]):
                assert isinstance(image.data, np.ndarray), key
                data = image.data.astype(image.data.dtype.newbyteorder("<"), copy=False)
                assert hashlib.sha256(data.tobytes()).hexdigest() == expected, key
                assert image.metadata["little_endian"] == case["little_endian"], key


def test_virtual_dataset_source_choices():
    import pytest

    folder = Path(__file__).resolve().parents[2] / "tests/data/vds"
    path = folder / "finite.h5"
    with pytest.raises(dio.Hdf5SourcesRequired) as err:
        dio.open(path, dataset="/data", hdf5_sources=dio.Hdf5Sources())
    missing = err.value.requirements
    assert len(missing) == 2
    assert {item["kind"] for item in missing} == {"virtual"}
    choices = tuple(dio.FileChoice(item["owner"], item["stored_filename"],
                                   folder / Path(item["stored_filename"]).name)
                    for item in missing)
    expected = json.loads((folder / "provenance.json").read_text())["expected_i16"]
    sources = dio.Hdf5Sources(file_choices=choices, declared_fill=True)
    with dio.open(path, dataset="/data", hdf5_sources=sources) as reader:
        assert reader.nframes == len(expected)
        for image, frame in zip(reader, expected):
            assert image.data.dtype == np.int16
            np.testing.assert_array_equal(image.data, frame)


def test_finite_printf_mapping():
    import pytest

    folder = Path(__file__).resolve().parents[2] / "tests/data/vds-printf"
    case = json.loads((folder / "provenance.json").read_text())["cases"]["files-u64"]
    path = folder / case["master"]
    with pytest.raises(dio.Hdf5SourcesRequired) as err:
        dio.open(path, dataset="/data", hdf5_sources=dio.Hdf5Sources())
    item, = err.value.requirements
    mapping, = case["printf_mappings"]
    assert item["kind"] == "printf"
    for key in ("dataset", "mapping_index", "stored_filename", "declared_dataset"):
        assert item[key] == mapping[key]
    plan = dio.PrintfMapping(**{**mapping, "source_file": item["owner"]})
    with dio.open(path, dataset="/data", hdf5_sources=dio.Hdf5Sources(printf_mappings=(plan,))) as reader:
        assert reader.nframes == len(case["frame_hex"])
        for image, expected in zip(reader, case["frame_hex"]):
            assert image.data.tobytes().hex() == expected


def test_external_raw_counts_and_mutation(tmp_path):
    import shutil
    import pytest

    folder = Path(__file__).resolve().parents[2] / "tests/data/hdf5-layouts"
    for name in ("external.h5", "external-a.bin", "external-b.bin"):
        shutil.copyfile(folder / name, tmp_path / name)
    case = json.loads((folder / "provenance.json").read_text())["datasets"]["external"]
    with dio.open(tmp_path / "external.h5", dataset="/data", hdf5_sources=dio.Hdf5Sources()) as reader:
        assert reader.nframes == len(case["frame_hex"])
        for image, expected in zip(reader, case["frame_hex"]):
            assert image.data.tobytes().hex() == expected
        with (tmp_path / "external-b.bin").open("ab") as changed:
            changed.write(b"x")
        with pytest.raises(OSError):
            reader.read(0)
