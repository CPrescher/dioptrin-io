//! Optional consumer-only JPEG RGB rendering through the existing libjpeg-turbo
//! dependency. The stored reader continues exposing native photometric samples.
use super::{
    Error, FrameBuffer, Metadata, Reader,
    view::{self, Image, Operation, Policy},
};
use crate::{PixelBuffer, PixelType};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
/// Read a versioned Pillow display view without changing stored-reader defaults.
/// JPEG uses the original admitted chunks and libjpeg-turbo RGB decoding so
/// chroma conversion and IDCT rounding follow the independent Pillow backend.
/// # Errors
/// Same source/index/payload failures as raw reads, view limits, or unsupported
/// JPEG planar/photometric layouts. No automatic backend retry occurs.
pub fn read_pillow13(
    reader: &mut Reader,
    index: usize,
    max_output_bytes: usize,
) -> Result<Image, Error> {
    let page = reader
        .metadata(index)
        .ok_or_else(|| Error("TIFF view page index outside file".into()))?;
    if page.photometric_interpretation == 6 && [1, 5, 6].contains(&page.compression) {
        let metadata = page.clone();
        let mut components = super::ycbcr::Buffer::default();
        reader.read_ycbcr_into(index, &mut components)?;
        return view::convert_ycbcr(&metadata, &components, Policy::Pillow13, max_output_bytes);
    }
    if page.compression != 7 {
        let mut frame = FrameBuffer::default();
        reader.read_frame_into(index, &mut frame)?;
        return view::convert(
            reader.metadata(index).unwrap(),
            &frame,
            Policy::Pillow13,
            max_output_bytes,
        );
    }
    if page.planar_configuration != 1
        || page.bits_per_sample != 8
        || page.pixel_type != PixelType::U8
        || ![0, 1, 2, 6].contains(&page.photometric_interpretation)
    {
        return Err(Error(
            "Pillow JPEG view admits only contiguous uint8 gray/RGB/YCbCr chunks".into(),
        ));
    }
    let count = page.shape[0]
        .checked_mul(page.shape[1])
        .ok_or_else(|| Error("TIFF JPEG view shape overflow".into()))?;
    let channels = if page.channels == 1 { 1 } else { 3 };
    let bytes = count
        .checked_mul(channels)
        .filter(|&n| n <= max_output_bytes)
        .ok_or_else(|| Error("TIFF JPEG view exceeds output limit".into()))?;
    if channels == 3 && count.checked_mul(4).is_none_or(|n| n > max_output_bytes) {
        return Err(Error(
            "TIFF JPEG luminance view exceeds output limit".into(),
        ));
    }
    reader
        .snapshot
        .check_handle(&reader.identity_handle)
        .map_err(Error)?;
    let mut file = File::open(&reader.snapshot.path).map_err(io)?;
    reader.snapshot.check_handle(&file).map_err(Error)?;
    let source_bytes = file.metadata().map_err(io)?.len();
    let tiled = page.tags.contains_key(&324);
    let offsets = vector(page, if tiled { 324 } else { 273 })?;
    let lengths = vector(page, if tiled { 325 } else { 279 })?;
    if offsets.len() != lengths.len() {
        return Err(Error("TIFF JPEG view chunk table mismatch".into()));
    }
    let (width, height) = if tiled {
        (number(page, 322)?, number(page, 323)?)
    } else {
        (
            page.shape[1],
            if page.tags.contains_key(&278) {
                number(page, 278)?.min(page.shape[0])
            } else {
                page.shape[0]
            },
        )
    };
    if width == 0 || height == 0 {
        return Err(Error("empty TIFF JPEG view chunk dimensions".into()));
    }
    let across = page.shape[1].div_ceil(width);
    let down = page.shape[0].div_ceil(height);
    if across.checked_mul(down) != Some(offsets.len()) {
        return Err(Error("TIFF JPEG view chunks disagree with shape".into()));
    }
    let tables = page
        .tags
        .get(&347)
        .map(|v| v.clone().into_u8_vec())
        .transpose()?;
    if tables
        .as_ref()
        .is_some_and(|v| v.len() < 4 || !v.starts_with(&[255, 216]) || !v.ends_with(&[255, 217]))
    {
        return Err(Error("invalid TIFF JPEG view shared-table envelope".into()));
    }
    let mut samples = vec![0; bytes];
    let mut encoded = Vec::new();
    for (index, (&offset, &length)) in offsets.iter().zip(&lengths).enumerate() {
        let length =
            usize::try_from(length).map_err(|_| Error("TIFF JPEG chunk length overflow".into()))?;
        if offset
            .checked_add(length as u64)
            .is_none_or(|n| n > source_bytes)
        {
            return Err(Error("TIFF JPEG view chunk exceeds source bounds".into()));
        }
        let prefix = tables.as_ref().map_or(0, |v| v.len() - 2);
        let combined = prefix
            .checked_add(length)
            .filter(|&n| n <= reader.intermediate_bytes)
            .ok_or_else(|| Error("TIFF JPEG view encoded chunk exceeds scratch limit".into()))?;
        encoded.clear();
        encoded.resize(combined, 0);
        file.seek(SeekFrom::Start(offset)).map_err(io)?;
        file.read_exact(&mut encoded[prefix..]).map_err(io)?;
        if length < 2 || encoded[prefix..prefix + 2] != [255, 216] {
            return Err(Error("TIFF JPEG view lacks SOI".into()));
        }
        if let Some(tables) = &tables {
            encoded[..prefix].copy_from_slice(&tables[..prefix]);
            encoded.copy_within(prefix + 2.., prefix);
            encoded.truncate(combined - 2);
        }
        let x = (index % across) * width;
        let y = (index / across) * height;
        let expected_height = if tiled {
            height
        } else {
            height.min(page.shape[0] - y)
        };
        let mut jpeg = turbojpeg_backend::Decompressor::new().map_err(native)?;
        jpeg.set_scan_limit(1000).map_err(native)?;
        let info = jpeg.read_header(&encoded).map_err(native)?;
        if info.width != width || info.height != expected_height {
            return Err(Error(
                "TIFF JPEG display dimensions disagree with admitted chunk".into(),
            ));
        }
        let chunk_bytes = width
            .checked_mul(expected_height)
            .and_then(|n| n.checked_mul(channels))
            .filter(|&n| n <= reader.intermediate_bytes)
            .ok_or_else(|| Error("TIFF JPEG RGB chunk exceeds scratch limit".into()))?;
        let mut chunk = vec![0; chunk_bytes];
        jpeg.decompress(
            &encoded,
            turbojpeg_backend::Image {
                pixels: chunk.as_mut_slice(),
                width,
                pitch: width * channels,
                height: expected_height,
                format: if channels == 1 {
                    turbojpeg_backend::PixelFormat::GRAY
                } else {
                    turbojpeg_backend::PixelFormat::RGB
                },
            },
        )
        .map_err(native)?;
        let valid_width = width.min(page.shape[1] - x);
        let valid_height = expected_height.min(page.shape[0] - y);
        for row in 0..valid_height {
            let dest = ((y + row) * page.shape[1] + x) * channels;
            let src = row * width * channels;
            samples[dest..dest + valid_width * channels]
                .copy_from_slice(&chunk[src..src + valid_width * channels]);
        }
    }
    reader.snapshot.check_handle(&file).map_err(Error)?;
    reader
        .snapshot
        .check_handle(&reader.identity_handle)
        .map_err(Error)?;
    let mut rendered = page.clone();
    rendered.channels = channels;
    rendered.planar_configuration = 1;
    rendered.photometric_interpretation = if channels == 1 {
        page.photometric_interpretation
    } else {
        2
    };
    let frame = FrameBuffer {
        pixels: PixelBuffer::U8(samples),
        shape: Some(page.shape),
        channels,
    };
    let mut image = view::convert(&rendered, &frame, Policy::Pillow13, max_output_bytes)?;
    image.provenance.source_shape = if page.sample_layout == super::SampleLayout::Uniform {
        page.sample_shape()
    } else {
        page.shape.to_vec()
    };
    image.provenance.source_layout = page.sample_layout;
    image.provenance.source_type = page.pixel_type;
    image.provenance.source_photometric = page.photometric_interpretation;
    image.provenance.source_planar_configuration = page.planar_configuration;
    image
        .provenance
        .operations
        .insert(0, Operation::JpegRgbDecode);
    Ok(image)
}
fn io(error: std::io::Error) -> Error {
    Error(error.to_string())
}
fn native(error: turbojpeg_backend::Error) -> Error {
    Error(error.to_string())
}
fn vector(page: &Metadata, tag: u16) -> Result<Vec<u64>, Error> {
    page.tags
        .get(&tag)
        .ok_or_else(|| Error(format!("TIFF JPEG view missing tag {tag}")))?
        .clone()
        .into_u64_vec()
        .map_err(Error::from)
}
fn number(page: &Metadata, tag: u16) -> Result<usize, Error> {
    let values = vector(page, tag)?;
    if values.len() != 1 {
        return Err(Error("TIFF JPEG view expected scalar field".into()));
    }
    usize::try_from(values[0]).map_err(|_| Error("TIFF JPEG view field exceeds platform".into()))
}
