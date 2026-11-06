//! Exercise mutation when permitted; verify Windows HDF5 locks otherwise.
//! Never swallow unexpected errors or accept a partially modified source.
pub fn mutate_source(path: &std::path::Path, change: impl FnOnce() -> std::io::Result<()>) -> bool {
    let original = std::fs::read(path).unwrap();
    match change() {
        Ok(()) => true,
        Err(error) => {
            assert!(
                cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33)),
                "source mutation failed unexpectedly: {error}"
            );
            assert_eq!(
                std::fs::read(path).unwrap(),
                original,
                "blocked mutation altered original source"
            );
            false
        }
    }
}
