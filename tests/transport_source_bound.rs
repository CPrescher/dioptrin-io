#![cfg(all(feature = "wrappers", feature = "hdf5"))]
//! `Limits::source_bytes` bounds staged wrapper output and whole-file decoders.
//! HDF5 selections read on demand, so an unwrapped HDF5 file larger than it is
//! still admitted through `Input::open`.
use dioptrin_io::{
    PixelBuffer,
    hdf5::typed::{Options, Selection},
    native, tiff,
    wrapped::{Format, FrameBuffer, Input, Limits, Reader},
};
use std::io::Write;

fn small(path: &std::path::Path) -> Limits {
    let bytes = std::fs::metadata(path).unwrap().len();
    Limits {
        source_bytes: bytes / 2,
        ..Limits::default()
    }
}
fn message<T>(result: Result<T, dioptrin_io::wrapped::Error>) -> String {
    match result {
        Ok(_) => panic!("expected the source bound to reject this read"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn unwrapped_hdf5_ignores_the_whole_file_bound_but_other_decoders_keep_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frames.h5");
    let values: Vec<u16> = (0..3 * 4 * 5).map(|i| i as u16 * 7).collect();
    {
        let file = hdf5_backend::File::create(&path).unwrap();
        file.new_dataset::<u16>()
            .shape((3, 4, 5))
            .create("data")
            .unwrap()
            .write_raw(&values)
            .unwrap();
    }
    let limits = small(&path);

    let mut reader = Input::open(&path, limits)
        .unwrap()
        .read_hdf5(Selection::Dataset("/data"), Options::default(), None)
        .unwrap();
    assert_eq!(reader.frames(), 3);
    let mut buffer = FrameBuffer::default();
    for frame in 0..3 {
        reader.read_frame_into(frame, &mut buffer).unwrap();
        assert_eq!(
            buffer.pixels(),
            Some(&PixelBuffer::U16(
                values[frame * 20..frame * 20 + 20].to_vec()
            ))
        );
    }

    // The same oversized file handed to a whole-file decoder is still refused
    // before decoding, through Input and through Reader::open alike.
    let format = Format::Tiff;
    assert!(
        message(Input::open(&path, limits).unwrap().read(
            format,
            native::Limits::default(),
            tiff::Limits::default()
        ))
        .contains("source exceeds file size limit")
    );
    assert!(
        message(Reader::open(
            &path,
            format,
            limits,
            native::Limits::default(),
            tiff::Limits::default()
        ))
        .contains("source exceeds file size limit")
    );

    // Wrapped HDF5 is decoded to disk first; that copy stays bounded.
    let wrapped = dir.path().join("frames.h5.gz");
    let mut encoder = flate2::write::GzEncoder::new(
        std::fs::File::create(&wrapped).unwrap(),
        flate2::Compression::fast(),
    );
    encoder.write_all(&std::fs::read(&path).unwrap()).unwrap();
    encoder.finish().unwrap();
    assert!(
        message(Input::open(&wrapped, limits))
            .contains("wrapper output exceeds staged source limit")
    );
    let mut staged = Input::open(&wrapped, Limits::default())
        .unwrap()
        .read_hdf5(Selection::Dataset("/data"), Options::default(), None)
        .unwrap();
    staged.read_frame_into(2, &mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::U16(values[40..60].to_vec()))
    );

    // Sparse HDF5 is read on demand too, including through discovery.
    #[cfg(feature = "sparse")]
    {
        let sparse = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/sparse/float-events.h5");
        let reader = Input::open(&sparse, small(&sparse))
            .unwrap()
            .read_discovered(native::Limits::default(), tiff::Limits::default())
            .unwrap();
        assert!(reader.sparse_reader().is_some());
    }

    // Captured streams keep rejecting oversized plain input when captured.
    let bytes = std::fs::read(&path).unwrap();
    assert!(Input::from_bytes(&bytes, Some(dir.path()), limits).is_err());
}
