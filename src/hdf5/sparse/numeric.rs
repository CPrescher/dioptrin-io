use super::*;
use hdf5_backend::Dataspace;
use hdf5_sys::{h5d::H5Dread, h5p::H5P_DEFAULT};
pub(super) fn validate_limits(limits: Limits) -> Result<(), Error> {
    let h = limits.hdf5;
    if [
        h.frames,
        h.frame_bytes,
        h.chunk_bytes,
        h.metadata_bytes,
        h.text_bytes,
        h.source_files,
        h.datasets,
        h.group_members,
        h.link_traversals,
        limits.events,
        limits.component_bytes,
    ]
    .contains(&0)
        || h.text_bytes == usize::MAX
    {
        return Err(Error(
            "sparse/HDF5 limits must be positive and finite".into(),
        ));
    }
    Ok(())
}
pub(super) fn is_integer(ty: PixelType) -> bool {
    matches!(
        ty,
        PixelType::U8
            | PixelType::U16
            | PixelType::U32
            | PixelType::U64
            | PixelType::I8
            | PixelType::I16
            | PixelType::I32
            | PixelType::I64
    )
}
pub(super) fn index(pixels: &PixelBuffer, index: usize) -> Result<usize, Error> {
    macro_rules! integers{($($variant:ident);*)=>{match pixels{$(PixelBuffer::$variant(values)=>usize::try_from(values[index]).map_err(|_|Error("negative/platform-overflow sparse index/pointer".into())),)*_=>Err(Error("sparse positions require integer storage".into()))}};}
    integers!(U8;U16;U32;U64;I8;I16;I32;I64)
}
pub(super) fn indices(pixels: &PixelBuffer) -> Result<Vec<usize>, Error> {
    (0..pixels.len()).map(|i| index(pixels, i)).collect()
}
pub(super) fn float(pixels: &PixelBuffer, index: usize) -> Result<f64, Error> {
    match pixels {
        PixelBuffer::F32(values) => Ok(f64::from(values[index])),
        PixelBuffer::F64(values) => Ok(values[index]),
        _ => Err(Error("sparse model requires IEEE float values".into())),
    }
}
pub(super) fn read(
    stored: &Stored,
    range: Option<(usize, usize)>,
    buffer: &mut ArrayBuffer,
    limit: usize,
) -> Result<(), Error> {
    buffer.shape = None;
    let meta = &stored.metadata;
    if stored.dataset.shape() != meta.shape {
        return Err(Error(
            "sparse HDF5 component shape changed; reopen snapshot".into(),
        ));
    }
    let ty = meta.pixel_type.ok_or_else(|| {
        Error("text component is retained in metadata, not a numeric array".into())
    })?;
    let (shape, space) = if let Some((start, stop)) = range {
        if meta.shape.len() == 1 {
            if start > stop || stop > meta.shape[0] {
                return Err(Error("sparse numeric slice outside source".into()));
            }
            (
                vec![stop - start],
                stored.dataset.space()?.select((start..stop,))?,
            )
        } else if meta.shape.len() == 2 && stop == start.saturating_add(1) && stop <= meta.shape[0]
        {
            (
                vec![meta.shape[1]],
                stored.dataset.space()?.select((start, ..))?,
            )
        } else {
            return Err(Error("unsupported sparse numeric selection".into()));
        }
    } else {
        (meta.shape.clone(), stored.dataset.space()?)
    };
    let count = shape
        .iter()
        .try_fold(1usize, |n, &d| n.checked_mul(d))
        .ok_or_else(|| Error("sparse numeric size overflow".into()))?;
    if count.checked_mul(ty.bytes()).is_none_or(|n| n > limit) {
        return Err(Error("sparse numeric read exceeds byte limit".into()));
    }
    buffer.pixels.resize(ty, count).map_err(Error)?;
    if count != 0 {
        let memory = Dataspace::try_new(shape.clone())?;
        let memory_type = typed::memory_type(ty)?;
        if space.selection_size() != count || memory.size() != count {
            return Err(Error("sparse HDF5 selection size mismatch".into()));
        }
        hdf5_backend::sync::sync(|| {
            // SAFETY: admitted standard native memory dtype and both spaces
            // select exactly count initialized writable native scalar elements.
            if unsafe {
                H5Dread(
                    stored.dataset.id(),
                    memory_type.id(),
                    memory.id(),
                    space.id(),
                    H5P_DEFAULT,
                    buffer.pixels.bytes_mut().as_mut_ptr().cast(),
                )
            } < 0
            {
                return Err(Error("sparse native HDF5 decode failed".into()));
            }
            Ok(())
        })?;
    }
    buffer.shape = Some(shape);
    Ok(())
}
pub(super) use crate::hdf5::attributes::{snapshot as attributes, text_attribute, texts};
