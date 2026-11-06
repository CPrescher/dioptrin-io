//! Scoped source recovery proof; no display data is relabeled as native counts.
use dioptrin_io::{
    PixelBuffer,
    tiff::{
        self, thunderscan,
        view::{self, Pixels, Policy},
    },
};
use serde_json::json;
use sha2::{Digest, Sha256};
fn hash(values: &PixelBuffer) -> String {
    let mut digest = Sha256::new();
    match values {
        PixelBuffer::U8(v) => digest.update(v),
        PixelBuffer::U16(v) => {
            for value in v {
                digest.update(value.to_le_bytes());
            }
        }
        PixelBuffer::F32(v) => {
            for value in v {
                digest.update(value.to_bits().to_le_bytes());
            }
        }
        _ => panic!("unsupported scoped proof sample type"),
    };
    format!("{:x}", digest.finalize())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: tiff_recovery_probe strict|compat FILE".into());
    }
    let options = match args[1].to_str() {
        Some("strict") => tiff::Options::default(),
        Some("compat") => tiff::Options {
            uncompressed_windows: tiff::UncompressedWindows::FromShape,
            tile_tables: tiff::TileTables::FromStripTags,
        },
        _ => return Err("unknown policy".into()),
    };
    let mut reader = tiff::Reader::open_with_options(&args[2], tiff::Limits::default(), options)?;
    let mut records = Vec::new();
    for index in 0..reader.frames() {
        let metadata = reader.metadata(index).unwrap().clone();
        if metadata.compression == 32809 {
            let mut frame = thunderscan::AvailableFrame::default();
            reader.read_thunderscan_available_into(index, &mut frame)?;
            records.push(json!({"frame":index,"shape":[frame.pixels().unwrap().len()],"dtype":"U8","hash":hash(frame.pixels().unwrap()),"source_recovery":{"format":"tiff-thunderscan","declared_shape":frame.shape(),"bits":metadata.bits_per_sample,"complete":frame.complete(),"available_rows":frame.available_rows().unwrap().iter().map(|s|json!({"start_row":s.start_row,"rows":s.rows,"sample_offset":s.sample_offset})).collect::<Vec<_>>(),"missing_rows":frame.missing_rows().unwrap().iter().map(|s|json!({"start_row":s.start_row,"rows":s.rows,"strip":s.strip,"reason":s.reason})).collect::<Vec<_>>(),"strips":frame.strips().unwrap().iter().map(|s|json!({"offset":s.offset,"declared_bytes":s.declared_bytes,"consumed_bytes":s.consumed_bytes,"requested_rows":s.requested_rows,"complete_rows":s.complete_rows,"integrity":format!("{:?}",s.integrity)})).collect::<Vec<_>>()}}));
        } else {
            let mut frame = tiff::FrameBuffer::default();
            reader.read_frame_into(index, &mut frame)?;
            let view = view::convert(&metadata, &frame, Policy::Pillow13, 1024 * 1024 * 1024)?;
            let (viewtype, viewhash) = match view.pixels {
                Pixels::Scalar(v) => (format!("{:?}", v.pixel_type()), hash(&v)),
                Pixels::Bool(v) => {
                    let bytes = PixelBuffer::U8(v.iter().map(|&v| u8::from(v)).collect());
                    ("bool".to_string(), hash(&bytes))
                }
            };
            let compatibility=metadata.compatibility.as_ref().map(|v|json!({"source_ifd_offset":v.source_ifd_offset,"operations":v.operations.iter().map(|s|format!("{s:?}")).collect::<Vec<_>>(),"kind":format!("{:?}",v.kind),"rows_per_strip":v.rows_per_strip,"windows":v.windows.iter().map(|s|json!({"offset":s.offset,"bytes":s.bytes})).collect::<Vec<_>>(),"source_entries":v.source_entries.iter().map(|s|json!({"tag":s.tag,"field_type":s.field_type,"count":s.count,"entry_offset":s.entry_offset,"ifd_entry":s.ifd_entry,"payload_offset":s.payload_offset})).collect::<Vec<_>>()}));
            records.push(json!({"frame":index,"shape":metadata.sample_shape(),"dtype":format!("{:?}",metadata.pixel_type),"hash":hash(frame.pixels().unwrap()),"bits":metadata.bits_per_sample,"compatibility":compatibility,"reference_view":{"shape":view.provenance.shape,"dtype":viewtype,"hash":viewhash}}));
        }
    }
    println!("{}", json!({"frames":records}));
    Ok(())
}
