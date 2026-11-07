"""Independent source headers verify values, source types and field boundaries."""
from pathlib import Path

import h5py
import numpy as np
import pytest
import tifffile

import dioptrin_io as dio


def test_tiff_original_tags_and_exact_rationals(tmp_path):
    path = tmp_path / "tags.tif"
    data = np.arange(6, dtype="u2").reshape(2, 3)
    tifffile.imwrite(path, data, bigtiff=True, metadata=None, description="my experiment",
                     extratags=[(65000, "Q", 1, 2**64 - 1, False),
                                (65001, "q", 1, -(2**63), False),
                                (65002, "2I", 1, (123, 7), False),
                                (65003, "2i", 1, (-9, 0), False),
                                (65004, "H", 3, (1, 2, 65535), False)])
    with dio.open(path) as source:
        header = source.header()
        image = source[0]
        assert image.header == header
        assert header[270] == "my experiment"
        assert header[65000] == 2**64 - 1
        assert header[65001] == -(2**63)
        assert header[65002] == (123, 7)
        assert header[65003] == (-9, 0)  # Do not divide, sanitize or discard.
        assert header[65004] == [1, 2, 65535]
        np.testing.assert_array_equal(image.data, data)
    assert image.header[270] == "my experiment"


def test_cbf_cif_fields_loops_and_binary_declarations(tmp_path):
    path = tmp_path / "header.cbf"
    text = (b"###CBF: VERSION 1.0\ndata_test\n"
            b"_experiment.title 'test sample'\n"
            b"_experiment.unknown ?\n"
            b"loop_\n_test.label\n_test.value\nalpha 001\nbeta 2.500\n"
            b"_array_data.data\n;\n--CIF-BINARY-FORMAT-SECTION--\n"
            b"Content-Type: application/octet-stream; conversions=\"x-CBF_BYTE_OFFSET\"\n"
            b"Content-Transfer-Encoding: BINARY\nX-Binary-Size: 2\n"
            b"X-Binary-ID: 1\nX-Binary-Element-Type: \"signed 32-bit integer\"\n"
            b"X-Binary-Element-Byte-Order: LITTLE_ENDIAN\n"
            b"X-Binary-Number-of-Elements: 2\nX-Binary-Size-Fastest-Dimension: 2\n"
            b"X-Binary-Size-Second-Dimension: 1\n\n")
    path.write_bytes(text + b"\x0c\x1a\x04\xd5\x01\x02\n--CIF-BINARY-FORMAT-SECTION----\n;\n")
    with dio.open(path) as source:
        image = source[0]
        assert image.header == source.header()
        assert image.header["cif"]["_experiment.title"] == "test sample"
        assert image.header["cif"]["_experiment.unknown"] == "?"
        assert image.header["loops"] == [{"columns": ["_test.label", "_test.value"],
                                           "rows": [["alpha", "001"], ["beta", "2.500"]]}]
        # Binary fields use the native reader's case-normalized MIME keys.
        assert any(v == "2" for v in image.header["binary"].values())
        np.testing.assert_array_equal(image.data, [[1, 3]])


def test_hdf5_attributes_are_scoped_typed_and_independent(tmp_path):
    path = tmp_path / "scan.h5"
    expected = np.arange(12, dtype="u4").reshape(2, 2, 3)
    with h5py.File(path, "w") as handle:
        handle.attrs["title"] = "source title"
        group = handle.create_group("entry")
        group.attrs["title"] = "different scope"
        data = group.create_dataset("counts", data=expected)
        data.attrs["gain"] = np.float32(1.25)
        data.attrs["limit"] = np.uint64(2**64 - 1)
        data.attrs["offset"] = np.int64(-(2**63))
        data.attrs["matrix"] = np.array([[1, 2], [3, 4]], dtype=">i2")
        data.attrs["units"] = np.bytes_("counts")
        data.attrs["labels"] = np.array(["α", "β"], dtype=h5py.string_dtype("utf-8"))
        data.attrs["enabled"] = True
        data.attrs["codes"] = np.array([0, 1, 2, 255], dtype="u1").view("bool")
        data.attrs["empty"] = np.empty((0, 3), dtype="f8")
        data.attrs["null"] = h5py.Empty("f4")
        data.attrs["record"] = np.array((9,), dtype=[("value", "u4")])
    with dio.open(path, dataset="/entry/counts") as source:
        header = source.header()
        image = source[1]
        assert image.header == header
        assert header["/"]["title"] == "source title"
        assert header["/entry"]["title"] == "different scope"
        fields = image.header["/entry/counts"]
        assert fields["gain"] == 1.25
        assert fields["limit"] == 2**64 - 1
        assert fields["offset"] == -(2**63)
        assert fields["matrix"] == [[1, 2], [3, 4]]
        assert fields["units"] == b"counts"
        assert fields["labels"] == ["α", "β"]
        assert fields["enabled"] is True
        assert fields["codes"] == [False, True, 2, 255]
        assert fields["empty"] == []
        assert fields["null"] is None
        assert "record" not in fields
        assert "/entry/counts@record" in image.metadata["header_unsupported"]
        assert image.metadata["header_attribute_shapes"]["/entry/counts"]["matrix"] == [2, 2]
        assert "uint64" in image.metadata["header_attribute_types"]["/entry/counts"]["limit"]
        np.testing.assert_array_equal(image.data, expected[1])
        fields["matrix"][0][0] = 999
        assert source[0].header["/entry/counts"]["matrix"][0][0] == 1
    assert image.header["/entry/counts"]["matrix"][0][0] == 999


@pytest.mark.parametrize("shape", [(140_000,), (600_000, 0)])
def test_hdf5_header_allocation_bounds(tmp_path, shape):
    path = tmp_path / "bounded.h5"
    with h5py.File(path, "w") as handle:
        data = handle.create_dataset("counts", data=np.ones((2, 3), dtype="u1"), track_order=True)
        data.attrs["large"] = np.zeros(shape, dtype="u1")
    with dio.open(path, dataset="/counts") as source:
        assert source.nframes == 1
        with pytest.raises(OSError, match="budget"):
            source.header()
        with pytest.raises(OSError, match="budget"):
            source[0]


def test_hdf5_declared_utf8_is_validated(tmp_path):
    path = tmp_path / "invalid-text.h5"
    with h5py.File(path, "w") as handle:
        data = handle.create_dataset("counts", data=np.ones((2, 3), dtype="u1"))
        # HDF5 accepts malformed bytes even when the attribute declares UTF-8.
        data.attrs.create("bad", b"\xff", dtype=h5py.string_dtype("utf-8"))
    with dio.open(path, dataset="/counts") as source:
        with pytest.raises(OSError, match="utf-8"):
            source.header()
        with pytest.raises(OSError, match="utf-8"):
            source[0]


@pytest.mark.parametrize("relative", [
    "legacy/oxford-raw.img", "legacy/raxis-big.img", "legacy/bruker86-u8.sfrm",
    "containers/mrc-mode3-le.mrc", "containers/dm3-uint16-le.dm3",
])
def test_existing_native_header_fields_are_structured(relative):
    path = Path(__file__).resolve().parents[2] / "tests/data" / relative
    with dio.open(path) as source:
        header = source.header()
        image = source[0]
        assert image.header == header
        assert header
        assert all(isinstance(key, (str, int)) for key in header)
    assert image.header == header
