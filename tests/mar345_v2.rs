#![cfg(feature = "mar345")]
use dioptrin_io::{
    PixelBuffer, PixelType,
    mar345::{FrameBuffer, Limits, Options, PckV2Dialect, Reader},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/mar345-v2")
        .join(name)
}
fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Reader, dioptrin_io::mar345::Error> {
    Reader::open_with_options(
        path,
        limits,
        Options {
            v2_dialect: PckV2Dialect::FabioLegacyEightBit,
        },
    )
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn words(buffer: &FrameBuffer) -> Vec<u8> {
    let Some(PixelBuffer::U32(values)) = buffer.pixels() else {
        panic!("original U32 output required")
    };
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
#[test]
fn all_v2_widths_runs_endianness_headers_and_overflow_words_match_independent_sources() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture("provenance.json")).unwrap()).unwrap();
    let cases = manifest["files"].as_object().unwrap();
    assert_eq!(cases.len(), 39);
    let mut output = FrameBuffer::default();
    for (name, case) in cases {
        let raw = fs::read(fixture(name)).unwrap();
        assert_eq!(digest(&raw), case["sha256"].as_str().unwrap());
        let reader = open(fixture(name), Limits::default()).unwrap();
        let meta = reader.metadata();
        assert_eq!(reader.frames(), 1);
        assert_eq!(meta.pck_version, 2);
        assert_eq!(meta.pck_v2_dialect, Some(PckV2Dialect::FabioLegacyEightBit));
        assert_eq!(meta.pixel_type, PixelType::U32);
        assert_eq!(meta.base_pixel_type, PixelType::U16);
        assert_eq!(meta.header, raw[..4096]);
        assert_eq!(
            meta.header
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            case["metadata"]["header_hex"].as_str().unwrap()
        );
        assert_eq!(
            meta.little_endian,
            case["metadata"]["little_endian"].as_bool().unwrap()
        );
        assert_eq!(
            meta.text_start,
            case["metadata"]["text_start"].as_u64().unwrap() as usize
        );
        assert_eq!(
            format!("{:?}", meta.overflow_layout),
            case["metadata"]["overflow_layout"].as_str().unwrap()
        );
        assert_eq!(
            meta.overflow_records.len(),
            case["metadata"]["overflow_records"]
                .as_array()
                .unwrap()
                .len()
        );
        for (actual, expected) in meta
            .overflow_records
            .iter()
            .zip(case["metadata"]["overflow_records"].as_array().unwrap())
        {
            assert_eq!(
                actual.one_based_index,
                expected["one_based_index"].as_u64().unwrap() as u32
            );
            assert_eq!(actual.value, expected["value"].as_u64().unwrap() as u32);
        }
        reader.read_frame_into(0, &mut output).unwrap();
        assert_eq!(
            digest(&words(&output)),
            case["sample_sha256"].as_str().unwrap(),
            "{name}"
        );
        assert_eq!(output.shape(), Some(meta.shape));
        assert!(reader.read_frame_into(1, &mut output).is_err());
        assert!(output.shape().is_none());
        assert!(output.pixels().is_none());
    }
}
#[test]
fn v2_signed_neighborhood_retains_unsigned_words_and_high_overlays_are_exact_u32() {
    let mut output = FrameBuffer::default();
    let reader = open(fixture("v2-signed-neighborhood.mar345"), Limits::default()).unwrap();
    reader.read_frame_into(0, &mut output).unwrap();
    let Some(PixelBuffer::U32(values)) = output.pixels() else {
        panic!()
    };
    assert_eq!(
        &values[..9],
        &[
            32768, 32769, 32770, 32771, 32772, 32773, 32774, 32775, 32776
        ]
    );
    let reader = open(fixture("v2-overflow-padded-be.mar345"), Limits::default()).unwrap();
    reader.read_frame_into(0, &mut output).unwrap();
    let Some(PixelBuffer::U32(values)) = output.pixels() else {
        panic!()
    };
    assert_eq!(
        [values[0], values[7], values[16], values[31]],
        [65536, 999999, 2147483648, u32::MAX]
    );
    // A V2 width5 block starts with ASCII space; it is a packed byte, never trim.
    let raw = fs::read(fixture("v2-partial-last-byte.mar345")).unwrap();
    let reader = open(fixture("v2-partial-last-byte.mar345"), Limits::default()).unwrap();
    let start = 4096 + reader.metadata().marker.len();
    assert_eq!(raw[start], b' ');
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(output.pixels(), Some(&PixelBuffer::U32(vec![0, 1, 2, 3])));
}
#[test]
fn reserved_v2_selector_block_overrun_truncation_tail_and_padding_invalidate_output() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bad.mar345");
    let source = fs::read(fixture("v2-width4.mar345")).unwrap();
    let good = open(fixture("v2-width4.mar345"), Limits::default()).unwrap();
    let offset = 4096 + good.metadata().marker.len();
    let mut output = FrameBuffer::default();
    let mut invalid = Vec::new();
    let mut reserved = source.clone();
    reserved[offset] = 0xf0;
    invalid.push((reserved, "reserved width selector"));
    let mut overrun = source.clone();
    overrun[offset] = 0x0f;
    invalid.push((overrun, "exceeds declared pixel count"));
    invalid.push((source[..source.len() - 1].to_vec(), "truncated"));
    for suffix in [b"\0".as_slice(), b"tail".as_slice()] {
        let mut bytes = source.clone();
        bytes.extend_from_slice(suffix);
        invalid.push((bytes, "trailing bytes"));
    }
    let mut padding = fs::read(fixture("v2-partial-last-byte.mar345")).unwrap();
    *padding.last_mut().unwrap() |= 0x80;
    invalid.push((padding, "padding bits"));
    for (bytes, error) in invalid {
        good.read_frame_into(0, &mut output).unwrap();
        assert!(output.pixels().is_some());
        fs::write(&path, bytes).unwrap();
        let reader = open(&path, Limits::default()).unwrap();
        let failure = reader.read_frame_into(0, &mut output).unwrap_err();
        assert!(
            failure.to_string().contains(error),
            "{failure} expected {error}"
        );
        assert!(output.shape().is_none());
        assert!(output.pixels().is_none());
    }
}
#[test]
fn v2_metadata_payload_limits_and_source_replacement_fail_before_visible_output() {
    let name = "v2-zero-run32768.mar345";
    assert!(
        open(
            fixture(name),
            Limits {
                frame_bytes: 32768 * 4 - 1,
                ..Default::default()
            }
        )
        .is_err()
    );
    let bytes = fs::read(fixture("v2-overflow-padded-le.mar345")).unwrap();
    for limits in [
        Limits {
            encoded_bytes: 1,
            ..Default::default()
        },
        Limits {
            metadata_bytes: 16384,
            ..Default::default()
        },
        Limits {
            header_bytes: 4096,
            ..Default::default()
        },
    ] {
        assert!(open(fixture("v2-overflow-padded-le.mar345"), limits).is_err());
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.mar345");
    fs::write(&path, &bytes).unwrap();
    let reader = open(&path, Limits::default()).unwrap();
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    let replacement = dir.path().join("replacement.mar345");
    fs::write(&replacement, &bytes).unwrap();
    fs::rename(replacement, &path).unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    assert!(output.shape().is_none());
}

#[cfg(feature = "wrappers")]
#[test]
fn v2_nonseekable_capture_and_outer_gzip_preserve_version_counts_and_error_invalidation() {
    use dioptrin_io::{
        native, tiff,
        wrapped::{self, Format, Input},
    };
    use std::io::{Read, Write};
    struct Chunks<'a>(&'a [u8]);
    impl Read for Chunks<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let count = out.len().min(11).min(self.0.len());
            out[..count].copy_from_slice(&self.0[..count]);
            self.0 = &self.0[count..];
            Ok(count)
        }
    }
    let raw = fs::read(fixture("v2-overflow-padded-be.mar345")).unwrap();
    let input = Input::from_reader(Chunks(&raw), None, Default::default()).unwrap();
    let mut captured = input
        .read(
            Format::Mar345WithOptions(Options {
                v2_dialect: PckV2Dialect::FabioLegacyEightBit,
            }),
            native::Limits::default(),
            tiff::Limits::default(),
        )
        .unwrap();
    let Some(wrapped::Metadata::Mar345(meta)) = captured.metadata(0) else {
        panic!()
    };
    assert_eq!(meta.pck_version, 2);
    assert!(!meta.little_endian);
    let mut output = wrapped::FrameBuffer::default();
    captured.read_frame_into(0, &mut output).unwrap();
    let Some(PixelBuffer::U32(values)) = output.pixels() else {
        panic!()
    };
    assert_eq!(
        [values[0], values[7], values[16], values[31]],
        [65536, 999999, 2147483648, u32::MAX]
    );
    let expected = values.clone();
    assert!(captured.read_frame_into(1, &mut output).is_err());
    assert!(output.pixels().is_none());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.mar345.gz");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&raw).unwrap();
    fs::write(&path, encoder.finish().unwrap()).unwrap();
    let mut compressed = Input::open(&path, Default::default())
        .unwrap()
        .read(
            Format::Mar345WithOptions(Options {
                v2_dialect: PckV2Dialect::FabioLegacyEightBit,
            }),
            native::Limits::default(),
            tiff::Limits::default(),
        )
        .unwrap();
    compressed.read_frame_into(0, &mut output).unwrap();
    assert_eq!(output.pixels(), Some(&PixelBuffer::U32(expected)));
    let Some(wrapped::Metadata::Mar345(meta)) = compressed.metadata(0) else {
        panic!()
    };
    assert_eq!(meta.pck_version, 2);
    // Indexed encoded wrapper identity is checked after its decoded capture exists.
    fs::write(&path, b"changed").unwrap();
    assert!(compressed.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    let raw_original = fs::read(original_fixture("original-signed-neighborhood.mar345")).unwrap();
    let mut canonical = Input::from_reader(Chunks(&raw_original), None, Default::default())
        .unwrap()
        .read(
            Format::Mar345,
            native::Limits::default(),
            tiff::Limits::default(),
        )
        .unwrap();
    let Some(wrapped::Metadata::Mar345(meta)) = canonical.metadata(0) else {
        panic!()
    };
    assert_eq!(meta.pck_v2_dialect, Some(PckV2Dialect::Original));
    canonical.read_frame_into(0, &mut output).unwrap();
    let Some(PixelBuffer::U32(values)) = output.pixels() else {
        panic!()
    };
    assert_eq!(
        &values[..9],
        &[
            32768, 32769, 32770, 32771, 32772, 32773, 32774, 32775, 32776
        ]
    );
    assert!(canonical.read_frame_into(1, &mut output).is_err());
    assert!(output.pixels().is_none());
}

fn original_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/mar345-v2-original")
        .join(name)
}
#[test]
fn original_seven_bit_v2_is_default_and_all_sixteen_width_codes_preserve_authored_words() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(original_fixture("provenance.json")).unwrap()).unwrap();
    let cases = manifest["files"].as_object().unwrap();
    assert_eq!(cases.len(), 32);
    let mut output = FrameBuffer::default();
    for (name, case) in cases {
        let raw = fs::read(original_fixture(name)).unwrap();
        assert_eq!(digest(&raw), case["sha256"].as_str().unwrap());
        let reader = Reader::open(original_fixture(name), Limits::default()).unwrap();
        let meta = reader.metadata();
        assert_eq!(meta.pck_version, 2);
        assert_eq!(meta.pck_v2_dialect, Some(PckV2Dialect::Original));
        assert_eq!(meta.header, raw[..4096]);
        assert_eq!(
            meta.little_endian,
            case["metadata"]["little_endian"].as_bool().unwrap()
        );
        assert_eq!(
            format!("{:?}", meta.overflow_layout),
            case["metadata"]["overflow_layout"].as_str().unwrap()
        );
        reader.read_frame_into(0, &mut output).unwrap();
        assert_eq!(
            digest(&words(&output)),
            case["sample_sha256"].as_str().unwrap(),
            "{name}"
        );
    }
    // Identical marker text never causes a failed-read retry with another grammar.
    let eight = Reader::open(fixture("v2-width4.mar345"), Limits::default()).unwrap();
    assert_eq!(
        eight.metadata().pck_v2_dialect,
        Some(PckV2Dialect::Original)
    );
    assert!(eight.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    let original_as_eight = open(
        original_fixture("original-width3.mar345"),
        Limits::default(),
    )
    .unwrap();
    assert!(original_as_eight.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    let v1 = Reader::open(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/mar345/base-le.mar345"),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(v1.metadata().pck_v2_dialect, None);
}
#[test]
fn original_v2_overrun_truncation_unused_bits_and_changed_source_never_expose_prior_words() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.mar345");
    let good = Reader::open(
        original_fixture("original-width3.mar345"),
        Limits::default(),
    )
    .unwrap();
    let raw = fs::read(original_fixture("original-width3.mar345")).unwrap();
    let start = 4096 + good.metadata().marker.len();
    let mut cases = Vec::new();
    let mut overrun = raw.clone();
    overrun[start] = 7;
    cases.push(overrun);
    cases.push(raw[..raw.len() - 1].to_vec());
    let mut tail = raw.clone();
    tail.push(0);
    cases.push(tail);
    let mut padding = fs::read(original_fixture("original-partial-last-byte.mar345")).unwrap();
    *padding.last_mut().unwrap() |= 0x80;
    cases.push(padding);
    let mut output = FrameBuffer::default();
    for bytes in cases {
        good.read_frame_into(0, &mut output).unwrap();
        fs::write(&path, bytes).unwrap();
        let reader = Reader::open(&path, Limits::default()).unwrap();
        assert!(reader.read_frame_into(0, &mut output).is_err());
        assert!(output.pixels().is_none());
        assert!(output.shape().is_none());
    }
    fs::write(&path, &raw).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    reader.read_frame_into(0, &mut output).unwrap();
    fs::write(&path, &raw[..4096]).unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    assert!(
        Reader::open(
            original_fixture("original-width3.mar345"),
            Limits {
                frame_bytes: 127,
                ..Default::default()
            }
        )
        .is_err()
    );
}
