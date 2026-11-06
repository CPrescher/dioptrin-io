//! Native detector-file input, independent of integration and Python.
//!
//! Optional HDF5, TIFF/BigTIFF, CBF and EDF backends expose bounded original
//! detector samples. Native buffers retain integer/floating types and stored
//! orientation. Geometry, masks, corrections and normalization belong to consumers.

pub mod discovery;

mod pixels;
pub use pixels::{PixelBuffer, PixelType};

#[cfg(any(
    feature = "hdf5",
    feature = "tiff",
    feature = "cbf",
    feature = "edf",
    feature = "native",
    feature = "jpeg",
    feature = "jpeg2000"
))]
mod source;

#[cfg(any(feature = "cbf", feature = "edf", feature = "native"))]
mod binary;
#[cfg(feature = "bruker")]
pub mod bruker;
#[cfg(feature = "cbf")]
pub mod cbf;
#[cfg(any(
    feature = "bruker",
    feature = "oxford",
    feature = "raxis",
    feature = "fit2d",
    feature = "kcd",
    feature = "mpa"
))]
#[path = "bruker/common.rs"]
mod detector_common;
#[cfg(feature = "containers")]
pub mod dm3;
#[cfg(feature = "edf")]
pub mod edf;
#[cfg(feature = "containers")]
pub mod mrc;
#[cfg(feature = "native")]
pub mod native;
#[cfg(feature = "oxford")]
pub mod oxford;
#[cfg(feature = "raxis")]
pub mod raxis;
#[cfg(feature = "containers")]
pub mod spe;
#[cfg(feature = "edf")]
mod transport;
#[cfg(feature = "wrappers")]
pub mod wrapped;
#[cfg(feature = "xsd")]
pub mod xsd;

#[cfg(feature = "hdf5")]
pub mod hdf5;

#[cfg(feature = "tiff")]
pub mod detector_tiff;
#[cfg(feature = "jpeg")]
pub mod jpeg;
#[cfg(feature = "jpeg2000")]
pub mod jpeg2000;
#[cfg(any(feature = "jpeg", feature = "jpeg2000"))]
mod raster;
#[cfg(feature = "tiff")]
pub mod tiff;

#[cfg(feature = "fit2d")]
pub mod fit2d;
#[cfg(feature = "fixed")]
pub mod ge;
#[cfg(feature = "kcd")]
pub mod kcd;
#[cfg(feature = "mpa")]
pub mod mpa;
#[cfg(feature = "fixed")]
pub mod pixi;

#[cfg(feature = "esperanto")]
pub mod esperanto;

#[cfg(feature = "mar345")]
pub mod mar345;
#[cfg(feature = "sparse")]
pub use hdf5::sparse;

#[cfg(any(feature = "cbf", feature = "tiff"))]
pub mod pilatus;

#[cfg(feature = "xcalibur")]
pub mod xcalibur;

#[cfg(feature = "wrappers")]
pub mod navigation;
#[cfg(feature = "wrappers")]
pub mod series;
#[cfg(any(
    feature = "hdf5",
    feature = "tiff",
    feature = "cbf",
    feature = "edf",
    feature = "native",
    feature = "jpeg",
    feature = "jpeg2000"
))]
pub use source::SourceBinding;

#[cfg(feature = "chiplot")]
pub mod chiplot;
#[cfg(feature = "columnfile")]
pub mod columnfile;
