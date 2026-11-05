#![cfg(any(feature = "fit2d", feature = "kcd", feature = "mpa"))]
use dioptrin_io::PixelBuffer;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/records")
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
        .map(|(name, case)| {
            assert_eq!(
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(fixture(name)).unwrap())
                ),
                case["file_sha256"].as_str().unwrap()
            );
            (name.clone(), case.clone())
        })
        .collect()
}
fn check(expected: &serde_json::Value, pixels: &PixelBuffer, shape: Option<[usize; 2]>) {
    assert_eq!(serde_json::json!(shape), expected["shape"]);
    assert_eq!(
        format!("{:?}", pixels.pixel_type()),
        expected["dtype"].as_str().unwrap()
    );
    let mut hash = Sha256::new();
    macro_rules! hash{($($variant:ident);*)=>{match pixels{$(PixelBuffer::$variant(v)=>{for value in v{hash.update(value.to_le_bytes());}},)*_=>panic!("unexpected type")}};}
    hash!(U8;U16;I32;F32;F64);
    assert_eq!(
        format!("{:x}", hash.finalize()),
        expected["frame_sha256"].as_str().unwrap()
    );
}
struct Temp(PathBuf);
impl Temp {
    fn new(data: &[u8]) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "dioptrin-records-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, data).unwrap();
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
fn replace(data: &mut [u8], old: &[u8], new: &[u8]) {
    assert_eq!(old.len(), new.len());
    let p = data.windows(old.len()).position(|b| b == old).unwrap();
    data[p..p + new.len()].copy_from_slice(new);
}
#[cfg(feature = "fit2d")]
#[test]
fn fit2d_named_arrays_masks_byte_orders_and_float_bits() {
    use dioptrin_io::fit2d::*;
    let mut buffer = FrameBuffer::default();
    for (name, case) in cases("fit2d") {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(fixture(&name)).unwrap())
            ),
            case["file_sha256"].as_str().unwrap()
        );
        let reader = Reader::open(fixture(&name), Limits::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(reader.frames(), 1);
        assert_eq!(
            reader.metadata().little_endian,
            case["little_endian"].as_bool().unwrap()
        );
        assert_eq!(
            reader.metadata().block_bytes,
            case["block_bytes"].as_u64().unwrap() as usize
        );
        let HeaderValue::Float(gain) = reader.metadata().fields["gain"] else {
            panic!("gain")
        };
        assert_eq!(gain.to_bits(), 0x3e800000);
        assert_eq!(
            reader.metadata().fields["flags"],
            HeaderValue::Integer(u32::MAX)
        );
        for (index, expected) in case["arrays"].as_array().unwrap().iter().enumerate().rev() {
            reader.read_array_into(index, &mut buffer).unwrap();
            assert_eq!(
                reader.metadata().arrays[index].name,
                expected["name"].as_str().unwrap()
            );
            check(expected, buffer.pixels().unwrap(), buffer.shape());
        }
        reader.read_frame_into(&mut buffer).unwrap();
        check(&case["arrays"][0], buffer.pixels().unwrap(), buffer.shape());
        assert!(
            reader
                .read_array_into(reader.metadata().arrays.len(), &mut buffer)
                .is_err()
        );
        assert!(buffer.pixels().is_none());
    }
}
#[cfg(feature = "fit2d")]
#[test]
fn fit2d_malformed_records_limits_and_changed_source() {
    use dioptrin_io::fit2d::*;
    let original = std::fs::read(fixture("fit2d-512-little.f2d")).unwrap();
    for (old, new) in [
        (
            b"00000001ai00000020".as_slice(),
            b"00000002ai00000020".as_slice(),
        ),
        (
            b"ai0000002000000002".as_slice(),
            b"az0000002000000002".as_slice(),
        ),
        (
            b"ai0000002000000002".as_slice(),
            b"ai0000002000000003".as_slice(),
        ),
        (b"$FFF_END".as_slice(), b"$FFF_BAD".as_slice()),
    ] {
        let mut bad = original.clone();
        replace(&mut bad, old, new);
        assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    }
    for len in [511, original.len() - 1] {
        assert!(Reader::open(Temp::new(&original[..len]).path(), Limits::default()).is_err());
    }
    let mut bad = original.clone();
    bad.extend_from_slice(&original[..512]);
    assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    for limits in [
        Limits {
            header_bytes: 511,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 23,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: 511,
            ..Limits::default()
        },
        Limits {
            frames: 2,
            ..Limits::default()
        },
    ] {
        assert!(Reader::open(fixture("fit2d-512-little.f2d"), limits).is_err());
    }
    let tmp = Temp::new(&original);
    let reader = Reader::open(tmp.path(), Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(&mut buffer).unwrap();
    std::fs::write(tmp.path(), &original[..512]).unwrap();
    assert!(reader.read_frame_into(&mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
#[cfg(feature = "kcd")]
#[test]
fn kcd_readout_order_is_default_and_sum_is_explicit() {
    use dioptrin_io::kcd::*;
    let mut buffer = FrameBuffer::default();
    for (name, case) in cases("kcd")
        .into_iter()
        .filter(|(_, c)| c["overflow"] != true)
    {
        let reader = Reader::open(fixture(&name), Limits::default()).unwrap();
        assert_eq!(reader.frames(), 3);
        assert_eq!(
            reader.metadata().interpretation,
            Interpretation::StoredReadouts
        );
        assert_eq!(reader.metadata().fields["Mode"], "Binned");
        for (index, expected) in case["arrays"].as_array().unwrap().iter().enumerate().rev() {
            reader.read_frame_into(index, &mut buffer).unwrap();
            check(expected, buffer.pixels().unwrap(), buffer.shape());
        }
        let sum = Reader::open_with_interpretation(
            fixture(&name),
            Limits::default(),
            Interpretation::SumReadouts,
        )
        .unwrap();
        assert_eq!(sum.frames(), 1);
        sum.read_frame_into(0, &mut buffer).unwrap();
        check(&case["summed"], buffer.pixels().unwrap(), buffer.shape());
        assert!(reader.read_frame_into(3, &mut buffer).is_err());
        assert!(buffer.pixels().is_none());
    }
    let stored = Reader::open(fixture("overflow.kcd"), Limits::default()).unwrap();
    assert_eq!(stored.frames(), 32769);
    stored.read_frame_into(32768, &mut buffer).unwrap();
    let PixelBuffer::U16(v) = buffer.pixels().unwrap() else {
        panic!("type")
    };
    assert_eq!(v, &[65535]);
    let sum = Reader::open_with_interpretation(
        fixture("overflow.kcd"),
        Limits::default(),
        Interpretation::SumReadouts,
    )
    .unwrap();
    assert!(sum.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
#[cfg(feature = "kcd")]
#[test]
fn kcd_malformed_headers_layouts_bounds_and_source_changes() {
    use dioptrin_io::kcd::*;
    let original = std::fs::read(fixture("readouts.kcd")).unwrap();
    for (old, new) in [
        (b"Data type = u16".as_slice(), b"Data type = u32".as_slice()),
        (b"readouts = 3".as_slice(), b"readouts = 0".as_slice()),
        (b"X dimension = 3".as_slice(), b"X dimension = 0".as_slice()),
    ] {
        let mut bad = original.clone();
        replace(&mut bad, old, new);
        assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    }
    for len in [4099, original.len() - 1] {
        assert!(Reader::open(Temp::new(&original[..len]).path(), Limits::default()).is_err());
    }
    let mut bad = original.clone();
    bad.push(0);
    assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    for limits in [
        Limits {
            header_bytes: 4099,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 11,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: 11,
            ..Limits::default()
        },
        Limits {
            frames: 2,
            ..Limits::default()
        },
    ] {
        assert!(Reader::open(fixture("readouts.kcd"), limits).is_err());
    }
    let tmp = Temp::new(&original);
    let reader = Reader::open(tmp.path(), Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    std::fs::write(tmp.path(), &original[..4100]).unwrap();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
#[cfg(feature = "mpa")]
#[test]
fn mpa_original_spectra_maps_f64_counts_and_frame_order() {
    use dioptrin_io::mpa::*;
    let mut buffer = FrameBuffer::default();
    for (name, case) in cases("mpa") {
        let reader = Reader::open(fixture(&name), Limits::default()).unwrap();
        assert_eq!(reader.frames(), 2);
        assert_eq!(reader.metadata().source_type, None);
        assert_eq!(reader.metadata().encoding, Encoding::AsciiF64);
        assert_eq!(reader.metadata().fields["ADC2_range"], "3");
        for (index, expected) in case["arrays"].as_array().unwrap().iter().enumerate().rev() {
            reader.read_section_into(index, &mut buffer).unwrap();
            check(expected, buffer.pixels().unwrap(), buffer.shape());
        }
        reader.read_frame_into(1, &mut buffer).unwrap();
        check(&case["arrays"][3], buffer.pixels().unwrap(), buffer.shape());
        assert!(reader.read_frame_into(2, &mut buffer).is_err());
        assert!(buffer.pixels().is_none());
    }
}
#[cfg(feature = "mpa")]
#[test]
fn mpa_truncation_precision_binary_duplicate_sections_bounds_and_mutation() {
    use dioptrin_io::mpa::*;
    let original = std::fs::read(fixture("sections.mpa")).unwrap();
    for (old, new) in [
        (b"mpafmt=asc".as_slice(), b"mpafmt=bin".as_slice()),
        (b"CDAT0,6".as_slice(), b"CDAT0,7".as_slice()),
        (b"CDAT1,6".as_slice(), b"CDAT0,6".as_slice()),
        (
            b"9007199254740992".as_slice(),
            b"9007199254740993".as_slice(),
        ),
    ] {
        let mut bad = original.clone();
        replace(&mut bad, old, new);
        assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    }
    let mut bad = original.clone();
    bad.truncate(original.len() - 3);
    assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    let mut bad = original.clone();
    bad.extend_from_slice(b"7\r\n");
    assert!(Reader::open(Temp::new(&bad).path(), Limits::default()).is_err());
    for limits in [
        Limits {
            header_bytes: 16,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 47,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: 5,
            ..Limits::default()
        },
        Limits {
            frames: 3,
            ..Limits::default()
        },
    ] {
        assert!(Reader::open(fixture("sections.mpa"), limits).is_err());
    }
    let tmp = Temp::new(&original);
    let reader = Reader::open(tmp.path(), Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    std::fs::write(tmp.path(), b"mpafmt=asc\n").unwrap();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
