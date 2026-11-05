//! Complete count/metadata validation for optional record-based readers.
use dioptrin_io::{PixelBuffer, fit2d, kcd, mpa};
use sha2::{Digest, Sha256};
fn frame(name: &str, buffer: &fit2d::FrameBuffer) -> serde_json::Value {
    let pixels = buffer.pixels().unwrap();
    let mut hash = Sha256::new();
    macro_rules! hash {($($variant:ident);*)=>{match pixels{$(PixelBuffer::$variant(v)=>{for x in v{hash.update(x.to_le_bytes());}},)*_=>panic!("unexpected source type")}};}
    hash!(U8;U16;I32;F32;F64);
    serde_json::json!({"name":name,"shape":buffer.shape(),"dtype":format!("{:?}",pixels.pixel_type()),"frame_sha256":format!("{:x}",hash.finalize())})
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: record_probe fit2d|kcd|kcd-summed|mpa FILE".into());
    }
    let mut output = Vec::new();
    let mut buffer = fit2d::FrameBuffer::default();
    match args[1].as_str() {
        "fit2d" => {
            let reader = fit2d::Reader::open(&args[2], fit2d::Limits::default())?;
            for (index, array) in reader.metadata().arrays.iter().enumerate() {
                reader.read_array_into(index, &mut buffer)?;
                output.push(frame(&array.name, &buffer));
            }
        }
        "kcd" | "kcd-summed" => {
            let summed = args[1] == "kcd-summed";
            let reader = kcd::Reader::open_with_interpretation(
                &args[2],
                kcd::Limits::default(),
                if summed {
                    kcd::Interpretation::SumReadouts
                } else {
                    kcd::Interpretation::StoredReadouts
                },
            )?;
            for index in 0..reader.frames() {
                reader.read_frame_into(index, &mut buffer)?;
                output.push(frame(
                    &if summed {
                        "sum".into()
                    } else {
                        format!("readout{index}")
                    },
                    &buffer,
                ));
            }
        }
        "mpa" => {
            let reader = mpa::Reader::open(&args[2], mpa::Limits::default())?;
            for (index, section) in reader.metadata().sections.iter().enumerate() {
                reader.read_section_into(index, &mut buffer)?;
                output.push(frame(&section.name, &buffer));
            }
        }
        _ => return Err("unsupported reader".into()),
    }
    println!("{}", serde_json::json!({"arrays":output}));
    Ok(())
}
