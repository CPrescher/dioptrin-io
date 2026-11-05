//! Reproducible one-frame validation for the optional legacy readers.
use dioptrin_io::{PixelBuffer, bruker, oxford, raxis};
use sha2::{Digest, Sha256};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err(
            "usage: legacy_probe bruker|oxford|raxis|raxis-expanded|raxis-little-expanded FILE"
                .into(),
        );
    }
    let mut buffer = bruker::FrameBuffer::default();
    match args[1].as_str() {
        "bruker" => bruker::Reader::open(&args[2], bruker::Limits::default())?
            .read_frame_into(&mut buffer)?,
        "oxford" => oxford::Reader::open(&args[2], oxford::Limits::default())?
            .read_frame_into(&mut buffer)?,
        "raxis" | "raxis-expanded" | "raxis-little-expanded" => raxis::Reader::open_with_options(
            &args[2],
            raxis::Limits::default(),
            raxis::Options {
                byte_order: if args[1].contains("little") {
                    raxis::ByteOrder::Little
                } else {
                    raxis::ByteOrder::Big
                },
                interpretation: if args[1].contains("expanded") {
                    raxis::Interpretation::PhotomultiplierExpanded
                } else {
                    raxis::Interpretation::Stored
                },
            },
        )?
        .read_frame_into(&mut buffer)?,
        _ => return Err("unsupported reader".into()),
    }
    let pixels = buffer.pixels().ok_or("invalid frame")?;
    let mut hash = Sha256::new();
    macro_rules! values {($($variant:ident);*)=> {match pixels {$(PixelBuffer::$variant(v)=>{for n in v {hash.update(n.to_le_bytes());}},)*_=>return Err("unsupported legacy sample type".into())}};}
    values!(U8;U16;U32;I8;I16;I32;I64);
    println!(
        "{}",
        serde_json::json!({"shape":buffer.shape(),"dtype":format!("{:?}",pixels.pixel_type()),"frame_sha256":format!("{:x}",hash.finalize())})
    );
    Ok(())
}
