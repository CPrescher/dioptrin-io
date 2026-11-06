//! Scoped EDF source-window audit. No global suite adapter or runtime Python.
use dioptrin_io::{PixelBuffer, PixelType, edf, native};
use sha2::{Digest, Sha256};
fn hash(pixels: &PixelBuffer) -> String {
    let mut h = Sha256::new();
    macro_rules! bytes{($($v:ident);*)=>{match pixels{$(PixelBuffer::$v(values)=>{for value in values{h.update(value.to_le_bytes());}},)*_=>panic!("not an EDF scalar")}};}
    bytes!(Bool;F16;U8;U16;U32;U64;I8;I16;I32;I64;F32;F64);
    format!("{:x}", h.finalize())
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 3 {
        return Err(
            "usage: edf_rejection_probe strict|up-to|raw FILE [WIDTH HEIGHT OFFSET DTYPE]".into(),
        );
    }
    let mut buffer = edf::FrameBuffer::default();
    if args[1] == "raw" {
        if args.len() != 7 {
            return Err("raw needs WIDTH HEIGHT OFFSET DTYPE".into());
        }
        let ty = match args[6].as_str() {
            "float32" => PixelType::F32,
            "int32" => PixelType::I32,
            _ => return Err("unsupported scoped raw dtype".into()),
        };
        let reader = native::Reader::open_raw(
            &args[2],
            native::RawLayout {
                shape: [args[4].parse()?, args[3].parse()?],
                pixel_type: ty,
                little_endian: true,
                offset: native::Offset::Absolute(args[5].parse()?),
            },
            edf::Limits::default(),
        )?;
        reader.read_frame_into(0, &mut buffer)?;
        println!(
            "{}",
            serde_json::json!({"frames":1,"frame":0,"shape":buffer.shape(),"dtype":format!("{ty:?}"),"sample_sha256":hash(buffer.pixels().unwrap()),"interpretation":"explicit raw layout, no EDF compression interpretation"})
        );
    } else {
        let window = match args[1].as_str() {
            "strict" => edf::ExternalWindow::Exact,
            "up-to" => edf::ExternalWindow::UpToDeclared,
            _ => return Err("unsupported mode".into()),
        };
        let reader = edf::Reader::open_with_options(
            &args[2],
            edf::Limits::default(),
            edf::Options {
                external_payloads: edf::ExternalPayloads::FollowCompressedSnapshot,
                external_basenames: true,
                external_window: window,
                ..edf::Options::default()
            },
        )?;
        for index in 0..reader.frames() {
            reader.read_frame_into(index, &mut buffer)?;
            let meta = reader.metadata(index).unwrap();
            let source = &reader.payload_sources()[meta.payload_source];
            println!(
                "{}",
                serde_json::json!({"frame":index,"frames":reader.frames(),"shape":meta.shape,"dtype":format!("{:?}",meta.pixel_type),"sample_sha256":hash(buffer.pixels().unwrap()),"payload_source":source.path,"outer_compression":format!("{:?}",source.outer_compression),"payload_offset":meta.payload_offset,"source_bytes":source.bytes,"declared_stored_bytes":meta.declared_stored_bytes,"stored_bytes":meta.stored_bytes,"external_window_policy":meta.external_window_policy.map(|w|format!("{w:?}"))})
            );
        }
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
