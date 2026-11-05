#![cfg(feature = "edf")]
use dioptrin_io::{
    PixelBuffer,
    edf::{ExternalPayloads, FrameBuffer, Limits, Options, Reader},
};
use std::{fs, io::Write};

// Independent test records: EDF grammar assembled here, without either writer.
fn block(fields: &str, end: &[u8]) -> Vec<u8> {
    let mut b = format!("\r\n{{\n{fields}").into_bytes();
    b.extend_from_slice(end);
    b
}
fn external(name: &str, size: usize) -> Vec<u8> {
    block(
        &format!(
            "EDF_BinarySize = 0; EDF_BinaryFileName = {name}; EDF_BinaryFilePosition = 2; EDF_BinaryFileSize = {size}; Dim_1 = 3; Dim_2 = 1; DataType = UnsignedShort; ByteOrder = LowByteFirst;"
        ),
        b"}\r\n",
    )
}
fn follow() -> Options {
    Options {
        external_payloads: ExternalPayloads::FollowSnapshot,
        ..Options::default()
    }
}
fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
#[test]
fn exact_header_end_general_inheritance_and_repeated_fields_keep_raw_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inline.edf");
    let general = block(
        "EDF_DataFormatVersion = 1.0; EDF_BinarySize = 0; Dim_1 = 3; Dim_2 = 1; DataType = UnsignedShort; ByteOrder = LowByteFirst; Motor = inherited;",
        b"}\r\n",
    );
    let mut local = block(
        "EDF_BinarySize = 6; EDF_HeaderSize = 1024; Motor = first; Motor = local;",
        b"",
    );
    // End marker deliberately crosses a traditional 512-byte boundary.
    local.resize(511, b' ');
    local.extend_from_slice(b"}\r\n");
    let mut bytes = general.clone();
    bytes.extend_from_slice(&local);
    bytes.extend_from_slice(&[0, 0, 255, 255, 1, 128]);
    fs::write(&path, &bytes).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(reader.general_headers()[0].header, general);
    let meta = reader.metadata(0).unwrap();
    assert_eq!(meta.header, local);
    assert_eq!(meta.fields["MOTOR"], "local");
    assert_eq!(meta.general_header, Some(0));
    assert_eq!(meta.payload_offset, (general.len() + local.len()) as u64);
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(
        output.pixels(),
        Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
    );
}
#[test]
fn external_sources_require_policy_and_snapshot_every_source() {
    let dir = tempfile::tempdir().unwrap();
    let header = dir.path().join("header.ehf");
    let data = dir.path().join("data.bin");
    fs::write(&header, external("data.bin", 6)).unwrap();
    fs::write(&data, [99, 98, 0, 0, 255, 255, 1, 128]).unwrap();
    assert!(Reader::open(&header, Limits::default()).is_err());
    let reader = Reader::open_with_options(&header, Limits::default(), follow()).unwrap();
    assert_eq!(reader.payload_sources().len(), 2);
    assert_eq!(reader.metadata(0).unwrap().payload_source, 1);
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(
        output.pixels(),
        Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
    );
    // Atomic replacement with the same length must still invalidate indexed data.
    let replacement = dir.path().join("replacement.bin");
    fs::write(&replacement, [99, 98, 0, 0, 0, 0, 0, 0]).unwrap();
    fs::rename(replacement, &data).unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    fs::write(&header, external("data.bin", 7)).unwrap();
    assert!(Reader::open_with_options(&header, Limits::default(), follow()).is_err());
    fs::write(&header, external("absent.bin", 6)).unwrap();
    assert!(Reader::open_with_options(&header, Limits::default(), follow()).is_err());
}
#[test]
fn compressed_external_sources_are_explicit_unambiguous_and_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let header = dir.path().join("header.ehf.gz");
    let data = dir.path().join("data.bin.gz");
    fs::write(&header, gzip(&external("old/acquisition/data.bin", 6))).unwrap();
    let encoded = gzip(&[99, 98, 0, 0, 255, 255, 1, 128]);
    fs::write(&data, &encoded).unwrap();
    let options = Options {
        external_payloads: ExternalPayloads::FollowCompressedSnapshot,
        external_basenames: true,
        ..Options::default()
    };
    assert!(Reader::open_with_options(&header, Limits::default(), follow()).is_err());
    let reader = Reader::open_with_options(&header, Limits::default(), options.clone()).unwrap();
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(
        output.pixels(),
        Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
    );
    assert!(
        Reader::open_with_options(
            &header,
            Limits::default(),
            Options {
                source_files: 1,
                ..options.clone()
            }
        )
        .is_err()
    );
    assert!(
        Reader::open_with_options(
            &header,
            Limits::default(),
            Options {
                transport: dioptrin_io::edf::TransportLimits {
                    source_bytes: 10,
                    ..Default::default()
                },
                ..options.clone()
            }
        )
        .is_err()
    );
    // Both alternate names exist: fail instead of choosing one silently.
    fs::write(dir.path().join("data.bin.bz2"), &encoded).unwrap();
    assert!(Reader::open_with_options(&header, Limits::default(), options.clone()).is_err());
    fs::remove_file(dir.path().join("data.bin.bz2")).unwrap();
    // A present exact file is authoritative even if unusable.
    fs::write(dir.path().join("data.bin"), [1]).unwrap();
    assert!(Reader::open_with_options(&header, Limits::default(), options).is_err());
    fs::write(&data, gzip(&[1, 2, 3, 4])).unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
}
#[test]
fn padded_compressed_slots_preserve_exact_decoded_size_and_crc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("padded.edf");
    let mut payload = gzip(&[0, 0, 255, 255, 1, 128]);
    let compressed_len = payload.len();
    payload.resize(512, 0);
    let header = block(
        "EDF_BinarySize = 512; Dim_1 = 3; Dim_2 = 1; DataType = UnsignedShort; ByteOrder = LowByteFirst; Compression = Gzip;",
        b"}\n",
    );
    let mut bytes = header.clone();
    bytes.extend_from_slice(&payload);
    fs::write(&path, &bytes).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(
        output.pixels(),
        Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
    );
    drop(reader);
    bytes[header.len() + compressed_len - 8] ^= 1;
    fs::write(&path, bytes).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
}

#[test]
fn only_bounded_zero_and_whitespace_final_padding_is_admitted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aligned.edf");
    let mut bytes = block(
        "Dim_1 = 3; Dim_2 = 1; DataType = UnsignedShort; ByteOrder = LowByteFirst;",
        b"}\n",
    );
    bytes.extend_from_slice(&[0, 0, 255, 255, 1, 128]);
    let used = bytes.len();
    bytes.resize(512, 0);
    fs::write(&path, &bytes).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(reader.trailing_padding(), 512 - used);
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(
        output.pixels(),
        Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
    );
    drop(reader);
    bytes.extend_from_slice(b" \n");
    fs::write(&path, &bytes).unwrap();
    assert!(Reader::open(&path, Limits::default()).is_ok());
    bytes[511] = 1;
    fs::write(&path, bytes).unwrap();
    assert!(Reader::open(&path, Limits::default()).is_err());
}

#[test]
fn encoded_tails_and_additional_members_cannot_hide_unexplained_counts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tails.edf");
    for codec in ["Gzip", "Zlib", "BZ2"] {
        let bytes = [0, 0, 255, 255, 1, 128];
        let encode = |samples: &[u8]| -> Vec<u8> {
            match codec {
                "Gzip" => gzip(samples),
                "Zlib" => {
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
        for extra in [vec![42, 37], encode(&[3, 4])] {
            let mut payload = encode(&bytes);
            payload.extend(extra);
            let header = block(
                &format!(
                    "Size = {}; Dim_1 = 3; Dim_2 = 1; DataType = UnsignedShort; ByteOrder = LowByteFirst; Compression = {codec};",
                    payload.len()
                ),
                b"}\n",
            );
            let mut file = header;
            file.extend(payload);
            fs::write(&path, file).unwrap();
            let reader = Reader::open(&path, Limits::default()).unwrap();
            let mut output = FrameBuffer::default();
            assert!(reader.read_frame_into(0, &mut output).is_err(), "{codec}");
            assert!(output.pixels().is_none());
        }
        let mut payload = encode(&bytes);
        payload.resize(512, 32);
        let header = block(
            &format!(
                "Size = 512; Dim_1 = 3; Dim_2 = 1; DataType = UnsignedShort; ByteOrder = LowByteFirst; Compression = {codec};"
            ),
            b"}\n",
        );
        let mut file = header;
        file.extend(payload);
        fs::write(&path, file).unwrap();
        let reader = Reader::open(&path, Limits::default()).unwrap();
        let mut output = FrameBuffer::default();
        reader.read_frame_into(0, &mut output).unwrap();
        assert_eq!(
            output.pixels(),
            Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
        );
    }
}
