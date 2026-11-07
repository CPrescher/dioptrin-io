"""The source bridge preserves the full-frame float64 scientific contract."""
from pathlib import Path
import shutil

import h5py
import hdf5plugin  # register the independent reference decoder
import numpy as np
import pytest
import dioptrin
import dioptrin_io_fused as fused

SOURCE = Path(__file__).parents[2] / "tests/data/counts-bitshuffle-lz4.h5"
PONI = dict(pixel1=75e-6, pixel2=75e-6, distance=.18, poni1=.0001,
            poni2=.0002, rot1=.01, rot2=.02, rot3=0., wavelength=3.7e-11)


@pytest.mark.parametrize("workers", [1, 8])
@pytest.mark.parametrize("errors", [False, True])
def test_bslz4_frames_match_current_float64_and_reuse_buffers(workers, errors):
    with h5py.File(SOURCE) as f: frames = f["detector/data"][:]
    shape = frames.shape[1:]
    mask = np.zeros(shape, np.uint8); mask[0, 1] = 1
    reader = fused.FusedHdf5Integrator(SOURCE, "detector/data", PONI, 8,
        mask=mask, polarization_factor=.965, num_workers=workers)
    reference = dioptrin.Integrator.from_poni_dict(PONI,
        polarization_factor=.965, unit="2th_deg")
    reference.set_mask(mask)
    for index in [0, 1, 0]:
        image = np.ascontiguousarray(frames[index, ::-1], dtype=np.float64)
        expected = reference.integrate1d(image, 8, errors=errors)
        actual = reader.integrate1d(index, errors=errors, expected_image=image)
        np.testing.assert_allclose(actual.radial, expected.radial)
        np.testing.assert_allclose(actual.intensity, expected.intensity, rtol=2e-12, atol=1e-10)
        if errors: np.testing.assert_allclose(actual.errors, expected.errors, rtol=2e-12, atol=1e-10)
        else: assert actual.errors is None
    assert reader.num_workers == workers
    changed = image.copy(); changed += 1
    with pytest.raises(fused.SourceImageMismatch): reader.integrate1d(0, expected_image=changed)
    reader.integrate1d(0, expected_image=image)  # failed states do not leak into reuse


def test_u16_counts_and_unsupported_layout(tmp_path):
    path = tmp_path / "u16.h5"
    values = np.arange(54, dtype=np.uint16).reshape(2, 3, 9)
    values[0, 0, 0] = 65535
    with h5py.File(path, "w") as f:
        f.create_dataset("raw", data=values, chunks=(1, 3, 9))
        f.create_dataset("gzip", data=values, chunks=(1, 3, 9), compression="gzip")
    reader = fused.FusedHdf5Integrator(path, "raw", PONI, 13)
    image = np.ascontiguousarray(values[0, ::-1], dtype=np.float64)
    expected = dioptrin.Integrator.from_poni_dict(PONI, unit="2th_deg").integrate1d(image, 13)
    pixels, combined = reader.load_integrate1d(0)
    np.testing.assert_array_equal(pixels, values[0])
    assert pixels.dtype == np.uint16
    np.testing.assert_allclose(combined.intensity, expected.intensity, rtol=2e-12, atol=1e-10)
    np.testing.assert_allclose(reader.integrate1d(0, expected_image=image).intensity,
                               expected.intensity, rtol=2e-12, atol=1e-10)
    with pytest.raises(fused.UnsupportedFusedLayout): fused.FusedHdf5Integrator(path, "gzip", PONI, 13)
    with pytest.raises(OSError): reader.integrate1d(2)


def test_source_changes_are_fatal_after_admission(tmp_path):
    path = tmp_path / "changed.h5"; shutil.copyfile(SOURCE, path)
    reader = fused.FusedHdf5Integrator(path, "detector/data", PONI, 8)
    with path.open("ab") as stream: stream.write(b"change")
    with pytest.raises(OSError): reader.integrate1d(0)


@pytest.mark.parametrize("kwargs", [dict(num_workers=0), dict(num_workers=257),
    dict(supersampling=0), dict(n_points=0)])
def test_invalid_options_rejected(kwargs):
    args = dict(n_points=8); args.update(kwargs)
    with pytest.raises(ValueError): fused.FusedHdf5Integrator(SOURCE, "detector/data", PONI, **args)


def test_combined_display_counts_pattern_ownership_and_source_switch(tmp_path):
    with h5py.File(SOURCE) as f: frames = f["detector/data"][:]
    reader = fused.FusedHdf5Integrator(SOURCE, "detector/data", PONI, 8)
    kept = []
    for index in [0, 1, 0]:
        pixels, pattern = reader.load_integrate1d(index, errors=True)
        np.testing.assert_array_equal(pixels, frames[index])
        assert pixels.dtype == frames.dtype and not pixels.flags.writeable
        with pytest.raises(ValueError): pixels.setflags(write=True)
        image = np.ascontiguousarray(frames[index, ::-1], dtype=np.float64)
        expected = dioptrin.Integrator.from_poni_dict(PONI, unit="2th_deg").integrate1d(image, 8, errors=True)
        np.testing.assert_allclose(pattern.intensity, expected.intensity, rtol=2e-12, atol=1e-10)
        np.testing.assert_allclose(pattern.errors, expected.errors, rtol=2e-12, atol=1e-10)
        kept.append(pixels)
    assert not np.shares_memory(kept[0], kept[1])
    other = tmp_path / "other.h5"; shutil.copyfile(SOURCE, other)
    reader.set_source(other, "detector/data")
    np.testing.assert_array_equal(reader.load_integrate1d(1)[0], frames[1])
    np.testing.assert_array_equal(kept[0], frames[0])
