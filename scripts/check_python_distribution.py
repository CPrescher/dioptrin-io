"""Inspect Python artifacts and optionally build/install/test the sdist in isolation."""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import venv
import zipfile


def check_wheel(path):
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        required = [
            "dioptrin_io/__init__.py", "dioptrin_io/__init__.pyi",
            "dioptrin_io_fused/__init__.py", "dioptrin_io_fused/__init__.pyi",
            "licenses/licenses/Dioptrin-MIT.txt",
            "licenses/licenses/tiff-0.8.1-LICENSE",
            "dioptrin_io/py.typed", "licenses/LICENSE",
            "licenses/licenses/hdf5/HDF5.txt",
            "licenses/licenses/native-readers/FabIO-2026.6.0-copyright.txt",
            "licenses/licenses/native-readers/IMAGE-CODECS-NOTICES.md",
            "licenses/native/tiff-thunderscan/LICENSE.md",
            "licenses/licenses/python/pyo3-LICENSE-MIT.txt",
            "licenses/licenses/python/rust-numpy-LICENSE.txt",
        ]
        for suffix in required:
            assert any(n.endswith(suffix) for n in names), (path, suffix)
        assert any(n.startswith("dioptrin_io/_native.") for n in names), path
        metadata_name = next(n for n in names if n.endswith(".dist-info/METADATA"))
        metadata = archive.read(metadata_name).decode()
        assert "Requires-Dist: numpy" in metadata
        assert not any(".cargo_vcs_info.json" in n for n in names)
    print(f"Verified wheel and notices: {path.name}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--rebuild", action="store_true")
    args = parser.parse_args()
    wheels = list(args.directory.glob("*.whl"))
    assert wheels, "no wheel found"
    for wheel in wheels:
        check_wheel(wheel)
    if not args.rebuild:
        return
    sources = list(args.directory.glob("*.tar.gz"))
    assert len(sources) == 1, "expected one sdist"
    with tempfile.TemporaryDirectory(prefix="dioptrin-python-sdist-") as temporary:
        root = Path(temporary)
        with tarfile.open(sources[0]) as archive:
            archive.extractall(root, filter="data")
        source = next(p for p in root.iterdir() if p.is_dir())
        for relative in [
            "src/lib.rs", "python/src/lib.rs", "python/Cargo.lock",
            "python/tests/test_read.py", "python-fused/src/bridge.rs",
            "python/dioptrin_io_fused/__init__.py",
            "native/tiff-thunderscan/wrapper.c",
            "vendor/tiff-0.11.3/src/lib.rs", "vendor/fax-0.2.7/src/lib.rs",
            "vendor/hdf5-bitshuffle-1.0.0/ext/bitshuffle/src/bitshuffle.c",
        ]:
            assert (source / relative).is_file(), relative
        output = root / "wheels"
        subprocess.run([
            sys.executable, "-m", "maturin", "build", "--locked", "--release",
            "--interpreter", sys.executable, "--out", str(output),
        ], cwd=source, check=True)
        wheel = next(output.glob("*.whl"))
        check_wheel(wheel)
        environment = root / "venv"
        venv.create(environment, with_pip=True)
        python = environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
        subprocess.run([
            str(python), "-m", "pip", "install", f"{wheel}[test]",
        ], cwd=root, check=True)
        subprocess.run([
            str(python), "-m", "pytest", str(source / "python/tests"), "-q",
        ], cwd=root, check=True)
    print("Verified isolated sdist build and installed-wheel tests", flush=True)


if __name__ == "__main__":
    main()
