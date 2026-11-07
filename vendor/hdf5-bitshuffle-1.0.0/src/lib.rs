//! This is a Rust binding of bitshuffle HDF5 filter.
//! Some HDF5 files use this not standard plugin
//! [bitshuffle](https://github.com/kiyo-masui/bitshuffle)
//! (mainly produced by the Dectris Eiger detectors).
//! This crate makes a binding for Rust.
//!
//! To use this plugin just call in your code before using any HDF5 API:
//! ```rust
//! use hdf5_bitshuffle::register_bitshuffle_plugin;
//!
//! fn main() {
//!      register_bitshuffle_plugin();
//! }
//! ```
//! Or, if you want more control, you can call it manually:
//! ```rust
//! use std::sync::Once;
//! use hdf5_bitshuffle::bshuf_register_h5filter;
//!
//! static REGISTER_BITSHUFFLE: Once = Once::new();
//!
//! fn main() {
//!     unsafe {
//!         REGISTER_BITSHUFFLE.call_once(|| {
//!                 if bshuf_register_h5filter() < 0 {
//!                     panic!("Could not register bitshuffle plugin for HDF5");
//!                 }
//!             });
//!         }
//! }
//! ```
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::sync::Once;

extern "C" {
    /// Unsafe function to registers the bitshuffle plugin.
    pub fn bshuf_register_h5filter() -> std::os::raw::c_int;
}

static REGISTER_BITSHUFFLE: Once = Once::new();

/// Convenient safe rust function to register the bitshuffle plugin.
/// It just panics if it fails.
pub fn register_bitshuffle_plugin() {
    REGISTER_BITSHUFFLE.call_once(|| {
        if unsafe { bshuf_register_h5filter() < 0 } {
            panic!("Could not register bitshuffle plugin for HDF5");
        }
    });
}
