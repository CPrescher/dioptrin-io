//! Explicit consumer display conversions. Stored-count readers never call these.
//! Versioned policies describe representation changes, not native-count parity.
//! Pillow conversion semantics were audited against its pinned Convert.c and
//! Unpack.c; copyright/license notice: licenses/native-readers/Pillow-13.0.0-LICENSE.txt.
#[cfg(feature = "jpeg")]
pub use super::view_jpeg::read_pillow13;
pub use super::view_ycbcr::{convert_ycbcr, render_ycbcr_rgb472};
use super::{Error, FrameBuffer, Metadata, SampleLayout};
use crate::{PixelBuffer, PixelType};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Pillow 13 / FabIO's Pillow fallback: bilevel bool, packed full-range
    /// grayscale, palette RGB8, or RGB/RGBA/CMYK float32 luminance. Known
    /// Pillow planar16 layout defects are rejected rather than reproduced.
    Pillow13,
    /// Literal ColorMap lookup retaining original 16-bit channel values.
    PaletteRgb16,
    /// Pinned FabIO TiffIO palette lookup: downscale maps containing values
    /// above 255, then retain the index container's u8/u16 output dtype.
    FabioTiffIo,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputType {
    Scalar(PixelType),
    Bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    WhiteIsZeroDisplay,
    FullRangePackedGray,
    BilevelBool,
    PaletteLookup,
    PaletteDownscale8,
    ReduceColor16To8,
    IgnoreAlpha,
    UnpremultiplyAlpha8,
    CmykToRgb8,
    RgbLuminanceF32,
    JpegRgbDecode,
    YcbcrNearestChromaExpansion,
    YcbcrLibTiff472Rgb8,
}
#[derive(Clone, Debug)]
pub struct Provenance {
    pub policy: Policy,
    /// Logical source raster; native component geometry is identified separately.
    pub source_shape: Vec<usize>,
    pub source_layout: SampleLayout,
    pub source_type: PixelType,
    pub source_bits_per_sample: u8,
    pub source_photometric: u16,
    pub source_planar_configuration: u16,
    pub source_orientation: u16,
    pub source_compression: u16,
    pub source_lossy: bool,
    pub shape: Vec<usize>,
    pub output_type: OutputType,
    pub operations: Vec<Operation>,
}
#[derive(Debug)]
pub enum Pixels {
    Scalar(PixelBuffer),
    Bool(Vec<bool>),
}
#[derive(Debug)]
pub struct Image {
    pub pixels: Pixels,
    pub provenance: Provenance,
}
/// Convert an already-read frame under an explicit display policy. The source
/// frame and metadata remain untouched; the output carries all transformation
/// steps and original storage description. Orientation is never applied.
/// # Errors
/// Mismatched input metadata, unsupported display semantics, known reference
/// layout defects, malformed palette/index or output-byte admission.
pub fn convert(
    metadata: &Metadata,
    frame: &FrameBuffer,
    policy: Policy,
    max_output_bytes: usize,
) -> Result<Image, Error> {
    let pixels = frame
        .pixels()
        .ok_or_else(|| Error("display conversion requires a valid stored frame".into()))?;
    if frame.shape() != Some(metadata.shape)
        || frame.sample_shape() != Some(metadata.sample_shape())
        || pixels.pixel_type() != metadata.pixel_type
    {
        return Err(Error(
            "TIFF display input disagrees with source metadata".into(),
        ));
    }
    let count = metadata.shape[0]
        .checked_mul(metadata.shape[1])
        .ok_or_else(|| Error("TIFF display shape overflow".into()))?;
    if count.checked_mul(metadata.channels) != Some(pixels.len()) {
        return Err(Error("TIFF display source sample count mismatch".into()));
    }
    let mut operations = Vec::new();
    let mut shape = metadata.shape.to_vec();
    let output = if metadata.photometric_interpretation == 3 {
        if policy == Policy::Pillow13 && metadata.bits_per_sample > 8 {
            return Err(Error(
                "Pillow13 does not admit 16-bit palette indices".into(),
            ));
        }
        let channels = metadata
            .tags
            .get(&320)
            .ok_or_else(|| Error("TIFF display palette is absent".into()))?
            .clone()
            .into_u16_vec()?;
        let size = 1usize
            .checked_shl(u32::from(metadata.bits_per_sample))
            .ok_or_else(|| Error("TIFF palette index precision overflow".into()))?;
        if channels.len()
            != size
                .checked_mul(3)
                .ok_or_else(|| Error("TIFF palette size overflow".into()))?
        {
            return Err(Error("TIFF display ColorMap length mismatch".into()));
        }
        operations.push(Operation::PaletteLookup);
        shape.push(3);
        let downscale = match policy {
            Policy::PaletteRgb16 => false,
            Policy::Pillow13 => true,
            Policy::FabioTiffIo => channels.iter().any(|&v| v > 255),
        };
        if downscale {
            operations.push(Operation::PaletteDownscale8);
        }
        let output_u16 = policy == Policy::PaletteRgb16
            || (policy == Policy::FabioTiffIo && metadata.pixel_type == PixelType::U16);
        admit(count, 3, if output_u16 { 2 } else { 1 }, max_output_bytes)?;
        let indices: Box<dyn Iterator<Item = usize> + '_> = match pixels {
            PixelBuffer::U8(v) => Box::new(v.iter().map(|&v| usize::from(v))),
            PixelBuffer::U16(v) => Box::new(v.iter().map(|&v| usize::from(v))),
            _ => return Err(Error("unsupported TIFF palette index type".into())),
        };
        let mut values = if output_u16 {
            PixelBuffer::U16(vec![0; count * 3])
        } else {
            PixelBuffer::U8(vec![0; count * 3])
        };
        for (position, index) in indices.enumerate() {
            if index >= size {
                return Err(Error("TIFF palette index exceeds ColorMap".into()));
            }
            for channel in 0..3 {
                let value = channels[channel * size + index];
                let value = if downscale { value >> 8 } else { value };
                match &mut values {
                    PixelBuffer::U16(v) => v[position * 3 + channel] = value,
                    PixelBuffer::U8(v) => v[position * 3 + channel] = value as u8,
                    _ => unreachable!(),
                }
            }
        }
        Pixels::Scalar(values)
    } else if policy == Policy::FabioTiffIo {
        admit(
            count,
            metadata.channels,
            metadata.pixel_type.bytes(),
            max_output_bytes,
        )?;
        shape = metadata.sample_shape();
        Pixels::Scalar(pixels.clone())
    } else if policy != Policy::Pillow13 {
        return Err(Error(
            "palette lookup policy requires original palette indices".into(),
        ));
    } else if metadata.channels == 1 {
        if ![0, 1].contains(&metadata.photometric_interpretation) {
            return Err(Error(
                "Pillow13 scalar view requires declared grayscale photometric samples".into(),
            ));
        }
        if !matches!(
            (metadata.pixel_type, metadata.bits_per_sample),
            (PixelType::U8, 1 | 2 | 4 | 8) | (PixelType::U16, 16) | (PixelType::F32, 32)
        ) {
            return Err(Error("Pillow13 scalar view requires validated uint8/uint16/float32 source representation".into()));
        }
        if matches!(metadata.pixel_type, PixelType::F32 | PixelType::U16) {
            admit(count, 1, metadata.pixel_type.bytes(), max_output_bytes)?;
            Pixels::Scalar(pixels.clone())
        } else if metadata.bits_per_sample == 1 {
            admit(count, 1, 1, max_output_bytes)?;
            let PixelBuffer::U8(values) = pixels else {
                return Err(Error("bilevel display requires u8 source indices".into()));
            };
            if values.iter().any(|&v| v > 1) {
                return Err(Error("bilevel source contains non-binary samples".into()));
            }
            operations.push(Operation::BilevelBool);
            let white = metadata.photometric_interpretation == 0;
            if white {
                operations.push(Operation::WhiteIsZeroDisplay);
            }
            Pixels::Bool(values.iter().map(|&value| (value != 0) ^ white).collect())
        } else if metadata.bits_per_sample < 8 {
            admit(count, 1, 1, max_output_bytes)?;
            let PixelBuffer::U8(values) = pixels else {
                return Err(Error(
                    "packed gray display requires u8 source indices".into(),
                ));
            };
            let maximum = (1u8 << metadata.bits_per_sample) - 1;
            if values.iter().any(|&v| v > maximum) {
                return Err(Error(
                    "packed source exceeds declared sample precision".into(),
                ));
            }
            operations.push(Operation::FullRangePackedGray);
            let white = metadata.photometric_interpretation == 0;
            if white {
                operations.push(Operation::WhiteIsZeroDisplay);
            }
            Pixels::Scalar(PixelBuffer::U8(
                values
                    .iter()
                    .map(|&v| if white { maximum - v } else { v } * (255 / maximum))
                    .collect(),
            ))
        } else if metadata.photometric_interpretation == 0 {
            admit(count, 1, metadata.pixel_type.bytes(), max_output_bytes)?;
            operations.push(Operation::WhiteIsZeroDisplay);
            macro_rules! invert {($($variant:ident);*)=>{match pixels {$(PixelBuffer::$variant(v)=>Pixels::Scalar(PixelBuffer::$variant(v.iter().map(|&v|!v).collect())),)*_=>return Err(Error("Pillow display inversion is admitted only for unsigned integer samples".into()))}};}
            invert!(U8;U16;U32;U64)
        } else {
            admit(count, 1, metadata.pixel_type.bytes(), max_output_bytes)?;
            Pixels::Scalar(pixels.clone())
        }
    } else {
        if metadata.bits_per_sample == 16 && metadata.planar_configuration == 2 {
            return Err(Error("reference layout defect: Pillow13 planar16 color reads alternate source bytes; stored samples retained".into()));
        }
        if ![2, 5].contains(&metadata.photometric_interpretation)
            || ![3, 4].contains(&metadata.channels)
            || metadata.photometric_interpretation == 5 && metadata.channels != 4
        {
            return Err(Error(
                "unsupported Pillow color display layout/colorspace".into(),
            ));
        }
        if metadata.photometric_interpretation == 5
            && metadata
                .tags
                .get(&332)
                .is_some_and(|tag| tag.clone().into_u16().map_or(true, |inkset| inkset != 1))
        {
            return Err(Error(
                "Pillow CMYK view requires default CMYK InkSet1".into(),
            ));
        }
        if metadata.photometric_interpretation == 2 && metadata.channels == 4 {
            let associated = metadata.tags.get(&338).is_some_and(|value| {
                value
                    .clone()
                    .into_u16_vec()
                    .is_ok_and(|v| v.first() == Some(&1))
            });
            if associated {
                if metadata.bits_per_sample != 8 {
                    return Err(Error("associated-alpha display conversion requires validated uint8 source samples".into()));
                }
                operations.push(Operation::UnpremultiplyAlpha8);
            }
            operations.push(Operation::IgnoreAlpha);
        }
        admit(count, 1, 4, max_output_bytes)?;
        let samples: Box<dyn Iterator<Item = u8> + '_> = match pixels {
            PixelBuffer::U8(v) if metadata.bits_per_sample == 8 => Box::new(v.iter().copied()),
            PixelBuffer::U16(v) if metadata.bits_per_sample == 16 => {
                operations.push(Operation::ReduceColor16To8);
                Box::new(v.iter().map(|&v| (v >> 8) as u8))
            }
            _ => {
                return Err(Error(
                    "Pillow color display admits only native uint8/uint16 sources".into(),
                ));
            }
        };
        let cmyk = metadata.photometric_interpretation == 5;
        if cmyk {
            operations.push(Operation::CmykToRgb8);
        }
        operations.push(Operation::RgbLuminanceF32);
        let mut samples = samples;
        let mut luminance = Vec::with_capacity(count);
        for _ in 0..count {
            let mut color = [0; 4];
            for slot in color.iter_mut().take(metadata.channels) {
                *slot = samples
                    .next()
                    .ok_or_else(|| Error("short TIFF display color source".into()))?;
            }
            if metadata.photometric_interpretation == 2
                && operations.contains(&Operation::UnpremultiplyAlpha8)
            {
                let alpha = u32::from(color[3]);
                if alpha == 0 {
                    color[..3].fill(0);
                } else if alpha != 255 {
                    for channel in color.iter_mut().take(3) {
                        *channel = (255 * u32::from(*channel) / alpha).min(255) as u8;
                    }
                }
            }
            if cmyk {
                let inverse_k = 255 - u32::from(color[3]);
                for channel in color.iter_mut().take(3) {
                    let product = u32::from(*channel) * inverse_k + 128;
                    let removed = ((product >> 8) + product) >> 8;
                    *channel = (inverse_k - removed) as u8;
                }
            }
            let weighted =
                u32::from(color[0]) * 299 + u32::from(color[1]) * 587 + u32::from(color[2]) * 114;
            luminance.push(weighted as f32 / 1000.0);
        }
        Pixels::Scalar(PixelBuffer::F32(luminance))
    };
    let output_type = match &output {
        Pixels::Scalar(v) => OutputType::Scalar(v.pixel_type()),
        Pixels::Bool(_) => OutputType::Bool,
    };
    Ok(Image {
        pixels: output,
        provenance: Provenance {
            policy,
            source_shape: metadata.sample_shape(),
            source_layout: metadata.sample_layout,
            source_type: metadata.pixel_type,
            source_bits_per_sample: metadata.bits_per_sample,
            source_photometric: metadata.photometric_interpretation,
            source_planar_configuration: metadata.planar_configuration,
            source_orientation: metadata.orientation,
            source_compression: metadata.compression,
            source_lossy: metadata.lossy,
            shape,
            output_type,
            operations,
        },
    })
}
fn admit(count: usize, channels: usize, bytes: usize, limit: usize) -> Result<(), Error> {
    if limit == 0
        || count
            .checked_mul(channels)
            .and_then(|v| v.checked_mul(bytes))
            .is_none_or(|bytes| bytes > limit)
    {
        return Err(Error(
            "TIFF display output exceeds explicit byte limit".into(),
        ));
    }
    Ok(())
}
