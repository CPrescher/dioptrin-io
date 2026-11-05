#![cfg(feature = "edf")]
// Independent generated detector/header fixture bytes are CC0-1.0.
// Rust test code follows the crate MIT license; no upstream writer is used.
use dioptrin_io::{
    PixelBuffer,
    edf::{ExternalPayloads, ExternalWindow, FrameBuffer, Limits, Options, Reader},
};
use std::{fs, io::Write};
fn header(name: &str, start: usize, window: usize, compression: &str) -> Vec<u8> {
    format!("{{\nEDF_BinarySize = 0 ; EDF_BinaryFileName = {name} ; EDF_BinaryFilePosition = {start} ; EDF_BinaryFileSize = {window} ; Dim_1 = 3 ; Dim_2 = 1 ; DataType = UnsignedShort ; ByteOrder = LowByteFirst ; Compression = {compression} ;\n}}\n").into_bytes()
}
fn options(policy: ExternalWindow) -> Options {
    Options {
        external_payloads: ExternalPayloads::FollowSnapshot,
        external_window: policy,
        ..Options::default()
    }
}
fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    out.write_all(data).unwrap();
    out.finish().unwrap()
}
#[test]
fn complete_external_samples_with_oversized_window_require_explicit_policy() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("header.ehf");
    let source = temp.path().join("source.bin");
    let raw = [77, 88, 0, 0, 255, 255, 1, 128, 0, 0];
    fs::write(&source, raw).unwrap();
    fs::write(&path, header("source.bin", 2, 140000, "NONE")).unwrap();
    assert!(
        Reader::open_with_options(&path, Limits::default(), options(ExternalWindow::Exact))
            .is_err()
    );
    let reader = Reader::open_with_options(
        &path,
        Limits::default(),
        options(ExternalWindow::UpToDeclared),
    )
    .unwrap();
    let meta = reader.metadata(0).unwrap();
    assert_eq!(meta.declared_stored_bytes, 140000);
    assert_eq!(meta.stored_bytes, 8);
    assert_eq!(
        meta.external_window_policy,
        Some(ExternalWindow::UpToDeclared)
    );
    assert_eq!(meta.fields["EDF_BINARYFILESIZE"], "140000");
    let mut frame = FrameBuffer::default();
    reader.read_frame_into(0, &mut frame).unwrap();
    assert_eq!(
        frame.pixels(),
        Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
    );
    // Source replacement remains an immutable-source failure under compatibility.
    fs::write(&source, &raw[..7]).unwrap();
    assert!(reader.read_frame_into(0, &mut frame).is_err());
    assert!(frame.pixels().is_none());
    assert!(
        Reader::open_with_options(
            &path,
            Limits::default(),
            options(ExternalWindow::UpToDeclared)
        )
        .is_err()
    );
}
#[test]
fn external_window_compatibility_does_not_pad_pixels_or_change_inline_navigation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("header.ehf");
    let source = temp.path().join("source.bin");
    for (start, window, data) in [
        (2, 140000, vec![0; 7]),
        (100, 140000, vec![0; 8]),
        (2, 0, vec![0; 8]),
        (2, 5, vec![0; 8]),
    ] {
        fs::write(&source, data).unwrap();
        fs::write(&path, header("source.bin", start, window, "NONE")).unwrap();
        assert!(
            Reader::open_with_options(
                &path,
                Limits::default(),
                options(ExternalWindow::UpToDeclared)
            )
            .is_err()
        );
    }
    fs::write(&path,b"{Dim_1=3;Dim_2=1;DataType=UnsignedShort;ByteOrder=LowByteFirst;EDF_BinarySize=100;}\n\0\0\xff\xff\x01\x80").unwrap();
    assert!(
        Reader::open_with_options(
            &path,
            Limits::default(),
            options(ExternalWindow::UpToDeclared)
        )
        .is_err()
    );
    // The oversized request does not allocate declared bytes: actual bytes alone
    // still obey the independent encoded budget, and cannot bypass that budget.
    fs::write(&source, [0, 0, 255, 255, 1, 128]).unwrap();
    fs::write(&path, header("source.bin", 0, usize::MAX, "NONE")).unwrap();
    let reader = Reader::open_with_options(
        &path,
        Limits {
            encoded_bytes: 6,
            ..Limits::default()
        },
        options(ExternalWindow::UpToDeclared),
    )
    .unwrap();
    let mut frame = FrameBuffer::default();
    reader.read_frame_into(0, &mut frame).unwrap();
    assert!(
        Reader::open_with_options(
            &path,
            Limits {
                encoded_bytes: 5,
                ..Limits::default()
            },
            options(ExternalWindow::UpToDeclared)
        )
        .is_err()
    );
}
#[test]
fn shortened_external_compressed_window_still_requires_full_stream_and_exact_output() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("header.ehf");
    let source = temp.path().join("source.bin");
    for codec in ["GZIP", "ZLIB", "BZ2"] {
        let encode = |samples: &[u8]| -> Vec<u8> {
            match codec {
                "GZIP" => gzip(samples),
                "ZLIB" => {
                    let mut e =
                        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                    e.write_all(samples).unwrap();
                    e.finish().unwrap()
                }
                _ => {
                    let mut e =
                        bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
                    e.write_all(samples).unwrap();
                    e.finish().unwrap()
                }
            }
        };
        let encoded = encode(&[0, 0, 255, 255, 1, 128]);
        fs::write(&path, header("source.bin", 2, 1000, codec)).unwrap();
        let mut complete = vec![77, 88];
        complete.extend(&encoded);
        fs::write(&source, &complete).unwrap();
        let reader = Reader::open_with_options(
            &path,
            Limits::default(),
            options(ExternalWindow::UpToDeclared),
        )
        .unwrap();
        let mut frame = FrameBuffer::default();
        reader.read_frame_into(0, &mut frame).unwrap();
        assert_eq!(
            frame.pixels(),
            Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
        );
        for data in [
            encoded[..encoded.len() - 1].to_vec(),
            encode(&[0, 0, 255, 255]),
            encode(&[0, 0, 255, 255, 1, 128, 0, 0]),
        ] {
            let mut prefixed = vec![77, 88];
            prefixed.extend(data);
            fs::write(&source, prefixed).unwrap();
            let reader = Reader::open_with_options(
                &path,
                Limits::default(),
                options(ExternalWindow::UpToDeclared),
            )
            .unwrap();
            assert!(reader.read_frame_into(0, &mut frame).is_err(), "{codec}");
            assert!(frame.pixels().is_none());
        }
    }
}
#[test]
fn outer_compressed_sources_keep_snapshot_and_checksum_contracts() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("header.ehf.gz");
    let source = temp.path().join("source.bin.gz");
    let raw = [99, 98, 0, 0, 255, 255, 1, 128];
    fs::write(&path, gzip(&header("old/source.bin", 2, 140000, "NONE"))).unwrap();
    let complete = gzip(&raw);
    fs::write(&source, &complete).unwrap();
    let option = Options {
        external_payloads: ExternalPayloads::FollowCompressedSnapshot,
        external_basenames: true,
        external_window: ExternalWindow::UpToDeclared,
        ..Options::default()
    };
    let reader = Reader::open_with_options(&path, Limits::default(), option.clone()).unwrap();
    let mut frame = FrameBuffer::default();
    reader.read_frame_into(0, &mut frame).unwrap();
    assert_eq!(reader.metadata(0).unwrap().stored_bytes, 6);
    assert_eq!(
        frame.pixels(),
        Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
    );
    fs::write(&source, &complete[..complete.len() - 1]).unwrap();
    assert!(Reader::open_with_options(&path, Limits::default(), option).is_err());
}
