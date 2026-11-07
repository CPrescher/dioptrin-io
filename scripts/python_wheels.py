"""Build/test CPython wheels on a native runner using managed interpreters."""
import argparse
from pathlib import Path
import subprocess
import sys
import tempfile


def run(*args, **kwargs):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--versions", nargs="+", required=True)
    parser.add_argument("--directory", type=Path, default=Path("dist"))
    parser.add_argument("--build", action="store_true")
    args = parser.parse_args()
    directory = args.directory.resolve()
    for version in args.versions:
        run("uv", "python", "install", version)
        interpreter = subprocess.check_output(
            ["uv", "python", "find", "--managed-python", version], text=True
        ).strip()
        if args.build:
            run(sys.executable, "-m", "maturin", "build", "--locked", "--release",
                "--compatibility", "pypi", "--interpreter", interpreter,
                "--out", directory)
        with tempfile.TemporaryDirectory(prefix="dioptrin-wheel-test-") as temporary:
            environment = Path(temporary) / "venv"
            run("uv", "venv", "--python", interpreter, environment)
            python = environment / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
            run("uv", "pip", "install", "--python", python, "--no-index", "--no-deps",
                "--find-links", directory, "dioptrin-io")
            run("uv", "pip", "install", "--python", python, "--only-binary", ":all:",
                "numpy>=2,<3", "pytest>=8", "tifffile>=2024.1.30")
            # h5py is a reference encoder only, never a reader runtime dependency.
            # Its wheels currently stop at 3.14; 3.15 still checks the independently
            # generated HDF5 corpus bit-for-bit through the NumPy consumer API.
            result = subprocess.run(["uv", "pip", "install", "--python", str(python),
                                     "--only-binary", ":all:", "h5py>=3.11"])
            if result.returncode and version != "3.15":
                result.check_returncode()
            run(python, "-m", "pytest", "python/tests", "-q", "-rs")


if __name__ == "__main__":
    main()
