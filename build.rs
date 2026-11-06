fn main() {
    #[cfg(feature = "tiff")]
    {
        for path in [
            "native/tiff-thunderscan/wrapper.c",
            "native/tiff-thunderscan/tiffiop.h",
            "native/tiff-thunderscan/tif_thunder.c",
        ] {
            println!("cargo:rerun-if-changed={path}");
        }
        cc::Build::new()
            .file("native/tiff-thunderscan/wrapper.c")
            .include("native/tiff-thunderscan")
            .compile("dioptrin_thunderscan");
    }
}
