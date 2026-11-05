#![cfg(any(feature = "bruker", feature = "oxford", feature = "raxis"))]
use dioptrin_io::PixelBuffer;
#[cfg(any(feature = "bruker", feature = "raxis"))]
use dioptrin_io::PixelType;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/legacy")
        .join(name)
}
fn cases(family: &str) -> Vec<(String, serde_json::Value)> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    manifest["files"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, case)| case["family"] == family)
        .map(|(name, case)| (name.clone(), case.clone()))
        .collect()
}
fn values(pixels: &PixelBuffer) -> Vec<i64> {
    macro_rules! integers {($($variant:ident);*)=> {match pixels {$(PixelBuffer::$variant(v)=>v.iter().map(|&n| i64::from(n)).collect(),)*_=>panic!("unexpected legacy pixel type")}};}
    integers!(U8;U16;U32;I8;I16;I32;I64)
}
fn check(name: &str, case: &serde_json::Value, pixels: &PixelBuffer, shape: Option<[usize; 2]>) {
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(std::fs::read(fixture(name)).unwrap())
        ),
        case["file_sha256"].as_str().unwrap(),
        "{name}"
    );
    assert_eq!(shape, Some([2, 3]));
    assert_eq!(
        format!("{:?}", pixels.pixel_type()),
        case["dtype"].as_str().unwrap(),
        "{name}"
    );
    assert_eq!(
        values(pixels),
        case["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_i64().unwrap())
            .collect::<Vec<_>>(),
        "{name}"
    );
    let mut hash = Sha256::new();
    macro_rules! hash_values {($($variant:ident);*)=> {match pixels {$(PixelBuffer::$variant(v)=>{for n in v {hash.update(n.to_le_bytes());}},)*_=>panic!("unexpected legacy type")}};}
    hash_values!(U8;U16;U32;I8;I16;I32;I64);
    assert_eq!(
        format!("{:x}", hash.finalize()),
        case["frame_sha256"].as_str().unwrap(),
        "{name}"
    );
}
struct Temp(PathBuf);
impl Temp {
    fn new(bytes: &[u8]) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "dioptrin-legacy-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(feature = "bruker")]
#[test]
fn bruker_counts_tables_types_and_linear_metadata() {
    use dioptrin_io::bruker::*;
    let mut buffer = FrameBuffer::default();
    for (name, case) in cases("bruker") {
        let reader = Reader::open(fixture(&name), Limits::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(reader.frames(), 1);
        assert_eq!(reader.metadata().header.len(), 2560);
        assert_eq!(
            reader.metadata().fields["TITLE"],
            "Independent counts\nsecond record"
        );
        reader
            .read_frame_into(&mut buffer)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        check(&name, &case, buffer.pixels().unwrap(), buffer.shape());
        reader.read_frame_into(&mut buffer).unwrap();
        if name.contains("linear") {
            assert_eq!(reader.metadata().linear, Some([0.25, 2.0]));
            assert_eq!(reader.metadata().storage_type, PixelType::U16);
        }
    }
}
#[cfg(feature = "bruker")]
#[test]
fn bruker_malformed_tables_bounds_and_mutated_source() {
    use dioptrin_io::bruker::*;
    let original = std::fs::read(fixture("bruker86-overflow.sfrm")).unwrap();
    let mut buffer = FrameBuffer::default();
    for (offset, replacement) in [
        (2572 + 16 + 9, b"      1".as_slice()),
        (2572 + 9, b"9999999".as_slice()),
        (2572, b"xxxxxxxxx".as_slice()),
        (2562, b"\x00\x00".as_slice()),
    ] {
        let mut bad = original.clone();
        bad[offset..offset + replacement.len()].copy_from_slice(replacement);
        let tmp = Temp::new(&bad);
        let reader = Reader::open(tmp.path(), Limits::default()).unwrap();
        assert!(reader.read_frame_into(&mut buffer).is_err());
        assert!(buffer.pixels().is_none());
    }
    let mut bad = original.clone();
    bad.truncate(bad.len() - 1);
    assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    let mut duplicate = original.clone();
    duplicate[14 * 80..15 * 80].copy_from_slice(&original[3 * 80..4 * 80]);
    assert!(Reader::open(Temp::new(&duplicate).path(), Limits::default()).is_err());
    for (key, value) in [
        ("WORDORD", "1"),
        ("HDRBLKS", "6"),
        ("COMPRES", "unknown"),
        ("NCOLS", "0"),
        ("FORMAT", "999"),
    ] {
        let mut bad = original.clone();
        let record = format!("{key:<7}:{value}");
        if let Some(offset) = bad
            .as_chunks::<80>()
            .0
            .iter()
            .position(|record| record.starts_with(key.as_bytes()))
            .map(|n| n * 80)
        {
            bad[offset..offset + 80].fill(b' ');
            bad[offset..offset + record.len()].copy_from_slice(record.as_bytes());
        } else {
            bad[15 * 80..16 * 80].fill(b' ');
            bad[15 * 80..15 * 80 + record.len()].copy_from_slice(record.as_bytes());
        }
        assert!(
            Reader::open(Temp::new(&bad).path(), Limits::default()).is_err(),
            "{key}"
        );
    }
    let mut baseline = std::fs::read(fixture("bruker100-under1.sfrm")).unwrap();
    let record = b"NEXP   :1 1 2147483647";
    baseline[6 * 80..7 * 80].fill(b' ');
    baseline[6 * 80..6 * 80 + record.len()].copy_from_slice(record);
    let tmp = Temp::new(&baseline);
    let reader = Reader::open(tmp.path(), Limits::default()).unwrap();
    assert!(reader.read_frame_into(&mut buffer).is_err());
    let overflow = std::fs::read(fixture("bruker100-under1.sfrm")).unwrap();
    // Existing zero no longer consumes a declared underflow entry.
    let mut bad = overflow.clone();
    bad[2560] = 2;
    let tmp = Temp::new(&bad);
    let reader = Reader::open(tmp.path(), Limits::default()).unwrap();
    assert!(reader.read_frame_into(&mut buffer).is_err());
    // Overflow1 marker count exceeds its table.
    let mut bad = overflow;
    bad[2563] = 255;
    let tmp = Temp::new(&bad);
    let reader = Reader::open(tmp.path(), Limits::default()).unwrap();
    assert!(reader.read_frame_into(&mut buffer).is_err());
    for limits in [
        Limits {
            header_bytes: 2559,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 23,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
        Limits {
            frames: 0,
            ..Limits::default()
        },
    ] {
        assert!(Reader::open(fixture("bruker86-overflow.sfrm"), limits).is_err());
    }
    let tmp = Temp::new(&original);
    let reader = Reader::open(tmp.path(), Limits::default()).unwrap();
    reader.read_frame_into(&mut buffer).unwrap();
    std::fs::write(tmp.path(), &original[..2560]).unwrap();
    assert!(reader.read_frame_into(&mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
#[cfg(feature = "oxford")]
#[test]
fn oxford_signed_ty1_all_widths_and_raw_sections() {
    use dioptrin_io::oxford::*;
    let mut buffer = FrameBuffer::default();
    for (name, case) in cases("oxford") {
        let reader = Reader::open(fixture(&name), Limits::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(reader.frames(), 1);
        assert_eq!(reader.metadata().sections.len(), 5);
        assert_eq!(
            reader.metadata().fields["Exposure time seconds"],
            HeaderValue::Float(2.5)
        );
        reader.read_frame_into(&mut buffer).unwrap();
        check(&name, &case, buffer.pixels().unwrap(), buffer.shape());
        reader.read_frame_into(&mut buffer).unwrap();
    }
}
#[cfg(feature = "oxford")]
#[test]
fn oxford_malformed_exceptions_sections_ty5_bounds_and_mutation() {
    use dioptrin_io::oxford::*;
    let original = std::fs::read(fixture("oxford-ty1-i32.img")).unwrap();
    // Every byte escape must consume exactly its declared side-table entry.
    for (offset, value) in [(5120, 255u8), (5121, 127), (5122, 255)] {
        let mut bad = original.clone();
        bad[offset] = value;
        assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    }
    let mut short = original.clone();
    short.pop();
    assert!(Reader::open(Temp::new(&short).path(), Limits::default()).is_err());
    let mut ty5 = original.clone();
    let pos = ty5.windows(3).position(|b| b == b"TY1").unwrap();
    ty5[pos + 2] = b'5';
    let error = Reader::open(Temp::new(&ty5).path(), Limits::default())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("TY5"));
    let mut bad = original.clone();
    let pos = bad.windows(10).position(|b| b == b"NG=    512").unwrap();
    bad[pos + 3..pos + 10].copy_from_slice(b"      1");
    assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    for limits in [
        Limits {
            header_bytes: 255,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 23,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(Reader::open(fixture("oxford-ty1-i32.img"), limits).is_err());
    }
    let tmp = Temp::new(&original);
    let reader = Reader::open(tmp.path(), Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(&mut buffer).unwrap();
    std::fs::write(tmp.path(), &original[..5120]).unwrap();
    assert!(reader.read_frame_into(&mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
#[cfg(feature = "raxis")]
#[test]
fn raxis_stored_words_and_explicit_photomultiplier_expansion() {
    use dioptrin_io::raxis::*;
    let mut buffer = FrameBuffer::default();
    for (name, case) in cases("raxis") {
        let byte_order = if case["little_endian"] == true {
            ByteOrder::Little
        } else {
            ByteOrder::Big
        };
        let reader = Reader::open_with_options(
            fixture(&name),
            Limits::default(),
            Options {
                byte_order,
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(reader.frames(), 1);
        assert_eq!(reader.metadata().header.len(), 1536);
        assert_eq!(reader.metadata().header[1400], b'P');
        assert_eq!(reader.metadata().storage_type, PixelType::U16);
        reader.read_frame_into(&mut buffer).unwrap();
        check(&name, &case, buffer.pixels().unwrap(), buffer.shape());
        let expanded = Reader::open_with_options(
            fixture(&name),
            Limits::default(),
            Options {
                byte_order,
                interpretation: Interpretation::PhotomultiplierExpanded,
            },
        )
        .unwrap();
        expanded.read_frame_into(&mut buffer).unwrap();
        assert_eq!(
            values(buffer.pixels().unwrap()),
            case["expanded"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_i64().unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            expanded.metadata().pixel_type,
            if expanded.metadata().has_range_flags {
                PixelType::U32
            } else {
                PixelType::U16
            }
        );
    }
}
#[cfg(feature = "raxis")]
#[test]
fn raxis_invalid_ratio_bounds_shape_and_source_mutation() {
    use dioptrin_io::raxis::*;
    let original = std::fs::read(fixture("raxis-big.img")).unwrap();
    for ratio in [f32::NAN, f32::INFINITY, -1.0, f32::MAX] {
        let mut bad = original.clone();
        bad[800..804].copy_from_slice(&ratio.to_be_bytes());
        let tmp = Temp::new(&bad);
        let stored = Reader::open(tmp.path(), Limits::default()).unwrap();
        let mut buffer = FrameBuffer::default();
        stored.read_frame_into(&mut buffer).unwrap();
        assert!(
            Reader::open_with_options(
                tmp.path(),
                Limits::default(),
                Options {
                    interpretation: Interpretation::PhotomultiplierExpanded,
                    ..Options::default()
                }
            )
            .is_err()
        );
    }
    let mut truncated = original.clone();
    truncated.pop();
    assert!(Reader::open(Temp::new(&truncated).path(), Limits::default()).is_err());
    let mut stacked = original.clone();
    stacked.extend_from_slice(&original[1536..]);
    assert!(Reader::open(Temp::new(&stacked).path(), Limits::default()).is_err());
    let mut bad = original.clone();
    bad[768..772].copy_from_slice(&i32::MAX.to_be_bytes());
    assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    assert!(Reader::open(Temp::new(&original[..1400]).path(), Limits::default()).is_err());
    for limits in [
        Limits {
            header_bytes: 1399,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 11,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(Reader::open(fixture("raxis-big.img"), limits).is_err());
    }
    let tmp = Temp::new(&original);
    let reader = Reader::open(tmp.path(), Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(&mut buffer).unwrap();
    std::fs::write(tmp.path(), &original[..1400]).unwrap();
    assert!(reader.read_frame_into(&mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}

#[cfg(feature = "bruker")]
#[test]
fn bruker_declared_underflows_require_nonzero_supported_storage_width() {
    use dioptrin_io::bruker::*;
    // This malformed source previously succeeded: no stored zero consumes the
    // declared underflow, and width0 silently gives the table zero bytes.
    let source = std::fs::read(fixture("bruker100-baseline.sfrm")).unwrap();
    for width in [0, 3] {
        let mut data = source.clone();
        for (key, value) in [
            ("NPIXELB", format!("1 {width}")),
            ("NOVERFL", "1 0 0".into()),
        ] {
            let offset = data
                .as_chunks::<80>()
                .0
                .iter()
                .position(|record| record.starts_with(key.as_bytes()))
                .unwrap()
                * 80;
            let record = format!("{key:<7}:{value}");
            data[offset..offset + 80].fill(b' ');
            data[offset..offset + record.len()].copy_from_slice(record.as_bytes());
        }
        data[2560..2566].fill(1);
        let file = Temp::new(&data);
        assert!(
            Reader::open(file.path(), Limits::default()).is_err(),
            "width {width}"
        );
    }
}

#[cfg(feature = "bruker")]
#[test]
fn explicit_bruker_linear_view_decodes_float_encoding_without_changing_raw_reads() {
    use dioptrin_io::bruker::{FrameBuffer, Limits, Reader, view};
    let reader = Reader::open(fixture("bruker86-linear.sfrm"), Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    view::read_linear_into(&reader, &mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::F32(vec![2., 3., 4., 5., 6., 7.]))
    );
    assert_eq!(reader.metadata().storage_type, PixelType::U16);
    assert_eq!(reader.metadata().linear, Some([0.25, 2.]));
    reader.read_frame_into(&mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::U16(vec![0, 4, 8, 12, 16, 20]))
    );
    let identity = Reader::open(fixture("bruker86-u8.sfrm"), Limits::default()).unwrap();
    view::read_linear_into(&identity, &mut buffer).unwrap();
    assert!(matches!(buffer.pixels(), Some(PixelBuffer::U8(_))));
    let small = Reader::open(
        fixture("bruker86-linear.sfrm"),
        Limits {
            frame_bytes: 12,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(view::read_linear_into(&small, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    let v100 = Reader::open(fixture("bruker100-u16.sfrm"), Limits::default()).unwrap();
    assert!(view::read_linear_into(&v100, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    let bytes = std::fs::read(fixture("bruker86-linear.sfrm")).unwrap();
    let temp = Temp::new(&bytes);
    let changed = Reader::open(temp.path(), Limits::default()).unwrap();
    std::fs::write(temp.path(), b"changed").unwrap();
    assert!(view::read_linear_into(&changed, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
