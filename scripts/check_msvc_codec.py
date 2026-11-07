"""Fast native MSVC compile check before the full dependency build in Windows CI."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
vendor = root / 'vendor/hdf5-bitshuffle-1.0.0'
include = vendor / 'ext/bitshuffle/src'
vswhere = Path(os.environ['ProgramFiles(x86)']) / 'Microsoft Visual Studio/Installer/vswhere.exe'
vs = subprocess.check_output([str(vswhere), '-latest', '-products', '*', '-property', 'installationPath'], text=True).strip()
vcvars = Path(vs) / 'VC/Auxiliary/Build/vcvarsall.bat'
arch = 'arm64' if os.environ['RUNNER_ARCH'] == 'ARM64' else 'x64'
with tempfile.TemporaryDirectory(prefix='dioptrin-msvc-') as temp:
    work = Path(temp)
    (work / 'src').mkdir()
    core = work / 'src/bitshuffle_core.c'
    shutil.copyfile(include / 'bitshuffle_core.c', core)
    # Explicit standalone mode matches the production build, even inside Git.
    for name in ['0002-upstream-neon-movemask.patch', '0003-upstream-msvc-alias.patch',
                 '0004-msvc-simd-selection.patch']:
        subprocess.run(['git', '--git-dir', str(work), '-C', str(work), 'apply', '-p1',
                        str(vendor / 'patches' / name)], check=True)
    batch = work / 'compile.cmd'
    batch.write_text('@echo off\n' + f'call "{vcvars}" {arch}\n' +
                     'if errorlevel 1 exit /b 1\n' +
                     f'cl /nologo /std:c11 /W0 /O2 /c /I"{include}" /Fo"{work / "core.obj"}" "{core}"\n')
    subprocess.run(['cmd.exe', '/d', '/c', str(batch)], check=True)
