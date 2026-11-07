//! Standalone development adapter; release wheels bundle this in dioptrin-io.
mod bridge;
use pyo3::prelude::*;

#[pymodule]
fn dioptrin_io_fused(m: &Bound<'_, PyModule>) -> PyResult<()> {
    bridge::register(m)
}
