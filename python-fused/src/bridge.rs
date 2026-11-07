//! Optional Python application bridge; detector I/O and integration remain in their crates.
use dioptrin::{
    geometry::Geometry,
    integration::{Fixed1dPlan, InputOrder, IntegrationMethod, Integrator, RadialUnit},
    mask::Mask,
    poni::Poni,
};
use dioptrin_io::hdf5::{
    Limits,
    raw::{ParallelBlockBuffer, PipelineOptions, PixelBlock, RawReader},
};
use dioptrin_io::{PixelBuffer, PixelType};
use numpy::{PyArray1, PyArrayMethods, PyReadonlyArray2, PyUntypedArrayMethods};
use pyo3::{
    exceptions::{PyOSError, PyValueError},
    prelude::*,
    types::PyDict,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

pyo3::create_exception!(dioptrin_io_fused, UnsupportedFusedLayout, PyValueError);
pyo3::create_exception!(dioptrin_io_fused, SourceImageMismatch, PyValueError);

#[pyclass(module = "dioptrin_io_fused", frozen)]
pub struct FusedResult {
    #[pyo3(get)]
    radial: Vec<f64>,
    #[pyo3(get)]
    intensity: Vec<f64>,
    #[pyo3(get)]
    errors: Option<Vec<f64>>,
}

/// Fixed-mask float64 plan and reusable, immutable-source HDF5 reader.
#[pyclass(module = "dioptrin_io_fused", unsendable)]
pub struct FusedHdf5Integrator {
    plan: Fixed1dPlan,
    reader: RawReader,
    buffer: ParallelBlockBuffer,
    workers: usize,
}

#[pymethods]
impl FusedHdf5Integrator {
    #[new]
    #[pyo3(signature = (path, dataset, poni, n_points, *, mask=None, polarization_factor=None, unit="2th_deg", supersampling=1, num_workers=8))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        path: PathBuf,
        dataset: String,
        poni: &Bound<'_, PyDict>,
        n_points: usize,
        mask: Option<PyReadonlyArray2<'_, u8>>,
        polarization_factor: Option<f64>,
        unit: &str,
        supersampling: usize,
        num_workers: usize,
    ) -> PyResult<Self> {
        if !(1..=256).contains(&num_workers) || n_points == 0 || supersampling == 0 {
            return Err(PyValueError::new_err(
                "positive bins/supersampling and 1..256 workers required",
            ));
        }
        let mut p = Poni::new();
        for (name, slot) in [
            ("pixel1", &mut p.pixel1),
            ("pixel2", &mut p.pixel2),
            ("distance", &mut p.distance),
            ("poni1", &mut p.poni1),
            ("poni2", &mut p.poni2),
            ("rot1", &mut p.rot1),
            ("rot2", &mut p.rot2),
            ("rot3", &mut p.rot3),
            ("wavelength", &mut p.wavelength),
        ] {
            *slot = poni
                .get_item(name)?
                .ok_or_else(|| PyValueError::new_err(format!("missing {name}")))?
                .extract()?;
            if !slot.is_finite() {
                return Err(PyValueError::new_err("finite geometry required"));
            }
        }
        if p.pixel1 <= 0. || p.pixel2 <= 0. || p.distance <= 0. || p.wavelength <= 0. {
            return Err(PyValueError::new_err(
                "positive pixels, distance and wavelength required",
            ));
        }
        let unit = match unit {
            "2th_deg" => RadialUnit::TthDeg,
            "2th_rad" => RadialUnit::TthRad,
            "q_A^-1" => RadialUnit::QAngstrom,
            "q_nm^-1" => RadialUnit::QNm,
            _ => return Err(PyValueError::new_err("unsupported fused radial unit")),
        };
        let mask = mask
            .map(|m| -> PyResult<_> {
                Ok(Mask {
                    dim1: m.shape()[0],
                    dim2: m.shape()[1],
                    data: m
                        .as_slice()
                        .map_err(|_| PyValueError::new_err("mask must be C contiguous"))?
                        .to_vec(),
                })
            })
            .transpose()?;
        py.detach(move || {
            // Only admission failure permits the application to choose its existing reader.
            let reader = RawReader::open_snapshot(path, &dataset, Limits::default())
                .map_err(|e| UnsupportedFusedLayout::new_err(e.to_string()))?;
            let [_, height, width] = reader.shape();
            let mut ai = Integrator::new(Geometry::new(height, width, p))
                .with_radial_unit(unit)
                .with_method(if supersampling > 1 {
                    IntegrationMethod::Supersampled {
                        n_ss: supersampling,
                    }
                } else {
                    IntegrationMethod::PixelSplit
                });
            ai.set_polarization_factor(polarization_factor);
            if let Some(mask) = mask {
                ai.set_mask(mask);
            }
            let plan = ai
                .prepare_fixed1d((height, width), n_points)
                .map_err(PyValueError::new_err)?;
            Ok(Self {
                plan,
                reader,
                buffer: ParallelBlockBuffer::default(),
                workers: num_workers,
            })
        })
    }

    /// Integrate ONE frame with parallel block decoding; source failures propagate.
    /// Optional expected_image is calibrated bottom-up float64 image data. Every
    /// decoded count is compared exactly before a result is accepted. It must not
    /// be mutated concurrently while native workers borrow it with the GIL released.
    #[pyo3(signature = (frame, *, errors=false, expected_image=None))]
    fn integrate1d(
        &mut self,
        py: Python<'_>,
        frame: usize,
        errors: bool,
        expected_image: Option<PyReadonlyArray2<'_, f64>>,
    ) -> PyResult<FusedResult> {
        let shape = self.plan.shape();
        let expected = expected_image
            .as_ref()
            .map(|image| {
                if image.shape() != [shape.0, shape.1] {
                    return Err(PyValueError::new_err("expected image shape mismatch"));
                }
                image
                    .as_slice()
                    .map_err(|_| PyValueError::new_err("expected image must be C contiguous"))
            })
            .transpose()?;
        py.detach(|| {
            let mismatch = AtomicBool::new(false);
            let result = self.reader.fold_frame_blocks(
                frame,
                &mut self.buffer,
                PipelineOptions {
                    workers: self.workers,
                    memory_bytes: 512 * 1024 * 1024,
                },
                |_, range| {
                    let mut worker = self.plan.worker(errors, InputOrder::FlipVertical);
                    worker
                        .begin_range(range)
                        .map_err(dioptrin_io::hdf5::Error::from)?;
                    Ok(worker)
                },
                |worker, offset, block| {
                    let valid = expected.is_none_or(|image| match block {
                        PixelBlock::U16(v) => matches_image(v, offset, image, shape),
                        PixelBlock::U32(v) => matches_image(v, offset, image, shape),
                    });
                    if !valid {
                        mismatch.store(true, Ordering::Relaxed);
                        return Err(
                            String::from("source image differs from captured pixels").into()
                        );
                    }
                    match block {
                        PixelBlock::U16(v) => worker.push_u16_block(offset, v),
                        PixelBlock::U32(v) => worker.push_u32_block(offset, v),
                    }
                    .map_err(Into::into)
                },
            );
            let mut states = result.map_err(|e| {
                if mismatch.load(Ordering::Relaxed) {
                    SourceImageMismatch::new_err(e.to_string())
                } else {
                    PyOSError::new_err(e.to_string())
                }
            })?;
            let result = self
                .plan
                .finish_ranges(&mut states)
                .map_err(PyValueError::new_err)?;
            Ok(FusedResult {
                radial: result.radial,
                intensity: result.intensity,
                errors: result.errors,
            })
        })
    }
    /// Replace only the immutable source; retain geometry, mask and normalization.
    fn set_source(&mut self, py: Python<'_>, path: PathBuf, dataset: String) -> PyResult<()> {
        py.detach(|| {
            let reader = RawReader::open_snapshot(path, &dataset, Limits::default())
                .map_err(|e| UnsupportedFusedLayout::new_err(e.to_string()))?;
            let [_, height, width] = reader.shape();
            if (height, width) != self.plan.shape() {
                return Err(UnsupportedFusedLayout::new_err(
                    "source shape differs from fixed plan",
                ));
            }
            self.reader = reader;
            Ok(())
        })
    }

    /// Decode once for BOTH display and integration. Returned stored-order pixels
    /// own their memory, preserve dtype/counts and are immutable for snapshot reuse.
    #[pyo3(signature = (frame, *, errors=false))]
    fn load_integrate1d(
        &mut self,
        py: Python<'_>,
        frame: usize,
        errors: bool,
    ) -> PyResult<(Py<PyAny>, FusedResult)> {
        let (pixels, result) = py.detach(|| {
            let kind = self.reader.pixel_type();
            let mut states = self
                .reader
                .fold_frame_blocks(
                    frame,
                    &mut self.buffer,
                    PipelineOptions {
                        workers: self.workers,
                        memory_bytes: 512 * 1024 * 1024,
                    },
                    |_, range| {
                        let mut worker = self.plan.worker(errors, InputOrder::FlipVertical);
                        worker
                            .begin_range(range.clone())
                            .map_err(dioptrin_io::hdf5::Error::from)?;
                        let pixels = reserve_pixels(kind, range.len())?;
                        Ok((worker, pixels))
                    },
                    |(worker, pixels), offset, block| {
                        // Private range-owned vectors need no shared mutable pointers.
                        match (pixels, block) {
                            (PixelBuffer::U16(out), PixelBlock::U16(v)) => {
                                out.extend_from_slice(v);
                                worker.push_u16_block(offset, v)
                            }
                            (PixelBuffer::U32(out), PixelBlock::U32(v)) => {
                                out.extend_from_slice(v);
                                worker.push_u32_block(offset, v)
                            }
                            _ => unreachable!("admitted dtype is stable"),
                        }
                        .map_err(Into::into)
                    },
                )
                .map_err(|e| PyOSError::new_err(e.to_string()))?;
            let total = self.plan.shape().0 * self.plan.shape().1;
            let mut pixels =
                reserve_pixels(kind, total).map_err(|e| PyValueError::new_err(e.to_string()))?;
            let mut workers = Vec::with_capacity(states.len());
            for (worker, part) in states.drain(..) {
                match (&mut pixels, part) {
                    (PixelBuffer::U16(out), PixelBuffer::U16(v)) => out.extend(v),
                    (PixelBuffer::U32(out), PixelBuffer::U32(v)) => out.extend(v),
                    _ => unreachable!(),
                }
                workers.push(worker);
            }
            let result = self
                .plan
                .finish_ranges(&mut workers)
                .map_err(PyValueError::new_err)?;
            Ok::<_, PyErr>((pixels, result))
        })?;
        let (height, width) = self.plan.shape();
        let array = match pixels {
            PixelBuffer::U16(v) => {
                let root = PyArray1::from_vec(py, v);
                root.call_method1("setflags", (false,))?;
                root.reshape([height, width])?.into_any()
            }
            PixelBuffer::U32(v) => {
                let root = PyArray1::from_vec(py, v);
                root.call_method1("setflags", (false,))?;
                root.reshape([height, width])?.into_any()
            }
            _ => unreachable!(),
        };
        array.call_method1("setflags", (false,))?;
        Ok((
            array.unbind(),
            FusedResult {
                radial: result.radial,
                intensity: result.intensity,
                errors: result.errors,
            },
        ))
    }

    #[getter]
    fn shape(&self) -> (usize, usize) {
        self.plan.shape()
    }
    #[getter]
    fn num_workers(&self) -> usize {
        self.workers
    }
}

fn reserve_pixels(kind: PixelType, count: usize) -> Result<PixelBuffer, dioptrin_io::hdf5::Error> {
    match kind {
        PixelType::U16 => {
            let mut pixels = Vec::new();
            pixels.try_reserve_exact(count).map_err(|e| e.to_string())?;
            Ok(PixelBuffer::U16(pixels))
        }
        PixelType::U32 => {
            let mut pixels = Vec::new();
            pixels.try_reserve_exact(count).map_err(|e| e.to_string())?;
            Ok(PixelBuffer::U32(pixels))
        }
        _ => unreachable!("raw admission allows only unsigned 16/32-bit counts"),
    }
}

fn matches_image<P: Copy + Into<f64>>(
    pixels: &[P],
    offset: usize,
    expected: &[f64],
    shape: (usize, usize),
) -> bool {
    let mut local = 0;
    while local < pixels.len() {
        let source = offset + local;
        let count = (shape.1 - source % shape.1).min(pixels.len() - local);
        let dest = (shape.0 - 1 - source / shape.1) * shape.1 + source % shape.1;
        if pixels[local..local + count]
            .iter()
            .zip(&expected[dest..dest + count])
            .any(|(&a, &b)| a.into() != b)
        {
            return false;
        }
        local += count;
    }
    true
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<FusedHdf5Integrator>()?;
    m.add_class::<FusedResult>()?;
    m.add(
        "UnsupportedFusedLayout",
        m.py().get_type::<UnsupportedFusedLayout>(),
    )?;
    m.add(
        "SourceImageMismatch",
        m.py().get_type::<SourceImageMismatch>(),
    )?;
    Ok(())
}
