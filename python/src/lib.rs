//! Python adapter; decoding and source validation belong to the Rust readers.
use dioptrin_io::{PixelBuffer, wrapped};
use numpy::PyArray1;
use pyo3::{
    exceptions::{PyIndexError, PyOSError, PyRuntimeError, PyValueError},
    prelude::*,
    types::{PyBytes, PyDict},
};
use std::{path::PathBuf, sync::Mutex};
mod headers;

fn read_error(error: impl std::fmt::Display) -> PyErr {
    PyOSError::new_err(error.to_string())
}

/// One decoded frame, parsed header and source metadata, independent of the reader.
#[pyclass(frozen, module = "dioptrin_io")]
struct Image {
    #[pyo3(get)]
    data: Py<PyAny>,
    #[pyo3(get)]
    header: Py<PyDict>,
    #[pyo3(get)]
    metadata: Py<PyDict>,
}

#[pyclass(module = "dioptrin_io")]
struct Reader {
    inner: Mutex<Option<wrapped::Reader>>,
}

#[pyclass(module = "dioptrin_io._native")]
struct FrameIterator {
    reader: Py<Reader>,
    index: usize,
    frames: usize,
}

#[pymethods]
impl FrameIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Image>> {
        if self.index >= self.frames {
            return Ok(None);
        }
        let frame = isize::try_from(self.index)
            .map_err(|_| PyRuntimeError::new_err("frame index exceeds Python index range"))?;
        let image = self.reader.borrow(py).read(py, frame)?;
        self.index += 1;
        Ok(Some(image))
    }
}

impl Reader {
    // Always acquire this mutex while detached: a thread holding it may need
    // to reattach before another Python thread can make progress.
    fn with_reader<T>(
        &self,
        operation: impl FnOnce(&mut wrapped::Reader) -> PyResult<T>,
    ) -> PyResult<T> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("reader lock poisoned"))?;
        operation(
            guard
                .as_mut()
                .ok_or_else(|| PyValueError::new_err("reader is closed"))?,
        )
    }
}

#[pymethods]
impl Reader {
    #[new]
    #[pyo3(signature = (path, *, dataset=None))]
    fn new(py: Python<'_>, path: &Bound<'_, PyAny>, dataset: Option<String>) -> PyResult<Self> {
        // PyO3's PathBuf extractor does not accept bytes on every platform.
        // fsdecode also preserves Unix non-UTF8 names via surrogateescape.
        let path: PathBuf = py
            .import("os")?
            .call_method1("fsdecode", (path,))?
            .extract()?;
        let reader = py.detach(move || {
            let input = wrapped::Input::open(path, Default::default()).map_err(read_error)?;
            if let Some(dataset) = dataset {
                #[cfg(feature = "hdf5")]
                return input
                    .read_hdf5(
                        dioptrin_io::hdf5::typed::Selection::Dataset(&dataset),
                        Default::default(),
                        None,
                    )
                    .map_err(read_error);
                #[cfg(not(feature = "hdf5"))]
                {
                    let _ = dataset;
                    return Err(PyValueError::new_err(
                        "HDF5 support is disabled in this build",
                    ));
                }
            }
            input
                .read_discovered(Default::default(), Default::default())
                .map_err(read_error)
        })?;
        Ok(Self {
            inner: Mutex::new(Some(reader)),
        })
    }

    fn __len__(&self, py: Python<'_>) -> PyResult<usize> {
        py.detach(|| self.with_reader(|reader| Ok(reader.frames())))
    }

    fn __getitem__(&self, py: Python<'_>, frame: isize) -> PyResult<Image> {
        self.read(py, frame)
    }

    fn __iter__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<FrameIterator> {
        let frames = slf.__len__(py)?;
        Ok(FrameIterator {
            reader: slf.into(),
            index: 0,
            frames,
        })
    }

    /// Number of frames detected when the file was opened.
    #[getter]
    fn nframes(&self, py: Python<'_>) -> PyResult<usize> {
        self.__len__(py)
    }

    #[getter]
    fn closed(&self, py: Python<'_>) -> PyResult<bool> {
        py.detach(|| {
            self.inner
                .lock()
                .map(|guard| guard.is_none())
                .map_err(|_| PyRuntimeError::new_err("reader lock poisoned"))
        })
    }

    /// Read one Image with independent NumPy data, header and source metadata.
    #[pyo3(signature = (frame=0))]
    fn read(&self, py: Python<'_>, frame: isize) -> PyResult<Image> {
        let (buffer, metadata) = py.detach(|| {
            self.with_reader(|reader| {
                let index = frame_index(frame, reader.frames())?;
                let mut buffer = wrapped::FrameBuffer::default();
                reader
                    .read_frame_into(index, &mut buffer)
                    .map_err(read_error)?;
                let metadata = source_snapshot(reader, index)?;
                reader.check_sources().map_err(read_error)?;
                Ok((buffer, metadata))
            })
        })?;
        let pixels = buffer
            .pixels()
            .ok_or_else(|| PyRuntimeError::new_err("read returned no pixels"))?;
        let shape = buffer
            .sample_shape()
            .ok_or_else(|| PyRuntimeError::new_err("read returned no shape"))?;
        if shape.iter().try_fold(1usize, |a, &b| a.checked_mul(b)) != Some(pixels.len()) {
            return Err(PyRuntimeError::new_err("pixel count does not match shape"));
        }
        let array = to_numpy(py, pixels)?;
        Ok(Image {
            data: array.call_method1("reshape", (shape,))?.unbind(),
            header: metadata.header_fields.to_dict(py)?.unbind(),
            metadata: metadata_dict(py, metadata)?.unbind(),
        })
    }

    /// Source metadata; details is a diagnostic Rust representation, not a schema.
    #[pyo3(signature = (frame=0))]
    fn metadata<'py>(&self, py: Python<'py>, frame: isize) -> PyResult<Bound<'py, PyDict>> {
        let metadata = py.detach(|| {
            self.with_reader(|reader| {
                let index = frame_index(frame, reader.frames())?;
                reader.check_sources().map_err(read_error)?;
                let info = source_snapshot(reader, index)?;
                reader.check_sources().map_err(read_error)?;
                Ok(info)
            })
        })?;
        metadata_dict(py, metadata)
    }

    /// Parsed source header fields without decoding image pixels.
    #[pyo3(signature = (frame=0))]
    fn header<'py>(&self, py: Python<'py>, frame: isize) -> PyResult<Bound<'py, PyDict>> {
        let fields = py.detach(|| {
            self.with_reader(|reader| {
                let index = frame_index(frame, reader.frames())?;
                Ok(source_snapshot(reader, index)?.header_fields)
            })
        })?;
        fields.to_dict(py)
    }

    fn close(&self, py: Python<'_>) -> PyResult<()> {
        py.detach(|| {
            self.inner
                .lock()
                .map_err(|_| PyRuntimeError::new_err("reader lock poisoned"))?
                .take();
            Ok(())
        })
    }

    fn __enter__<'py>(slf: PyRef<'py, Self>, py: Python<'py>) -> PyResult<PyRef<'py, Self>> {
        let reader: &Self = &slf;
        py.detach(|| reader.with_reader(|_| Ok(())))?;
        Ok(slf)
    }

    fn __exit__(
        &self,
        py: Python<'_>,
        _exc_type: &Bound<'_, PyAny>,
        _exc_value: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        self.close(py)
    }
}

fn frame_index(frame: isize, frames: usize) -> PyResult<usize> {
    usize::try_from(frame)
        .ok()
        .filter(|&index| index < frames)
        .ok_or_else(|| PyIndexError::new_err(format!("frame {frame} outside 0..{frames}")))
}

// Copies directly into NumPy-owned storage, without a full-frame float cast.
// Views reinterpret storage bits; they never perform a numerical conversion.
fn to_numpy<'py>(py: Python<'py>, pixels: &PixelBuffer) -> PyResult<Bound<'py, PyAny>> {
    macro_rules! array {
        ($values:expr) => {
            PyArray1::from_slice(py, $values).into_any()
        };
    }
    let array = match pixels {
        PixelBuffer::U8(v) => array!(v),
        PixelBuffer::U16(v) => array!(v),
        PixelBuffer::U32(v) => array!(v),
        PixelBuffer::U64(v) => array!(v),
        PixelBuffer::I8(v) => array!(v),
        PixelBuffer::I16(v) => array!(v),
        PixelBuffer::I32(v) => array!(v),
        PixelBuffer::I64(v) => array!(v),
        PixelBuffer::F32(v) => array!(v),
        PixelBuffer::F64(v) => array!(v),
        PixelBuffer::Bool(v) => return array!(v).call_method1("view", ("bool",)),
        PixelBuffer::F16(v) => return array!(v).call_method1("view", ("float16",)),
        PixelBuffer::C32(v) => {
            let components: &[f32] = bytemuck::cast_slice(v);
            return array!(components).call_method1("view", ("complex64",));
        }
        PixelBuffer::C64(v) => {
            let components: &[f64] = bytemuck::cast_slice(v);
            return array!(components).call_method1("view", ("complex128",));
        }
        PixelBuffer::CI16(v) => {
            let components: &[i16] = bytemuck::cast_slice(v);
            // NumPy has no complex integer scalar. Preserve exact signed pairs.
            let dtype = vec![("real", "=i2"), ("imag", "=i2")];
            return array!(components).call_method1("view", (dtype,));
        }
    };
    Ok(array)
}

struct SourceMetadata {
    format: &'static str,
    details: String,
    header: Option<Vec<u8>>,
    little_endian: Option<bool>,
    source_shape: Option<Vec<usize>>,
    type_descriptor: Option<String>,
    header_fields: headers::Fields,
    additional: headers::Fields,
}

fn source_snapshot(reader: &wrapped::Reader, frame: usize) -> PyResult<SourceMetadata> {
    reader.check_sources().map_err(read_error)?;
    let header = headers::snapshot(reader, frame)?;
    let mut metadata = source_metadata(
        reader
            .metadata(frame)
            .ok_or_else(|| PyRuntimeError::new_err("frame has no source metadata"))?,
    );
    metadata.header_fields = header.fields;
    metadata.additional = header.additional;
    reader.check_sources().map_err(read_error)?;
    Ok(metadata)
}

fn metadata_dict(py: Python<'_>, metadata: SourceMetadata) -> PyResult<Bound<'_, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("format", metadata.format)?;
    dict.set_item("details", metadata.details)?;
    if let Some(header) = metadata.header {
        dict.set_item("raw_header", PyBytes::new(py, &header))?;
    }
    if let Some(little_endian) = metadata.little_endian {
        dict.set_item("little_endian", little_endian)?;
    }
    if let Some(shape) = metadata.source_shape {
        dict.set_item("source_shape", shape)?;
    }
    if let Some(descriptor) = metadata.type_descriptor {
        dict.set_item("type_descriptor", descriptor)?;
    }
    dict.call_method1("update", (metadata.additional.to_dict(py)?,))?;
    Ok(dict)
}

fn source_metadata(metadata: wrapped::Metadata<'_>) -> SourceMetadata {
    use wrapped::Metadata;
    macro_rules! info {
        ($name:expr, $meta:expr) => {
            SourceMetadata {
                format: $name,
                details: format!("{:?}", $meta),
                header: None,
                little_endian: None,
                source_shape: None,
                type_descriptor: None,
                header_fields: headers::Fields::default(),
                additional: headers::Fields::default(),
            }
        };
    }
    match metadata {
        Metadata::Native(m) => SourceMetadata {
            header: Some(m.header.clone()),
            little_endian: Some(m.little_endian),
            source_shape: Some(m.source_shape.clone()),
            ..info!("native", m)
        },
        Metadata::Edf(m) => SourceMetadata {
            header: Some(m.header.clone()),
            little_endian: Some(m.little_endian),
            source_shape: Some(m.source_shape.clone()),
            ..info!("edf", m)
        },
        Metadata::Cbf(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("cbf", m)
        },
        Metadata::Tiff(m) => SourceMetadata {
            little_endian: Some(m.little_endian),
            ..info!("tiff", m)
        },
        Metadata::DetectorTiff(m) => info!("detector_tiff", (m.image, m.header)),
        #[cfg(feature = "hdf5")]
        Metadata::Hdf5(m) => SourceMetadata {
            little_endian: Some(m.little_endian),
            source_shape: Some(m.source_shape.clone()),
            type_descriptor: Some(m.type_descriptor.clone()),
            ..info!("hdf5", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Mar345(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("mar345", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Esperanto(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("esperanto", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Fit2d(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("fit2d", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Kcd(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("kcd", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Mpa(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("mpa", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Ge(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("ge", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Pixi(m) => info!("pixi", m),
        #[cfg(feature = "formats")]
        Metadata::Jpeg(m) => info!("jpeg", m),
        #[cfg(feature = "formats")]
        Metadata::Jpeg2000(m) => info!("jpeg2000", m),
        #[cfg(feature = "formats")]
        Metadata::Spe(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("spe", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Mrc(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("mrc", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Dm3(m) => SourceMetadata {
            header: Some(m.header.to_vec()),
            ..info!("dm3", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Bruker(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("bruker", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Oxford(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("oxford", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Raxis(m) => SourceMetadata {
            header: Some(m.header.clone()),
            ..info!("raxis", m)
        },
        #[cfg(feature = "formats")]
        Metadata::Xsd(m) => info!("xsd", m),
    }
}

#[pymodule(gil_used = true)]
mod _native {
    #[pymodule_export]
    use super::{Image, Reader};

    #[pymodule_init]
    fn init(module: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
        use pyo3::types::PyModuleMethods;
        module.add("__version__", env!("CARGO_PKG_VERSION"))
    }
}
