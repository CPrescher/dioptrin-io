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
 "original_jpeg_header_bytes":info.original_jpeg_header.len(),"original_jpeg_header_sha256":format!("{:x}",Sha256::digest(&info.original_jpeg_header)),"subsampling":buffer.subsampling(),
 "source_windows":info.source_windows.iter().map(|w|serde_json::json!({"offset":w.offset,"bytes":w.bytes,"role":w.role})).collect::<Vec<_>>(),
 "restart_interval":info.restart_interval,"restart_markers_inserted":info.restart_markers_inserted,
 "fragments":info.fragments.iter().map(|f|serde_json::json!({"index":f.index,"source_offset":f.source_offset,"source_bytes":f.source_bytes,"entropy_offset":f.entropy_offset,"entropy_bytes":f.entropy_bytes,"origin":f.origin,"stream_origin":f.stream_origin,"image_shape":f.image_shape,"encoded_shape":f.encoded_shape})).collect::<Vec<_>>(),
 "chunks":buffer.chunks().unwrap().iter().map(|chunk|serde_json::json!({"origin":chunk.origin,
 "image_shape":chunk.image_shape,"planes":chunk.planes.iter().map(|plane|serde_json::json!({
 "component":format!("{:?}",plane.component),"shape":plane.shape,"valid_shape":plane.valid_shape,
 "sampling":plane.sampling,"sample_sha256":format!("{:x}",Sha256::digest(&plane.samples))})).collect::<Vec<_>>()})).collect::<Vec<_>>() })
    );
    Ok(())
}
