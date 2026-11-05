//! Bounded legacy TIFF JPEG container adaptation through libjpeg-turbo.
//! JPEG sample decoding reuses the existing optional native codec. Header/table
//! assembly follows libtiff's permissive OJPEG codec; its notice is retained.
use super::{
    Error, Limits,
    ycbcr::{Chunk, Component, Plane},
};
use turbojpeg_backend::{Colorspace, Decompressor, YuvImage};

/// Decode an already bounded, complete JPEG stream to its original sampling
/// grids. This helper is shared with modern JPEG-in-TIFF; RGB is never implied.
pub(super) fn decode_jpeg(
    encoded: &[u8],
    origin: [usize; 2],
    image_shape: [usize; 2],
    expected_shape: [usize; 2],
    subsampling: [usize; 2],
    limits: Limits,
) -> Result<Chunk, Error> {
    if encoded.len() > limits.intermediate_bytes {
        return Err(Error(
            "TIFF JPEG encoded stream exceeds intermediate limit".into(),
        ));
    }
    let (segments, _) = crate::jpeg::inspect_dct_bytes(encoded, limits.metadata_bytes)
        .map_err(|e| Error(e.to_string()))?;
    let frame = &segments
        .iter()
        .find(|s| matches!(s.marker, 0xc0..=0xc2))
        .ok_or_else(|| Error("TIFF JPEG lacks DCT frame".into()))?
        .payload;
    if frame.len() != 15 || frame[5] != 3 {
        return Err(Error(
            "TIFF YCbCr JPEG requires exactly three components".into(),
        ));
    }
    let factors = subsampling;
    if ![1, 2, 4].contains(&factors[0])
        || ![1, 2, 4].contains(&factors[1])
        || frame[7] != ((factors[0] as u8) << 4 | factors[1] as u8)
        || frame[10] != 0x11
        || frame[13] != 0x11
        || [frame[6], frame[9], frame[12]]
            .iter()
            .enumerate()
            .any(|(i, id)| [frame[6], frame[9], frame[12]][..i].contains(id))
    {
        return Err(Error(
            "TIFF JPEG component grids contradict source subsampling".into(),
        ));
    }
    let mut decoder = Decompressor::new().map_err(native)?;
    decoder.set_scan_limit(1000).map_err(native)?;
    let header = decoder.read_header(encoded).map_err(native)?;
    if [header.height, header.width] != expected_shape
        || header.colorspace != Colorspace::YCbCr
        || header.subsamp.width() != factors[0]
        || header.subsamp.height() != factors[1]
        || image_shape
            .iter()
            .zip(expected_shape)
            .any(|(&valid, stored)| valid == 0 || valid > stored)
    {
        return Err(Error(
            "TIFF JPEG frame dimensions/colorspace/sampling disagree with source layout".into(),
        ));
    }
    let length = turbojpeg_backend::yuv_pixels_len(header.width, 1, header.height, header.subsamp)
        .map_err(native)?;
    if length > limits.frame_bytes
        || length
            .checked_add(encoded.len())
            .is_none_or(|n| n > limits.intermediate_bytes)
    {
        return Err(Error(
            "TIFF JPEG component output/scratch exceeds limits".into(),
        ));
    }
    let mut image = YuvImage {
        pixels: vec![0; length],
        width: header.width,
        align: 1,
        height: header.height,
        subsamp: header.subsamp,
    };
    decoder
        .decompress_to_yuv(encoded, image.as_deref_mut())
        .map_err(native)?;
    let y_shape = [image.y_height(), image.y_width()];
    let uv_shape = [image.uv_height(), image.uv_width()];
    let y_len = y_shape[0] * y_shape[1];
    let uv_len = uv_shape[0] * uv_shape[1];
    if y_len.checked_add(2 * uv_len) != Some(length) {
        return Err(Error("TIFF native JPEG component layout mismatch".into()));
    }
    let plane = |component, shape, valid_shape, sampling, start, length| Plane {
        component,
        shape,
        valid_shape,
        sampling,
        samples: image.pixels[start..start + length].to_vec(),
    };
    let uv_valid = [
        image_shape[0].div_ceil(factors[1]),
        image_shape[1].div_ceil(factors[0]),
    ];
    Ok(Chunk {
        origin,
        image_shape,
        sampling_units: None,
        planes: [
            plane(Component::Y, y_shape, image_shape, [1, 1], 0, y_len),
            plane(
                Component::Cb,
                uv_shape,
                uv_valid,
                [factors[1], factors[0]],
                y_len,
                uv_len,
            ),
            plane(
                Component::Cr,
                uv_shape,
                uv_valid,
                [factors[1], factors[0]],
                y_len + uv_len,
                uv_len,
            ),
        ],
    })
}
fn native(error: turbojpeg_backend::Error) -> Error {
    Error(error.to_string())
}
