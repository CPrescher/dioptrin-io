//! Native detector-file input, independent of integration and Python.
//!
//! Optional HDF5, TIFF/BigTIFF, CBF and EDF backends expose bounded original
//! detector samples. Native buffers retain integer/floating types and stored
//! orientation. Geometry, masks, corrections and normalization belong to consumers.

mod pixels;
pub use pixels::{PixelBuffer, PixelType};

#[cfg(any(
    feature = "hdf5",
    feature = "tiff",
    feature = "cbf",
    feature = "edf",
    feature = "native"
))]
mod source;

#[cfg(any(feature = "cbf", feature = "edf", feature = "native"))]
mod binary;
#[cfg(feature = "cbf")]
pub mod cbf;
#[cfg(feature = "edf")]
pub mod edf;
#[cfg(feature = "native")]
pub mod native;
#[cfg(feature = "edf")]
mod transport;
#[cfg(feature = "wrappers")]
pub mod wrapped;
#[cfg(feature = "xsd")]
pub mod xsd;

#[cfg(feature = "hdf5")]
pub mod hdf5;

#[cfg(feature = "tiff")]
pub mod tiff;
