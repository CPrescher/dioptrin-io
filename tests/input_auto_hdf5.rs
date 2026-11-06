#![cfg(all(feature = "wrappers", feature = "hdf5"))]
use dioptrin_io::{
    hdf5::typed::{Options, Selection},
    native, tiff,
    wrapped::{FrameBuffer, Input, Limits},
};
#[test]
fn automatic_hdf5_conventions_reach_the_same_source_arrays_as_explicit_selection() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/hdf5-native");
    for (name, selection) in [
        ("lima.h5", Selection::Lima),
        ("lambda.h5", Selection::Lambda),
    ] {
        let bytes = std::fs::read(root.join(name)).unwrap();
        let mut automatic = Input::from_bytes(&bytes, None, Limits::default())
            .unwrap()
            .read_discovered(native::Limits::default(), tiff::Limits::default())
            .unwrap();
        let mut explicit = Input::from_bytes(&bytes, None, Limits::default())
            .unwrap()
            .read_hdf5(selection, Options::default(), None)
            .unwrap();
        assert_eq!(automatic.frames(), explicit.frames());
        let mut a = FrameBuffer::default();
        let mut b = FrameBuffer::default();
        for i in 0..automatic.frames() {
            automatic.read_frame_into(i, &mut a).unwrap();
            explicit.read_frame_into(i, &mut b).unwrap();
            assert_eq!(a.shape(), b.shape());
            assert_eq!(a.pixels(), b.pixels());
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not-an-extension");
    let values = vec![0u32, 1, 2, u32::MAX, 32768, 65535, 5, 6];
    {
        let f = hdf5_backend::File::create(&path).unwrap();
        let entry = f.create_group("entry").unwrap();
        let data = entry
            .new_dataset::<u32>()
            .shape((2, 2, 2))
            .create("data")
            .unwrap();
        data.write_raw(&values).unwrap();
    }
    let bytes = std::fs::read(&path).unwrap();
    let mut reader = Input::from_bytes(&bytes, None, Limits::default())
        .unwrap()
        .read_discovered(native::Limits::default(), tiff::Limits::default())
        .unwrap();
    assert_eq!(reader.frames(), 2);
    let mut buffer = FrameBuffer::default();
    for i in 0..2 {
        reader.read_frame_into(i, &mut buffer).unwrap();
        assert_eq!(buffer.shape(), Some([2, 2]));
        assert_eq!(
            buffer.pixels(),
            Some(&dioptrin_io::PixelBuffer::U32(
                values[i * 4..i * 4 + 4].to_vec()
            ))
        );
    }
}
