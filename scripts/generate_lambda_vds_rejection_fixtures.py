#!/usr/bin/env python3
"""Generate CC0 VDS source-contract fixtures from independent explicit samples."""
import hashlib
import json
from pathlib import Path
import h5py
import numpy as np
ROOT = Path(__file__).resolve().parents[1] / 'tests/data/lambda-vds-rejection'
ROOT.mkdir(parents=True, exist_ok=True)
source = ROOT/'module.h5'
with h5py.File(source, 'w') as f:
    f['images'] = np.array([[[1, -2, 32767], [-32768, 5, 6]]], dtype='<i2')
for name, filename in [('missing-absolute.nxs', '/dioptrin-io-independent-fixture-source-does-not-exist/module.h5'),
                       ('present-relative.nxs', 'module.h5')]:
    layout = h5py.VirtualLayout(shape=(1, 2, 3), dtype='<i2')
    layout[:] = h5py.VirtualSource(filename, '/images', shape=(1, 2, 3))
    with h5py.File(ROOT/name, 'w', libver='latest') as f:
        d = f.require_group('/entry/instrument/detector')
        d['description'] = np.bytes_('Lambda')
        d.create_virtual_dataset('data', layout, fillvalue=-999)
(ROOT/'provenance.json').write_text(json.dumps(dict(
    license='CC0-1.0', generator='scripts/generate_lambda_vds_rejection_fixtures.py',
    source='Independent explicit extrema; missing absolute target deliberately differs from local basename decoy',
    files={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in ROOT.iterdir() if p.suffix in ('.nxs','.h5')}), indent=2)+'\n')
