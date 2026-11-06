//! Scoped explicit-VDS source proof; no production reference runtime.
use dioptrin_io::{
    PixelBuffer,
    hdf5::typed::{self, FilenameBinding, UnmappedRegions, VdsOptions},
};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
fn hash(pixels: &PixelBuffer) -> String {
    let mut hash = Sha256::new();
    match pixels {
        PixelBuffer::C32(v) => {
            for x in v.iter().flatten() {
                hash.update(x.to_le_bytes());
            }
            return format!("{:x}", hash.finalize());
        }
        PixelBuffer::C64(v) => {
            for x in v.iter().flatten() {
                hash.update(x.to_le_bytes());
            }
            return format!("{:x}", hash.finalize());
        }
        PixelBuffer::CI16(v) => {
            for x in v.iter().flatten() {
                hash.update(x.to_le_bytes());
            }
            return format!("{:x}", hash.finalize());
        }
        _ => {}
    }
    macro_rules! values{($($v:ident);*)=>{match pixels{$(PixelBuffer::$v(values)=>for value in values{hash.update(value.to_le_bytes());},)*_=>unreachable!()}};}
    values!(Bool;F16;U8;I8;U16;I16;U32;I32;U64;I64;F32;F64);
    format!("{:x}", hash.finalize())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: vds_probe MASTER BINDINGS.json".into());
    }
    let input: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let mut bindings = Vec::new();
    for entry in input.as_array().ok_or("bindings must be an array")? {
        bindings.push(FilenameBinding {
            stored_filename: entry["stored_filename"]
                .as_str()
                .ok_or("missing stored_filename")?
                .into(),
            actual_file: PathBuf::from(entry["actual_file"].as_str().ok_or("missing actual_file")?),
        });
    }
    let reader = typed::Reader::open_snapshot_with_vds(
        &args[1],
        typed::Selection::Lambda,
        typed::Options {
            external_links: typed::ExternalLinks::FollowSnapshot,
            ..typed::Options::default()
        },
        VdsOptions {
            filenames: &bindings,
            unmapped_regions: UnmappedRegions::DeclaredFill,
        },
    )?;
    let mut output = typed::FrameBuffer::default();
    for index in 0..reader.frames() {
        reader.read_frame_into(index, &mut output)?;
        let meta = reader.metadata(index).unwrap();
        let vds = meta
            .virtual_dataset
            .as_ref()
            .ok_or("not a virtual dataset")?;
        println!(
            "{}",
            serde_json::json!({"frame":index,"frames":reader.frames(),"shape":output.shape(),"dtype":format!("{:?}",meta.pixel_type),"sample_sha256":hash(output.pixels().unwrap()),"unmapped_pixels_per_frame":vds.unmapped_pixels_per_frame,"declared_shape":vds.declared_shape,"frozen_shape":vds.frozen_shape,"sources":reader.sources().iter().map(|s|&s.path).collect::<Vec<_>>(),"mappings":vds.mappings.iter().map(|m|serde_json::json!({"stored_filename":m.stored_filename,"source_file":m.source_file,"declared_dataset":m.declared_dataset,"resolved_dataset":m.resolved_dataset,"source_shape":m.source_shape,"source_origin":m.source_origin,"virtual_origin":m.virtual_origin,"mapped_shape":m.mapped_shape})).collect::<Vec<_>>() })
        );
    }
    Ok(())
}
