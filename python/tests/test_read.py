"""Independent Python encoders verify counts, dtype, axes and lifetime contracts."""
import bz2
from concurrent.futures import ThreadPoolExecutor
import gzip
import os
import struct

import numpy as np
import pytest
import tifffile

import dioptrin_io as dio


@pytest.mark.parametrize("dtype", [
    "u1", "u2", "u4", "u8", "i1", "i2", "i4", "i8",
    "f2", "f4", "f8", "c8", "c16", "bool",
])
@pytest.mark.parametrize("byteorder", ["<", ">"])
def test_native_types(tmp_path, dtype, byteorder):
    dtype = np.dtype(dtype).newbyteorder(byteorder)
    if dtype.kind in "ui":
        limits = np.iinfo(dtype)
        values = [limits.min, limits.max, 0, 1, 2, 3]
    elif dtype.kind in "fc":
        values = [0, -0.0, 1.5, -2.25, float("inf"), float("nan")]
        if dtype.kind == "c":
            values = [complex(x, 2 - i) for i, x in enumerate(values)]
    else:
        values = [False, True, True, False, False, True]
    expected = np.array(values, dtype=dtype).reshape(2, 3)
    path = tmp_path / "not-an-npy-extension.detector"
    with path.open("wb") as out:
        np.save(out, expected)
    with dio.open(path) as source:
        assert source.nframes == 1
        image = source[0]
        assert isinstance(image, dio.Image)
        assert image.metadata == source.metadata(0)
        assert image.header["descr"] == dtype.str
        assert image.header["shape"] == (2, 3)
        assert image.header["fortran_order"] is False
        assert image.header == source.header(0)
        result = image.data
    assert isinstance(result, np.ndarray)
    assert result.dtype == dtype.newbyteorder("=")
    np.testing.assert_array_equal(result, expected)
    # Preserve signed zero, NaN payloads, complex components and integer extrema.
    assert result.tobytes() == expected.astype(dtype.newbyteorder("=")).tobytes()
    assert result.flags.writeable


@pytest.mark.parametrize("order", ["C", "F"])
def test_stack_axes_and_frame_lifetime(tmp_path, order):
    expected = np.array(np.arange(24).reshape(2, 3, 4), dtype="u4", order=order)
    path = tmp_path / "stack.npy"
    np.save(path, expected)
    with dio.open(path) as reader:
        assert len(reader) == 2
        assert reader.nframes == 2
        first = reader[0]
        second = reader[1]
        np.testing.assert_array_equal(reader.read(1).data, second.data)
        first.data[0, 0] = 999
        np.testing.assert_array_equal(reader.read().data, expected[0])
        np.testing.assert_array_equal(second.data, expected[1])
        metadata = first.metadata
        assert first.header["shape"] == (2, 3, 4)
        assert first.header["fortran_order"] is (order == "F")
        assert metadata["source_shape"] == [2, 3, 4]
        assert b"descr" in metadata["raw_header"]
        for frame in [-1, 2]:
            with pytest.raises(IndexError):
                reader.read(frame)
            with pytest.raises(IndexError):
                reader[frame]
            with pytest.raises(IndexError):
                reader.metadata(frame)
            with pytest.raises(IndexError):
                reader.header(frame)
    assert reader.closed
    assert first.data[0, 0] == 999
    assert first.metadata == metadata
    np.testing.assert_array_equal(second.data, expected[1])
    reader.close()  # idempotent
    for operation in [reader.read, lambda: reader[0], lambda: iter(reader),
                      reader.metadata, reader.header, lambda: len(reader),
                      lambda: reader.nframes, reader.__enter__]:
        with pytest.raises(ValueError, match="closed"):
            operation()


@pytest.mark.parametrize("format", ["npy", "tiff", "hdf5"])
def test_iteration_order_and_independent_positions(tmp_path, format):
    expected = np.arange(24, dtype="u4").reshape(3, 2, 4)
    path = tmp_path / f"scan.{format}"
    options = {}
    if format == "npy":
        np.save(path, expected)
    elif format == "tiff":
        tifffile.imwrite(path, expected, photometric="minisblack")
    else:
        h5py = pytest.importorskip("h5py", reason="reference encoder unavailable")
        with h5py.File(path, "w") as handle:
            handle.create_dataset("/counts", data=expected)
        options["dataset"] = "/counts"

    with dio.open(path, **options) as scan:
        first_iterator = iter(scan)
        assert iter(first_iterator) is first_iterator
        second_iterator = iter(scan)
        first = next(first_iterator)
        assert isinstance(first, dio.Image)
        np.testing.assert_array_equal(first.data, expected[0])
        first.data[0, 0] = 999
        np.testing.assert_array_equal(scan[0].data, expected[0])
        np.testing.assert_array_equal(scan.read(2).data, expected[2])
        np.testing.assert_array_equal(next(second_iterator).data, expected[0])
        np.testing.assert_array_equal(next(first_iterator).data, expected[1])
        np.testing.assert_array_equal(next(first_iterator).data, expected[2])
        for _ in range(2):
            with pytest.raises(StopIteration):
                next(first_iterator)
        for index, image in enumerate(scan):
            np.testing.assert_array_equal(image.data, expected[index])
            assert image.metadata == scan.metadata(index)
        assert index == scan.nframes - 1
        for key in [slice(None), (0, 0), "0", 1.5]:
            with pytest.raises(TypeError):
                scan[key]
    assert first.data[0, 0] == 999
    np.testing.assert_array_equal(image.data, expected[-1])
    # A closed source still cannot restart an already exhausted iterator.
    with pytest.raises(StopIteration):
        next(first_iterator)
    with pytest.raises(ValueError, match="closed"):
        next(second_iterator)


def test_iterator_source_change_and_owner_lifetime(tmp_path):
    path = tmp_path / "scan.npy"
    expected = np.arange(24, dtype="u2").reshape(3, 2, 4)
    np.save(path, expected)
    with dio.open(path) as scan:
        frames = iter(scan)
        original = path.stat()
        # Creating an iterator does not bypass source checks on the next frame.
        os.utime(path, ns=(original.st_atime_ns, original.st_mtime_ns + 2_000_000_000))
        with pytest.raises(OSError):
            next(frames)
    # The iterator retains its owner even without another reference to the reader.
    frames = iter(dio.open(path))
    for index, image in enumerate(frames):
        np.testing.assert_array_equal(image.data, expected[index])
    assert index == 2


def test_storage_bits(tmp_path):
    cases = [
        (np.array([0x8000, 0x7E01, 0x7C01, 0xFC00, 0x0001, 0], "u2").view("f2"), "u2"),
        (np.array([0, 1, 2, 255, 128, 0], "u1").view("bool"), "u1"),
    ]
    for expected, view in cases:
        expected = expected.reshape(2, 3)
        path = tmp_path / "bits.npy"
        np.save(path, expected)
        with dio.open(path) as source:
            result = source.read(0).data
        assert result.dtype == expected.dtype
        np.testing.assert_array_equal(result.view(view), expected.view(view))


@pytest.mark.parametrize("compress", [gzip.compress, bz2.compress])
def test_wrapped_input(tmp_path, compress):
    path = tmp_path / "image.npy"
    expected = np.array([[2**32 - 1, 3, 42]], "u4")
    np.save(path, expected)
    wrapped = tmp_path / "compressed.unknown"
    wrapped.write_bytes(compress(path.read_bytes()))
    with dio.open(wrapped) as source:
        np.testing.assert_array_equal(source.read(0).data, expected)


def test_paths_errors_and_mutation(tmp_path):
    path = tmp_path / "detector.npy"
    expected = np.arange(6, dtype="i2").reshape(2, 3)
    np.save(path, expected)
    for name in [str(path), os.fsencode(path)]:
        with dio.open(name) as source:
            np.testing.assert_array_equal(source.read(0).data, expected)
    with dio.open(path) as reader:
        saved = reader.read()
        original = path.stat()
        os.utime(path, ns=(original.st_atime_ns, original.st_mtime_ns + 2_000_000_000))
        with pytest.raises(OSError):
            reader.read()
        with pytest.raises(OSError):
            reader[0]
        with pytest.raises(OSError):
            reader.metadata()
        with pytest.raises(OSError):
            reader.header()
    np.testing.assert_array_equal(saved.data, expected)
    assert saved.metadata["source_shape"] == [2, 3]
    with pytest.raises(OSError):
        dio.open(tmp_path / "missing")
    bad = tmp_path / "bad.tif"
    bad.write_bytes(b"not an image")
    with pytest.raises(OSError):
        dio.open(bad)
    with pytest.raises(TypeError):
        dio.open(123)


def test_edf_independent_header_and_counts(tmp_path):
    expected = np.array([[0, 65535, 7], [19, 40000, 2]], dtype=">u2")
    header = (b"{\nDim_1 = 3;\nDim_2 = 2;\nDataType = UnsignedShort;\n"
              b"ByteOrder = HighByteFirst;\nSize = 12;\nComment = original;\n")
    header += b" " * (512 - len(header) - 2) + b"}\n"
    path = tmp_path / "frame.edf"
    path.write_bytes(header + expected.tobytes())
    with dio.open(path) as reader:
        image = reader.read(0)
        np.testing.assert_array_equal(image.data, expected)
        assert image.metadata["raw_header"] == header
        assert image.header["COMMENT"] == "original"
        assert image.header["DIM_1"] == "3"  # Native textual values stay text.
        assert image.header == reader.header()
        assert reader.nframes == 1
        assert reader.metadata()["raw_header"] == header
        assert reader.metadata()["little_endian"] is False


@pytest.mark.parametrize("access", ["index", "read", "iterate"])
def test_image_carries_matching_frame_metadata(tmp_path, access):
    path = tmp_path / "scan.edf"
    headers = []
    expected = []
    records = []
    for index in range(3):
        data = (np.arange(6, dtype="u2") + 100 * index).reshape(2, 3)
        header = (f"{{\nDim_1 = 3;\nDim_2 = 2;\nDataType = UnsignedShort;\n"
                  f"ByteOrder = LowByteFirst;\nSize = 12;\nComment = frame_{index};\n").encode()
        header += b" " * (512 - len(header) - 2) + b"}\n"
        headers.append(header)
        expected.append(data)
        records.append(header + data.astype("<u2").tobytes())
    path.write_bytes(b"".join(records))
    with dio.open(path) as scan:
        assert scan.nframes == 3
        if access == "index":
            images = [scan[i] for i in range(3)]
        elif access == "read":
            images = [scan.read(i) for i in range(3)]
        else:
            images = list(scan)
        for index, image in enumerate(images):
            assert isinstance(image, dio.Image)
            assert isinstance(image.data, np.ndarray)
            np.testing.assert_array_equal(image.data, expected[index])
            assert image.metadata["raw_header"] == headers[index]
            assert image.header["COMMENT"] == f"frame_{index}"
            assert image.header == scan.header(index)
            assert image.metadata == scan.metadata(index)
        # Snapshots are independent of the reader and of other reads.
        images[0].metadata["raw_header"] = b"local edit"
        images[0].header["COMMENT"] = "local edit"
        assert scan[0].header["COMMENT"] == "frame_0"
        assert images[1].header["COMMENT"] == "frame_1"
        assert scan[0].metadata["raw_header"] == headers[0]
        assert images[1].metadata["raw_header"] == headers[1]
        images[0].data[0, 0] = 999
        np.testing.assert_array_equal(scan[0].data, expected[0])
    assert images[0].metadata["raw_header"] == b"local edit"
    assert images[0].header["COMMENT"] == "local edit"
    assert images[0].data[0, 0] == 999
    assert images[2].metadata["raw_header"] == headers[2]
    np.testing.assert_array_equal(images[2].data, expected[2])


def test_tiff_orientation_counts_and_color(tmp_path):
    expected = np.array([[0, 2**32 - 1, 7], [23, 5, 42]], "u4")
    path = tmp_path / "image.tif"
    tifffile.imwrite(path, expected, photometric="miniswhite",
                     extratags=[(274, "H", 1, 4, False)])
    with dio.open(path) as reader:
        image = reader.read(0)
        np.testing.assert_array_equal(image.data, expected)
        assert "orientation: 4" in image.metadata["details"]
        assert image.header[274] == 4
        assert "orientation: 4" in reader.metadata()["details"]
    rgb = np.arange(18, dtype="u1").reshape(2, 3, 3)
    tifffile.imwrite(path, rgb, photometric="rgb")
    with dio.open(path) as source:
        np.testing.assert_array_equal(source.read(0).data, rgb)


def test_tiff_stack(tmp_path):
    path = tmp_path / "stack.tif"
    expected = np.arange(24, dtype="u2").reshape(2, 3, 4)
    tifffile.imwrite(path, expected, photometric="minisblack")
    with dio.open(path) as reader:
        assert len(reader) == 2
        for frame in range(2):
            np.testing.assert_array_equal(reader.read(frame).data, expected[frame])


@pytest.mark.parametrize("dtype", [">u4", "<u8", ">i8", "f2", "c8", "c16", "bool"])
def test_hdf5_explicit_dataset(tmp_path, dtype, h5py):
    path = tmp_path / "images.h5"
    expected = np.arange(24).reshape(2, 3, 4).astype(dtype)
    if expected.dtype.kind == "u":
        expected[0, 0, 0] = np.iinfo(dtype).max
    with h5py.File(path, "w") as handle:
        handle.create_dataset("/detector/counts", data=expected)
    with pytest.raises(OSError):  # No arbitrary dataset selected implicitly.
        dio.open(path)
    with dio.open(path, dataset="/detector/counts") as reader:
        assert len(reader) == 2
        image = reader.read(1)
        np.testing.assert_array_equal(image.data, expected[1])
        metadata = image.metadata
        assert metadata["format"] == "hdf5"
        assert metadata["source_shape"] == [2, 3, 4]
        assert metadata["type_descriptor"]
        np.testing.assert_array_equal(reader.read(0).data, expected[0])


def test_hdf5_convention_discovery(tmp_path, h5py):
    path = tmp_path / "eiger.h5"
    expected = np.arange(24, dtype="u4").reshape(2, 3, 4)
    with h5py.File(path, "w") as handle:
        handle.create_dataset("/entry/data/data", data=expected)
    with dio.open(path) as source:
        assert source.nframes == 2
        np.testing.assert_array_equal(source.read(1).data, expected[1])


def test_external_hdf5_links_rejected(tmp_path, h5py):
    target = tmp_path / "target.h5"
    master = tmp_path / "master.h5"
    with h5py.File(target, "w") as handle:
        handle.create_dataset("data", data=np.ones((2, 3), "u4"))
    with h5py.File(master, "w") as handle:
        handle["data"] = h5py.ExternalLink(target.name, "/data")
    with pytest.raises(OSError, match="external"):
        dio.open(master, dataset="/data")


def test_complex_integer_mrc(tmp_path):
    # Independent minimal MRC2014 mode 3: exact signed16 complex pairs.
    header = bytearray(1024)
    struct.pack_into("<4i", header, 0, 3, 2, 1, 3)
    struct.pack_into("<3i", header, 28, 3, 2, 1)
    struct.pack_into("<3f", header, 40, 3, 2, 1)
    struct.pack_into("<3f", header, 52, 90, 90, 90)
    struct.pack_into("<3i", header, 64, 1, 2, 3)
    header[208:212] = b"MAP "
    header[212:216] = b"DD\0\0"
    pairs = np.array([[-32768, 32767], [0, -1], [12, -4], [7, 2], [1, 6], [-9, 19]], "<i2")
    path = tmp_path / "complex.mrc"
    path.write_bytes(header + pairs.tobytes())
    with dio.open(path) as source:
        result = source.read(0).data
    assert result.shape == (2, 3)
    assert result.dtype.names == ("real", "imag")
    np.testing.assert_array_equal(result["real"].ravel(), pairs[:, 0])
    np.testing.assert_array_equal(result["imag"].ravel(), pairs[:, 1])


def test_concurrent_reads_and_source_limit(tmp_path):
    path = tmp_path / "frame.npy"
    expected = np.arange(24, dtype="u2").reshape(2, 3, 4)
    np.save(path, expected)
    with dio.open(path) as reader, ThreadPoolExecutor(max_workers=4) as workers:
        actual = list(workers.map(reader.read, [0, 1] * 12))
    for i, image in enumerate(actual):
        np.testing.assert_array_equal(image.data, expected[i % 2])
    # A huge declared array is refused before NumPy output allocation.
    header = "{'descr': '<u8', 'fortran_order': False, 'shape': (999999999, 999999999), }\n".encode()
    path.write_bytes(b"\x93NUMPY\x01\x00" + struct.pack("<H", len(header)) + header)
    with pytest.raises(OSError):
        dio.open(path)
