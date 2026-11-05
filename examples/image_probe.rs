//! Scoped oracle probe for image codec and detector-header comparisons.
use dioptrin_io::{PixelBuffer, detector_tiff, jpeg, jpeg2000, tiff};
use serde_json::json;
use sha2::{Digest, Sha256};
fn sample_hash(pixels: &PixelBuffer) -> String {
    let mut hash = Sha256::new();
    macro_rules! scalar {
        ($v:expr) => {
            for value in $v {
                hash.update(value.to_le_bytes());
            }
        };
    }
    match pixels {
        PixelBuffer::U8(v) => hash.update(v),
        PixelBuffer::I8(v) => {
            for &value in v {
                hash.update([value as u8]);
            }
        }
        PixelBuffer::U16(v) => scalar!(v),
        PixelBuffer::U32(v) => scalar!(v),
        PixelBuffer::U64(v) => scalar!(v),
        PixelBuffer::I16(v) => scalar!(v),
        PixelBuffer::I32(v) => scalar!(v),
        PixelBuffer::I64(v) => scalar!(v),
        PixelBuffer::F32(v) => scalar!(v),
        PixelBuffer::F64(v) => scalar!(v),
        _ => panic!("unexpected codec complex samples"),
    }
    format!("{:x}", hash.finalize())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: image_probe FORMAT FILE".into());
    }
    let format = args[1].to_str().ok_or("invalid format")?;
    let path = &args[2];
    let mut frames = Vec::new();
    let mut header = serde_json::Value::Null;
    match format {
        "tiff" => {
            let mut reader = tiff::Reader::open(path, tiff::Limits::default())?;
            let mut buffer = tiff::FrameBuffer::default();
            for i in 0..reader.frames() {
                reader.read_frame_into(i, &mut buffer)?;
                let m = reader.metadata(i).unwrap();
                frames.push(json!({"shape":m.sample_shape(),"dtype":format!("{:?}",m.pixel_type),"source_bits":m.bits_per_sample,"hash":sample_hash(buffer.pixels().unwrap()),"lossy":m.lossy}));
            }
        }
        "pilatus" | "mar" => {
            let mut reader = detector_tiff::Reader::open(
                path,
                if format == "pilatus" {
                    detector_tiff::Format::Pilatus
                } else {
                    detector_tiff::Format::MarCcd
                },
                tiff::Limits::default(),
            )?;
            let mut buffer = tiff::FrameBuffer::default();
            for i in 0..reader.frames() {
                reader.read_frame_into(i, &mut buffer)?;
                let m = reader.metadata(i).unwrap();
                frames.push(json!({"shape":m.image.sample_shape(),"dtype":format!("{:?}",m.image.pixel_type),"hash":sample_hash(buffer.pixels().unwrap())}));
                if i == 0 {
                    header = match m.header {
                        detector_tiff::Header::Pilatus(h) => json!(h.fields),
                        detector_tiff::Header::MarCcd(h) => serde_json::Value::Object(
                            h.fields
                                .iter()
                                .map(|(k, v)| {
                                    (
                                        k.clone(),
                                        match v {
                                            detector_tiff::Value::Unsigned(v) => json!(v),
                                            detector_tiff::Value::Signed(v) => json!(v),
                                            detector_tiff::Value::UnsignedArray(v) => json!(v),
                                            detector_tiff::Value::SignedArray(v) => json!(v),
                                            detector_tiff::Value::ShortArray(v) => json!(v),
                                            detector_tiff::Value::Bytes(v) => json!(v),
                                        },
                                    )
                                })
                                .collect(),
                        ),
                    };
                }
            }
        }
        "jpeg" => {
            let reader = jpeg::Reader::open(path, jpeg::Limits::default())?;
            let mut buffer = jpeg::FrameBuffer::default();
            reader.read_frame_into(&mut buffer)?;
            let m = reader.metadata();
            frames.push(json!({"shape":m.sample_shape(),"dtype":format!("{:?}",m.pixel_type),"hash":sample_hash(buffer.pixels().unwrap()),"lossy":m.lossy}));
        }
        "jpeg2000" => {
            let reader = jpeg2000::Reader::open(path, jpeg2000::Limits::default())?;
            let mut buffer = jpeg2000::FrameBuffer::default();
            reader.read_frame_into(&mut buffer)?;
            let m = reader.metadata();
            frames.push(json!({"shape":m.sample_shape(),"dtype":format!("{:?}",m.pixel_type),"hash":sample_hash(buffer.pixels().unwrap()),"lossy":m.lossy}));
        }
        _ => return Err("unknown format".into()),
    }
    println!("{}", json!({"frames":frames,"header":header}));
    Ok(())
}
