//! Exercise mutation when permitted; distinguish Windows HDF5 lock failures
//! that leave bytes unchanged from failed writes that already truncated a source.
//! The latter must invalidate the reader just like a successful mutation.
pub fn mutate_source(path: &std::path::Path, change: impl FnOnce() -> std::io::Result<()>) -> bool {
    let original = std::fs::read(path).unwrap();
    match change() {
        Ok(()) => true,
        Err(error) => {
            assert!(
                cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33)),
                "source mutation failed unexpectedly: {error}"
            );
            // Windows can truncate through CreateFile before the subsequent
            // locked WriteFile fails. An error therefore does not prove that
            // the source stayed unchanged: require invalidation in that case.
            std::fs::read(path).unwrap() != original
        }
    }
}
