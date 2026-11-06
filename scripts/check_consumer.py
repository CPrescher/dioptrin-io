"""Verify dioptrin-io as another application's dependency, without root patches.

The temporary Rust consumer compiles all requested features and checks original
CCITT RLE/Group3 counts plus source shape/precision/compression/fill-order metadata.
Python is test orchestration only; the consumer and library perform all decoding.
"""
import argparse
import json
import os
import pathlib
import subprocess
import tempfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
RUST = r'''
use dioptrin_io::{PixelBuffer, tiff::{FrameBuffer, Limits, Reader}};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::path::PathBuf::from(std::env::args_os().nth(1).ok_or("fixture directory required")?);
    let mut files = 0;
    for (compression, options) in [(2, 0), (3, 1), (3, 5)] {
        for fill in [1, 2] {
            let name = format!("fax-{compression}-options{options}-fill{fill}.tif");
            let mut reader = Reader::open(base.join(&name), Limits::default())?;
            let mut output = FrameBuffer::default();
            reader.read_frame_into(0, &mut output)?;
            let expected = std::fs::read(base.join(format!("{name}.raw")))?;
            assert_eq!(output.pixels(), Some(&PixelBuffer::U8(expected)), "{name}");
            let metadata = reader.metadata(0).ok_or("metadata unavailable")?;
            assert_eq!(metadata.shape, [17, 93]);
            assert_eq!(metadata.bits_per_sample, 1);
            assert_eq!(metadata.compression, compression);
            assert_eq!(metadata.fill_order, fill);
            files += 1;
        }
    }
    println!("consumer verified {files} source images and metadata");
    Ok(())
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--all-features', action='store_true')
    parser.add_argument('--git', help='Git source URL instead of a local path dependency')
    parser.add_argument('--rev', help='Exact Git revision; requires --git')
    args = parser.parse_args()
    if bool(args.git) != bool(args.rev):
        parser.error('--git and --rev must be supplied together')
    features = sorted(k for k in tomllib.loads((ROOT / 'Cargo.toml').read_text())['features'] if k != 'default') if args.all_features else ['tiff']
    location = f'git = {json.dumps(args.git)}, rev = {json.dumps(args.rev)}' if args.git else f'path = {json.dumps(str(ROOT))}'
    with tempfile.TemporaryDirectory(prefix='dioptrin-consumer-') as temp:
        path = pathlib.Path(temp)
        (path / 'src').mkdir()
        (path / 'src/main.rs').write_text(RUST)
        (path / 'Cargo.toml').write_text('[package]\nname = "dioptrin-io-consumer-check"\nversion = "0.0.0"\nedition = "2024"\n\n[dependencies]\n' +
            f'dioptrin-io = {{ {location}, default-features = false, features = {json.dumps(features)} }}\n')
        env = dict(os.environ)
        env.setdefault('CARGO_TARGET_DIR', str(ROOT / 'target'))
        command = ['cargo', 'run', '--manifest-path', str(path / 'Cargo.toml'), '--', str(ROOT / 'tests/data/tiff-layouts')]
        subprocess.run(command, check=True, env=env)
        metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--manifest-path', str(path / 'Cargo.toml'), '--format-version', '1'], env=env, text=True))
        backends = []
        for name, version in [('fax', '0.2.7'), ('tiff', '0.11.3')]:
            matches = [p for p in metadata['packages'] if p['name'] == name]
            if len(matches) != 1 or matches[0]['version'] != version:
                raise ValueError(f'unexpected codec resolution: {name}')
            package = matches[0]
            manifest = pathlib.Path(package['manifest_path'])
            if manifest.parts[-3:] != ('vendor', name + '-' + version, 'Cargo.toml'):
                raise ValueError(f'patched source was not preserved: {manifest}')
            if not args.git and manifest.resolve() != (ROOT / 'vendor' / (name + '-' + version) / 'Cargo.toml').resolve():
                raise ValueError('path consumer substituted a codec source')
            if args.git and (not package['source'] or not package['source'].startswith('git+') or not package['source'].endswith('#' + args.rev)):
                raise ValueError('Git consumer did not retain the exact codec revision')
            backends.append(dict(name=name, version=version, source=package['source'], manifest_path=str(manifest)))
        print(json.dumps(dict(passed=True, dependency='git' if args.git else 'path', features=features, source_images=6, backends=backends)))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
