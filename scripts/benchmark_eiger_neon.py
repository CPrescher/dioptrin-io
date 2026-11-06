"""Compare native readers and Jon's CPU APIs on the complete immutable Eiger9M scan.

Run with the Dioptas reference Python environment (h5py, NumPy, bslz4_to_sparse).
Native baseline/current binaries must be built from the same benchmark example.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
from importlib.metadata import version
import json
import os
from pathlib import Path
import platform
from statistics import median
import struct
import subprocess
import sys
import threading
import time

os.environ.setdefault("OMP_NUM_THREADS", "1")
os.environ.setdefault("OPENBLAS_NUM_THREADS", "1")


def identity(stat):
    return stat.st_dev, stat.st_ino, stat.st_size, stat.st_mtime_ns


def jon_pass(args):
    import bslz4_to_sparse as codec
    import h5py
    import numpy as np

    expected = json.loads(args.reference.read_text())["frame_sha256"]
    setup = time.perf_counter()
    source_identity = identity(args.source.stat())
    with h5py.File(args.source, "r") as handle:
        dataset = handle[args.dataset]
        shape, dtype = dataset.shape, dataset.dtype
        assert shape == (625, 3262, 3108) and dtype == np.dtype("uint32")
        assert dataset.chunks == (1, *shape[1:])
        properties = dataset.id.get_create_plist()
        assert properties.get_nfilters() == 1
        filter_id, _, parameters, _ = properties.get_filter(0)
        assert filter_id == 32008 and parameters[4] == 2
        info = [dataset.id.get_chunk_info_by_coord((i, 0, 0)) for i in range(shape[0])]
        assert all(c.filter_mask == 0 and c.byte_offset is not None for c in info)
        locations = [(c.byte_offset, c.size) for c in info]
    raw_bytes = shape[1] * shape[2] * dtype.itemsize
    payload = 3 * raw_bytes + max(length for _, length in locations)
    assert args.workers * payload <= 2 << 30
    setup_seconds = time.perf_counter() - setup
    local = threading.local()
    fd = os.open(args.source, os.O_RDONLY)
    assert identity(os.fstat(fd)) == source_identity

    def check_source():
        if (identity(os.fstat(fd)) != source_identity
                or identity(args.source.stat()) != source_identity):
            raise RuntimeError("source changed during benchmark")

    def read(index):
        check_source()
        offset, length = locations[index]
        encoded = os.pread(fd, length, offset)
        assert len(encoded) == length and struct.unpack(">Q", encoded[:8])[0] == raw_bytes
        if not hasattr(local, "pixels"):
            local.pixels = np.empty(shape[1:], dtype)
        offsets, lengths = np.array([0], np.int64), np.array([length], np.int64)
        if args.jon_child == "jon-kcb":
            codec.decompress_from_buffer(encoded, offsets, lengths, dtype.type, out=local.pixels)
        else:
            # Explicit diagnostic variant, not a public-API/default change.
            pointers, lengths, header = codec._offset_pointers(encoded, offsets, lengths)
            workspace = np.empty(3 * min(codec._blocksize_bytes(header), raw_bytes), np.uint8)
            status = codec._ext.decompress(
                pointers, lengths, local.pixels.view(np.uint8).reshape(-1), workspace,
                dtype.itemsize, codec._pipeline.NAMES["decode"].index("lz4-stock"),
                codec._pipeline.NAMES["untranspose"].index("neon"), 1,
            )
            if status < 0:
                raise RuntimeError(f"Jon NEON decoder failed: {status}")
        check_source()
        if args.mode == "verify":
            digest = hashlib.sha256(local.pixels.data).hexdigest()
            assert digest == expected[index], f"pixel mismatch at frame {index}"
            return index, digest
        return index, None

    hashes = [None] * shape[0]
    next_index = 0
    index_lock = threading.Lock()

    def worker(_):
        nonlocal next_index
        count = 0
        while True:
            with index_lock:
                index = next_index
                next_index += 1
            if index >= shape[0]:
                return count
            index, digest = read(index)
            hashes[index] = digest
            count += 1

    start = time.perf_counter()
    try:
        with ThreadPoolExecutor(max_workers=args.workers) as pool:
            count = sum(pool.map(worker, range(args.workers)))
        seconds = time.perf_counter() - start
        assert count == shape[0]
    finally:
        os.close(fd)
    return {"workers": args.workers, "mode": args.mode, "frames": count,
            "seconds": seconds, "fps": count / seconds, "setup_seconds": setup_seconds,
            "worker_payload_bytes": payload,
            "frame_sha256": hashes if args.mode == "verify" else None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--dataset", default="entry_0000/instrument/eiger/data")
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--native", type=Path)
    parser.add_argument("--baseline-label", default="native-before")
    parser.add_argument("--native-label", default="native-neon")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--jon-child", choices=["jon-kcb", "jon-neon"])
    parser.add_argument("--workers", type=int, choices=[1, 8], default=1)
    parser.add_argument("--mode", choices=["bench", "verify"], default="bench")
    args = parser.parse_args()
    if args.jon_child:
        print(json.dumps(jon_pass(args)))
        return
    if not all([args.baseline, args.native, args.output]) or args.repeats < 1:
        parser.error("baseline, native, output and positive repeats are required")
    if (not all(v.startswith("native-") for v in [args.baseline_label, args.native_label])
            or args.baseline_label == args.native_label):
        parser.error("native labels must be distinct and start with native-")
    import bslz4_to_sparse as codec
    import psutil

    reference = json.loads(args.reference.read_text())
    before = identity(args.source.stat())
    with args.source.open("rb") as source:
        source_sha256 = hashlib.file_digest(source, "sha256").hexdigest()
    assert source_sha256 == reference["sha256"]
    variants = [args.baseline_label, args.native_label, "jon-kcb", "jon-neon"]
    record = {
        "source": str(args.source), "source_sha256": source_sha256,
        "reference_sha256": hashlib.sha256(args.reference.read_bytes()).hexdigest(),
        "shape": reference["shape"], "workers": [1, 8], "repeats": args.repeats,
        "python": sys.version, "platform": platform.platform(),
        "cpu_physical": psutil.cpu_count(logical=False), "cpu_logical": os.cpu_count(),
        "ram_bytes": psutil.virtual_memory().total,
        "versions": {name: version(name) for name in ["bslz4-to-sparse", "h5py", "numpy"]},
        "jon_build": codec.build_info(),
        "binary_sha256": {name: hashlib.sha256(path.read_bytes()).hexdigest()
                          for name, path in [(args.baseline_label, args.baseline),
                                             (args.native_label, args.native)]},
        "validation": [], "samples": [],
        "limits": (
            "Complete 625-frame Eiger9M warm-cache read/decode to stored-orientation uint32 "
            "consumer; no masking, float conversion, normalization or integration. Raw file "
            "reads, decompression, worker/buffer creation, source checks and delivery are timed. "
            "Opening/chunk indexing, interpreter startup and SHA256 validation are excluded. "
            "Fresh process and workers per pass, one reusable output per worker; both readers "
            "dynamically assign frame indices to fixed worker loops without a decoded-frame queue. "
            "Each implementation/worker count first validates all 625 frames against the "
            "independent h5py/HDF5 reference; those full passes also warm the file cache. "
            "Native uses bounded lz4_flex plus Bitshuffle; Jon uses his installed package's "
            "LZ4 and public default KCB API. jon-neon is an explicitly selected private native "
            "entry-point diagnostic with the same per-frame buffer policy. Native compressed "
            "buffers are reusable; os.pread returns new bytes for Python. Each API's required "
            "overheads are included. Host not reserved; background load recorded, no cold-disk claim."
        ),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)

    def save():
        args.output.write_text(json.dumps(record, indent=2) + "\n")

    def run(variant, workers, mode):
        if variant.startswith("native"):
            binary = args.baseline if variant == args.baseline_label else args.native
            cmd = [str(binary.resolve()), str(args.source), args.dataset, str(workers),
                   mode, str(args.reference)]
        else:
            cmd = [sys.executable, str(Path(__file__).resolve()), "--jon-child", variant,
                   "--workers", str(workers), "--mode", mode, "--source", str(args.source),
                   "--dataset", args.dataset, "--reference", str(args.reference)]
        load = os.getloadavg()
        result = subprocess.run(cmd, text=True, capture_output=True, check=True)
        item = json.loads(result.stdout)
        item.update(implementation=variant, load_average_before=load)
        if mode == "verify":
            hashes = item.pop("frame_sha256")
            assert hashes == reference["frame_sha256"]
            item["all_count_hashes_match"] = True
            item["ordered_count_hashes_sha256"] = hashlib.sha256(
                "".join(hashes).encode()).hexdigest()
        else:
            item.pop("frame_sha256")
        assert identity(args.source.stat()) == before
        return item

    for workers in [1, 8]:
        for variant in variants:
            item = run(variant, workers, "verify")
            record["validation"].append(item)
            save()
            print("Validated", variant, workers, "workers: all 625 frame hashes match", flush=True)
    for repeat in range(args.repeats):
        for workers in [1, 8]:
            for variant in variants if repeat % 2 == 0 else reversed(variants):
                item = run(variant, workers, "bench")
                item["repeat"] = repeat
                record["samples"].append(item)
                save()
                print(repeat, variant, workers, "workers:", round(item["fps"], 2), "fps", flush=True)
    record["summary"] = {}
    for workers in [1, 8]:
        record["summary"][str(workers)] = {}
        for variant in variants:
            samples = [s["fps"] for s in record["samples"]
                       if s["workers"] == workers and s["implementation"] == variant]
            record["summary"][str(workers)][variant] = {
                "median_fps": median(samples), "min_fps": min(samples), "max_fps": max(samples)}
    save()
    print(json.dumps(record["summary"], indent=2), flush=True)


if __name__ == "__main__":
    main()
