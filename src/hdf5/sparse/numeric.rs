use super::*;
use hdf5_backend::{Container, Dataspace, Datatype};
use hdf5_sys::{
    h5::{H5_index_t, H5_iter_order_t},
    h5a::{H5Aget_name_by_idx, H5Aread},
    h5d::H5Dread,
    h5p::H5P_DEFAULT,
    h5s::H5S_ALL,
};
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
pub(super) fn attributes(
    sources: &mut Sources,
    source: usize,
    location: &Location,
    limits: typed::Limits,
) -> Result<Attributes, Error> {
    if location.loc_info()?.num_attrs > limits.group_members {
        return Err(Error("sparse HDF5 attribute count exceeds limit".into()));
    }
    let mut values = BTreeMap::new();
    let mut declarations = BTreeMap::new();
    let path = location.name();
    sources.charge(
        path.len()
            .saturating_mul(2)
            .saturating_add(sources.path(source).as_os_str().len())
            .saturating_add(256),
    )?;
    for name in attribute_names(sources, location, limits)? {
        sources.charge(name.len().saturating_mul(2).saturating_add(128))?;
        let attribute = location.attr(&name)?;
        let dtype = attribute.dtype()?;
        let descriptor = dtype.to_descriptor()?;
        let type_descriptor = format!("{descriptor:?}");
        let shape = attribute.shape();
        sources.charge(
            type_descriptor
                .len()
                .saturating_add(shape.len().saturating_mul(8))
                .saturating_add(64),
        )?;
        declarations.insert(
            name.clone(),
            AttributeStorage {
                shape,
                type_descriptor,
            },
        );
        let value = if matches!(
            descriptor,
            TypeDescriptor::VarLenAscii
                | TypeDescriptor::VarLenUnicode
                | TypeDescriptor::FixedAscii(_)
                | TypeDescriptor::FixedUnicode(_)
        ) {
            let text = texts(&attribute, limits.text_bytes, true)?;
            sources.charge(
                text.iter()
                    .map(String::len)
                    .sum::<usize>()
                    .saturating_mul(2)
                    .saturating_add(text.len().saturating_mul(48)),
            )?;
            AttributeValue::Text(text)
        } else {
            let (ty, little) = typed::standard_type(&dtype)?;
            let count = attribute.size();
            let bytes = count
                .checked_mul(ty.bytes())
                .filter(|&n| n <= limits.metadata_bytes)
                .ok_or_else(|| Error("sparse attribute value exceeds metadata budget".into()))?;
            sources.charge(bytes.saturating_mul(2).saturating_add(128))?;
            let mut pixels = PixelBuffer::default();
            pixels.resize(ty, count).map_err(Error)?;
            let memory = typed::memory_type(ty)?;
            if count != 0 {
                hdf5_backend::sync::sync(|| {
                    // SAFETY: admitted attribute element count and exact native memory type.
                    if unsafe {
                        H5Aread(
                            attribute.id(),
                            memory.id(),
                            pixels.bytes_mut().as_mut_ptr().cast(),
                        )
                    } < 0
                    {
                        return Err(Error("cannot read sparse numeric attribute".into()));
                    }
                    Ok(())
                })?;
            }
            AttributeValue::Numeric {
                shape: attribute.shape(),
                pixel_type: ty,
                little_endian: little,
                values: pixels,
            }
        };
        values.insert(name, value);
    }
    Ok(Attributes {
        source_file: sources.path(source).to_path_buf(),
        path,
        values,
        declarations,
    })
}
pub(super) fn text_attribute(attributes: &Attributes, name: &str) -> Result<String, Error> {
    match attributes.values.get(name) {
        Some(AttributeValue::Text(values)) if values.len() == 1 && !values[0].is_empty() => {
            Ok(values[0].clone())
        }
        _ => Err(Error(format!(
            "sparse discovery requires one nonempty {name} string"
        ))),
    }
}
pub(super) fn texts(
    container: &Container,
    limit: usize,
    attribute: bool,
) -> Result<Vec<String>, Error> {
    let count = container.size();
    if count > limit {
        return Err(Error("sparse text element count exceeds text limit".into()));
    }
    let descriptor = container.dtype()?.to_descriptor()?;
    let bytes: Vec<Vec<u8>> = match descriptor {
        TypeDescriptor::VarLenAscii => container
            .read_raw::<hdf5_backend::types::VarLenAscii>()?
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect(),
        TypeDescriptor::VarLenUnicode => container
            .read_raw::<hdf5_backend::types::VarLenUnicode>()?
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect(),
        TypeDescriptor::FixedAscii(n) | TypeDescriptor::FixedUnicode(n) => {
            let total = count
                .checked_mul(n)
                .filter(|&n| n <= limit)
                .ok_or_else(|| Error("sparse fixed text exceeds limit".into()))?;
            if n == 0 {
                return Err(Error("zero-width sparse text unsupported".into()));
            }
            let memory = Datatype::from_descriptor(&descriptor)?;
            let mut raw = vec![0u8; total];
            if total != 0 {
                hdf5_backend::sync::sync(|| {
                    // SAFETY: complete admitted fixed-string destination of total exact bytes.
                    let status = unsafe {
                        if attribute {
                            H5Aread(container.id(), memory.id(), raw.as_mut_ptr().cast())
                        } else {
                            H5Dread(
                                container.id(),
                                memory.id(),
                                H5S_ALL,
                                H5S_ALL,
                                H5P_DEFAULT,
                                raw.as_mut_ptr().cast(),
                            )
                        }
                    };
                    if status < 0 {
                        return Err(Error("cannot decode sparse fixed text".into()));
                    }
                    Ok(())
                })?;
            }
            raw.chunks_exact(n).map(|s| s.to_vec()).collect()
        }
        _ => {
            return Err(Error(
                "sparse text component must use HDF5 string storage".into(),
            ));
        }
    };
    let total = bytes
        .iter()
        .try_fold(0usize, |n, s| n.checked_add(s.len()))
        .filter(|&n| n <= limit)
        .ok_or_else(|| Error("sparse variable text exceeds limit after native read".into()))?;
    let _ = total;
    bytes
        .into_iter()
        .map(|mut value| {
            let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
            value.truncate(end);
            String::from_utf8(value)
                .map_err(|_| Error("non-UTF8 sparse HDF5 text unsupported".into()))
        })
        .collect()
}

fn attribute_names(
    sources: &mut Sources,
    location: &Location,
    limits: typed::Limits,
) -> Result<Vec<String>, Error> {
    let count = location.loc_info()?.num_attrs;
    if count > limits.group_members {
        return Err(Error("sparse attribute name count exceeds limit".into()));
    }
    let mut names = Vec::new();
    for index in 0..count {
        let name = hdf5_backend::sync::sync(|| -> Result<String, Error> {
            // SAFETY: live object, current-object name, bounded valid index.
            let len = unsafe {
                H5Aget_name_by_idx(
                    location.id(),
                    c".".as_ptr(),
                    H5_index_t::H5_INDEX_NAME,
                    H5_iter_order_t::H5_ITER_INC,
                    index as u64,
                    std::ptr::null_mut(),
                    0,
                    H5P_DEFAULT,
                )
            };
            let len = usize::try_from(len)
                .map_err(|_| Error("cannot determine sparse attribute name length".into()))?;
            if len > limits.text_bytes {
                return Err(Error("sparse attribute name exceeds text limit".into()));
            }
            sources.charge(len.saturating_mul(2).saturating_add(128))?;
            let mut raw = vec![0u8; len + 1];
            // SAFETY: admitted exact name length plus NUL terminator.
            let actual = unsafe {
                H5Aget_name_by_idx(
                    location.id(),
                    c".".as_ptr(),
                    H5_index_t::H5_INDEX_NAME,
                    H5_iter_order_t::H5_ITER_INC,
                    index as u64,
                    raw.as_mut_ptr().cast(),
                    raw.len(),
                    H5P_DEFAULT,
                )
            };
            if actual < 0 || actual as usize != len {
                return Err(Error(
                    "sparse attribute name changed during snapshot".into(),
                ));
            }
            raw.truncate(len);
            String::from_utf8(raw)
                .map_err(|_| Error("non-UTF8 sparse attribute name unsupported".into()))
        })?;
        names.push(name);
    }
    Ok(names)
}
