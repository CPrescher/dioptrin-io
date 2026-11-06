"""Qualify Pillow 13's raw planar16 defect without reproducing it in Rust.

Test-only: independent tifffile + native libtiff source words, an exact byte
unpacker model, and controlled Pillow backend comparisons. Unsupported layouts
or any failed check remain unqualified differences, never expected successes.
"""
import ctypes as ct
import hashlib
import inspect
import pathlib

import numpy as np
import tifffile
from PIL import Image, TiffImagePlugin, __version__ as pillow_version
import PIL._imaging


def sha_file(path):
    with pathlib.Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def signature(array):
    array = np.asarray(array)
    canonical = np.ascontiguousarray(array.astype(array.dtype.newbyteorder("<"), copy=False))
    return {"shape": list(array.shape), "dtype": array.dtype.name,
            "sample_sha256": hashlib.sha256(canonical.tobytes()).hexdigest()}


class Reference:
    def __init__(self, library):
        self.path = pathlib.Path(library).resolve(strict=True)
        self.lib = ct.CDLL(str(self.path))
        lib = self.lib
        lib.TIFFGetVersion.restype = ct.c_char_p
        lib.TIFFOpen.argtypes = [ct.c_char_p, ct.c_char_p]
        lib.TIFFOpen.restype = ct.c_void_p
        lib.TIFFClose.argtypes = [ct.c_void_p]
        lib.TIFFSetDirectory.argtypes = [ct.c_void_p, ct.c_uint32]
        lib.TIFFSetDirectory.restype = ct.c_int
        lib.TIFFReadScanline.argtypes = [ct.c_void_p, ct.c_void_p, ct.c_uint32, ct.c_uint16]
        lib.TIFFReadScanline.restype = ct.c_int
        self.files = [self.path, pathlib.Path(inspect.getfile(TiffImagePlugin)).resolve(),
                      pathlib.Path(inspect.getfile(Image)).resolve(),
                      pathlib.Path(PIL._imaging.__file__).resolve(),
                      pathlib.Path(inspect.getfile(tifffile.TiffFile)).resolve()]
        self.hashes = {str(path): sha_file(path) for path in self.files}
        self.metadata = {"Pillow": pillow_version, "tifffile": tifffile.__version__,
                         "libtiff_version": lib.TIFFGetVersion().decode(errors="replace"),
                         "files_sha256": self.hashes,
                         "qualification": "only default Pillow13 raw, uint16, planar2, RGB/RGBA-unassociated/CMYK, strips, fillorder1, orientation1; all independent checks mandatory"}

    def unchanged(self):
        return all(sha_file(path) == expected for path, expected in self.hashes.items())

    def verify(self, path, index, native, reference):
        """Return proof or None for an unsupported layout. No source writes."""
        if pillow_version != "13.0.0" or TiffImagePlugin.READ_LIBTIFF:
            return None
        path = pathlib.Path(path)
        before = sha_file(path)
        with tifffile.TiffFile(path) as file:
            page = file.pages[index]
            height, width, channels = page.imagelength, page.imagewidth, page.samplesperpixel
            if (page.is_tiled or page.compression != 1 or page.planarconfig != 2 or page.bitspersample != 16
                    or page.dtype.kind != "u" or page.dtype.itemsize != 2 or channels not in [3, 4]
                    or page.photometric not in [2, 5] or page.photometric == 5 and channels != 4
                    or page.fillorder != 1 or page.tags.get(274) is not None and page.tags[274].value != 1
                    or page.photometric == 5 and page.tags.get(332) is not None and page.tags[332].value != 1
                    or page.photometric == 2 and channels == 4 and tuple(page.extrasamples) != (2,)
                    or height * width * channels * 2 > 64 * 1024 * 1024):
                return None
            source = np.ascontiguousarray(page.asarray().transpose(1, 2, 0))
            source_info = {"byteorder": file.byteorder, "shape": list(source.shape),
                           "bits_per_sample": 16, "planar_configuration": 2, "compression": 1,
                           "photometric": int(page.photometric), "rows_per_strip": page.rowsperstrip,
                           "offsets": list(page.dataoffsets), "bytecounts": list(page.databytecounts)}
        # Native libtiff returns host-endian scalar words, never an RGBA display.
        handle = self.lib.TIFFOpen(str(path).encode(), b"r")
        if not handle:
            raise ValueError("independent libtiff could not open source")
        rows = np.empty_like(source, dtype="=u2")
        try:
            if not self.lib.TIFFSetDirectory(handle, index):
                raise ValueError("independent libtiff missing IFD")
            packed = ct.create_string_buffer(width * 2)
            for channel in range(channels):
                for y in range(height):
                    if self.lib.TIFFReadScanline(handle, packed, y, channel) != 1:
                        raise ValueError("independent libtiff failed a complete source row")
                    rows[y, :, channel] = np.frombuffer(packed.raw, dtype="=u2")
        finally:
            self.lib.TIFFClose(handle)

        encoded = path.read_bytes()
        with Image.open(path) as image:
            image.seek(index)
            tiles = list(image.tile)
            bands, mode = image.getbands(), image.mode
            use_libtiff = image.use_load_libtiff
            corrupt = np.asarray(image).copy()
            corrupt_f = np.asarray(image.convert("F")).copy()
        simulated = np.zeros((height, width, channels), dtype="u1")
        covered = np.zeros_like(simulated, dtype="bool")
        descriptors = []
        windows = dict(zip(source_info["offsets"], source_info["bytecounts"]))
        for tile in tiles:
            band, stride, direction = tile.args
            if tile.codec_name != "raw" or band not in bands or direction != 1 or stride < 0:
                return None
            left, top, right, bottom = tile.extents
            if not (0 <= left < right <= width and 0 <= top < bottom <= height):
                return None
            channel = bands.index(band)
            stride = stride or right - left
            window = windows.get(tile.offset)
            if window is None or stride < right - left:
                return None
            for y in range(top, bottom):
                offset = tile.offset + (y - top) * stride
                end = offset + right - left
                if end > tile.offset + window or end > len(encoded) or covered[y, left:right, channel].any():
                    return None
                simulated[y, left:right, channel] = np.frombuffer(encoded[offset:end], dtype="u1")
                covered[y, left:right, channel] = True
            descriptors.append({"codec": tile.codec_name, "extents": list(tile.extents),
                                "offset": tile.offset, "band": band, "effective_byte_stride": stride})
        if not covered.all():
            return None
        high = (source >> 8).astype("u1")
        with Image.frombytes(mode, (width, height), simulated.tobytes()) as image:
            simulated_f = np.asarray(image.convert("F")).copy()
        with Image.frombytes(mode, (width, height), high.tobytes()) as image:
            corrected_f = np.asarray(image.convert("F")).copy()
        previous = TiffImagePlugin.READ_LIBTIFF
        try:
            TiffImagePlugin.READ_LIBTIFF = True
            with Image.open(path) as image:
                image.seek(index)
                forced = np.asarray(image).copy()
                forced_f = np.asarray(image.convert("F")).copy()
        finally:
            TiffImagePlugin.READ_LIBTIFF = previous
        checks = {
            "source_tifffile_equals_native_libtiff": signature(source) == signature(rows),
            "source_native_equals_independent_words": all(native.get(key) == value for key, value in signature(source).items()),
            "default_pillow_uses_raw_decoder": not use_libtiff,
            "raw_8bit_descriptor_model_equals_pillow": np.array_equal(simulated, corrupt),
            "model_luminance_equals_pillow": signature(simulated_f) == signature(corrupt_f),
            "reference_luminance_equals_default_pillow": signature(reference) == signature(corrupt_f),
            "forced_libtiff_equals_correct_source_high_bytes": np.array_equal(forced, high),
            "forced_libtiff_luminance_equals_corrected": signature(forced_f) == signature(corrected_f),
            "default_pillow_color_is_wrong": not np.array_equal(corrupt, high),
            "default_pillow_luminance_is_wrong": signature(corrupt_f) != signature(corrected_f),
            "native_declines_corrupt_reference_view": "reference layout defect" in native.get("view_error", "") and "reference_view" not in native,
            "source_unchanged": sha_file(path) == before,
            "reference_libraries_unchanged": self.unchanged(),
        }
        return {"qualified": all(checks.values()), "classification": "reference_decoder_defect",
                "owner": "Pillow13 raw TIFF planar16 unpacker; FabIO inherits its corrupted color samples",
                "checks": {key: bool(value) for key, value in checks.items()},
                "source_sha256": before, "source": source_info, "raw_descriptors": descriptors,
                "independent_source": signature(source), "native_libtiff_source": signature(rows),
                "default_pillow_color": signature(corrupt), "simulated_8bit_bands": signature(simulated),
                "correct_high_byte_color": signature(high), "default_pillow_luminance": signature(corrupt_f),
                "corrected_luminance": signature(corrected_f),
                "wrong_color_samples": int(np.count_nonzero(corrupt != high)),
                "wrong_luminance_pixels": int(np.count_nonzero(corrupt_f != corrected_f)),
                "first_source_pixels": source[0, :6].tolist(), "first_pillow_pixels": corrupt[0, :6].tolist(),
                "first_correct_pixels": high[0, :6].tolist()}
