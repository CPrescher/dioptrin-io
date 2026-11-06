"""Independent, import-safe EDF prefix oracle. No FabIO padding or runtime Python.

read() hashes only physically present complete scalar words; compare() checks
the native structured recovery record, including missing/partial bytes and
compression integrity. Python zlib/bz2 and NumPy are reference tools only.
"""

from __future__ import annotations

import bz2
import hashlib
import json
from pathlib import Path
import re
import string
import struct
import zlib

import numpy as np

_UPPER = str.maketrans(string.ascii_lowercase, string.ascii_uppercase)
_SPACE = b" \t\r\n\v\f"
_TYPES = {
    "signedbyte": "i1",
    "signed8": "i1",
    "unsignedbyte": "u1",
    "unsigned8": "u1",
    "signedshort": "i2",
    "signed16": "i2",
    "unsignedshort": "u2",
    "unsigned16": "u2",
    "unsignedshortinteger": "u2",
    "signedinteger": "i4",
    "signed32": "i4",
    "signedlong": "i4",
    "unsignedinteger": "u4",
    "unsigned32": "u4",
    "unsignedlong": "u4",
    "signed64": "i8",
    "unsigned64": "u8",
    "floatvalue": "f4",
    "float": "f4",
    "floatieee32": "f4",
    "float32": "f4",
    "double": "f8",
    "doublevalue": "f8",
    "floatieee64": "f8",
    "doubleieee64": "f8",
}
_CODECS = {
    "NONE": "NONE",
    "NO": "NONE",
    "NO COMPRESSION": "NONE",
    "FALSE": "NONE",
    "GZIP": "GZIP",
    "GZIPCOMPRESSION": "GZIP",
    "Z": "ZLIB",
    "ZLIB": "ZLIB",
    "ZCOMPRESSION": "ZLIB",
    "BZ2": "BZ2",
    "BZ": "BZ2",
    "BZIP2": "BZ2",
    "BZCOMPRESSION": "BZ2",
    "BZ2COMPRESSION": "BZ2",
}


def _sha(data):
    return hashlib.sha256(data).hexdigest()


def _integer(fields, name, default=None):
    value = fields.get(name)
    if value is None:
        return default
    if not re.fullmatch(r"\+?[0-9]+", value):
        raise ValueError(f"invalid integer {name}")
    result = int(value)
    if result > np.iinfo(np.uintp).max:
        raise ValueError("integer exceeds native index type")
    return result


def _fault(state, diagnostic):
    return state, diagnostic


def _gzip_member(data, start, output_limit, header_limit):
    """Parse real gzip framing, independently decode raw DEFLATE, verify CRC/ISIZE."""
    end = start

    def take(n):
        nonlocal end
        if end - start + n > header_limit:
            raise ValueError("reference gzip header limit")
        result = data[end : end + n]
        end += len(result)
        return result if len(result) == n else None

    fixed = take(10)
    if fixed is None:
        return b"", end, _fault("UnverifiedTruncated", "incomplete gzip header")
    if fixed[:3] != b"\x1f\x8b\x08" or fixed[3] & 0xE0:
        return b"", end, _fault("DecodeFailure", "invalid gzip header")
    flags = fixed[3]
    if flags & 4:
        length = take(2)
        if length is None or take(int.from_bytes(length, "little")) is None:
            return b"", end, _fault("UnverifiedTruncated", "incomplete gzip extra")
    for flag in (8, 16):
        if flags & flag:
            while True:
                byte = take(1)
                if byte is None:
                    return (
                        b"",
                        end,
                        _fault("UnverifiedTruncated", "incomplete gzip name/comment"),
                    )
                if byte == b"\0":
                    break
    if flags & 2:
        crc_header_end = end
        crc = take(2)
        if crc is None:
            return b"", end, _fault("UnverifiedTruncated", "incomplete gzip header CRC")
        if (
            int.from_bytes(crc, "little")
            != zlib.crc32(data[start:crc_header_end]) & 0xFFFF
        ):
            return b"", end, _fault("ChecksumFailure", "gzip header CRC mismatch")
    decoder = zlib.decompressobj(-15)
    output = bytearray()
    while end < len(data) and not decoder.eof:
        byte = data[end : end + 1]
        end += 1
        try:
            output.extend(decoder.decompress(byte, output_limit - len(output) + 1))
        except zlib.error as error:
            return bytes(output), end, _fault("DecodeFailure", str(error))
        if len(output) > output_limit:
            raise ValueError("reference decoded byte/shape limit")
    if not decoder.eof:
        return (
            bytes(output),
            end,
            _fault("UnverifiedTruncated", "incomplete DEFLATE body"),
        )
    footer = data[end : end + 8]
    end += len(footer)
    if len(footer) != 8:
        return (
            bytes(output),
            end,
            _fault("UnverifiedTruncated", "incomplete gzip footer"),
        )
    crc, size = struct.unpack("<II", footer)
    if crc != zlib.crc32(output) or size != len(output) & 0xFFFFFFFF:
        return bytes(output), end, _fault("ChecksumFailure", "gzip CRC/ISIZE mismatch")
    return bytes(output), end, None


def _bzip_member(data, start, output_limit, header_limit):
    if header_limit < 4:
        raise ValueError("reference bzip2 header limit")
    if len(data) - start < 4:
        return b"", len(data), _fault("UnverifiedTruncated", "incomplete bzip2 header")
    if data[start : start + 3] != b"BZh" or data[start + 3] not in b"123456789":
        return b"", start + 4, _fault("DecodeFailure", "invalid bzip2 header")
    decoder = bz2.BZ2Decompressor()
    output = bytearray()
    end = start
    while end < len(data) and not decoder.eof:
        byte = data[end : end + 1]
        end += 1
        try:
            output.extend(
                decoder.decompress(byte, max_length=output_limit - len(output) + 1)
            )
            while not decoder.needs_input and not decoder.eof:
                output.extend(
                    decoder.decompress(b"", max_length=output_limit - len(output) + 1)
                )
                if len(output) > output_limit:
                    raise ValueError("reference decoded byte/shape limit")
        except OSError as error:
            return bytes(output), end, _fault("DecodeFailure", str(error))
        if len(output) > output_limit:
            raise ValueError("reference decoded byte/shape limit")
    if not decoder.eof:
        return (
            bytes(output),
            end,
            _fault("UnverifiedTruncated", "incomplete bzip2 stream"),
        )
    return bytes(output), end, None


def _decode(
    data,
    codec,
    output_limit,
    header_limit=1024 * 1024,
    members_limit=100_000,
    padding=False,
):
    output = bytearray()
    members = []
    encoded = verified = 0
    integrity = "ContainerComplete"
    diagnostic = None
    while encoded < len(data):
        tail = data[encoded:]
        if (
            padding
            and len(data) % 512 == 0
            and len(tail) < 512
            and all(v == 0 or v in _SPACE for v in tail)
        ):
            break
        if len(members) >= members_limit:
            raise ValueError("reference member limit")
        first = encoded
        decoded_start = len(output)
        if codec == "Gzip":
            decoded, encoded, fault = _gzip_member(
                data, first, output_limit - len(output), header_limit
            )
        elif codec == "Bzip2":
            decoded, encoded, fault = _bzip_member(
                data, first, output_limit - len(output), header_limit
            )
        else:
            raise ValueError("unsupported reference member codec")
        output.extend(decoded)
        state, message = ("VerifiedMember", None) if fault is None else fault
        members.append(
            dict(
                encoded_range=[first, encoded],
                decoded_range=[decoded_start, len(output)],
                integrity=state,
                diagnostic=message,
            )
        )
        if fault is not None:
            integrity, diagnostic = fault
            break
        verified = len(output)
    if not members:
        integrity, diagnostic = "UnverifiedTruncated", "no compressed member bytes"
    return bytes(output), dict(
        integrity=integrity,
        verified_decoded_bytes=verified,
        members=members,
        diagnostic=diagnostic,
    )


def _zlib(data, output_limit):
    decoder = zlib.decompressobj()
    output = bytearray()
    integrity, diagnostic = "ContainerComplete", None
    encoded = 0
    while encoded < len(data) and not decoder.eof:
        byte = data[encoded : encoded + 1]
        encoded += 1
        try:
            output.extend(decoder.decompress(byte, output_limit - len(output) + 1))
        except zlib.error as error:
            integrity, diagnostic = "DecodeFailure", str(error)
            break
        if len(output) > output_limit:
            raise ValueError("reference decoded byte/shape limit")
    if integrity == "ContainerComplete":
        if not decoder.eof:
            integrity, diagnostic = "UnverifiedTruncated", "incomplete zlib stream"
        elif encoded < len(data) and not (
            len(data) % 512 == 0
            and len(data) - encoded < 512
            and all(v == 0 or v in _SPACE for v in data[encoded:])
        ):
            integrity, diagnostic = "DecodeFailure", "unexplained zlib tail"
    return bytes(output), dict(
        integrity=integrity,
        verified_decoded_bytes=len(output) if integrity == "ContainerComplete" else 0,
        members=[],
        diagnostic=diagnostic,
    )


def _header(data, offset, header_limit):
    end = offset
    opened = False
    while end < len(data):
        if end - offset >= header_limit:
            raise ValueError("reference EDF header limit")
        byte = data[end]
        end += 1
        if not opened:
            if byte == ord("{"):
                opened = True
            elif byte not in _SPACE:
                return (
                    None,
                    end,
                    dict(
                        kind="UnparsedRecord",
                        offset=offset,
                        header_hex=data[offset:end].hex(),
                        reason="invalid EDF header opening",
                    ),
                )
        if data[max(offset, end - 3) : end].endswith((b"}\n", b"}\r\n")):
            raw = data[offset:end]
            start = raw.index(b"{") + 1
            body = raw[start : -3 if raw.endswith(b"}\r\n") else -2].decode("latin1")
            fields = {}
            for part in body.split(";"):
                if "=" in part:
                    key, value = part.split("=", 1)
                    key = key.strip("\0 \t\r\n\v\f").translate(_UPPER)
                    if not key:
                        return (
                            None,
                            end,
                            dict(
                                kind="UnparsedRecord",
                                offset=offset,
                                header_hex=raw.hex(),
                                reason="empty EDF header key",
                            ),
                        )
                    fields[key] = value.strip("\0 \t\r\n\v\f")
            return (raw, fields), end, None
    raw = data[offset:end]
    if not opened and all(v in _SPACE for v in raw):
        return None, end, None
    return (
        None,
        end,
        dict(
            kind="TruncatedHeader", offset=offset, bytes_hex=raw.hex(), sha256=_sha(raw)
        ),
    )


def read(
    path,
    inline_size=False,
    raw_label=None,
    *,
    source_limit=512 * 1024 * 1024,
    frame_limit=256 * 1024 * 1024,
    header_limit=1024 * 1024,
    members_limit=100_000,
):
    """Return JSON signatures of exact present words and explicit integrity state."""
    path = Path(path).resolve(strict=True)
    if path.stat().st_size > source_limit:
        raise ValueError("reference encoded source limit")
    source = path.read_bytes()
    source_sha = _sha(source)
    compression = (
        "Gzip"
        if source.startswith(b"\x1f\x8b")
        else "Bzip2"
        if source.startswith(b"BZh")
        else "None"
    )
    if compression == "None":
        decoded = source
        envelope = dict(
            integrity="ContainerComplete",
            verified_decoded_bytes=len(decoded),
            members=[],
            diagnostic=None,
        )
    else:
        decoded, envelope = _decode(
            source, compression, source_limit, header_limit, members_limit
        )
    source_report = dict(
        path=str(path),
        encoded_bytes=len(source),
        decoded_bytes=len(decoded),
        compression=compression,
        **envelope,
    )
    images = []
    stop = None
    offset = 0
    general = []
    active = None
    while offset < len(decoded):
        parsed, payload, stop = _header(decoded, offset, header_limit)
        if parsed is None:
            break
        raw_header, local = parsed
        if "EDF_DATAFORMATVERSION" in local:
            try:
                size = _integer(local, "EDF_BINARYSIZE", _integer(local, "SIZE", 0))
                reason = "general binary metadata payload" if size else None
            except ValueError as error:
                reason = str(error)
            if reason is not None:
                stop = dict(
                    kind="UnparsedRecord",
                    offset=offset,
                    header_hex=raw_header.hex(),
                    reason=reason,
                )
                break
            active = len(general)
            general.append(local)
            offset = payload
            continue
        fields = dict(local)
        if active is not None:
            for key, value in general[active].items():
                if not key.startswith("EDF_") and key not in (
                    "SIZE",
                    "IMAGE",
                    "HEADERID",
                ):
                    fields.setdefault(key, value)
        try:
            if "EDF_BINARYFILENAME" in fields:
                raise ValueError("external recovery is unsupported")
            if any(
                key.startswith("DIM_")
                and key not in ("DIM_1", "DIM_2")
                and value != "1"
                for key, value in fields.items()
            ):
                raise ValueError("volume unsupported")
            shape = [_integer(fields, "DIM_2"), _integer(fields, "DIM_1")]
            scalar = np.dtype(_TYPES[fields["DATATYPE"].lower()])
            count = shape[0] * shape[1]
            required = count * scalar.itemsize
            if count <= 0 or required > frame_limit:
                raise ValueError("reference shape/frame budget")
            order = fields["BYTEORDER"].translate(_UPPER)
            if order not in ("LOWBYTEFIRST", "HIGHBYTEFIRST"):
                raise ValueError("byte order unsupported")
            codec = fields.get("COMPRESSION", "NONE").translate(_UPPER)
            if raw_label is not None:
                if not raw_label or codec != raw_label.translate(_UPPER):
                    raise ValueError("stored raw label mismatch")
                codec = "NONE"
            codec = _CODECS[codec]
            declared = _integer(
                fields,
                "EDF_BINARYSIZE",
                _integer(fields, "SIZE", required if codec == "NONE" else 0),
            )
            expected = _integer(fields, "SIZE") if inline_size else declared
            if inline_size and (codec != "NONE" or expected < required):
                raise ValueError("legacy Size requires complete raw samples")
            if not expected:
                raise ValueError("missing payload window")
        except (KeyError, TypeError, ValueError) as error:
            stop = dict(
                kind="UnparsedRecord",
                offset=offset,
                header_hex=raw_header.hex(),
                reason=str(error),
            )
            break
        available = min(expected, len(decoded) - payload)
        encoded = decoded[payload : payload + available]
        if codec == "NONE":
            pixels = encoded[:required]
            body = dict(
                integrity="ContainerComplete",
                verified_decoded_bytes=len(pixels),
                members=[],
                diagnostic=None,
            )
        elif codec == "ZLIB":
            pixels, body = _zlib(encoded, required)
        else:
            pixels, body = _decode(
                encoded,
                {"GZIP": "Gzip", "BZ2": "Bzip2"}[codec],
                required,
                header_limit,
                members_limit,
                True,
            )
        present = len(pixels) // scalar.itemsize
        typed_bytes = present * scalar.itemsize
        typed = np.frombuffer(
            pixels[:typed_bytes],
            dtype=scalar.newbyteorder("<" if order == "LOWBYTEFIRST" else ">"),
        )
        canonical = (
            typed
            if order == "LOWBYTEFIRST" or scalar.itemsize == 1
            else typed.byteswap()
        )
        outer = envelope["integrity"]
        if (
            outer != "ContainerComplete"
            and payload + available <= envelope["verified_decoded_bytes"]
        ):
            outer = "VerifiedMember"
        verified = (
            min(typed_bytes, max(0, envelope["verified_decoded_bytes"] - payload))
            // scalar.itemsize
            if codec == "NONE"
            else min(typed_bytes, body["verified_decoded_bytes"]) // scalar.itemsize
            if outer in ("ContainerComplete", "VerifiedMember")
            else 0
        )
        complete = (
            present == count
            and len(pixels) == required
            and available == expected
            and outer in ("ContainerComplete", "VerifiedMember")
            and body["integrity"] == "ContainerComplete"
        )
        availability = dict(
            declared_shape=shape,
            declared_samples=count,
            present_samples=[0, present],
            missing_samples=[present, count] if present < count else None,
            verified_samples=[0, verified],
            outer_integrity=outer,
            payload_integrity=body["integrity"],
            expected_payload_bytes=expected,
            available_payload_bytes=available,
            availability="AllSamples"
            if present == count
            else "HeadersOnly"
            if not pixels
            else "Prefix",
            payload_members=body["members"],
            diagnostic=body["diagnostic"],
            complete_verified_frame=complete,
            partial_element_bytes=pixels[typed_bytes:].hex(),
        )
        window = dict(
            declared_bytes=declared,
            effective_bytes=available,
            external_policy=None,
            source=0,
            offset=payload,
            inline_layout_policy="LegacySizeAtParsedBoundary"
            if inline_size
            else "Declared",
            declared_header_bytes=fields.get(
                "EDF_HEADERSIZE", fields.get("HEADERSIZE")
            ),
            parsed_header_bytes=len(raw_header),
            legacy_size_bytes=_integer(fields, "SIZE"),
            declared_compression=fields.get("COMPRESSION"),
            payload_interpretation="Declared"
            if raw_label is None
            else f"StoredRawForLabel({json.dumps(raw_label)})",
        )
        images.append(
            dict(
                frame=len(images),
                shape=[present],
                dtype=scalar.name,
                sample_sha256=_sha(canonical.tobytes()),
                fields=fields,
                availability=availability,
                payload_window=window,
                header_sha256=_sha(raw_header),
            )
        )
        if available < expected:
            stop = dict(
                kind="TruncatedPayload",
                image_record=len(images) - 1,
                offset=payload,
                expected_bytes=expected,
                available_bytes=available,
            )
            break
        offset = payload + expected
    if not images and stop is None:
        stop = dict(kind="NoImageHeader")
    for image in images:
        image["frames"] = len(images)
    if _sha(path.read_bytes()) != source_sha:
        raise RuntimeError("reference source changed")
    return dict(
        format="edf",
        images=images,
        source_report=source_report,
        stop=stop,
        source_sha256=source_sha,
    )


def compare(path, native_source_recovery):
    """Validate all present sample bytes/ranges and integrity, never padded zeros."""
    first = next(iter(native_source_recovery.get("images", [])), {})
    window = first.get("payload_window", {})
    inline = window.get("inline_layout_policy") == "LegacySizeAtParsedBoundary"
    raw = window.get("payload_interpretation", "Declared")
    raw_label = (
        json.loads(raw[len("StoredRawForLabel(") : -1])
        if raw.startswith("StoredRawForLabel(")
        else None
    )
    reference = read(path, inline, raw_label)
    differences = []

    def check(label, actual, expected):
        if actual != expected:
            differences.append(dict(field=label, native=actual, reference=expected))

    def members(label, actual, expected):
        check(label + ".length", len(actual), len(expected))
        for i, (left, right) in enumerate(zip(actual, expected)):
            for key in ("encoded_range", "decoded_range", "integrity"):
                check(f"{label}[{i}].{key}", left.get(key), right[key])

    native_images = native_source_recovery.get("images", [])
    check("images.length", len(native_images), len(reference["images"]))
    for i, (native, expected) in enumerate(zip(native_images, reference["images"])):
        for key in (
            "frame",
            "frames",
            "shape",
            "dtype",
            "sample_sha256",
            "fields",
            "payload_window",
        ):
            check(f"images[{i}].{key}", native.get(key), expected[key])
        if "header_sha256" in native:
            check(
                f"images[{i}].header_sha256",
                native["header_sha256"],
                expected["header_sha256"],
            )
        for key, value in expected["availability"].items():
            if key not in ("diagnostic", "payload_members"):
                check(
                    f"images[{i}].availability.{key}",
                    native.get("availability", {}).get(key),
                    value,
                )
        members(
            f"images[{i}].payload_members",
            native.get("availability", {}).get("payload_members", []),
            expected["availability"]["payload_members"],
        )
    native_report = native_source_recovery.get("source_report", {})
    for key in (
        "path",
        "encoded_bytes",
        "decoded_bytes",
        "compression",
        "integrity",
        "verified_decoded_bytes",
    ):
        check(
            "source_report." + key,
            native_report.get(key),
            reference["source_report"][key],
        )
    members(
        "source_report.members",
        native_report.get("members", []),
        reference["source_report"]["members"],
    )
    native_stop = native_source_recovery.get("stop")
    expected_stop = reference["stop"]
    if native_stop is None or expected_stop is None:
        check("stop", native_stop, expected_stop)
    else:
        for key, value in expected_stop.items():
            if key != "reason":
                check("stop." + key, native_stop.get(key), value)
    return dict(differences=differences, reference=reference)


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument("path")
    parser.add_argument("--inline-size", action="store_true")
    parser.add_argument("--raw-label")
    arguments = parser.parse_args()
    print(
        json.dumps(
            read(arguments.path, arguments.inline_size, arguments.raw_label), indent=2
        )
    )
