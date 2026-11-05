#![cfg(feature = "esperanto")]
use dioptrin_io::{
    PixelBuffer, PixelType,
    esperanto::{self, Encoding, FrameBuffer, Limits},
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/esperanto")
        .join(name)
}
fn hash(pixels: &PixelBuffer) -> String {
    let mut hash = Sha256::new();
    match pixels {
        PixelBuffer::I32(v) => {
            for x in v {
                hash.update(x.to_le_bytes());
            }
        }
        _ => panic!("type"),
    };
    format!("{:x}", hash.finalize())
}
fn compare(path: &Path, case: &serde_json::Value) {
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(path).unwrap())),
        case["sha256"].as_str().unwrap(),
        "{}",
        path.display()
    );
    let mut buffer = FrameBuffer::default();
    let reader = esperanto::Reader::open(path, Limits::default())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(reader.frames(), 1);
    assert_eq!(reader.metadata().source_pixel_type, PixelType::I32);
    assert_eq!(reader.metadata().pixel_type, PixelType::I32);
    reader
        .read_frame_into(0, &mut buffer)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        buffer.shape(),
        Some([
            case["shape"][0].as_u64().unwrap() as usize,
            case["shape"][1].as_u64().unwrap() as usize
        ])
    );
    assert_eq!(
        hash(buffer.pixels().unwrap()),
        case["native_sha256"].as_str().unwrap(),
        "{}",
        path.display()
    );
    assert_eq!(buffer.pixels().unwrap().pixel_type(), PixelType::I32);
    assert!(reader.read_frame_into(1, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
#[test]
fn independent_agi_all_widths_escapes_tails_wraps_and_original_shapes() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    for (name, case) in manifest["files"].as_object().unwrap() {
        compare(&fixture(name), case);
    }
    let reader =
        esperanto::Reader::open(fixture("agi-field1.esperanto"), Limits::default()).unwrap();
    assert_eq!(reader.metadata().shape, [2, 33]);
    assert_eq!(
        reader.metadata().fields["UNKNOWN_FIELD"],
        "\"Never discard original metadata\""
    );
    assert_eq!(&reader.metadata().header[6398..], b"\r\x1a");
    let mut output = FrameBuffer::default();
    let reader =
        esperanto::Reader::open(fixture("agi-extrema-cols2.esperanto"), Limits::default()).unwrap();
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(
        output.pixels(),
        Some(&PixelBuffer::I32(vec![i32::MIN, i32::MAX, 0, -32768]))
    );
}
#[test]
fn actual_upstream_files_and_both_reference_writer_encodings() {
    if let Ok(path) = std::env::var("ESPERANTO_PARITY_MANIFEST") {
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for (path, case) in manifest["files"].as_object().unwrap() {
            compare(Path::new(path), case);
        }
    }
}
#[test]
fn malformed_headers_indices_fields_padding_limits_and_changed_sources_fail() {
    let temp = std::env::temp_dir().join(format!("dioptrin-esperanto-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    let path = temp.join("invalid.esperanto");
    let original = std::fs::read(fixture("agi-field8.esperanto")).unwrap();
    for n in [0, 255, 6399, original.len() - 1] {
        std::fs::write(&path, &original[..n]).unwrap();
        assert!(esperanto::Reader::open(&path, Limits::default()).is_err());
    }
    for offset in [254, 510, 6398] {
        let mut bytes = original.clone();
        bytes[offset] = 0;
        std::fs::write(&path, bytes).unwrap();
        assert!(esperanto::Reader::open(&path, Limits::default()).is_err());
    }
    for (line, text) in [
        (
            0,
            "ESPERANTO FORMAT 2 CONSISTING OF 25 LINES OF 256 BYTES EACH".to_owned(),
        ),
        (
            0,
            "ESPERANTO FORMAT 1 CONSISTING OF 999999999999 LINES OF 256 BYTES EACH".to_owned(),
        ),
        (1, "IMAGE 0 2 1 1 \"AGI_BITFIELD\"".to_owned()),
        (1, format!("IMAGE {} 2 1 1 \"AGI_BITFIELD\"", usize::MAX)),
        (1, "IMAGE 33 2 1 1 \"UNKNOWN\"".to_owned()),
        (4, "IMAGE 33 2 1 1 \"AGI_BITFIELD\"".to_owned()),
    ] {
        let mut bytes = original.clone();
        let start = line * 256;
        bytes[start..start + 254].fill(b' ');
        bytes[start..start + text.len()].copy_from_slice(text.as_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(esperanto::Reader::open(&path, Limits::default()).is_err());
    }
    let mut bytes = original.clone();
    bytes[600] = 128;
    std::fs::write(&path, bytes).unwrap();
    assert!(esperanto::Reader::open(&path, Limits::default()).is_err());
    let mut bytes = std::fs::read(fixture("agi-extrema-cols1.esperanto")).unwrap();
    let data_bytes = u32::from_le_bytes(bytes[6400..6404].try_into().unwrap()) as usize;
    bytes[6408 + data_bytes..6412 + data_bytes].copy_from_slice(&3u32.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    let reader = esperanto::Reader::open(&path, Limits::default()).unwrap();
    let mut output = FrameBuffer::default();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    let bytes = std::fs::read(fixture("long32-extrema-cols34.esperanto")).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
    assert!(esperanto::Reader::open(&path, Limits::default()).is_err());
    let mut bytes = bytes;
    bytes.push(0);
    std::fs::write(&path, bytes).unwrap();
    assert!(esperanto::Reader::open(&path, Limits::default()).is_err());
    let dsize = u32::from_le_bytes(original[6400..6404].try_into().unwrap()) as usize;
    for (offset, value) in [(6400, u32::MAX), (6404 + dsize, 1), (6408 + dsize, 0)] {
        let mut bytes = original.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(esperanto::Reader::open(&path, Limits::default()).is_err());
    }
    for value in [0, 9, 0x90] {
        let mut bytes = original.clone();
        let first_size = match bytes[6404] {
            254 => 3,
            255 => 5,
            _ => 1,
        };
        bytes[6404 + first_size] = value;
        std::fs::write(&path, bytes).unwrap();
        let reader = esperanto::Reader::open(&path, Limits::default()).unwrap();
        let mut output = FrameBuffer::default();
        assert!(reader.read_frame_into(0, &mut output).is_err());
        assert!(output.pixels().is_none());
    }
    let mut padded = original.clone();
    padded.resize(original.len() + 1, 0);
    std::fs::write(&path, &padded).unwrap();
    assert!(esperanto::Reader::open(&path, Limits::default()).is_err());
    // Padding alignment applies to the encoded payload after the ASCII header.
    let payload = original.len() - 6400;
    let padded_length = 6400 + payload.div_ceil(1024) * 1024;
    padded = original.clone();
    padded.resize(padded_length, 0);
    std::fs::write(&path, &padded).unwrap();
    let reader = esperanto::Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(
        reader.metadata().padding_bytes,
        padded_length - original.len()
    );
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    drop(reader);
    *padded.last_mut().unwrap() = 1;
    std::fs::write(&path, &padded).unwrap();
    assert!(esperanto::Reader::open(&path, Limits::default()).is_err());
    for limits in [
        Limits {
            frame_bytes: 1,
            ..Limits::default()
        },
        Limits {
            header_bytes: 255,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 22400,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: 1,
            ..Limits::default()
        },
    ] {
        std::fs::write(&path, &original).unwrap();
        assert!(esperanto::Reader::open(&path, limits).is_err());
    }
    std::fs::write(&path, &original).unwrap();
    let native_bytes = 2 * 33 * 4;
    let reader = esperanto::Reader::open(
        &path,
        Limits {
            frame_bytes: native_bytes,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(reader.metadata().encoding, Encoding::AgiBitfield);
    reader.read_frame_into(0, &mut output).unwrap();
    std::fs::write(&path, b"changed").unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    std::fs::remove_dir_all(temp).unwrap();
}
