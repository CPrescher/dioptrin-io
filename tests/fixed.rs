#![cfg(feature = "fixed")]
use dioptrin_io::{PixelBuffer, PixelType, ge, pixi};
use ge::{FrameBuffer, Limits};
use sha2::{Digest, Sha256};
use std::{
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/fixed")
        .join(name)
}
fn hash(pixels: &PixelBuffer) -> String {
    let mut hash = Sha256::new();
    match pixels {
        PixelBuffer::U8(v) => hash.update(v),
        PixelBuffer::U16(v) => {
            for x in v {
                hash.update(x.to_le_bytes());
            }
        }
        PixelBuffer::U32(v) => {
            for x in v {
                hash.update(x.to_le_bytes());
            }
        }
        _ => panic!("unsupported fixture type"),
    }
    format!("{:x}", hash.finalize())
}
fn temporary(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("dioptrin-fixed-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}
fn pixi_source(count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for i in 0..count {
        bytes.extend(243722i32.to_le_bytes());
        bytes.extend((0..20).map(|j| (j + i) as u8));
        for n in 0..512 * 476 {
            let x = match n {
                0 => 0,
                1 => 65535,
                2 => 32768,
                _ => ((n * 17 + i * 1009) % 65536) as u16,
            };
            bytes.extend(x.to_le_bytes());
        }
    }
    bytes
}
#[test]
fn independent_ge_depths_headers_unsigned_extrema_and_stored_order() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    let mut output = FrameBuffer::default();
    for (name, case) in manifest["ge"].as_object().unwrap() {
        let path = fixture(name);
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
            case["sha256"].as_str().unwrap()
        );
        let reader = ge::Reader::open(path, Limits::default()).unwrap();
        assert_eq!(reader.frames(), 2);
        assert_eq!(reader.metadata().kind, ge::HeaderKind::Standard8001);
        assert_eq!(reader.metadata().shape, [2, 3]);
        assert_eq!(&reader.metadata().header[3332..], b"ORIGINAL USERHDR");
        for flag in [
            "Swapped",
            "Reordered",
            "HorizontalFlipped",
            "VerticalFlipped",
        ] {
            assert!(matches!(
                reader.metadata().fields[flag],
                ge::HeaderValue::U16(1)
            ));
        }
        let ty = match case["dtype"].as_str().unwrap() {
            "uint8" => PixelType::U8,
            "uint16" => PixelType::U16,
            "uint32" => PixelType::U32,
            _ => unreachable!(),
        };
        for (i, expected) in case["frame_sha256"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .rev()
        {
            reader.read_frame_into(i, &mut output).unwrap();
            assert_eq!(output.pixels().unwrap().pixel_type(), ty);
            assert_eq!(hash(output.pixels().unwrap()), expected.as_str().unwrap());
        }
        assert!(reader.read_frame_into(2, &mut output).is_err());
        assert!(output.pixels().is_none());
    }
}
#[test]
fn independent_pixi_complete_records_and_opaque_headers() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    let temp = temporary("pixi-source");
    for count in [1, 3] {
        let bytes = pixi_source(count);
        let case = &manifest["pixi"][count.to_string()];
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            case["sha256"].as_str().unwrap()
        );
        let path = temp.join(format!("pixi-{count}"));
        std::fs::write(&path, &bytes).unwrap();
        let reader = pixi::Reader::open(path, Limits::default()).unwrap();
        assert_eq!(reader.frames(), count);
        assert_eq!(reader.metadata().shape, [512, 476]);
        assert_eq!(reader.metadata().pixel_type, PixelType::U16);
        assert_eq!(reader.metadata().headers.len(), count);
        let mut output = FrameBuffer::default();
        for (i, expected) in case["frame_sha256"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .rev()
        {
            assert_eq!(
                &reader.metadata().headers[i][4..],
                &(0..20).map(|n| (n + i) as u8).collect::<Vec<_>>()
            );
            reader.read_frame_into(i, &mut output).unwrap();
            assert_eq!(hash(output.pixels().unwrap()), expected.as_str().unwrap());
        }
        assert!(reader.read_frame_into(count, &mut output).is_err());
        assert!(output.pixels().is_none());
    }
    std::fs::remove_dir_all(temp).unwrap();
}
#[test]
fn aps_blank_and_edf_headers_have_explicit_layout_provenance() {
    let temp = temporary("aps");
    let path = temp.join("image.ge");
    let mut file = std::fs::File::create(&path).unwrap();
    file.set_len(8192 + 2048 * 2048 * 2).unwrap();
    file.seek(SeekFrom::Start(8192)).unwrap();
    file.write_all(&[0, 0, 255, 255, 0, 128]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let reader = ge::Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(reader.metadata().kind, ge::HeaderKind::ApsBlank8192);
    assert!(reader.metadata().header.iter().all(|&b| b == 0));
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    let raw = std::fs::read(&path).unwrap();
    assert_eq!(
        hash(output.pixels().unwrap()),
        format!("{:x}", Sha256::digest(&raw[8192..]))
    );
    drop(reader);
    let text=b"\n{EDF_HeaderSize = 8192 ; Num_Images = 1 ; DataType = UnsignedShort ; ByteOrder = LowByteFirst ; Dim_1 = 2048 ; Dim_2 = 2048 ; Size = 8388608 ; Original = \xff ; ";
    let mut header = vec![b' '; 8192];
    header[..text.len()].copy_from_slice(text);
    header[8191] = b'}';
    let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.write_all(&header).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let reader = ge::Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(reader.metadata().kind, ge::HeaderKind::ApsEdf8192);
    assert_eq!(reader.metadata().edf_fields["Original"], "ÿ");
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(
        hash(output.pixels().unwrap()),
        format!("{:x}", Sha256::digest(&raw[8192..]))
    );
    drop(reader);
    header[0] = b'X';
    let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.write_all(&header).unwrap();
    drop(file);
    assert!(ge::Reader::open(&path, Limits::default()).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}
#[test]
fn malformed_sources_limits_and_mutation_invalidate_output() {
    let temp = temporary("invalid");
    let path = temp.join("bad.ge");
    let original = std::fs::read(fixture("ge-uint16.ge")).unwrap();
    for n in [0, 29, 3331, original.len() - 1] {
        std::fs::write(&path, &original[..n]).unwrap();
        assert!(ge::Reader::open(&path, Limits::default()).is_err());
    }
    for (offset, value) in [(10, 8002u16), (22, 3), (24, 0), (28, 12)] {
        let mut bytes = original.clone();
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(ge::Reader::open(&path, Limits::default()).is_err());
    }
    std::fs::write(&path, &original).unwrap();
    let reader = ge::Reader::open(&path, Limits::default()).unwrap();
    let mut output = FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    std::fs::write(&path, b"changed").unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    for limits in [
        Limits {
            encoded_bytes: 1,
            ..Limits::default()
        },
        Limits {
            frames: 1,
            ..Limits::default()
        },
        Limits {
            header_bytes: 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
    ] {
        std::fs::write(&path, &original).unwrap();
        assert!(ge::Reader::open(&path, limits).is_err());
    }
    let original = pixi_source(3);
    let path = temp.join("bad.pixi");
    for n in [0, 23, original.len() - 1] {
        std::fs::write(&path, &original[..n]).unwrap();
        assert!(pixi::Reader::open(&path, Limits::default()).is_err());
    }
    let mut bytes = original.clone();
    bytes[487448] = 0;
    std::fs::write(&path, &bytes).unwrap();
    assert!(pixi::Reader::open(&path, Limits::default()).is_err());
    std::fs::write(&path, &original).unwrap();
    let reader = pixi::Reader::open(&path, Limits::default()).unwrap();
    reader.read_frame_into(0, &mut output).unwrap();
    std::fs::write(&path, b"changed").unwrap();
    assert!(reader.read_frame_into(0, &mut output).is_err());
    assert!(output.pixels().is_none());
    for limits in [
        Limits {
            encoded_bytes: 1,
            ..Limits::default()
        },
        Limits {
            frames: 2,
            ..Limits::default()
        },
        Limits {
            header_bytes: 23,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
    ] {
        std::fs::write(&path, &original).unwrap();
        assert!(pixi::Reader::open(&path, limits).is_err());
    }
    std::fs::remove_dir_all(temp).unwrap();
}
#[test]
fn actual_upstream_ge_and_test_generated_pixi_files() {
    let Ok(path) = std::env::var("FIXED_PARITY_MANIFEST") else {
        return;
    };
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut output = FrameBuffer::default();
    for (path, case) in manifest["files"].as_object().unwrap() {
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(path).unwrap())),
            case["sha256"].as_str().unwrap(),
            "{path}"
        );
        match case["format"].as_str().unwrap() {
            "ge" => {
                let reader = ge::Reader::open(path, Limits::default())
                    .unwrap_or_else(|e| panic!("{path}: {e}"));
                assert_eq!(reader.frames(), case["shape"][0].as_u64().unwrap() as usize);
                for (i, expected) in case["frame_sha256"].as_array().unwrap().iter().enumerate() {
                    reader.read_frame_into(i, &mut output).unwrap();
                    assert_eq!(
                        hash(output.pixels().unwrap()),
                        expected.as_str().unwrap(),
                        "{path} frame {i}"
                    );
                }
            }
            "pixi" => {
                let reader = pixi::Reader::open(path, Limits::default())
                    .unwrap_or_else(|e| panic!("{path}: {e}"));
                assert_eq!(reader.frames(), case["shape"][0].as_u64().unwrap() as usize);
                for (i, expected) in case["frame_sha256"].as_array().unwrap().iter().enumerate() {
                    reader.read_frame_into(i, &mut output).unwrap();
                    assert_eq!(
                        hash(output.pixels().unwrap()),
                        expected.as_str().unwrap(),
                        "{path} frame {i}"
                    );
                }
            }
            _ => unreachable!(),
        }
        assert_eq!(output.pixels().unwrap().pixel_type(), PixelType::U16);
        assert_eq!(
            output.shape(),
            Some([
                case["shape"][1].as_u64().unwrap() as usize,
                case["shape"][2].as_u64().unwrap() as usize
            ])
        );
    }
}

#[test]
fn aps_edf_cannot_silently_decode_compressed_external_or_volume_storage_as_raw() {
    let temp = temporary("edf-layout");
    let path = temp.join("image.ge");
    let base = "{ EDF_HeaderSize = 8192 ; Num_Images = 1 ; DataType = UnsignedShort ; ByteOrder = LowByteFirst ; Dim_1 = 2048 ; Dim_2 = 2048 ; Size = 8388608 ; ";
    for declaration in [
        "Compression = GZIP ;",
        "compression = ZLIB ;",
        "Compression = BYTE_OFFSET ;",
        "EDF_BinaryFileName = other.bin ;",
        "EDF_BinaryFilePosition = 64 ;",
        "EDF_BinarySize = 8388608 ;",
        "Dim_3 = 2 ;",
        "Dim_4 = 2 ;",
        "EDF_DataBlockID = 0.General ;",
    ] {
        let text = format!("{base}{declaration}");
        let mut header = vec![b' '; 8192];
        header[..text.len()].copy_from_slice(text.as_bytes());
        header[8191] = b'}';
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&header).unwrap();
        file.set_len(8192 + 8388608).unwrap();
        drop(file);
        assert!(
            ge::Reader::open(&path, Limits::default()).is_err(),
            "{declaration}"
        );
    }
    for declaration in [
        "Compression = NONE ;",
        "Compression = No ;",
        "EDF_DataBlockID = 1.Image.Psd ;",
    ] {
        let text = format!("{base}{declaration}");
        let mut header = vec![b' '; 8192];
        header[..text.len()].copy_from_slice(text.as_bytes());
        header[8191] = b'}';
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&header).unwrap();
        file.set_len(8192 + 8388608).unwrap();
        file.seek(SeekFrom::Start(8192)).unwrap();
        file.write_all(&[255, 255, 0, 128]).unwrap();
        drop(file);
        let reader = ge::Reader::open(&path, Limits::default()).unwrap();
        let mut buffer = FrameBuffer::default();
        reader.read_frame_into(0, &mut buffer).unwrap();
        let PixelBuffer::U16(pixels) = buffer.pixels().unwrap() else {
            panic!("type")
        };
        assert_eq!(&pixels[..2], &[65535, 32768]);
    }
    std::fs::remove_dir_all(temp).unwrap();
}
