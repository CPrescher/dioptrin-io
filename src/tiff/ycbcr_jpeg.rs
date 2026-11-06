//! Modern JPEG-in-TIFF assembly feeding the shared native component decoder.
use super::{
    Error, Limits, Metadata,
    ycbcr::{self, Chunk},
};
use crate::source::Snapshot;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
pub(super) fn decode(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limits: Limits,
) -> Result<Vec<Chunk>, Error> {
    if page.photometric_interpretation != 6
        || page.compression != 7
        || page.planar_configuration != 1
        || page.pixel_type != crate::PixelType::U8
        || page.bits_per_sample != 8
        || page.channels != 3
    {
        return Err(Error(
            "modern JPEG component decoding requires contiguous unsigned8 YCbCr".into(),
        ));
    }
    snapshot.check_handle(identity).map_err(Error)?;
    let tiled = page.tags.contains_key(&324);
    let offsets = vector(page, if tiled { 324 } else { 273 })?;
    let lengths = vector(page, if tiled { 325 } else { 279 })?;
    let width = if tiled {
        number(page, 322, 0)?
    } else {
        page.shape[1]
    };
    let rows = if tiled {
        number(page, 323, 0)?
    } else {
        number(page, 278, page.shape[0])?.min(page.shape[0])
    };
    if width == 0 || rows == 0 {
        return Err(Error("empty JPEG TIFF component chunk".into()));
    }
    let across = page.shape[1].div_ceil(width);
    let down = page.shape[0].div_ceil(rows);
    if across.checked_mul(down) != Some(offsets.len()) || lengths.len() != offsets.len() {
        return Err(Error("modern JPEG component chunk table mismatch".into()));
    }
    let factors = ycbcr::subsampling(page)?;
    let tables = page
        .tags
        .get(&347)
        .map(|v| v.clone().into_u8_vec())
        .transpose()?;
    if tables
        .as_ref()
        .is_some_and(|b| b.len() < 4 || !b.starts_with(&[255, 216]) || !b.ends_with(&[255, 217]))
    {
        return Err(Error("invalid JPEG component table envelope".into()));
    }
    let mut file = File::open(&snapshot.path).map_err(io)?;
    snapshot.check_handle(&file).map_err(Error)?;
    let source_bytes = file.metadata().map_err(io)?.len();
    let mut retained = 0usize;
    let mut chunks = Vec::new();
    chunks
        .try_reserve_exact(offsets.len())
        .map_err(|e| Error(e.to_string()))?;
    for (i, (&offset, &length)) in offsets.iter().zip(&lengths).enumerate() {
        let length = usize::try_from(length)
            .map_err(|_| Error("modern JPEG encoded length overflow".into()))?;
        let prefix = tables.as_ref().map_or(0, |b| b.len() - 2);
        let combined = prefix
            .checked_add(length)
            .filter(|&n| n <= limits.intermediate_bytes)
            .ok_or_else(|| Error("modern JPEG table/chunk exceeds scratch limit".into()))?;
        if length < 2
            || offset
                .checked_add(length as u64)
                .is_none_or(|end| end > source_bytes)
        {
            return Err(Error("modern JPEG chunk exceeds source window".into()));
        }
        let mut encoded = Vec::new();
        encoded
            .try_reserve_exact(combined)
            .map_err(|e| Error(e.to_string()))?;
        encoded.resize(combined, 0);
        file.seek(SeekFrom::Start(offset)).map_err(io)?;
        file.read_exact(&mut encoded[prefix..]).map_err(io)?;
        if encoded[prefix..prefix + 2] != [255, 216] {
            return Err(Error("modern JPEG component chunk lacks SOI".into()));
        }
        if let Some(tables) = &tables {
            encoded[..prefix].copy_from_slice(&tables[..prefix]);
            encoded.copy_within(prefix + 2.., prefix);
            encoded.truncate(combined - 2);
        }
        let origin = [i / across * rows, i % across * width];
        let image_shape = [
            rows.min(page.shape[0] - origin[0]),
            width.min(page.shape[1] - origin[1]),
        ];
        let expected = [if tiled { rows } else { image_shape[0] }, width];
        let unit_count = expected[0]
            .div_ceil(factors[1])
            .checked_mul(expected[1].div_ceil(factors[0]))
            .ok_or_else(|| Error("modern JPEG native grid overflow".into()))?;
        let component_bytes = unit_count
            .checked_mul(factors[0] * factors[1] + 2)
            .ok_or_else(|| Error("modern JPEG native sample count overflow".into()))?;
        retained = retained
            .checked_add(component_bytes)
            .filter(|&n| n <= limits.frame_bytes)
            .ok_or_else(|| Error("modern JPEG component grids exceed frame budget".into()))?;
        let chunk = super::jpeg_components::decode_jpeg(
            &encoded,
            origin,
            image_shape,
            expected,
            factors,
            limits,
        )?;
        if chunk.planes.iter().map(|p| p.samples.len()).sum::<usize>() != component_bytes {
            return Err(Error(
                "modern JPEG native component byte count mismatch".into(),
            ));
        }
        chunks.push(chunk);
    }
    snapshot.check_handle(&file).map_err(Error)?;
    snapshot.check_handle(identity).map_err(Error)?;
    Ok(chunks)
}
fn vector(page: &Metadata, tag: u16) -> Result<Vec<u64>, Error> {
    page.tags
        .get(&tag)
        .ok_or_else(|| Error(format!("modern JPEG missing tag {tag}")))?
        .clone()
        .into_u64_vec()
        .map_err(Error::from)
}
fn number(page: &Metadata, tag: u16, default: usize) -> Result<usize, Error> {
    let Some(v) = page.tags.get(&tag) else {
        return Ok(default);
    };
    let a = v.clone().into_u64_vec()?;
    if a.len() != 1 {
        return Err(Error("modern JPEG dimension tag must be scalar".into()));
    }
    usize::try_from(a[0]).map_err(|_| Error("modern JPEG dimension exceeds platform".into()))
}
fn io(error: std::io::Error) -> Error {
    Error(error.to_string())
}
