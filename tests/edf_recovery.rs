#![cfg(feature = "edf")]
//! Independent generated CC0 fixture bytes; test code follows the crate MIT license.
use dioptrin_io::{
    PixelBuffer,
    edf::{
        self, FrameBuffer, InlineLayout, Limits, Options, PayloadInterpretation,
        recovery::{self, Availability, Integrity, SampleBuffer, Stop},
    },
};
use std::{fs, io::Write};
fn header(
    dtype: &str,
    endian: &str,
    shape: [usize; 2],
    size: usize,
    physical: usize,
    codec: &str,
) -> Vec<u8> {
    format!("{{\nDim_1={};Dim_2={};DataType={dtype};ByteOrder={endian};Size={size};EDF_BinarySize={physical};EDF_HeaderSize=8192;Compression={codec};\n}}\n",shape[1],shape[0]).into_bytes()
}
fn gzip(input: &[u8]) -> Vec<u8> {
    let mut out = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    out.write_all(input).unwrap();
    out.finish().unwrap()
}
fn bzip(input: &[u8]) -> Vec<u8> {
    let mut out = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    out.write_all(input).unwrap();
    out.finish().unwrap()
}
#[test]
fn legacy_inline_size_and_stored_raw_label_are_explicit_and_preserve_every_declaration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.edf");
    let h = header("UnsignedShort", "LowByteFirst", [1, 3], 6, 202, "NONE");
    let mut bytes = h.clone();
    bytes.extend([0, 0, 255, 255, 1, 128]);
    fs::write(&path, &bytes).unwrap();
    assert!(edf::Reader::open(&path, Limits::default()).is_err());
    let reader = edf::Reader::open_with_options(
        &path,
        Limits::default(),
        Options {
            inline_layout: InlineLayout::LegacySizeAtParsedBoundary,
            ..Options::default()
        },
    )
    .unwrap();
    let meta = reader.metadata(0).unwrap();
    assert_eq!(meta.header, h);
    assert_eq!(meta.declared_stored_bytes, 202);
    assert_eq!(meta.stored_bytes, 6);
    assert_eq!(meta.declared_header_bytes.as_deref(), Some("8192"));
    assert_eq!(meta.parsed_header_bytes, h.len());
    assert_eq!(meta.legacy_size_bytes, Some(6));
    let mut out = FrameBuffer::default();
    reader.read_frame_into(0, &mut out).unwrap();
    assert_eq!(out.pixels(), Some(&PixelBuffer::U16(vec![0, 65535, 32769])));
    let mut raw = header("SignedInteger", "HighByteFirst", [1, 3], 12, 12, "TY1");
    for n in [0i32, i32::MIN, i32::MAX] {
        raw.extend(n.to_be_bytes());
    }
    fs::write(&path, raw).unwrap();
    assert!(edf::Reader::open(&path, Limits::default()).is_err());
    let options = Options {
        payload_interpretation: PayloadInterpretation::StoredRawForLabel("TY1".into()),
        ..Options::default()
    };
    let reader = edf::Reader::open_with_options(&path, Limits::default(), options).unwrap();
    reader.read_frame_into(0, &mut out).unwrap();
    assert_eq!(
        out.pixels(),
        Some(&PixelBuffer::I32(vec![0, i32::MIN, i32::MAX]))
    );
    let meta = reader.metadata(0).unwrap();
    assert_eq!(meta.declared_compression.as_deref(), Some("TY1"));
    assert_eq!(meta.compression, "NONE");
    assert_eq!(meta.fields["COMPRESSION"], "TY1");
    assert!(
        edf::Reader::open_with_options(
            &path,
            Limits::default(),
            Options {
                payload_interpretation: PayloadInterpretation::StoredRawForLabel("GZIP".into()),
                ..Options::default()
            }
        )
        .is_err()
    );
}
#[test]
fn known_complete_frames_and_truncated_header_keep_typed_counts_and_exact_header_tail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tail.edf");
    let mut data = header("UnsignedShort", "HighByteFirst", [1, 3], 6, 6, "NONE");
    let payload = data.len();
    data.extend([0, 0, 255, 255, 128, 1]);
    let tail = b"{Dim_1=3;Dim_2=";
    data.extend(tail);
    fs::write(&path, data).unwrap();
    assert!(edf::Reader::open(&path, Limits::default()).is_err());
    let reader =
        recovery::Reader::open(&path, Limits::default(), recovery::Options::default()).unwrap();
    assert_eq!(reader.image_records(), 1);
    assert!(
        matches!(reader.stop(),Some(Stop::TruncatedHeader{offset,bytes})if *offset==payload as u64+6&&bytes==tail)
    );
    let mut out = SampleBuffer::default();
    reader.read_samples_into(0, &mut out).unwrap();
    assert_eq!(out.pixels(), Some(&PixelBuffer::U16(vec![0, 65535, 32769])));
    let report = out.report().unwrap();
    assert_eq!(report.present_samples, 0..3);
    assert_eq!(report.missing_samples, None);
    assert!(report.complete_verified_frame);
    assert_eq!(report.availability, Availability::AllSamples);
    fs::write(&path, b"changed").unwrap();
    assert!(reader.read_samples_into(0, &mut out).is_err());
    assert!(out.pixels().is_none());
    assert!(out.report().is_none());
    assert!(out.partial_element_bytes().is_none());
}
#[test]
fn contiguous_prefix_partial_word_and_header_only_never_allocate_missing_samples() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("partial.edf");
    for n in [0, 1, 4, 5, 6, 7] {
        let mut data = header("UnsignedShort", "HighByteFirst", [2, 2], 8, 8, "NONE");
        let raw = [0, 0, 255, 255, 128, 1, 171];
        data.extend(&raw[..n]);
        fs::write(&path, data).unwrap();
        let reader =
            recovery::Reader::open(&path, Limits::default(), recovery::Options::default()).unwrap();
        let mut out = SampleBuffer::default();
        reader.read_samples_into(0, &mut out).unwrap();
        let count = n / 2;
        assert_eq!(out.pixels().unwrap().len(), count);
        assert_eq!(out.partial_element_bytes(), Some(&raw[count * 2..n]));
        let report = out.report().unwrap();
        assert_eq!(report.declared_shape, [2, 2]);
        assert_eq!(report.present_samples, 0..count);
        assert_eq!(report.missing_samples, Some(count..4));
        assert_eq!(report.verified_samples, 0..count);
        assert!(!report.complete_verified_frame);
        assert!(
            matches!(reader.stop(),Some(Stop::TruncatedPayload{available_bytes,..})if *available_bytes==n)
        );
    }
}
#[test]
fn outer_member_integrity_is_independent_of_all_sample_availability() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("members.edf.gz");
    let h = header("UnsignedShort", "LowByteFirst", [1, 3], 6, 6, "NONE");
    let raw = [0, 0, 255, 255, 1, 128];
    let mut good = gzip(&h);
    good.extend(gzip(&raw));
    let mut truncated = good[..good.len() - 1].to_vec();
    fs::write(&path, &truncated).unwrap();
    let reader =
        recovery::Reader::open(&path, Limits::default(), recovery::Options::default()).unwrap();
    assert_eq!(
        reader.source_report().verified_decoded_bytes,
        h.len() as u64
    );
    assert_eq!(
        reader.source_report().integrity,
        Integrity::UnverifiedTruncated
    );
    let mut out = SampleBuffer::default();
    reader.read_samples_into(0, &mut out).unwrap();
    assert_eq!(out.pixels(), Some(&PixelBuffer::U16(vec![0, 65535, 32769])));
    let report = out.report().unwrap();
    assert_eq!(report.availability, Availability::AllSamples);
    assert_eq!(report.verified_samples, 0..0);
    assert!(!report.complete_verified_frame);
    assert_eq!(report.outer_integrity, Integrity::UnverifiedTruncated);
    truncated = good.clone();
    let n = truncated.len();
    truncated[n - 8] ^= 1;
    fs::write(&path, truncated).unwrap();
    let reader =
        recovery::Reader::open(&path, Limits::default(), recovery::Options::default()).unwrap();
    assert_eq!(reader.source_report().integrity, Integrity::ChecksumFailure);
    reader.read_samples_into(0, &mut out).unwrap();
    assert!(!out.report().unwrap().complete_verified_frame);
    let mut earlier = gzip(&[h.clone(), raw.to_vec()].concat());
    earlier.extend(&gzip(b"{partial")[..15]);
    fs::write(&path, earlier).unwrap();
    let reader =
        recovery::Reader::open(&path, Limits::default(), recovery::Options::default()).unwrap();
    reader.read_samples_into(0, &mut out).unwrap();
    assert_eq!(
        out.report().unwrap().outer_integrity,
        Integrity::VerifiedMember
    );
    assert!(out.report().unwrap().complete_verified_frame);
}
#[test]
fn compressed_payload_truncation_footer_integrity_and_overlong_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("internal.edf");
    let raw = [0, 0, 255, 255, 1, 128];
    for codec in ["GZIP", "ZLIB", "BZ2"] {
        let encode = |bytes: &[u8]| match codec {
            "GZIP" => gzip(bytes),
            "BZ2" => bzip(bytes),
            _ => {
                let mut e =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(bytes).unwrap();
                e.finish().unwrap()
            }
        };
        let encoded = encode(&raw);
        let mut out = SampleBuffer::default();
        for drop in [0, 1, 2] {
            let mut data = header(
                "UnsignedShort",
                "LowByteFirst",
                [1, 3],
                6,
                encoded.len(),
                codec,
            );
            data.extend(&encoded[..encoded.len() - drop]);
            fs::write(&path, data).unwrap();
            let reader =
                recovery::Reader::open(&path, Limits::default(), recovery::Options::default())
                    .unwrap();
            reader.read_samples_into(0, &mut out).unwrap();
            assert_eq!(
                out.pixels(),
                Some(&PixelBuffer::U16(vec![0, 65535, 32769])),
                "{codec} drop{drop}"
            );
            if drop == 0 {
                assert!(out.report().unwrap().complete_verified_frame);
            } else {
                assert_ne!(
                    out.report().unwrap().payload_integrity,
                    Integrity::ContainerComplete,
                    "{codec}"
                );
                assert!(!out.report().unwrap().complete_verified_frame);
            }
        }
        let long = encode(&[0, 0, 255, 255, 1, 128, 99, 100]);
        let mut data = header(
            "UnsignedShort",
            "LowByteFirst",
            [1, 3],
            6,
            long.len(),
            codec,
        );
        data.extend(long);
        fs::write(&path, data).unwrap();
        let reader =
            recovery::Reader::open(&path, Limits::default(), recovery::Options::default()).unwrap();
        assert!(reader.read_samples_into(0, &mut out).is_err());
        assert!(out.report().is_none());
        assert!(out.pixels().is_none());
        assert!(out.partial_element_bytes().is_none());
    }
}
#[test]
fn resource_limits_and_member_header_bounds_are_hard_failures() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bounds.edf");
    let mut data = header("UnsignedShort", "LowByteFirst", [1, 3], 6, 6, "NONE");
    data.extend([0; 6]);
    fs::write(&path, &data).unwrap();
    for limits in [
        Limits {
            frame_bytes: 5,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: 5,
            ..Limits::default()
        },
        Limits {
            header_bytes: 10,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 10,
            ..Limits::default()
        },
    ] {
        assert!(recovery::Reader::open(&path, limits, recovery::Options::default()).is_err());
    }
    let mut members = gzip(&data);
    members.extend(gzip(&[]));
    fs::write(&path, members).unwrap();
    assert!(
        recovery::Reader::open(
            &path,
            Limits::default(),
            recovery::Options {
                members: 1,
                ..recovery::Options::default()
            }
        )
        .is_err()
    );
    let mut named = gzip(&data);
    named[3] |= 8;
    named.splice(10..10, [b'a'; 128]);
    fs::write(&path, named).unwrap();
    assert!(
        recovery::Reader::open(
            &path,
            Limits {
                header_bytes: 32,
                ..Limits::default()
            },
            recovery::Options::default()
        )
        .is_err()
    );
}

#[test]
fn malformed_later_semantic_headers_and_general_headers_retain_original_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("malformed.edf");
    let mut first = header("UnsignedShort", "LowByteFirst", [1, 3], 6, 6, "NONE");
    first.extend([0, 0, 255, 255, 1, 128]);
    for bad in [
        header("UnknownSourceType", "LowByteFirst", [1, 3], 6, 6, "NONE"),
        b"{EDF_DataFormatVersion=1;EDF_BinarySize=not-a-number;}\n".to_vec(),
    ] {
        let mut bytes = first.clone();
        bytes.extend(&bad);
        fs::write(&path, bytes).unwrap();
        let reader =
            recovery::Reader::open(&path, Limits::default(), recovery::Options::default()).unwrap();
        assert_eq!(reader.image_records(), 1);
        assert!(matches!(reader.stop(),Some(Stop::UnparsedRecord{header,..})if header==&bad));
        let mut out = SampleBuffer::default();
        reader.read_samples_into(0, &mut out).unwrap();
        assert!(out.report().unwrap().complete_verified_frame);
    }
}
#[test]
fn scalar_u64_and_float_bits_survive_partial_word_recovery_in_both_byte_orders() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wide.edf");
    for little in [false, true] {
        for dtype in ["Unsigned64", "DoubleValue"] {
            let values = [u64::MAX, 0x8000000000000000, 0x7ff8123456789abc];
            let mut raw = Vec::new();
            for value in values {
                raw.extend(if little {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                });
            }
            let mut bytes = header(
                dtype,
                if little {
                    "LowByteFirst"
                } else {
                    "HighByteFirst"
                },
                [1, 3],
                24,
                24,
                "NONE",
            );
            bytes.extend(&raw[..23]);
            fs::write(&path, bytes).unwrap();
            let reader =
                recovery::Reader::open(&path, Limits::default(), recovery::Options::default())
                    .unwrap();
            let mut out = SampleBuffer::default();
            reader.read_samples_into(0, &mut out).unwrap();
            assert_eq!(out.partial_element_bytes(), Some(&raw[16..23]));
            match out.pixels().unwrap() {
                PixelBuffer::U64(v) => assert_eq!(v, &values[..2]),
                PixelBuffer::F64(v) => assert_eq!(
                    v.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                    values[..2]
                ),
                _ => panic!("source dtype changed"),
            }
        }
    }
}
#[test]
fn missing_compressed_payload_and_recorded_codec_padding_have_distinct_integrity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("padding.edf");
    for codec in ["GZIP", "BZ2", "ZLIB"] {
        let encoded = match codec {
            "GZIP" => gzip(&[0, 0, 255, 255, 1, 128]),
            "BZ2" => bzip(&[0, 0, 255, 255, 1, 128]),
            _ => {
                let mut out =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                out.write_all(&[0, 0, 255, 255, 1, 128]).unwrap();
                out.finish().unwrap()
            }
        };
        let mut data = header("UnsignedShort", "LowByteFirst", [1, 3], 6, 512, codec);
        data.extend(&encoded);
        data.resize(data.len() + 512 - encoded.len(), 0);
        fs::write(&path, data).unwrap();
        let reader =
            recovery::Reader::open(&path, Limits::default(), recovery::Options::default()).unwrap();
        let mut out = SampleBuffer::default();
        reader.read_samples_into(0, &mut out).unwrap();
        assert_eq!(out.pixels(), Some(&PixelBuffer::U16(vec![0, 65535, 32769])));
        assert!(out.report().unwrap().complete_verified_frame, "{codec}");
        fs::write(
            &path,
            header(
                "UnsignedShort",
                "LowByteFirst",
                [1, 3],
                6,
                encoded.len(),
                codec,
            ),
        )
        .unwrap();
        let reader =
            recovery::Reader::open(&path, Limits::default(), recovery::Options::default()).unwrap();
        reader.read_samples_into(0, &mut out).unwrap();
        assert_eq!(
            out.report().unwrap().availability,
            Availability::HeadersOnly
        );
        assert_eq!(out.pixels().unwrap().len(), 0);
        assert_ne!(
            out.report().unwrap().payload_integrity,
            Integrity::ContainerComplete,
            "{codec}"
        );
    }
}

#[cfg(unix)]
#[test]
fn immutable_recovery_rejects_fifo_before_opening_a_blocking_read_handle() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("not-a-file.edf");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let error = recovery::Reader::open(path, Limits::default(), recovery::Options::default())
            .err()
            .map(|e| e.to_string());
        let _ = sender.send(error);
    });
    let error = receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("opening a FIFO must fail before blocking")
        .expect("FIFO must be rejected");
    assert!(error.contains("regular immutable source"));
}

#[test]
fn encoded_stream_recovery_retains_truncated_and_corrupt_member_provenance_and_cleanup() {
    struct OneByte<'a>(&'a [u8]);
    impl std::io::Read for OneByte<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let n = out.len().min(1).min(self.0.len());
            out[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }
    let h = header("UnsignedShort", "HighByteFirst", [1, 3], 6, 6, "NONE");
    let raw = [0, 0, 255, 255, 128, 1];
    for compressed in [false, true] {
        let complete = if compressed {
            [gzip(&h), gzip(&raw)].concat()
        } else {
            [h.clone(), raw.to_vec()].concat()
        };
        let variants = if compressed {
            let mut crc = complete.clone();
            let n = crc.len();
            crc[n - 8] ^= 1;
            vec![
                (complete.clone(), Integrity::ContainerComplete),
                (
                    complete[..complete.len() - 1].to_vec(),
                    Integrity::UnverifiedTruncated,
                ),
                (crc, Integrity::ChecksumFailure),
            ]
        } else {
            vec![
                (complete.clone(), Integrity::ContainerComplete),
                (
                    complete[..complete.len() - 1].to_vec(),
                    Integrity::ContainerComplete,
                ),
            ]
        };
        for (bytes, integrity) in variants {
            let reader = recovery::Reader::from_reader(
                OneByte(&bytes),
                None,
                Limits::default(),
                recovery::Options::default(),
            )
            .unwrap();
            let captured = reader.source_report().path.clone();
            assert_eq!(fs::read(&captured).unwrap(), bytes);
            assert_eq!(reader.source_report().encoded_bytes, bytes.len() as u64);
            assert_eq!(reader.source_report().integrity, integrity);
            let mut buffer = SampleBuffer::default();
            reader.read_samples_into(0, &mut buffer).unwrap();
            if compressed || bytes.len() == complete.len() {
                assert_eq!(
                    buffer.pixels(),
                    Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
                );
            } else {
                assert_eq!(buffer.pixels(), Some(&PixelBuffer::U16(vec![0, 65535])));
                assert_eq!(buffer.partial_element_bytes(), Some(&[128][..]));
            }
            assert_eq!(
                buffer.report().unwrap().complete_verified_frame,
                integrity == Integrity::ContainerComplete && bytes.len() == complete.len()
            );
            // The encoded capture remains tracked even after private expansion.
            fs::write(&captured, b"replaced capture").unwrap();
            assert!(reader.read_samples_into(0, &mut buffer).is_err());
            assert!(buffer.pixels().is_none());
            assert!(buffer.report().is_none());
            assert!(buffer.partial_element_bytes().is_none());
            drop(reader);
            assert!(!captured.exists());
        }
    }
}

#[test]
fn recovery_stream_current_position_byte_limits_and_external_origin_are_explicit() {
    let h = header("UnsignedShort", "LowByteFirst", [1, 3], 6, 6, "NONE");
    let bytes = [h, [0, 0, 255, 255, 1, 128].to_vec()].concat();
    let mut prefixed = b"not part of input".to_vec();
    let start = prefixed.len();
    prefixed.extend(&bytes);
    let mut input = std::io::Cursor::new(prefixed);
    input.set_position(start as u64);
    let reader = recovery::Reader::from_reader(
        &mut input,
        None,
        Limits::default(),
        recovery::Options::default(),
    )
    .unwrap();
    assert_eq!(input.position(), input.get_ref().len() as u64);
    assert_eq!(reader.source_report().encoded_bytes, bytes.len() as u64);
    let mut output = SampleBuffer::default();
    reader.read_samples_into(0, &mut output).unwrap();
    assert_eq!(
        output.pixels(),
        Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
    );
    for options in [
        recovery::Options {
            members: 0,
            ..Default::default()
        },
        recovery::Options {
            edf: Options {
                external_payloads: edf::ExternalPayloads::FollowSnapshot,
                ..Default::default()
            },
            ..Default::default()
        },
    ] {
        let mut input = std::io::Cursor::new(&bytes);
        assert!(
            recovery::Reader::from_reader(&mut input, None, Limits::default(), options).is_err()
        );
        assert_eq!(input.position(), 0);
    }
    let mut input = std::io::Cursor::new(&bytes);
    assert!(
        recovery::Reader::from_reader(
            &mut input,
            None,
            Limits {
                frame_bytes: 0,
                ..Limits::default()
            },
            recovery::Options::default()
        )
        .is_err()
    );
    assert_eq!(input.position(), 0);
    let mut input = std::io::Cursor::new(&bytes);
    let options = recovery::Options {
        edf: Options {
            transport: edf::TransportLimits {
                encoded_bytes: 4,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(recovery::Reader::from_reader(&mut input, None, Limits::default(), options).is_err());
    assert_eq!(input.position(), 5);
    // An origin never turns unsupported external recovery into an implicit file read.
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("data.bin"), [0u8; 6]).unwrap();
    let external=b"{EDF_BinarySize=0;EDF_BinaryFileName=data.bin;EDF_BinaryFileSize=6;Dim_1=3;Dim_2=1;DataType=UnsignedShort;ByteOrder=LowByteFirst;}\n";
    assert!(
        recovery::Reader::from_reader(
            &external[..],
            Some(dir.path()),
            Limits::default(),
            recovery::Options {
                edf: Options {
                    external_payloads: edf::ExternalPayloads::FollowSnapshot,
                    ..Default::default()
                },
                ..Default::default()
            }
        )
        .is_err()
    );
}
