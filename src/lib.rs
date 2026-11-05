//! Native detector-file input, independent of integration and Python.
//!
//! Format backends are optional. The initial `hdf5` feature provides the
//! validated unsigned-count reader adapted from PhaseSmith. It preserves raw
//! counts and stored orientation; geometry, masks and normalization belong to
//! consumers. Parallel raw-chunk decoding is a subsequent performance step.

#[cfg(feature = "hdf5")]
pub mod hdf5;
