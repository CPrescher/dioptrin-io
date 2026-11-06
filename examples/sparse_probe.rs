//! Test-only same-file sparse/reconstruction evidence; no Python runtime.
use dioptrin_io::{PixelBuffer, sparse::*};
use sha2::{Digest, Sha256};
fn hash(pixels: &PixelBuffer) -> String {
    let mut hash = Sha256::new();
    match pixels {
        PixelBuffer::C32(v) => {
            for pair in v {
                for x in pair {
                    hash.update(x.to_le_bytes());
                }
            }
            return format!("{:x}", hash.finalize());
        }
        PixelBuffer::C64(v) => {
            for pair in v {
                for x in pair {
                    hash.update(x.to_le_bytes());
                }
            }
            return format!("{:x}", hash.finalize());
        }
        PixelBuffer::CI16(v) => {
            for pair in v {
                for x in pair {
                    hash.update(x.to_le_bytes());
                }
            }
            return format!("{:x}", hash.finalize());
        }
        _ => {}
    }
    macro_rules! bytes{($($variant:ident);*)=>{match pixels{$(PixelBuffer::$variant(v)=>{for n in v{hash.update(n.to_le_bytes());}},)*_=>panic!("unsupported sparse samples")}};}
    bytes!(Bool;F16;U8;U16;U32;U64;I8;I16;I32;I64;F32;F64);
    format!("{:x}", hash.finalize())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 2 {
        return Err("usage: sparse_probe FILE [FRAME]".into());
    }
    if args[1] == "--components" {
        let path = args.get(2).ok_or("missing source file")?;
        let reader = Reader::open(path, Options::default())?;
        let mut array = ArrayBuffer::default();
        let mut components = Vec::new();
        for component in &reader.metadata().components {
            if let Some(pixel_type) = component.pixel_type {
                reader.read_component_into(&component.name, &mut array)?;
                components.push(serde_json::json!({"name":component.name,"dataset":component.dataset,"shape":component.shape,"dtype":format!("{pixel_type:?}"),"little_endian":component.little_endian,"sha256":hash(array.pixels().unwrap())}));
            } else {
                components.push(serde_json::json!({"name":component.name,"dataset":component.dataset,"shape":component.shape,"text":component.text}));
            }
        }
        println!(
            "{}",
            serde_json::json!({"components":components,"sources":reader.sources().iter().map(|s|s.path.to_string_lossy().into_owned()).collect::<Vec<_>>(),"links":reader.links().len()})
        );
        return Ok(());
    }
    let reader = Reader::open(
        &args[1],
        Options {
            reconstruction: Reconstruction::DeterministicBackground,
            ..Options::default()
        },
    )?;
    let frames: Vec<usize> = if let Some(frame) = args.get(2) {
        vec![frame.parse()?]
    } else {
        (0..reader.frames()).collect()
    };
    let mut sparse = SparseFrameBuffer::default();
    let mut buffer = FrameBuffer::default();
    for frame in frames {
        reader.read_sparse_frame_into(frame, &mut sparse)?;
        reader.read_frame_into(frame, &mut buffer)?;
        println!(
            "{}",
            serde_json::json!({"frame":frame,"frames":reader.frames(),"shape":reader.metadata().shape,"dtype":format!("{:?}",reader.metadata().intensity_type),"index_sha256":hash(sparse.index().unwrap()),"intensity_sha256":hash(sparse.intensity().unwrap()),"events":sparse.index().unwrap().len(),"dense_sha256":hash(buffer.pixels().unwrap()),"dense_is_derived":reader.metadata().dense_is_derived})
        );
    }
    Ok(())
}
