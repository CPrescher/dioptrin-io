"""Independent test-only libtiff sampling-unit/component signatures.

No production reader uses this library. An explicit library path makes the
reference implementation, version and binary hash part of each parity report.
"""
import ctypes as ct
import hashlib
import importlib
import pathlib

import imagecodecs
import numpy as np


def signature(data, shape):
    return {"dtype": "uint8", "shape": list(shape),
            "sample_sha256": hashlib.sha256(data).hexdigest()}


class Reference:
    def __init__(self, library):
        self.path = pathlib.Path(library).resolve(strict=True)
        self.library = ct.CDLL(str(self.path))
        lib = self.library
        lib.TIFFGetVersion.restype = ct.c_char_p
        lib.TIFFOpen.argtypes = [ct.c_char_p, ct.c_char_p]
        lib.TIFFOpen.restype = ct.c_void_p
        lib.TIFFClose.argtypes = [ct.c_void_p]
        lib.TIFFSetDirectory.argtypes = [ct.c_void_p, ct.c_uint32]
        lib.TIFFSetDirectory.restype = ct.c_int
        # libtiff's Get/SetField APIs are variadic; only fixed arguments have
        # declared argtypes. Every variadic value below is explicitly typed.
        for name in ["TIFFGetField", "TIFFGetFieldDefaulted", "TIFFSetField"]:
            fn = getattr(lib, name)
            fn.argtypes = [ct.c_void_p, ct.c_uint32]
            fn.restype = ct.c_int
        lib.TIFFIsTiled.argtypes = [ct.c_void_p]
        lib.TIFFIsTiled.restype = ct.c_int
        for name in ["TIFFNumberOfTiles", "TIFFNumberOfStrips"]:
            fn = getattr(lib, name)
            fn.argtypes = [ct.c_void_p]
            fn.restype = ct.c_uint32
        for name in ["TIFFTileSize64", "TIFFStripSize64"]:
            fn = getattr(lib, name)
            fn.argtypes = [ct.c_void_p]
            fn.restype = ct.c_uint64
        for name in ["TIFFReadEncodedTile", "TIFFReadEncodedStrip"]:
            fn = getattr(lib, name)
            fn.argtypes = [ct.c_void_p, ct.c_uint32, ct.c_void_p, ct.c_ssize_t]
            fn.restype = ct.c_ssize_t
        lib.TIFFGetStrileByteCount.argtypes = [ct.c_void_p, ct.c_uint32]
        lib.TIFFGetStrileByteCount.restype = ct.c_uint64
        for name in ["TIFFReadRawTile", "TIFFReadRawStrip"]:
            fn = getattr(lib, name)
            fn.argtypes = [ct.c_void_p, ct.c_uint32, ct.c_void_p, ct.c_ssize_t]
            fn.restype = ct.c_ssize_t
        imcd = pathlib.Path(importlib.import_module("imagecodecs._imcd").__file__).resolve()
        self.lzw_path = imcd
        with imcd.open("rb") as source:
            lzw_digest = hashlib.file_digest(source, "sha256").hexdigest()
        self.metadata = {"path": str(self.path), "sha256": self.digest(),
                         "version": lib.TIFFGetVersion().decode("utf8", errors="replace"),
                         "lzw_reference": {"package": "imagecodecs", "version": imagecodecs.__version__,
                             "codec": imagecodecs.lzw_version(), "path": str(imcd), "sha256": lzw_digest,
                             "policy": "decode complete raw LZW payload, retaining full final-strip padding rather than libtiff logical-row clipping"}}
        self.jpeg_path = pathlib.Path(importlib.import_module("imagecodecs._jpeg8").__file__).resolve()
        with self.jpeg_path.open("rb") as source:
            jpeg_digest = hashlib.file_digest(source, "sha256").hexdigest()
        self.metadata["modern_jpeg_reference"] = {
            "package": "imagecodecs", "version": imagecodecs.__version__,
            "codec": imagecodecs.jpeg8_version(), "path": str(self.jpeg_path), "sha256": jpeg_digest,
            "policy": "aligned original JPEG chunks, explicit YCbCr output with nearest replication; exact inverse decimation recovers source chroma grids. LibTIFF RAW can return partially zero-filled modern JPEG units and is not used for compression7."}

    def digest(self):
        with self.path.open("rb") as source:
            return hashlib.file_digest(source, "sha256").hexdigest()

    def unchanged(self):
        with self.lzw_path.open("rb") as source:
            lzw_digest = hashlib.file_digest(source, "sha256").hexdigest()
        with self.jpeg_path.open("rb") as source:
            jpeg_digest = hashlib.file_digest(source, "sha256").hexdigest()
        return (self.digest() == self.metadata["sha256"]
                and lzw_digest == self.metadata["lzw_reference"]["sha256"]
                and jpeg_digest == self.metadata["modern_jpeg_reference"]["sha256"])

    def read(self, path, index=0, max_decoded_bytes=512 * 1024 * 1024):
        lib = self.library
        handle = lib.TIFFOpen(str(path).encode(), b"r")
        if not handle:
            raise ValueError("independent libtiff could not open source")
        try:
            if not lib.TIFFSetDirectory(handle, ct.c_uint32(index)):
                raise ValueError("independent libtiff cannot select source IFD")

            def field(tag, kind=ct.c_uint32, defaulted=False):
                value = kind()
                fn = lib.TIFFGetFieldDefaulted if defaulted else lib.TIFFGetField
                if not fn(handle, ct.c_uint32(tag), ct.byref(value)):
                    raise ValueError(f"independent libtiff lacks field {tag}")
                return value.value

            width, height = field(256), field(257)
            bits = field(258, ct.c_uint16, True)
            photo = field(262, ct.c_uint16, True)
            samples = field(277, ct.c_uint16, True)
            planar = field(284, ct.c_uint16, True)
            sample_format = field(339, ct.c_uint16, True)
            if not width or not height or (bits, photo, samples, planar, sample_format) != (8, 6, 3, 1, 1):
                raise ValueError("independent component oracle admits unsigned8 chunky YCbCr only")
            horizontal, vertical = ct.c_uint16(), ct.c_uint16()
            if not lib.TIFFGetFieldDefaulted(handle, ct.c_uint32(530), ct.byref(horizontal), ct.byref(vertical)):
                raise ValueError("independent libtiff lacks subsampling")
            horizontal, vertical = horizontal.value, vertical.value
            if horizontal not in (1, 2, 4) or vertical not in (1, 2, 4):
                raise ValueError("unsupported independent reference subsampling")
            compression = field(259, ct.c_uint16, True)
            if compression == 7:
                # JPEGCOLORMODE_RAW: native decoded components, no RGB or
                # upsampling. OJPEG has no color-mode pseudo-tag and already
                # returns native sampling units from encoded-strip/tile reads.
                if not lib.TIFFSetField(handle, ct.c_uint32(65538), ct.c_int(0)):
                    raise ValueError("independent libtiff rejected native JPEG component mode")
            tiled = bool(lib.TIFFIsTiled(handle))
            if tiled:
                chunk_width, chunk_height = field(322), field(323)
                count = lib.TIFFNumberOfTiles(handle)
                capacity = lib.TIFFTileSize64(handle)
                across = (width + chunk_width - 1) // chunk_width
                expected_count = across * ((height + chunk_height - 1) // chunk_height)
                read = lib.TIFFReadEncodedTile
            else:
                chunk_width = width
                chunk_height = min(field(278, defaulted=True), height)
                if not chunk_height:
                    raise ValueError("zero independent reference rows per strip")
                count = lib.TIFFNumberOfStrips(handle)
                capacity = lib.TIFFStripSize64(handle)
                across, expected_count = 1, (height + chunk_height - 1) // chunk_height
                read = lib.TIFFReadEncodedStrip
            if count != expected_count or capacity <= 0 or capacity > max_decoded_bytes:
                raise ValueError("independent reference chunk count/resource bound")
            chunks, total = [], 0
            for chunk_index in range(count):
                row, column = divmod(chunk_index, across)
                origin = [row * chunk_height, column * chunk_width]
                image_shape = [min(chunk_height, height - origin[0]), min(chunk_width, width - origin[1])]
                stored_height = chunk_height if tiled else image_shape[0]
                unit_rows = (stored_height + vertical - 1) // vertical
                unit_columns = (chunk_width + horizontal - 1) // horizontal
                expected = unit_rows * unit_columns * (horizontal * vertical + 2)
                if compression in (5, 7):
                    # libtiff clips decoded last strips to logical ImageLength,
                    # concealing encoded full-strip padding. Independently
                    # decode the original complete compressed chunk instead.
                    nominal_rows = (chunk_height + vertical - 1) // vertical
                    nominal_bytes = nominal_rows * unit_columns * (horizontal * vertical + 2)
                    encoded_size = lib.TIFFGetStrileByteCount(handle, ct.c_uint32(chunk_index))
                    if not encoded_size or encoded_size > max_decoded_bytes or nominal_bytes > max_decoded_bytes:
                        raise ValueError("independent raw LZW reference byte bound")
                    encoded = ct.create_string_buffer(encoded_size)
                    read_raw = lib.TIFFReadRawTile if tiled else lib.TIFFReadRawStrip
                    if read_raw(handle, ct.c_uint32(chunk_index), encoded, ct.c_ssize_t(encoded_size)) != encoded_size:
                        raise ValueError("incomplete independent compressed source chunk")
                    if compression == 7:
                        if stored_height % vertical or chunk_width % horizontal:
                            raise ValueError("independent modern JPEG oracle requires aligned source grids, not inferred padded pixels")
                        table_size, table_pointer = ct.c_uint32(), ct.c_void_p()
                        tables = None
                        if lib.TIFFGetField(handle, ct.c_uint32(347), ct.byref(table_size), ct.byref(table_pointer)):
                            if not table_pointer.value or table_size.value > max_decoded_bytes:
                                raise ValueError("independent JPEG tables resource bound")
                            tables = ct.string_at(table_pointer, table_size.value)
                        rgb_bytes = stored_height * chunk_width * 3
                        if rgb_bytes > max_decoded_bytes:
                            raise ValueError("independent JPEG replicated-grid byte bound")
                        pixels = imagecodecs.jpeg8_decode(encoded.raw, tables=tables,
                            colorspace="YCbCr", outcolorspace="YCbCr", fancyupsampling=False,
                            out=bytearray(rgb_bytes))
                        if pixels.dtype != np.uint8 or list(pixels.shape) != [stored_height, chunk_width, 3]:
                            raise ValueError("independent JPEG source dimensions/precision disagree")
                        arrays = [pixels[:, :, 0], pixels[::vertical, ::horizontal, 1], pixels[::vertical, ::horizontal, 2]]
                        raw = None
                    else:
                        raw = bytes(imagecodecs.lzw_decode(encoded.raw, out=nominal_bytes))
                        if len(raw) not in {expected, nominal_bytes}:
                            raise ValueError("independent LZW output is neither minimal nor full declared strip units")
                        unit_rows = len(raw) // (unit_columns * (horizontal * vertical + 2))
                else:
                    if expected > capacity:
                        raise ValueError("independent reference component byte bound")
                    buffer = ct.create_string_buffer(capacity)
                    actual = read(handle, ct.c_uint32(chunk_index), buffer, ct.c_ssize_t(capacity))
                    if actual != expected:
                        raise ValueError(f"incomplete independent source chunk {chunk_index}: {actual} != {expected}")
                    raw = buffer.raw[:actual]
                total += expected if raw is None else len(raw)
                if total > max_decoded_bytes:
                    raise ValueError("independent reference total component byte bound")
                if raw is not None:
                    units = np.frombuffer(raw, dtype=np.uint8).reshape(unit_rows, unit_columns, horizontal * vertical + 2)
                    y = units[:, :, :horizontal * vertical].reshape(unit_rows, unit_columns, vertical, horizontal).transpose(0, 2, 1, 3)
                    arrays = [y.reshape(unit_rows * vertical, unit_columns * horizontal),
                              units[:, :, horizontal * vertical], units[:, :, horizontal * vertical + 1]]
                planes = []
                for name, array, sampling in zip(["Y", "Cb", "Cr"], arrays, [[1, 1], [vertical, horizontal], [vertical, horizontal]]):
                    valid = [(image_shape[axis] + sampling[axis] - 1) // sampling[axis] for axis in range(2)]
                    planes.append({"component": name, "sampling": sampling, "valid_shape": valid,
                                   **signature(np.ascontiguousarray(array).tobytes(), array.shape)})
                chunks.append({"origin": origin, "image_shape": image_shape,
                               "sampling_units": None if raw is None else signature(raw, [unit_rows, unit_columns, horizontal * vertical + 2]),
                               "planes": planes})
            return {"shape": [height, width], "subsampling": [horizontal, vertical],
                    "precision": bits, "compression": compression,
                    "storage": "TiffSamplingUnits" if compression == 5 else "JpegDecodedComponents",
                    "chunks": chunks}
        finally:
            lib.TIFFClose(handle)
