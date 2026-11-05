//! Scoped native components and explicit display proof, independent of the suite.
use dioptrin_io::{
    PixelBuffer,
    tiff::{self, Limits, Reader, ycbcr},
};
use sha2::{Digest, Sha256};
fn hash(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 2 {
        return Err("usage: ycbcr_probe FILE".into());
    }
    let mut reader = Reader::open(&args[1], Limits::default())?;
    let mut buffer = ycbcr::Buffer::default();
    reader.read_ycbcr_into(0, &mut buffer)?;
    let meta = reader.metadata(0).unwrap();
    let [h, v] = buffer.subsampling().unwrap();
    let chunks:Vec<_>=buffer.chunks().unwrap().iter().map(|chunk|{let planes:Vec<_>=chunk.planes.iter().map(|p|serde_json::json!({"component":format!("{:?}",p.component),"shape":p.shape,"valid_shape":p.valid_shape,"sampling":p.sampling,"dtype":"uint8","sample_sha256":hash(&p.samples)})).collect();let units=chunk.sampling_units.as_ref().map(|u|serde_json::json!({"dtype":"uint8","shape":[chunk.planes[0].shape[0]/v,chunk.planes[0].shape[1]/h,h*v+2],"sample_sha256":hash(u)}));serde_json::json!({"origin":chunk.origin,"image_shape":chunk.image_shape,"sampling_units":units,"planes":planes})}).collect();
    let mut out = serde_json::json!({"shape":buffer.shape(),"subsampling":buffer.subsampling(),"storage":format!("{:?}",buffer.storage().unwrap()),"precision":buffer.precision(),"compression":meta.compression,"orientation":meta.orientation,"sample_layout":format!("{:?}",meta.sample_layout),"uniform_sample_shape":meta.uniform_sample_shape(),"tags":format!("{:?}",meta.tags),"chunks":chunks});
    if buffer.storage() == Some(ycbcr::Storage::TiffSamplingUnits) {
        let rgb = tiff::view::render_ycbcr_rgb472(meta, &buffer, 16 * 1024 * 1024)?;
        let tiff::view::Pixels::Scalar(PixelBuffer::U8(rgb)) = rgb.pixels else {
            unreachable!()
        };
        let view = tiff::view::convert_ycbcr(
            meta,
            &buffer,
            tiff::view::Policy::Pillow13,
            16 * 1024 * 1024,
        )?;
        let tiff::view::Pixels::Scalar(PixelBuffer::F32(values)) = view.pixels else {
            unreachable!()
        };
        let bytes: Vec<_> = values.iter().flat_map(|x| x.to_le_bytes()).collect();
        out["display"] = serde_json::json!({"rgb_sha256":hash(&rgb),"luminance_f32_sha256":hash(&bytes),"policy":format!("{:?}",view.provenance.policy),"source_layout":format!("{:?}",view.provenance.source_layout),"source_shape":view.provenance.source_shape,"operations":format!("{:?}",view.provenance.operations)});
    }
    println!("{}", out);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
