#![cfg(feature = "containers")]
use dioptrin_io::{PixelBuffer, PixelType, dm3, mrc, spe};
use sha2::{Digest, Sha256};
use spe::{FrameBuffer, Limits};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/containers")
        .join(name)
}
fn hash(pixels: &PixelBuffer) -> String {
    let mut h = Sha256::new();
    macro_rules! scalar { ($($ty:ident),*) => {match pixels {$(PixelBuffer::$ty(v) => for x in v {h.update(x.to_le_bytes());},)* _=>unreachable!()}}; }
    match pixels {
        PixelBuffer::CI16(v) => {
            for pair in v {
                for x in pair {
                    h.update(x.to_le_bytes());
                }
            }
        }
        PixelBuffer::C32(v) => {
            for pair in v {
                for x in pair {
                    h.update(x.to_le_bytes());
                }
            }
        }
        PixelBuffer::C64(v) => {
            for pair in v {
                for x in pair {
                    h.update(x.to_le_bytes());
                }
            }
        }
        _ => scalar!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64),
    }
    format!("{:x}", h.finalize())
}
enum Reader {
    Spe(spe::Reader),
    Mrc(Box<mrc::Reader>),
    Dm3(dm3::Reader),
}
impl Reader {
    fn open(path: &Path, kind: &str, limits: Limits) -> Result<Self, spe::Error> {
        match kind {
            "spe" => spe::Reader::open(path, limits).map(Self::Spe),
            "mrc" => mrc::Reader::open(path, limits).map(Box::new).map(Self::Mrc),
            "dm3" => dm3::Reader::open(path, limits).map(Self::Dm3),
            _ => panic!("unknown format"),
        }
    }
    fn frames(&self) -> usize {
        match self {
            Self::Spe(r) => r.frames(),
            Self::Mrc(r) => r.frames(),
            Self::Dm3(r) => r.frames(),
        }
    }
    fn read(&self, i: usize, b: &mut FrameBuffer) -> Result<(), spe::Error> {
        match self {
            Self::Spe(r) => r.read_frame_into(i, b),
            Self::Mrc(r) => r.read_frame_into(i, b),
            Self::Dm3(r) => r.read_frame_into(i, b),
        }
    }
}
fn compare_manifest(path: &Path, directory: &Path) {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut output = FrameBuffer::default();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let path = directory.join(name);
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
            case["sha256"].as_str().unwrap(),
            "{name}"
        );
        let reader = Reader::open(&path, case["format"].as_str().unwrap(), Limits::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let frames = case["frame_sha256"].as_array().unwrap();
        assert_eq!(reader.frames(), frames.len(), "{name}");
        for (i, expected) in frames.iter().enumerate().rev() {
            reader
                .read(i, &mut output)
                .unwrap_or_else(|e| panic!("{name} frame {i}: {e}"));
            let source_type = match case["dtype"].as_str().unwrap() {
                "int8" => PixelType::I8,
                "uint8" => PixelType::U8,
                "int16" => PixelType::I16,
                "uint16" => PixelType::U16,
                "int32" => PixelType::I32,
                "uint32" => PixelType::U32,
                "float32" => PixelType::F32,
                "float64" => PixelType::F64,
                "complex64" => PixelType::C32,
                "complex_int16" => PixelType::CI16,
                other => panic!("unlisted source dtype {other}"),
            };
            assert_eq!(
                output.pixels().unwrap().pixel_type(),
                source_type,
                "{name} dtype"
            );
            assert_eq!(
                hash(output.pixels().unwrap()),
                expected.as_str().unwrap(),
                "{name} frame {i}"
            );
            assert_eq!(
                output.shape(),
                Some([
                    case["shape"][1].as_u64().unwrap() as usize,
                    case["shape"][2].as_u64().unwrap() as usize
                ]),
                "{name}"
            );
        }
        assert!(reader.read(reader.frames(), &mut output).is_err());
        assert!(output.pixels().is_none());
    }
}
#[test]
fn independent_native_samples_endianness_and_stored_frame_order() {
    compare_manifest(&fixture("provenance.json"), fixture("").as_path());
}
/// Opt-in external corpus; no large or upstream-licensed files are bundled.
#[test]
fn upstream_corpus_all_declared_frames() {
    if let Ok(path) = std::env::var("CONTAINER_PARITY_MANIFEST") {
        let path = Path::new(&path);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        compare_manifest(path, Path::new(json["directory"].as_str().unwrap()));
    }
}
#[test]
fn original_headers_and_explicit_complex_conversion() {
    let reader = spe::Reader::open(fixture("spe-v2-int32.spe"), Limits::default()).unwrap();
    assert_eq!(reader.metadata().fields["time"], "11/05/2026 12:13:14");
    assert_eq!(reader.metadata().fields["exposure_time"], "0.5");
    assert_eq!(reader.metadata().x_calibration, [0.5, 1., 1.5]);
    assert_eq!(
        reader.metadata().calibration_coefficients,
        Some([0., 0.5, 0., 0., 0., 0.])
    );
    let reader = spe::Reader::open(fixture("spe-v3-uint16.spe"), Limits::default()).unwrap();
    assert_eq!(
        reader.metadata().fields["time"],
        "11/05/2026 12:13:14.123456"
    );
    assert_eq!(reader.metadata().fields["exposure_time"], "0.1");
    assert_eq!(reader.metadata().fields["detector"], "Synthetic");
    assert_eq!(reader.metadata().x_calibration, [0.5, 1., 1.5]);
    let reader = mrc::Reader::open(fixture("mrc-mode4-be.mrc"), Limits::default()).unwrap();
    assert_eq!(reader.metadata().extended_header, b"OriginalExtHdr!!");
    assert_eq!(
        [
            reader.metadata().parsed.mapc,
            reader.metadata().parsed.mapr,
            reader.metadata().parsed.maps
        ],
        [3, 1, 2]
    );
    assert_eq!(&reader.metadata().header[224..241], b"Stored rows only!");
    for endian in ["le", "be"] {
        let reader = mrc::Reader::open(
            fixture(&format!("mrc-mode3-{endian}.mrc")),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(reader.metadata().pixel_type, PixelType::CI16);
        let mut output = FrameBuffer::default();
        reader.read_frame_as_complex32_into(0, &mut output).unwrap();
        let PixelBuffer::C32(v) = output.pixels().unwrap() else {
            panic!("type")
        };
        assert_eq!(
            v,
            &[
                [-32768., 32767.],
                [0., -1.],
                [1., 2.],
                [3., 4.],
                [5., 6.],
                [7., 8.]
            ]
        );
    }
    let mut reader = dm3::Reader::open(fixture("dm3-int32-be.dm3"), Limits::default()).unwrap();
    assert!(!reader.metadata().little_endian);
    assert_eq!(reader.metadata().images[0].source_dimensions, [3, 2]);
    let tag = reader
        .metadata()
        .tags
        .iter()
        .position(|t| t.path.last().is_some_and(|p| p == "Binning"))
        .unwrap();
    assert_eq!(reader.read_tag_raw(tag).unwrap(), [0, 0, 0, 1, 0, 0, 0, 1]);
    assert!(reader.select_image(1).is_err());
    reader.select_image(0).unwrap();
}
#[test]
fn truncated_mutated_unsupported_and_resource_limited_sources_fail() {
    let temp = std::env::temp_dir().join(format!("dioptrin-containers-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    for (name, kind) in [
        ("spe-v2-int16.spe", "spe"),
        ("spe-v3-float32.spe", "spe"),
        ("mrc-mode4-le.mrc", "mrc"),
        ("dm3-uint32-le.dm3", "dm3"),
    ] {
        let original = std::fs::read(fixture(name)).unwrap();
        let path = temp.join(name);
        for n in [0, 11, original.len() - 1] {
            std::fs::write(&path, &original[..n]).unwrap();
            assert!(
                Reader::open(&path, kind, Limits::default()).is_err(),
                "{name} truncate {n}"
            );
        }
        std::fs::write(&path, &original).unwrap();
        let reader = Reader::open(&path, kind, Limits::default()).unwrap();
        let mut output = FrameBuffer::default();
        reader.read(0, &mut output).unwrap();
        std::fs::write(&path, b"changed").unwrap();
        assert!(reader.read(0, &mut output).is_err());
        assert!(output.pixels().is_none());
        std::fs::write(&path, &original).unwrap();
        for limits in [
            Limits {
                frame_bytes: 1,
                ..Limits::default()
            },
            Limits {
                metadata_bytes: 1,
                ..Limits::default()
            },
            Limits {
                frames: 1,
                ..Limits::default()
            },
        ] {
            // Single-plane DM3 fits the frame-count limit, but its tag index does not.
            assert!(
                Reader::open(&path, kind, limits).is_err(),
                "{name} {limits:?}"
            );
        }
    }
    let path = temp.join("bad.mrc");
    let mut bytes = std::fs::read(fixture("mrc-mode1-le.mrc")).unwrap();
    bytes[212..216].fill(0);
    std::fs::write(&path, &bytes).unwrap();
    assert!(mrc::Reader::open(&path, Limits::default()).is_err());
    let reader =
        mrc::Reader::open_with_byte_order(&path, Limits::default(), mrc::ByteOrder::Little)
            .unwrap();
    assert_eq!(&reader.metadata().header[212..216], &[0, 0, 0, 0]);
    bytes[12..16].copy_from_slice(&12i32.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();
    assert!(
        mrc::Reader::open_with_byte_order(&path, Limits::default(), mrc::ByteOrder::Little)
            .is_err()
    );
    let path = temp.join("bad.spe");
    let mut bytes = std::fs::read(fixture("spe-v2-int16.spe")).unwrap();
    bytes[108..110].copy_from_slice(&99u16.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();
    assert!(spe::Reader::open(&path, Limits::default()).is_err());
    let path = temp.join("bad.dm3");
    let mut bytes = std::fs::read(fixture("dm3-int32-le.dm3")).unwrap();
    bytes[11] = 2;
    std::fs::write(&path, &bytes).unwrap();
    assert!(dm3::Reader::open(&path, Limits::default()).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn multiple_regions_padding_and_frame_metadata_are_not_pixels() {
    let reader =
        spe::Reader::open(fixture("spe-v3-multiple-regions.spe"), Limits::default()).unwrap();
    assert_eq!(reader.metadata().frame_stride, 26);
    assert_eq!(reader.metadata().frame_metadata_bytes, 8);
    assert_eq!(reader.metadata().regions.len(), 2);
    assert_eq!(reader.metadata().regions[1].offset, 14);
    let mut output = FrameBuffer::default();
    reader.read_region_into(1, 1, &mut output).unwrap();
    assert_eq!(output.shape(), Some([1, 2]));
    assert_eq!(output.pixels(), Some(&PixelBuffer::U16(vec![300, 400])));
    assert!(reader.read_region_into(0, 2, &mut output).is_err());
    assert!(output.pixels().is_none());
    for endian in ["le", "be"] {
        let mut reader = dm3::Reader::open(
            fixture(&format!("dm3-multiple-images-stack-{endian}.dm3")),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(reader.metadata().images.len(), 2);
        assert_eq!(reader.metadata().selected_image, 1);
        assert_eq!(reader.metadata().images[1].source_dimensions, [3, 2, 2]);
        assert_eq!(reader.frames(), 2);
        reader.read_frame_into(1, &mut output).unwrap();
        assert_eq!(
            output.pixels(),
            Some(&PixelBuffer::I32(vec![4, 5, 6, 7, 8, 9]))
        );
        reader.select_image(0).unwrap();
        reader.read_frame_into(0, &mut output).unwrap();
        assert_eq!(output.shape(), Some([1, 1]));
        assert_eq!(output.pixels(), Some(&PixelBuffer::U16(vec![999])));
        let tag = reader
            .metadata()
            .tags
            .iter()
            .find(|t| t.path.last().is_some_and(|n| n == "Named Struct"))
            .unwrap();
        assert_eq!(tag.bytes, 9);
        assert_eq!(&tag.raw.as_ref().unwrap()[..5], b"abcxy");
        let tag = reader
            .metadata()
            .tags
            .iter()
            .find(|t| t.path.last().is_some_and(|n| n == "Unicode"))
            .unwrap();
        assert_eq!(tag.bytes, 6);
    }
}

#[test]
fn malformed_layouts_fail_before_exposing_counts() {
    let directory = std::env::temp_dir().join(format!(
        "dioptrin-container-bad-layouts-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("malformed.spe");
    let original = std::fs::read(fixture("spe-v3-multiple-regions.spe")).unwrap();
    let xml_start = u64::from_le_bytes(original[678..686].try_into().unwrap()) as usize;
    let xml = std::str::from_utf8(&original[xml_start..]).unwrap();
    for invalid in [
        xml.replace("count=\"2\"", "count=\"3\""),
        xml.replace("size=\"12\"", "size=\"10\""),
        xml.replace("stride=\"26\"", "stride=\"17\""),
        xml.replace("width=\"2\"", "width=\"0\""),
        xml.replace("MonochromeUnsigned16", "MonochromeSigned16"),
        format!("<!DOCTYPE SpeFormat [<!ENTITY x 'danger'>]>{xml}"),
    ] {
        let mut bytes = original[..xml_start].to_vec();
        bytes.extend(invalid.as_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(spe::Reader::open(&path, Limits::default()).is_err());
    }
    let original = std::fs::read(fixture("dm3-uint32-le.dm3")).unwrap();
    let path = directory.join("malformed.dm3");
    let separator = original.windows(4).position(|b| b == b"%%%%").unwrap();
    for mutation in [0, 1, 2, 3] {
        let mut bytes = original.clone();
        match mutation {
            0 => bytes[separator] = 0,
            1 => bytes[separator + 4..separator + 8].copy_from_slice(&u32::MAX.to_be_bytes()),
            2 => bytes[separator + 8..separator + 12].copy_from_slice(&99u32.to_be_bytes()),
            3 => bytes[separator + 16..separator + 20].copy_from_slice(&u32::MAX.to_be_bytes()),
            _ => unreachable!(),
        }
        std::fs::write(&path, bytes).unwrap();
        assert!(dm3::Reader::open(&path, Limits::default()).is_err());
    }
    let mut bytes = std::fs::read(fixture("mrc-mode1-le.mrc")).unwrap();
    let path = directory.join("malformed.mrc");
    bytes[212..216].copy_from_slice(&[0x11, 0x11, 0, 0]);
    std::fs::write(&path, &bytes).unwrap();
    assert!(mrc::Reader::open(&path, Limits::default()).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}
