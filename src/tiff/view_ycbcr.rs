//! Explicit TIFF sampling-unit display, following libtiff 4.7.2 tif_color.c.
//! Copyright (c) 1988-1997 Sam Leffler; Copyright (c) 1991-1997 Silicon Graphics.
//! Permissive notice retained in licenses/native-readers/libtiff-4.7.2-LICENSE.txt.
use super::view::{self, Image, Operation, OutputType, Pixels, Policy, Provenance};
use super::{Error, FrameBuffer, Metadata, SampleLayout, ycbcr};
use crate::{PixelBuffer, PixelType};

/// Explicit Pillow/libtiff RGB8 view with nearest sampling-unit chroma expansion.
/// Original components, padding, orientation and rational metadata stay unchanged.
/// # Errors
/// Invalid/mismatched source components, unsupported policy or output bounds.
pub fn render_ycbcr_rgb472(
    metadata: &Metadata,
    source: &ycbcr::Buffer,
    max_output_bytes: usize,
) -> Result<Image, Error> {
    let factors = ycbcr::subsampling(metadata)?;
    if metadata.photometric_interpretation != 6
        || source.shape() != Some(metadata.shape)
        || source.subsampling() != Some(factors)
        || !matches!(
            (metadata.compression, source.storage()),
            (1 | 5, Some(ycbcr::Storage::TiffSamplingUnits))
                | (6, Some(ycbcr::Storage::JpegDecodedComponents))
        )
    {
        return Err(Error(
            "libtiff sampling-unit view requires matching original TIFF components".into(),
        ));
    }
    let count = metadata.shape[0]
        .checked_mul(metadata.shape[1])
        .ok_or_else(|| Error("YCbCr display size overflow".into()))?;
    let bytes = count
        .checked_mul(3)
        .filter(|&n| n <= max_output_bytes)
        .ok_or_else(|| Error("YCbCr RGB view exceeds output limit".into()))?;
    let color = ycbcr::colorimetry(metadata)?;
    // Preserve the source positioning in metadata. This versioned libtiff RGBA
    // view replicates each unit's chroma without a centered/cosited interpolation.
    let luma = color.coefficients.map(|n| n as f32);
    let reference = color.reference_black_white.map(|n| n as f32);
    if [reference[0], reference[2] - 128.0, reference[4] - 128.0]
        .iter()
        .any(|&n| {
            f64::from(n.trunc()) < f64::from(i32::MIN) + 256.0
                || f64::from(n.trunc()) > f64::from(i32::MAX) - 256.0
        })
    {
        return Err(Error(
            "libtiff472 display reference black exceeds defined integer conversion bounds".into(),
        ));
    }
    let fix = |n: f32| (f64::from(n.clamp(0.0, 2.0)) * 65536.0 + 0.5) as i32;
    let red = 2.0 - 2.0 * luma[0];
    let blue = 2.0 - 2.0 * luma[2];
    let d = [
        fix(red),
        -fix(luma[0] * red / luma[1]),
        fix(blue),
        -fix(luma[2] * blue / luma[1]),
    ];
    let code = |n: i32, black: f32, white: f32, range: f32| {
        (((n - black as i32) as f32 * range) / (if white == black { 1.0 } else { white - black }))
            .clamp(-4096.0, 4096.0) as i32
    };
    let mut y_table = [0i32; 256];
    let mut cr_red = [0i32; 256];
    let mut cb_blue = [0i32; 256];
    let mut cr_green = [0i32; 256];
    let mut cb_green = [0i32; 256];
    for i in 0..256 {
        let cr = code(
            i as i32 - 128,
            reference[4] - 128.0,
            reference[5] - 128.0,
            127.0,
        );
        let cb = code(
            i as i32 - 128,
            reference[2] - 128.0,
            reference[3] - 128.0,
            127.0,
        );
        y_table[i] = code(i as i32, reference[0], reference[1], 255.0);
        cr_red[i] = (d[0] * cr + 32768) >> 16;
        cb_blue[i] = (d[2] * cb + 32768) >> 16;
        cr_green[i] = d[1] * cr;
        cb_green[i] = d[3] * cb + 32768;
    }
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(bytes)
        .map_err(|e| Error(e.to_string()))?;
    samples.resize(bytes, 0);
    let chunks = source
        .chunks()
        .ok_or_else(|| Error("invalid YCbCr source buffer".into()))?;
    for chunk in chunks {
        let [yp, cbp, crp] = &chunk.planes;
        for y in 0..chunk.image_shape[0] {
            for x in 0..chunk.image_shape[1] {
                let yv = usize::from(yp.samples[y * yp.shape[1] + x]);
                let uv = (y / factors[1]) * cbp.shape[1] + x / factors[0];
                let cb = usize::from(cbp.samples[uv]);
                let cr = usize::from(crp.samples[uv]);
                let offset = ((chunk.origin[0] + y) * metadata.shape[1] + chunk.origin[1] + x) * 3;
                samples[offset] = (y_table[yv] + cr_red[cr]).clamp(0, 255) as u8;
                samples[offset + 1] =
                    (y_table[yv] + ((cb_green[cb] + cr_green[cr]) >> 16)).clamp(0, 255) as u8;
                samples[offset + 2] = (y_table[yv] + cb_blue[cb]).clamp(0, 255) as u8;
            }
        }
    }
    Ok(Image {
        pixels: Pixels::Scalar(PixelBuffer::U8(samples)),
        provenance: Provenance {
            policy: Policy::Pillow13,
            source_shape: metadata.shape.to_vec(),
            source_layout: metadata.sample_layout,
            source_type: metadata.pixel_type,
            source_bits_per_sample: metadata.bits_per_sample,
            source_photometric: metadata.photometric_interpretation,
            source_planar_configuration: metadata.planar_configuration,
            source_orientation: metadata.orientation,
            source_compression: metadata.compression,
            source_lossy: metadata.lossy,
            shape: vec![metadata.shape[0], metadata.shape[1], 3],
            output_type: OutputType::Scalar(PixelType::U8),
            operations: vec![
                Operation::YcbcrNearestChromaExpansion,
                Operation::YcbcrLibTiff472Rgb8,
            ],
        },
    })
}
/// Pinned FabIO/Pillow consumer luminance from the explicit libtiff RGB8 view.
/// # Errors
/// Same component/policy/bounds failures as render_ycbcr_rgb472.
pub fn convert_ycbcr(
    metadata: &Metadata,
    source: &ycbcr::Buffer,
    policy: Policy,
    max_output_bytes: usize,
) -> Result<Image, Error> {
    if policy != Policy::Pillow13 {
        return Err(Error(
            "YCbCr component conversion requires explicit Pillow13 policy".into(),
        ));
    }
    let rgb = render_ycbcr_rgb472(metadata, source, max_output_bytes)?;
    let Pixels::Scalar(pixels) = rgb.pixels else {
        unreachable!()
    };
    let frame = FrameBuffer {
        pixels,
        shape: Some(metadata.shape),
        channels: 3,
    };
    let mut converted_metadata = metadata.clone();
    converted_metadata.photometric_interpretation = 2;
    converted_metadata.sample_layout = SampleLayout::Uniform;
    let mut image = view::convert(
        &converted_metadata,
        &frame,
        Policy::Pillow13,
        max_output_bytes,
    )?;
    image.provenance.source_shape = metadata.shape.to_vec();
    image.provenance.source_layout = metadata.sample_layout;
    image.provenance.source_photometric = 6;
    image
        .provenance
        .operations
        .splice(0..0, rgb.provenance.operations);
    Ok(image)
}
