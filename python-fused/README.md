# Python bridge for fused fixed-mask integration

This optional source-built adapter joins the Rust `dioptrin-io` block reader and
Dioptrin's `Fixed1dPlan`. It does not change the standalone reader package or its
Python wheel. The integration mathematics and float64 weights remain in Dioptrin;
the detector counts, block decoding and source checks remain in dioptrin-io.

The adapter requires Dioptrin commit `72e9b72fa3ccaed19aa4ee09e3cb8bf002590a78`.
The current source dependencies are not published Rust registry packages. In this
directory create an ignored `.cargo/config.toml` pointing to that checkout:

```toml
[patch.crates-io]
dioptrin = { path = "/absolute/path/to/Dioptrin-fused-blocks" }
```

Build from this directory so Cargo metadata and Maturin both find the patch:

```sh
maturin build --release --interpreter /path/to/python --out /tmp/fused-wheels
python -m pip install /tmp/fused-wheels/dioptrin_io_fused-*.whl
python -m pytest tests
```

The Python module is `dioptrin_io_fused`. No package publication is implied by a
local build. Distribution through normal release wheels needs a separate paired
release/packaging decision; Qt can detect an absent adapter and retain its existing
backend. The core reader and its ordinary Python distribution stay independent.

`FusedHdf5Integrator(path, dataset, poni, n_points, mask=..., unit="2th_deg",
polarization_factor=..., supersampling=1, num_workers=8)` admits one immutable
self-contained HDF5 snapshot, then builds a reusable fixed-mask plan. The PONI dict
contains `pixel1`, `pixel2`, `distance`, `poni1`, `poni2`, `rot1`, `rot2`, `rot3`,
and `wavelength`. Mask arrays are contiguous uint8 in calibrated bottom-up order.
Native samples remain uint16/uint32; weights, sums and Poisson terms use float64.

`integrate1d(frame, errors=False, expected_image=None)` reads the compressed frame
once and splits blocks among workers. It maps stored rows into Dioptas's flipped
image order, merges raw histograms, and normalizes once. It releases the GIL during
native work. An optional contiguous float64 `expected_image` makes every source
count match the captured image before accepting a result. A mismatch raises
`SourceImageMismatch`; a decode/source failure raises `OSError`. Partial results
are never returned. Native values are not silently clipped or reduced in precision.

Admission failures raise `UnsupportedFusedLayout`. Dynamic masks, spread statistics,
and 2D integration are not supported by this plan. Like the existing Dioptrin
Python integrator, each adapter object must stay on its owning Python thread.
Recreate it when geometry, mask, corrections, bins, input file or dataset change.

`load_integrate1d(frame, errors=False)` performs one block decode and returns a
pair `(pixels, result)`. Pixels preserve original stored axes, unsigned dtype,
reserved values and counts; both the returned NumPy view and its base are
read-only and own their memory independently of future frames. The caller maps
rows for display. Integration uses float64 throughout and returns calibrated
bottom-up geometry, with cached fixed-mask normalization applied once.

`set_source(path, dataset)` changes the immutable source without rebuilding the
fixed geometry plan when shape/settings stay compatible. It rejects unsupported
sources or a shape mismatch before replacing the current reader. Private worker
pixel vectors and the final display array are consumer storage beyond the
decoder payload budget; no complete decoded scan is cached. Allocation failures
are reported rather than publishing a partial image or pattern.

**Qt integration:** the combined image loader invokes this operation on the native
owner thread, retaining the source/header selection and returning both display
pixels and pattern. Prepared-result reuse requires the immutable image/mask and
matching geometry/corrections/bins/statistics. Edits invalidate reuse and process
the resident image; there is no automatic fused reread. Dynamic-mask and batch/map
routes are unchanged. The owner is never waited on behind an active bulk job.

The historical reread adapter measured 6.5 ms for array integration versus 10.5 ms
for fused rereading because the image was already decoded for display. The new
combined loader avoids that duplicate work. Full Qt/controller timing shows about
20% lower open-to-pattern latency on the tested M4 Pro/Eiger 9M setup, including
25 ms debounce; physical monitor/GPU presentation is not measured. See
`docs/benchmarks/2026-11-07-qt-combined/README.md` for raw observations, source
provenance, one-off setup, numerical and count validation, and qualification scope.
