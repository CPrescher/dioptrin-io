//! Scoped OJPEG source-component proof; not a runtime reference bridge.
use dioptrin_io::tiff::{Limits, Reader, ycbcr};
use sha2::{Digest, Sha256};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: ojpeg_probe FILE")?;
    let mut reader = Reader::open(path, Limits::default())?;
    let mut buffer = ycbcr::Buffer::default();
    reader.read_ycbcr_into(0, &mut buffer)?;
    let info = reader.ojpeg_metadata(0).ok_or("not OJPEG")?;
    println!(
        "{}",
        serde_json::json!({"shape":buffer.shape(),"storage":format!("{:?}",buffer.storage()),
 "reconstruction":format!("{:?}",info.reconstruction),"sos_normalized":info.baseline_sos_normalized,
 "original_jpeg_header_bytes":info.original_jpeg_header.len(),"subsampling":buffer.subsampling(),
 "chunks":buffer.chunks().unwrap().iter().map(|chunk|serde_json::json!({"origin":chunk.origin,
 "image_shape":chunk.image_shape,"planes":chunk.planes.iter().map(|plane|serde_json::json!({
 "component":format!("{:?}",plane.component),"shape":plane.shape,"valid_shape":plane.valid_shape,
 "sampling":plane.sampling,"sample_sha256":format!("{:x}",Sha256::digest(&plane.samples))})).collect::<Vec<_>>()})).collect::<Vec<_>>() })
    );
    Ok(())
}
