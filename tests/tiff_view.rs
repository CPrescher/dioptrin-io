#![cfg(feature = "tiff")]
use dioptrin_io::{
    PixelBuffer,
    tiff::{
        FrameBuffer, Limits, Reader, SampleLayout, TagValue,
        view::{self, Operation, Pixels, Policy},
    },
};
use std::path::{Path, PathBuf};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/tiff-view")
        .join(name)
}
fn bytes(values: &PixelBuffer) -> Vec<u8> {
    macro_rules! scalar {
        ($v:expr) => {
            $v.iter().flat_map(|v| v.to_le_bytes()).collect()
        };
    }
    match values {
        PixelBuffer::U8(v) => v.clone(),
        PixelBuffer::U16(v) => scalar!(v),
        PixelBuffer::F32(v) => scalar!(v),
        _ => panic!("unexpected independent fixture scalar"),
    }
}
fn output_bytes(image: &view::Image) -> Vec<u8> {
    match &image.pixels {
        Pixels::Scalar(v) => bytes(v),
        Pixels::Bool(v) => v.iter().map(|&v| u8::from(v)).collect(),
    }
}
#[test]
fn independent_raw_samples_palette_indices_and_float_whiteiszero_remain_exact() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let mut reader = Reader::open(fixture(name), Limits::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut frame = FrameBuffer::default();
        if reader.metadata(0).unwrap().sample_layout != SampleLayout::Uniform {
            // Historical .raw JPEG fixtures contain expanded full-raster YCbCr.
            // Native sampled grids now have their own component API/oracles.
            assert!(reader.metadata(0).unwrap().uniform_sample_shape().is_none());
            assert!(reader.read_frame_into(0, &mut frame).is_err());
            assert!(frame.pixels().is_none());
            continue;
        }
        reader
            .read_frame_into(0, &mut frame)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let expected = std::fs::read(fixture(&(name.clone() + ".raw"))).unwrap();
        if case["lossy"] == true {
            let actual = bytes(frame.pixels().unwrap());
            assert_eq!(actual.len(), expected.len());
            assert!(
                actual
                    .iter()
                    .zip(&expected)
                    .all(|(&a, &b)| a.abs_diff(b) <= 1),
                "lossy IDCT qualification: {name}"
            );
        } else {
            assert_eq!(bytes(frame.pixels().unwrap()), expected, "{name}");
        }
        let metadata = reader.metadata(0).unwrap();
        assert_eq!(
            serde_json::json!(metadata.sample_shape()),
            case["raw_shape"]
        );
        if name.starts_with("palette") {
            assert_eq!(metadata.photometric_interpretation, 3);
            assert_eq!(metadata.tags.get(&262), Some(&TagValue::Short(3)));
            assert!(metadata.tags.contains_key(&320));
        }
        if name.starts_with("white-float") {
            assert_eq!(metadata.photometric_interpretation, 0);
            assert_eq!(metadata.tags.get(&262), Some(&TagValue::Short(0)));
        }
        if name == "palette-bigtiff.tif" {
            assert_eq!(reader.frames(), 2);
            reader.read_frame_into(1, &mut frame).unwrap();
            assert_eq!(frame.pixels(), Some(&PixelBuffer::U8(vec![1, 2, 4, 3])));
        }
    }
}
#[test]
fn independently_decoded_pillow_views_match_without_modifying_source_counts() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("provenance.json")).unwrap()).unwrap();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let mut reader = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut frame = FrameBuffer::default();
        if reader.metadata(0).unwrap().sample_layout != SampleLayout::Uniform {
            // Historical .raw JPEG fixtures contain expanded full-raster YCbCr.
            // Native sampled grids now have their own component API/oracles.
            assert!(reader.metadata(0).unwrap().uniform_sample_shape().is_none());
            assert!(reader.read_frame_into(0, &mut frame).is_err());
            assert!(frame.pixels().is_none());
            continue;
        }
        reader.read_frame_into(0, &mut frame).unwrap();
        let meta = reader.metadata(0).unwrap();
        let before = bytes(frame.pixels().unwrap());
        if case["reference_defect"] == true {
            let error = view::convert(meta, &frame, Policy::Pillow13, 1024).unwrap_err();
            assert!(error.to_string().contains("reference layout defect"));
            // Independent 16-bit source values survive; the defect is confined to
            // the pinned display oracle's planar unpacking, not our sample decode.
        } else if case.get("pillow_dtype").is_some() && case["lossy"] != true {
            let image = view::convert(meta, &frame, Policy::Pillow13, 2048)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                output_bytes(&image),
                std::fs::read(fixture(&(name.clone() + ".pillow"))).unwrap(),
                "{name}"
            );
            assert_eq!(
                serde_json::json!(image.provenance.shape),
                case["pillow_shape"]
            );
            assert_eq!(image.provenance.source_shape, meta.sample_shape());
            assert_eq!(
                image.provenance.source_bits_per_sample,
                meta.bits_per_sample
            );
            assert_eq!(
                image.provenance.source_photometric,
                meta.photometric_interpretation
            );
        }
        assert_eq!(
            bytes(frame.pixels().unwrap()),
            before,
            "source mutated: {name}"
        );
        assert!(view::convert(meta, &frame, Policy::Pillow13, 1).is_err());
    }
}
#[test]
fn palette_lookup_and_tiffio_view_keep_distinct_precision_and_provenance() {
    for name in ["palette8.tif", "palette16.tif"] {
        let mut reader = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut frame = FrameBuffer::default();
        reader.read_frame_into(0, &mut frame).unwrap();
        let meta = reader.metadata(0).unwrap();
        for (policy, suffix) in [
            (Policy::PaletteRgb16, ".palette16"),
            (Policy::FabioTiffIo, ".tiffio"),
        ] {
            let image = view::convert(meta, &frame, policy, 1024).unwrap();
            assert_eq!(
                output_bytes(&image),
                std::fs::read(fixture(&(name.to_owned() + suffix))).unwrap()
            );
            assert!(
                image
                    .provenance
                    .operations
                    .contains(&Operation::PaletteLookup)
            );
            assert_eq!(
                image
                    .provenance
                    .operations
                    .contains(&Operation::PaletteDownscale8),
                policy == Policy::FabioTiffIo
            );
        }
    }
}
#[test]
fn adapter_admission_rejects_tag_aliases_cycles_truncation_and_metadata_bounds() {
    let source = std::fs::read(fixture("palette8.tif")).unwrap();
    let path = std::env::temp_dir().join(format!(
        "dioptrin-tiff-view-admission-{}",
        std::process::id()
    ));
    let ifd = u32::from_le_bytes(source[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(source[ifd..ifd + 2].try_into().unwrap()) as usize;
    let tags: Vec<_> = (0..count).map(|i| ifd + 2 + 12 * i).collect();
    let photo = *tags
        .iter()
        .find(|&&p| source[p..p + 2] == 262u16.to_le_bytes())
        .unwrap();
    let strip = *tags
        .iter()
        .find(|&&p| source[p..p + 2] == 273u16.to_le_bytes())
        .unwrap();
    let mut cases = Vec::new();
    let mut alias = source.clone();
    alias[strip + 8..strip + 12].copy_from_slice(&((photo + 8) as u32).to_le_bytes());
    cases.push(alias);
    let mut cycle = source.clone();
    let next = ifd + 2 + 12 * count;
    cycle[next..next + 4].copy_from_slice(&(ifd as u32).to_le_bytes());
    cases.push(cycle);
    cases.push(source[..ifd + 5].to_vec());
    let mut huge = source.clone();
    huge[ifd..ifd + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    cases.push(huge);
    for case in cases {
        std::fs::write(&path, case).unwrap();
        assert!(Reader::open(&path, Limits::default()).is_err());
    }
    assert!(
        Reader::open(
            fixture("palette16.tif"),
            Limits {
                metadata_bytes: 1024,
                ..Limits::default()
            }
        )
        .is_err()
    );
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "jpeg")]
#[test]
fn native_jpeg_display_views_match_independent_pillow_including_edge_tiles() {
    for name in ["jpeg-ycbcr-strips.tif", "jpeg-ycbcr-tiles.tif"] {
        let mut reader = Reader::open(fixture(name), Limits::default()).unwrap();
        let image = view::read_pillow13(&mut reader, 0, 4096).unwrap();
        assert_eq!(
            output_bytes(&image),
            std::fs::read(fixture(&(name.to_owned() + ".pillow"))).unwrap(),
            "{name}"
        );
        assert_eq!(image.provenance.source_photometric, 6);
        assert_eq!(image.provenance.shape, vec![17, 19]);
        assert!(
            image
                .provenance
                .operations
                .contains(&Operation::JpegRgbDecode)
        );
        assert!(image.provenance.source_lossy);
        assert!(view::read_pillow13(&mut reader, 0, 1).is_err());
    }
}

#[test]
fn opaque_description_retains_original_type_count_ifd_and_literal_inline_byte() {
    for name in [
        "opaque-description-uint16.tif",
        "opaque-description-float32.tif",
    ] {
        let mut reader = Reader::open(fixture(name), Limits::default()).unwrap();
        let metadata = reader.metadata(0).unwrap();
        let tag = &metadata.opaque_tags[&270];
        assert_eq!(tag.field_type, 2);
        assert_eq!(tag.count, 1);
        assert_eq!(tag.value_bytes, vec![175]);
        assert_eq!(tag.ifd_entry.len(), 12);
        assert_eq!(&tag.ifd_entry[..4], &[14, 1, 2, 0]);
        assert!(!metadata.tags.contains_key(&270));
        assert!(tag.reason.contains("without text/offset repair"));
        let mut frame = FrameBuffer::default();
        reader.read_frame_into(0, &mut frame).unwrap();
        assert_eq!(
            bytes(frame.pixels().unwrap()),
            std::fs::read(fixture(&(name.to_owned() + ".raw"))).unwrap()
        );
    }
    // The same malformed representation on a structural tag is never quarantined.
    let mut source = std::fs::read(fixture("opaque-description-uint16.tif")).unwrap();
    let ifd = u32::from_le_bytes(source[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(source[ifd..ifd + 2].try_into().unwrap()) as usize;
    let description = (0..count)
        .map(|i| ifd + 2 + 12 * i)
        .find(|&p| source[p..p + 2] == 270u16.to_le_bytes())
        .unwrap();
    source[description..description + 2].copy_from_slice(&339u16.to_le_bytes());
    let path =
        std::env::temp_dir().join(format!("dioptrin-opaque-structural-{}", std::process::id()));
    std::fs::write(&path, source).unwrap();
    assert!(Reader::open(&path, Limits::default()).is_err());
    std::fs::remove_file(path).unwrap();
}
