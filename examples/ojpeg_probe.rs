//! Scoped OJPEG source-component proof; not a runtime reference bridge.
use dioptrin_io::tiff::{Limits, Reader, ycbcr};
use sha2::{Digest, Sha256};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().collect::<Vec<_>>();
    let path = args
        .get(1)
        .ok_or("usage: ojpeg_probe FILE [declared|independent-reset-tiles]")?;
    let policy = match args.get(2).and_then(|v| v.to_str()) {
        None | Some("declared") => dioptrin_io::tiff::OjpegTiles::Declared,
        Some("independent-reset-tiles") => dioptrin_io::tiff::OjpegTiles::IndependentResetTiles,
        _ => return Err("unknown original-JPEG tile interpretation".into()),
    };
    let mut reader = Reader::open_with_options(
        path,
        Limits::default(),
        dioptrin_io::tiff::Options {
            ojpeg_tiles: policy,
            ..Default::default()
        },
    )?;
    let mut buffer = ycbcr::Buffer::default();
    reader.read_ycbcr_into(0, &mut buffer)?;
    let info = reader.ojpeg_metadata(0).ok_or("not OJPEG")?;
    println!(
        "{}",
        serde_json::json!({"ojpeg_tiles_policy":format!("{policy:?}"),"shape":buffer.shape(),"storage":format!("{:?}",buffer.storage()),
 "reconstruction":format!("{:?}",info.reconstruction),"sos_normalized":info.baseline_sos_normalized,
 "original_jpeg_header_bytes":info.original_jpeg_header.len(),"original_jpeg_header_sha256":format!("{:x}",Sha256::digest(&info.original_jpeg_header)),"subsampling":buffer.subsampling(),
 "source_windows":info.source_windows.iter().map(|w|serde_json::json!({"offset":w.offset,"bytes":w.bytes,"role":w.role})).collect::<Vec<_>>(),
 "restart_interval":info.restart_interval,"restart_markers_inserted":info.restart_markers_inserted,
 "tile_policy":format!("{:?}",info.tile_policy),"partition":format!("{:?}",info.partition),"scan_sources":info.scan_sources.iter().map(|scan|serde_json::json!({"component":format!("{:?}",scan.component),"header_windows":scan.header_windows.iter().map(|w|serde_json::json!({"offset":w.offset,"bytes":w.bytes,"role":w.role})).collect::<Vec<_>>(),"entropy_windows":scan.entropy_windows.iter().map(|w|serde_json::json!({"offset":w.offset,"bytes":w.bytes,"role":w.role})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
 "fragments":info.fragments.iter().map(|f|serde_json::json!({"index":f.index,"source_offset":f.source_offset,"source_bytes":f.source_bytes,"header_bytes":f.header_bytes,"entropy_offset":f.entropy_offset,"entropy_bytes":f.entropy_bytes,"origin":f.origin,"stream_origin":f.stream_origin,"image_shape":f.image_shape,"encoded_shape":f.encoded_shape})).collect::<Vec<_>>(),
 "chunks":buffer.chunks().unwrap().iter().map(|chunk|serde_json::json!({"origin":chunk.origin,
 "image_shape":chunk.image_shape,"planes":chunk.planes.iter().map(|plane|serde_json::json!({
 "component":format!("{:?}",plane.component),"shape":plane.shape,"valid_shape":plane.valid_shape,
 "sampling":plane.sampling,"sample_sha256":format!("{:x}",Sha256::digest(&plane.samples))})).collect::<Vec<_>>()})).collect::<Vec<_>>() })
    );
    Ok(())
}
