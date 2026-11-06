#![cfg(feature = "xsd")]
use dioptrin_io::{
    PixelBuffer,
    xsd::{FrameBuffer, Limits, Reader},
};
use sha2::{Digest, Sha256};
fn sample_hash(pixels: &PixelBuffer) -> String {
    let mut h = Sha256::new();
    macro_rules! scalar {($($v:ident),*)=> { match pixels {$(PixelBuffer::$v(a)=>{for x in a {h.update(x.to_le_bytes());}}),*, _=>unreachable!()} };}
    match pixels {
        PixelBuffer::C32(a) => {
            for x in a {
                for y in x {
                    h.update(y.to_le_bytes());
                }
            }
        }
        PixelBuffer::C64(a) => {
            for x in a {
                for y in x {
                    h.update(y.to_le_bytes());
                }
            }
        }
        _ => {
            scalar!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64);
        }
    }
    format!("{:x}", h.finalize())
}
#[test]
fn independent_base_encodings_scalars_and_complex_samples_match_all_bits() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/xsd");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("provenance.json")).unwrap()).unwrap();
    let mut buffer = FrameBuffer::default();
    for (name, case) in manifest["files"].as_object().unwrap() {
        let path = root.join(name);
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
            case["file_sha256"].as_str().unwrap()
        );
        let reader = Reader::open(path, Limits::default()).unwrap();
        reader
            .read_frame_into(&mut buffer)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(buffer.shape(), Some([2, 3]));
        assert_eq!(
            sample_hash(buffer.pixels().unwrap()),
            case["sample_sha256"].as_str().unwrap(),
            "{name}"
        );
    }
}
#[test]
fn malformed_xml_ambiguous_shapes_checksums_lengths_and_changed_sources_fail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.xml");
    let good = "<XSDataImage><shape>3</shape><shape>1</shape><size>3</size><dtype>uint8</dtype><coding><value>base64</value></coding><data>AP8B</data></XSDataImage>";
    for bad in [
        good.replace("<size>3", "<size>2"),
        good.replace("AP8B", "AP8="),
        good.replace("uint8", "object"),
        good.replace("base64", "guess"),
        good.replace("AP8B", "AP8?"),
        good.replace("</XSDataImage>", "<shape>1</shape></XSDataImage>"),
        good.replace("</XSDataImage>", "<dtype>uint8</dtype></XSDataImage>"),
        format!("<!DOCTYPE X [<!ENTITY a '3'>]>{good}"),
    ] {
        std::fs::write(&path, bad).unwrap();
        if let Ok(reader) = Reader::open(&path, Limits::default()) {
            let mut b = FrameBuffer::default();
            assert!(reader.read_frame_into(&mut b).is_err());
            assert!(b.pixels().is_none());
        }
    }
    std::fs::write(&path, good).unwrap();
    assert!(
        Reader::open(
            &path,
            Limits {
                encoded_bytes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        Reader::open(
            &path,
            Limits {
                metadata_bytes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let mut b = FrameBuffer::default();
    reader.read_frame_into(&mut b).unwrap();
    std::fs::write(&path, good.replace("AP8B", "AQIB")).unwrap();
    // Keep this a same-size edit, but make its metadata change observable even
    // when both writes fall within one Windows filesystem timestamp tick.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified + std::time::Duration::from_secs(1))
        .unwrap();
    assert!(reader.read_frame_into(&mut b).is_err());
    assert!(b.pixels().is_none());
    let bad = good.replace(
        "</XSDataImage>",
        "<md5sum><value>00000000000000000000000000000000</value></md5sum></XSDataImage>",
    );
    std::fs::write(&path, bad).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert!(reader.read_frame_into(&mut b).is_err());
    assert!(b.pixels().is_none());
}

#[test]
fn descriptors_must_belong_to_one_image_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ambiguous.xml");
    for xml in [
        "<root><a><shape>3</shape><shape>1</shape></a><b><size>3</size><dtype>uint8</dtype><coding><value>base64</value></coding><data>AP8B</data></b></root>",
        "<XSDataImage><shape>3</shape><shape>1</shape><size>3</size><dtype>uint8</dtype><coding><value>base64</value><value>base16</value></coding><data>AP8B</data></XSDataImage>",
        "<root><XSDataImage><shape>3</shape><shape>1</shape><size>3</size><dtype>uint8</dtype><coding><value>base64</value></coding><data>AP8B</data></XSDataImage><XSDataImage><data>AP8B</data></XSDataImage></root>",
    ] {
        std::fs::write(&path, xml).unwrap();
        assert!(Reader::open(&path, Limits::default()).is_err());
    }
}

#[test]
fn xml_data_attributes_are_retained_charged_and_namespaces_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("attributes.xml");
    let xml = "<XSDataImage><shape>1</shape><shape>1</shape><size>1</size><dtype>uint8</dtype><coding><value>base64</value></coding><data label=\"stored\">/w==</data></XSDataImage>";
    std::fs::write(&path, xml).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let meta = reader.metadata();
    let mut reconstructed = meta.header_parts[0].clone();
    reconstructed.extend(b"/w==");
    reconstructed.extend(&meta.header_parts[1]);
    assert_eq!(reconstructed, xml.as_bytes());
    let large = xml.replace("stored", &"x".repeat(2048));
    std::fs::write(&path, &large).unwrap();
    assert!(
        Reader::open(
            &path,
            Limits {
                header_bytes: 512,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let namespaced = xml.replace("<XSDataImage>", "<XSDataImage xmlns=\"urn:unrelated\">");
    std::fs::write(&path, namespaced).unwrap();
    assert!(Reader::open(&path, Limits::default()).is_err());
}
