//! Shared bounded original HDF5 attribute metadata. Extracted unchanged from
//! the sparse reader so read-only metadata APIs share its exact typed handling.
use super::{Error, sources::Sources, typed};
use crate::{PixelBuffer, PixelType};
use hdf5_backend::{Container, Datatype, Location, types::TypeDescriptor};
use hdf5_sys::{
    h5::{H5_index_t, H5_iter_order_t},
    h5a::{H5Aget_name_by_idx, H5Aread},
    h5d::H5Dread,
    h5p::H5P_DEFAULT,
    h5s::H5S_ALL,
};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Debug, Clone)]
pub enum AttributeValue {
    Numeric {
        shape: Vec<usize>,
        pixel_type: PixelType,
        little_endian: bool,
        values: PixelBuffer,
    },
    Text(Vec<String>),
}
#[derive(Debug, Clone)]
pub struct AttributeStorage {
    pub shape: Vec<usize>,
    pub type_descriptor: String,
}
#[derive(Debug, Clone)]
pub struct Attributes {
    pub source_file: PathBuf,
    pub path: String,
    pub values: BTreeMap<String, AttributeValue>,
    /// Original shape/character representation for text and numeric attributes.
    pub declarations: BTreeMap<String, AttributeStorage>,
}
pub(super) fn snapshot(
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
#[cfg(feature = "sparse")]
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
