#!/usr/bin/env python3
"""Audit four baseline events without changing the global parity inventory.

Test/reference tooling only: invokes a separately compiled, pinned BSD dxtbx
native decoder, the existing Rust legacy_probe, FabIO, h5py and NumPy. Production
readers never invoke this script or a subprocess.
"""
import argparse
import ctypes
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

import h5py
import hdf5plugin
import numpy as np
from fabio.OXDimage import OxdImage

DXTBX = '41fc3d3cfd26c12e8cb3595b68360a9a9bdbcb05'
ROOT = Path(__file__).resolve().parents[1]
def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def signature(array):
    array = np.asarray(array)
    return dict(dtype=str(array.dtype), shape=list(array.shape),
                sample_sha256=hashlib.sha256(array.astype(array.dtype.newbyteorder('<'), copy=False).tobytes()).hexdigest())
def native(probe, path):
    return json.loads(subprocess.check_output([str(probe), 'oxford', str(path)], text=True))
def build_oracle(dxtbx, temporary):
    commit = subprocess.check_output(['git', '-C', str(dxtbx), 'rev-parse', 'HEAD'], text=True).strip()
    assert commit == DXTBX, commit
    source = dxtbx/'src/dxtbx/boost_python/compression.cc'
    text = source.read_text()
    text = text[text.index('inline uint32_t read_uint32_from_bytearray'):]
    text = text.replace('void dxtbx::boost_python::rod_TY5_TY6_decompress',
                        'extern "C" void rod_TY5_TY6_decompress')
    cpp = temporary/'oracle.cpp'
    cpp.write_text('#include <cstdint>\n#include <cstddef>\n#include <cassert>\n' + text)
    library = temporary/'oracle.dylib'
    subprocess.run(['c++', '-std=c++20', '-O2', '-shared', '-fPIC', str(cpp), '-o', str(library)], check=True)
    lib = ctypes.CDLL(str(library))
    function = lib.rod_TY5_TY6_decompress
    function.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p,
                         ctypes.c_int, ctypes.c_int, ctypes.c_int]
    def decode(path):
        data = path.read_bytes()
        fields = {key.decode(): int(value) for key, value in re.findall(rb'(NX|NY|OI|OL|NHEADER)=\s*(\d+)', data[:256])}
        width, height = fields['NX'], fields['NY']
        size = width * height + 2 * fields['OI'] + 4 * fields['OL']
        start = fields['NHEADER']
        encoded = data[start:start + size]
        offsets = data[start + size:start + size + 4 * height]
        result = np.empty((height, width), dtype=np.int32)
        function(result.ctypes.data, encoded, offsets, height, width, 5)
        return result
    return decode

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, default=Path('/tmp/dioptrin-io-fabio-corpus'))
    parser.add_argument('--dxtbx', type=Path, required=True)
    parser.add_argument('--native-probe', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    hdf5plugin.register(force=True)
    baseline = ROOT/'docs/fabio-parity/2026-11-05-mar-sparse.json'
    report = json.loads(baseline.read_text())
    records = []
    with tempfile.TemporaryDirectory(prefix='dioptrin-lambda-ty5-audit-') as directory:
        temporary = Path(directory)
        oracle = build_oracle(args.dxtbx, temporary)
        ty5 = args.corpus/'100nmfilmonglass_1_1.img'
        decoded = oracle(ty5)
        reference = OxdImage().read(str(ty5)).data
        assert np.array_equal(decoded, reference)
        native_result = native(args.native_probe, ty5)
        sig = signature(decoded)
        assert native_result['dtype'] == 'I32'
        assert native_result['shape'] == sig['shape']
        assert native_result['frame_sha256'] == sig['sample_sha256']
        fixture_checks = []
        manifest = json.loads((ROOT/'tests/data/ty5/provenance.json').read_text())
        for name, expected in manifest['files'].items():
            path = ROOT/'tests/data/ty5'/name
            actual = signature(oracle(path))
            rust = native(args.native_probe, path)
            assert actual['sample_sha256'] == expected['samples_sha256'] == rust['frame_sha256']
            fixture_checks.append(dict(file=name, samples_sha256=actual['sample_sha256'], native_matches=True))
        ty5_evidence = dict(
            pinned_dxtbx_commit=DXTBX,
            authority=[dict(path=str(p.relative_to(args.dxtbx)), sha256=sha(p),
                            url=f'https://github.com/cctbx/dxtbx/blob/{DXTBX}/{p.relative_to(args.dxtbx)}')
                       for p in [args.dxtbx/'src/dxtbx/boost_python/compression.cc',
                                 args.dxtbx/'src/dxtbx/format/FormatROD.py', args.dxtbx/'LICENSE.txt']],
            source_layout='NHEADER + NX*NY+2*OI+4*OL inline bytecode + NY little-endian u32 row offsets + retained trailer',
            original_row_offsets=1024, original_trailer_bytes=24,
            reference_bug='Pinned FabIO ignores row offset table and resets on height rather than width; square baseline hides row-reset defect',
            verification=dict(independent_dxtbx_signature=sig, native_signature=native_result,
                              fabio_signature=signature(reference), all_samples_equal=True,
                              sample_count=decoded.size, minimum=int(decoded.min()), maximum=int(decoded.max()),
                              representation_policy='Native I32 retained; test-only exact I32-to-F64 view required for FabIO dtype comparison',
                              independent_fixtures=fixture_checks))
        records.append(dict(source=str(ty5.resolve()), baseline_sha256=sha(ty5), baseline_event_count=2,
                            input_condition='well_formed_variant', reader_action='implemented',
                            classification='closed_native_codec_gap', format='Oxford TY5', evidence=ty5_evidence))
        master = args.corpus/'l1_test02_00002_master.nxs'
        mapping = []
        with h5py.File(master) as f:
            dataset = f['/entry/instrument/detector/data']
            shape, dtype, fill = list(dataset.shape), str(dataset.dtype), int(dataset.fillvalue)
            signatures = [signature(dataset[i]) for i in range(dataset.shape[0])]
            for item in dataset.virtual_sources():
                mapping.append(dict(stored_filename=item.file_name, exact_path_available=Path(item.file_name).is_file(),
                                    stored_dataset=item.dset_name,
                                    virtual_regular_hyperslab=[list(v) for v in item.vspace.get_regular_hyperslab()],
                                    source_regular_hyperslab=[list(v) for v in item.src_space.get_regular_hyperslab()],
                                    corpus_basename=Path(item.file_name).name,
                                    corpus_basename_sha256=sha(args.corpus/Path(item.file_name).name)))
        isolated = temporary/master.name
        shutil.copyfile(master, isolated)
        with h5py.File(isolated) as f:
            missing_shape = list(f['/entry/instrument/detector/data'].shape)
            assert missing_shape[0] == 0
        # Available modules establish the unlimited frame extent. Missing other
        # mapped modules then silently contribute declared fill to that extent.
        for item in mapping[1:]:
            shutil.copyfile(args.corpus/item['corpus_basename'], temporary/item['corpus_basename'])
        with h5py.File(isolated) as f:
            partial = f['/entry/instrument/detector/data']
            partial_shape = list(partial.shape)
            partial_signatures = [signature(partial[i]) for i in range(partial.shape[0])]
            partial_first_max = int(partial[0].max())
        assert partial_shape == shape and partial_signatures != signatures
        for item in mapping:
            shutil.copyfile(args.corpus/item['corpus_basename'], temporary/item['corpus_basename'])
        with h5py.File(isolated) as f:
            relocated_signatures = [signature(frame) for frame in f['/entry/instrument/detector/data']]
        assert signatures == relocated_signatures
        records.append(dict(source=str(master.resolve()), baseline_sha256=sha(master), baseline_event_count=2,
                            input_condition='incomplete_exact_source_graph', reader_action='unsupported',
                            classification='source_contract_and_virtual_layout_gap', format='Lambda VDS',
                            malformed=False, evidence=dict(shape=shape, dtype=dtype, declared_fill_value=fill,
                            mappings=mapping, all_exact_paths_missing=True, hdf5_runtime=h5py.version.hdf5_version,
                            reference_reads_via_implicit_basename_fallback=True,
                            verification=dict(original_frame_signatures=signatures,
                                              isolated_master_missing_sources_shape=missing_shape,
                                              isolated_master_partial_sources_shape=partial_shape,
                                              isolated_master_partial_sources_frame_signatures=partial_signatures,
                                              isolated_master_partial_sources_first_frame_max=partial_first_max,
                                              missing_module_silently_contributes_fill=True,
                                              isolated_master_with_basename_files_matches_all_frames=True),
                            required_contract='Explicit caller-approved filename mapping, complete immutable source graph, validated bounded hyperslabs and declared unmapped-region fill; no missing-source fill or native path search',
                            production_policy='VDS remains rejected under both Reject and FollowSnapshot external-link policies')))
    for record in records:
        matching = [item for item in report['reads'] if item.get('file_sha256') == record['baseline_sha256'] and item.get('status') == 'native_rejected']
        assert len(matching) == record['baseline_event_count']
        record['baseline_tests'] = [item['test'] for item in matching]
    result = dict(schema=1, baseline=str(baseline.relative_to(ROOT)), baseline_sha256=sha(baseline),
                  audited_events=4, audited_unique_sources=2, malformed_sources=0,
                  summary=dict(implemented_events=2, remaining_source_contract_layout_events=2), records=records)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2)+'\n')
if __name__ == '__main__':
    main()
