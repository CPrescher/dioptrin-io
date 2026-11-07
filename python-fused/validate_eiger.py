"""Validate the combined API on all Eiger frames without a decoded scan cache."""
import argparse
import hashlib
import json
from pathlib import Path
import time

import h5py
import numpy as np
import pyFAI
import dioptrin
import dioptrin_io_fused


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--hdf5',type=Path,required=True)
    p.add_argument('--poni',type=Path,required=True)
    p.add_argument('--mask-u8',type=Path,required=True)
    p.add_argument('--reference',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    args=p.parse_args()
    reference=json.loads(args.reference.read_text())
    dataset=reference['dataset']
    with h5py.File(args.hdf5) as f: shape=f[dataset].shape
    ai=pyFAI.load(str(args.poni))
    poni=dict(pixel1=ai.detector.pixel1,pixel2=ai.detector.pixel2,distance=ai.dist,
              poni1=ai.poni1,poni2=ai.poni2,rot1=ai.rot1,rot2=ai.rot2,rot3=ai.rot3,wavelength=ai.wavelength)
    mask=np.fromfile(args.mask_u8,np.uint8).reshape(shape[1:]).astype(bool)
    if ai.detector.mask is not None:mask=np.logical_or(mask,ai.detector.mask)
    mask=np.ascontiguousarray(mask,dtype=np.uint8)
    native=dioptrin_io_fused.FusedHdf5Integrator(args.hdf5,dataset,poni,2000,
        mask=mask,unit='2th_deg',polarization_factor=.965,num_workers=8)
    baseline=dioptrin.Integrator.from_poni_dict(poni,unit='2th_deg',polarization_factor=.965)
    baseline.set_mask(mask)
    max_intensity=max_sigma=0.
    started=time.perf_counter()
    for frame,expected_hash in enumerate(reference['frame_sha256']):
        pixels,pattern=native.load_integrate1d(frame,errors=True)
        assert pixels.shape==shape[1:] and pixels.dtype==np.uint32
        assert not pixels.flags.writeable
        assert hashlib.sha256(pixels.tobytes()).hexdigest()==expected_hash
        current=baseline.integrate1d(np.ascontiguousarray(pixels[::-1],dtype=np.float64),2000,errors=True)
        np.testing.assert_allclose(pattern.radial,current.radial)
        for actual,expected in [(pattern.intensity,current.intensity),(pattern.errors,current.errors)]:
            np.testing.assert_allclose(actual,expected,rtol=2e-12,atol=1e-10)
        a,b=np.asarray(pattern.intensity),np.asarray(current.intensity)
        max_intensity=max(max_intensity,float(np.max(np.abs(a-b)/np.maximum(1,np.abs(b)))))
        a,b=np.asarray(pattern.errors),np.asarray(current.errors)
        max_sigma=max(max_sigma,float(np.max(np.abs(a-b)/np.maximum(1,np.abs(b)))))
        if (frame+1)%100==0:print('Validated',frame+1,flush=True)
    report=dict(frames=shape[0],shape=shape,original_count_hashes='all match independent h5py references',
        mask_sha256=hashlib.sha256(mask.tobytes()).hexdigest(),workers=8,bins=2000,polarization=.965,
        weights='float64',sums='float64',poisson=True,max_scaled_intensity_error=max_intensity,
        max_scaled_sigma_error=max_sigma,validation_seconds=time.perf_counter()-started,
        reference_sha256=hashlib.sha256(args.reference.read_bytes()).hexdigest(),
        method='one combined decode per frame; reference integration uses returned resident pixels')
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(report,flush=True)


if __name__=='__main__':main()
