"""Require a complete portable wheel matrix before PyPI publication."""
import argparse
from pathlib import Path
import tomllib

from packaging.utils import parse_wheel_filename


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    version = tomllib.loads(Path("pyproject.toml").read_text())["project"]["version"]
    common = {"linux-x86_64", "linux-aarch64", "macos-x86_64", "macos-arm64", "windows-x86_64"}
    expected = {(f"cp3{minor}", target) for minor in range(10, 16) for target in common}
    expected.update((f"cp3{minor}", "windows-arm64") for minor in range(13, 16))
    actual = set()
    for wheel in args.directory.glob("*.whl"):
        name, wheel_version, _, tags = parse_wheel_filename(wheel.name)
        assert name == "dioptrin-io" and str(wheel_version) == version, wheel.name
        cases = set()
        for tag in tags:
            assert tag.abi == tag.interpreter, wheel.name
            platform = tag.platform
            if platform.startswith("manylinux_"):
                target = "linux-" + ("aarch64" if platform.endswith("aarch64") else "x86_64")
            elif platform.startswith("macosx_"):
                target = "macos-" + ("arm64" if platform.endswith("arm64") else "x86_64")
            elif platform == "win_amd64":
                target = "windows-x86_64"
            elif platform == "win_arm64":
                target = "windows-arm64"
            else:
                raise AssertionError(f"Nonportable/unexpected platform tag: {wheel.name}")
            cases.add((tag.interpreter, target))
        assert len(cases) == 1 and not (actual & cases), wheel.name
        actual.update(cases)
    assert actual == expected, {"missing": sorted(expected-actual), "unexpected": sorted(actual-expected)}
    sources = list(args.directory.glob("*.tar.gz"))
    assert len(sources) == 1 and sources[0].name == f"dioptrin_io-{version}.tar.gz", sources
    print(f"Verified {len(actual)} portable wheels and one source archive for {version}")


if __name__ == "__main__":
    main()
