#![cfg(all(feature = "wrappers", unix))]
use dioptrin_io::{
    PixelBuffer, native, tiff,
    wrapped::{Format, FrameBuffer, Limits, Reader},
};
use std::{os::unix::fs::symlink, path::Path};
fn open(path: &Path, format: Format) -> Reader {
    Reader::open(
        path,
        format,
        Limits::default(),
        native::Limits::default(),
        tiff::Limits::default(),
    )
    .unwrap()
}
#[test]
fn root_alias_retargeting_invalidates_counts_even_when_original_file_survives() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.edf");
    let second = dir.path().join("second.edf");
    let alias = dir.path().join("alias.edf");
    let header = b"{Dim_1=1;Dim_2=1;DataType=UnsignedShort;ByteOrder=LowByteFirst;Size=2;}\n";
    let mut bytes = header.to_vec();
    bytes.extend([1, 0]);
    std::fs::write(&first, &bytes).unwrap();
    bytes[header.len()] = 2;
    std::fs::write(&second, &bytes).unwrap();
    symlink(&first, &alias).unwrap();
    let mut reader = open(&alias, Format::Edf);
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(output.pixels(), Some(&PixelBuffer::U16(vec![1])));
    let bindings = reader.source_bindings().unwrap();
    assert!(
        bindings
            .iter()
            .any(|b| b.requested == alias && b.resolved == std::fs::canonicalize(&first).unwrap())
    );
    std::fs::remove_file(&alias).unwrap();
    symlink(&second, &alias).unwrap();
    assert!(first.exists());
    assert!(reader.check_sources().is_err());
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
}
#[test]
fn external_edf_deduplicated_target_aliases_are_all_retained() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data.bin");
    let alternate = dir.path().join("alternate.bin");
    let a = dir.path().join("a.bin");
    let b = dir.path().join("b.bin");
    let root = dir.path().join("header.edf");
    std::fs::write(&data, [1, 0]).unwrap();
    std::fs::write(&alternate, [2, 0]).unwrap();
    symlink(&data, &a).unwrap();
    symlink(&data, &b).unwrap();
    let block = |name: &str| {
        format!(
            "{{EDF_BinarySize=0;EDF_BinaryFileName={name};EDF_BinaryFileSize=2;EDF_BinaryFilePosition=0;Dim_1=1;Dim_2=1;DataType=UnsignedShort;ByteOrder=LowByteFirst;}}\n"
        )
    };
    std::fs::write(&root, format!("{}{}", block("a.bin"), block("b.bin"))).unwrap();
    let reader = dioptrin_io::edf::Reader::open_with_options(
        &root,
        native::Limits::default(),
        dioptrin_io::edf::Options {
            external_payloads: dioptrin_io::edf::ExternalPayloads::FollowSnapshot,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reader.payload_sources().len(), 2);
    assert_eq!(reader.source_bindings().len(), 3);
    let mut output = dioptrin_io::edf::FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    std::fs::remove_file(&b).unwrap();
    symlink(&alternate, &b).unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
}
#[cfg(feature = "hdf5")]
#[test]
fn same_root_target_external_alias_survives_owned_graph_export() {
    use hdf5_backend::File;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("master.h5");
    let alias = dir.path().join("alias.h5");
    {
        let file = File::create(&root).unwrap();
        let dataset = file
            .new_dataset::<u16>()
            .shape((1, 1))
            .create("data")
            .unwrap();
        dataset.write_raw(&[7u16]).unwrap();
        file.link_external("alias.h5", "/data", "external").unwrap();
    }
    symlink(&root, &alias).unwrap();
    let input = dioptrin_io::wrapped::Input::open(&root, Default::default()).unwrap();
    let reader = input
        .read_hdf5(
            dioptrin_io::hdf5::typed::Selection::Dataset("/external"),
            dioptrin_io::hdf5::typed::Options {
                external_links: dioptrin_io::hdf5::typed::ExternalLinks::FollowSnapshot,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    assert!(reader.source_bindings().unwrap().iter().any(|b| b.requested
        == std::fs::canonicalize(dir.path()).unwrap().join("alias.h5")
        && b.resolved == std::fs::canonicalize(&root).unwrap()));
}
