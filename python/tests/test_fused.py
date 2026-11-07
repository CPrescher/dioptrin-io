"""Every published wheel must contain a working fixed-mask bridge."""
import hashlib
import json
from pathlib import Path

import numpy as np
import pytest
import dioptrin_io_fused as fused

ROOT = Path(__file__).parents[2]
REFERENCE = json.loads(Path(__file__).with_name("fused-reference.json").read_text())
SOURCE = ROOT / "tests/data/counts-bitshuffle-lz4.h5"


@pytest.mark.parametrize("workers", [1, 8])
@pytest.mark.parametrize("errors", [False, True])
def test_bundled_bridge_counts_float64_and_owned_reuse(workers, errors):
    assert hashlib.sha256(SOURCE.read_bytes()).hexdigest() == REFERENCE["source_sha256"]
    mask = np.zeros((3, 4), np.uint8)
    mask[0, 1] = 1
    reader = fused.FusedHdf5Integrator(SOURCE, "detector/data", REFERENCE["poni"], 8,
        mask=mask, polarization_factor=.965, num_workers=workers)
    held = []
    for index in [0, 1, 0]:
        pixels, actual = reader.load_integrate1d(index, errors=errors)
        expected = REFERENCE["frames"][index]
        assert pixels.dtype == np.uint32 and not pixels.flags.writeable
        np.testing.assert_array_equal(pixels, expected["counts"])
        for field in ["radial", "intensity"] + (["errors"] if errors else []):
            values = np.asarray(getattr(actual, field))
            assert values.dtype == np.float64
            np.testing.assert_allclose(values, expected[field], rtol=2e-12, atol=1e-10)
        if not errors:
            assert actual.errors is None
        with pytest.raises(ValueError):
            pixels.setflags(write=True)
        held.append(pixels)
    assert not np.shares_memory(held[0], held[1])
    np.testing.assert_array_equal(held[0], REFERENCE["frames"][0]["counts"])
