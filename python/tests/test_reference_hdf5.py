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
