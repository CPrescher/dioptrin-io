//! Scoped source-versus-display oracle probe; never a runtime adapter.
use dioptrin_io::{
    PixelBuffer, PixelType,
    tiff::{
        self,
        view::{self, Image, Pixels, Policy},
    },
};
use serde_json::json;
use sha2::{Digest, Sha256};
fn hash(values: &PixelBuffer) -> String {
    let mut digest = Sha256::new();
    macro_rules! scalar {
        ($v:expr) => {
            for value in $v {
                digest.update(value.to_le_bytes());
            }
        };
    }
    match values {
        PixelBuffer::U8(v) => digest.update(v),
        PixelBuffer::U16(v) => scalar!(v),
        PixelBuffer::U32(v) => scalar!(v),
        PixelBuffer::U64(v) => scalar!(v),
        PixelBuffer::I8(v) => {
            for &v in v {
                digest.update([v as u8]);
            }
        }
        PixelBuffer::I16(v) => scalar!(v),
        PixelBuffer::I32(v) => scalar!(v),
        PixelBuffer::I64(v) => scalar!(v),
        PixelBuffer::F32(v) => scalar!(v),
        PixelBuffer::F64(v) => scalar!(v),
        _ => panic!("unsupported TIFF complex samples"),
    };
    format!("{:x}", digest.finalize())
}
fn dtype(ty: PixelType) -> String {
    format!("{ty:?}")
}
fn signature(image: &Image) -> serde_json::Value {
    let (dtype, hash) = match &image.pixels {
        Pixels::Scalar(v) => (dtype(v.pixel_type()), hash(v)),
        Pixels::Bool(v) => {
            let mut hash = Sha256::new();
            for &v in v {
                hash.update([u8::from(v)]);
            }
            ("bool".into(), format!("{:x}", hash.finalize()))
        }
    };
    json!({"shape":image.provenance.shape,"dtype":dtype,"hash":hash,"operations":image.provenance.operations.iter().map(|op|format!("{op:?}")).collect::<Vec<_>>()})
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: tiff_view_probe POLICY FILE".into());
    }
    let policy = match args[1].to_str() {
        Some("pillow13") => Policy::Pillow13,
        Some("tiffio") => Policy::FabioTiffIo,
        Some("palette16") => Policy::PaletteRgb16,
        _ => return Err("unknown view policy".into()),
    };
    let mut reader = tiff::Reader::open(&args[2], tiff::Limits::default())?;
    let mut source = tiff::FrameBuffer::default();
    let mut frames = Vec::new();
    for index in 0..reader.frames() {
        reader.read_frame_into(index, &mut source)?;
        let metadata = reader.metadata(index).unwrap();
        let raw = json!({"shape":metadata.sample_shape(),"dtype":dtype(metadata.pixel_type),"bits":metadata.bits_per_sample,"photo":metadata.photometric_interpretation,"planar":metadata.planar_configuration,"hash":hash(source.pixels().unwrap())});
        let view = if policy == Policy::Pillow13 {
            view::read_pillow13(&mut reader, index, tiff::Limits::default().frame_bytes)
        } else {
            view::convert(
                metadata,
                &source,
                policy,
                tiff::Limits::default().frame_bytes,
            )
        };
        frames.push(match view {
            Ok(image) => json!({"raw":raw,"view":signature(&image)}),
            Err(error) => json!({"raw":raw,"view_error":error.to_string()}),
        });
    }
    println!("{}", json!({"frames":frames}));
    Ok(())
}
