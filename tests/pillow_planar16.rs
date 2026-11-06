#![cfg(feature = "tiff")]
use dioptrin_io::{
    PixelBuffer,
    tiff::{
        FrameBuffer, Limits, Reader,
        view::{self, Operation, Pixels, Policy},
    },
};
use std::path::Path;

#[test]
fn planar16_default_pillow_path_distinguishes_raw_defect_from_correct_compressed_views() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/pillow-planar16");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("provenance.json")).unwrap()).unwrap();
    let mut compressed = 0;
    let mut defective = 0;
    for (name, case) in manifest["files"].as_object().unwrap() {
        let mut reader = Reader::open(root.join(name), Limits::default()).unwrap();
        let mut frame = FrameBuffer::default();
        reader.read_frame_into(0, &mut frame).unwrap();
        let metadata = reader.metadata(0).unwrap();
        let PixelBuffer::U16(source) = frame.pixels().unwrap() else {
            panic!("{name}")
        };
        let before = source.clone();
        let original: Vec<_> = source.iter().flat_map(|word| word.to_le_bytes()).collect();
        assert_eq!(
            original,
            std::fs::read(root.join(format!("{name}.raw"))).unwrap(),
            "{name}"
        );
        assert_eq!(metadata.bits_per_sample, 16);
        assert_eq!(metadata.planar_configuration, 2);
        assert_eq!(serde_json::json!(metadata.compression), case["compression"]);
        assert_eq!(serde_json::json!(metadata.sample_shape()), case["shape"]);
        assert_eq!(metadata.little_endian, case["byteorder"] == "<");
        let result = view::convert(metadata, &frame, Policy::Pillow13, 4096);
        if case["reference_defect"] == true {
            defective += 1;
            assert_eq!(metadata.compression, 1);
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("reference layout defect"),
                "{name}"
            );
        } else {
            compressed += 1;
            let image = result.unwrap_or_else(|error| panic!("{name}: {error}"));
            let Pixels::Scalar(PixelBuffer::F32(values)) = &image.pixels else {
                panic!("{name}")
            };
            let bytes: Vec<_> = values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            assert_eq!(
                bytes,
                std::fs::read(root.join(format!("{name}.pillow"))).unwrap(),
                "{name}"
            );
            assert_eq!(image.provenance.source_planar_configuration, 2);
            assert_eq!(image.provenance.source_compression, metadata.compression);
            assert_eq!(image.provenance.source_bits_per_sample, 16);
            assert!(
                image
                    .provenance
                    .operations
                    .contains(&Operation::ReduceColor16To8)
            );
            assert!(
                image
                    .provenance
                    .operations
                    .contains(&Operation::RgbLuminanceF32)
            );
            assert!(view::convert(metadata, &frame, Policy::Pillow13, 1).is_err());
        }
        assert_eq!(
            frame.pixels(),
            Some(&PixelBuffer::U16(before)),
            "source altered: {name}"
        );
    }
    assert_eq!(compressed, 48);
    assert_eq!(defective, 12);
}

#[test]
fn every_planar_tile_uses_its_spatial_grid_for_padding_across_sample_types() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/pillow-planar16");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("multitile-provenance.json")).unwrap())
            .unwrap();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let mut reader = Reader::open(root.join(name), Limits::default()).unwrap();
        let mut frame = FrameBuffer::default();
        reader.read_frame_into(0, &mut frame).unwrap();
        macro_rules! bytes {($($variant:ident);*) => {match frame.pixels().unwrap() {
            $(PixelBuffer::$variant(values) => values.iter().flat_map(|value|value.to_le_bytes()).collect::<Vec<_>>(),)*
            _ => panic!("unexpected type in {name}"),
        }}}
        let decoded = bytes!(U8;U16;U32;I16;F32;F64);
        assert_eq!(
            decoded,
            std::fs::read(root.join(format!("{name}.raw"))).unwrap(),
            "{name}"
        );
        assert_eq!(
            serde_json::json!(reader.metadata(0).unwrap().sample_shape()),
            case["shape"]
        );
        assert!(reader.read_frame_into(1, &mut frame).is_err());
        assert!(frame.pixels().is_none());
    }
}
