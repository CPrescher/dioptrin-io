//! Bounded attribute inspection using the same HDF5 backend as the image reader.
//! The caller brackets this read with the existing reader's source checks.
use super::{Fields, Snapshot, Value, list};
use hdf5_backend::{Attribute, File, Location, types::TypeDescriptor};
use pyo3::{exceptions::PyOSError, prelude::*};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ATTRIBUTES: usize = 4096;
const MAX_DEPTH: usize = 64;

fn error(error: impl std::fmt::Display) -> PyErr {
    PyOSError::new_err(error.to_string())
}

struct Budget(usize);
impl Budget {
    fn charge(&mut self, bytes: usize) -> PyResult<()> {
        self.0 = self
            .0
            .checked_sub(bytes)
            .ok_or_else(|| error("HDF5 header attributes exceed metadata budget"))?;
        Ok(())
    }
}

pub fn attributes(metadata: &dioptrin_io::hdf5::typed::Metadata) -> PyResult<Snapshot> {
    let file = File::open(&metadata.source_file).map_err(error)?;
    let mut scopes = vec!["/".to_owned()];
    let mut current = String::new();
    for part in metadata.dataset.split('/').filter(|p| !p.is_empty()) {
        current.push('/');
        current.push_str(part);
        scopes.push(current.clone());
        if scopes.len() > MAX_DEPTH {
            return Err(error("HDF5 header ancestry exceeds depth limit"));
        }
    }
    let mut fields = Fields::default();
    let mut types = Fields::default();
    let mut shapes = Fields::default();
    let mut unsupported = Fields::default();
    let mut budget = Budget(MAX_BYTES);
    let mut count = 0usize;
    for scope in scopes {
        let location: Location = if scope == metadata.dataset {
            file.dataset(&scope)
                .map_err(error)?
                .as_location()
                .map_err(error)?
        } else {
            file.group(&scope)
                .map_err(error)?
                .as_location()
                .map_err(error)?
        };
        let names = location.attr_names().map_err(error)?;
        count = count
            .checked_add(names.len())
            .filter(|&n| n <= MAX_ATTRIBUTES)
            .ok_or_else(|| error("HDF5 header attribute count exceeds limit"))?;
        let mut values = Fields::default();
        let mut scope_types = Fields::default();
        let mut scope_shapes = Fields::default();
        for name in names {
            budget.charge(scope.len().saturating_add(name.len()).saturating_add(512))?;
            let attr = location.attr(&name).map_err(error)?;
            let shape = attr.shape();
            // Empty trailing axes do not make preceding Python list nodes free.
            // For example, (huge, 0) contains no scalars but needs huge empty lists.
            let mut prefix = 1usize;
            for &dimension in &shape {
                prefix = prefix
                    .checked_mul(dimension)
                    .ok_or_else(|| error("HDF5 attribute shape overflow"))?;
                budget.charge(
                    prefix
                        .checked_mul(32)
                        .ok_or_else(|| error("HDF5 attribute shape budget overflow"))?,
                )?;
            }
            scope_shapes.insert(name.clone(), list(shape.iter().map(|&n| n as u64)));
            let datatype = attr.dtype().map_err(error)?;
            let descriptor = match datatype.to_descriptor() {
                Ok(value) => value,
                Err(reason) => {
                    unsupported.insert(format!("{scope}@{name}"), reason.to_string());
                    continue;
                }
            };
            scope_types.insert(
                name.clone(),
                format!("{descriptor}; byte_order={:?}", datatype.byte_order()),
            );
            if attr.space().map_err(error)?.is_null() {
                values.insert(name, Value::Null);
                continue;
            }
            let bytes = attr
                .size()
                .checked_mul(128usize.max(datatype.size()))
                .ok_or_else(|| error("HDF5 attribute size overflow"))?;
            budget.charge(bytes)?;
            match value(&attr, &descriptor, &mut budget)? {
                Some(value) => values.insert(name, reshape(value, &shape)?),
                None => unsupported.insert(
                    format!("{scope}@{name}"),
                    format!("unsupported attribute type: {descriptor}"),
                ),
            }
        }
        if !values.0.is_empty() {
            fields.insert(scope.clone(), Value::Dict(values));
        }
        if !scope_types.0.is_empty() {
            types.insert(scope.clone(), Value::Dict(scope_types));
        }
        if !scope_shapes.0.is_empty() {
            shapes.insert(scope, Value::Dict(scope_shapes));
        }
    }
    let mut additional = Fields::default();
    additional.insert("header_attribute_types", Value::Dict(types));
    additional.insert("header_attribute_shapes", Value::Dict(shapes));
    additional.insert("header_unsupported", Value::Dict(unsupported));
    Ok(Snapshot { fields, additional })
}

fn value(
    attr: &Attribute,
    descriptor: &TypeDescriptor,
    budget: &mut Budget,
) -> PyResult<Option<Vec<Value>>> {
    use TypeDescriptor as T;
    use hdf5_backend::types::{VarLenAscii, VarLenUnicode};
    let values = match descriptor {
        T::Integer(_) => attr
            .read_raw::<i64>()
            .map_err(error)?
            .into_iter()
            .map(Value::from)
            .collect(),
        T::Unsigned(_) => attr
            .read_raw::<u64>()
            .map_err(error)?
            .into_iter()
            .map(Value::from)
            .collect(),
        T::Float(_) => attr
            .read_raw::<f64>()
            .map_err(error)?
            .into_iter()
            .map(Value::from)
            .collect(),
        T::Boolean => {
            let dtype = attr.dtype().map_err(error)?;
            if dtype.size() != 1 {
                return Err(error("unsupported HDF5 Boolean header width"));
            }
            let mut raw = vec![0u8; attr.size()];
            hdf5_backend::sync::sync(|| {
                // SAFETY: a validated one-byte enum and exactly size() bytes.
                // Read into u8, not Rust bool, so invalid source codes remain safe.
                let result = unsafe {
                    hdf5_sys::h5a::H5Aread(attr.id(), dtype.id(), raw.as_mut_ptr().cast())
                };
                if result < 0 {
                    Err(error("cannot read HDF5 Boolean header attribute"))
                } else {
                    Ok(())
                }
            })?;
            raw.into_iter()
                .map(|v| match v {
                    0 => Value::Bool(false),
                    1 => Value::Bool(true),
                    code => Value::from(code),
                })
                .collect()
        }
        T::VarLenAscii => {
            let values = attr.read_raw::<VarLenAscii>().map_err(error)?;
            let mut output = Vec::with_capacity(values.len());
            for value in values {
                budget.charge(value.as_bytes().len())?;
                output.push(Value::Bytes(value.as_bytes().to_vec()));
            }
            output
        }
        T::VarLenUnicode => {
            let values = attr.read_raw::<VarLenUnicode>().map_err(error)?;
            let mut output = Vec::with_capacity(values.len());
            for value in values {
                budget.charge(value.as_bytes().len())?;
                // The declared HDF5 character set does not validate source bytes.
                output.push(Value::Text(
                    String::from_utf8(value.as_bytes().to_vec()).map_err(error)?,
                ));
            }
            output
        }
        T::FixedAscii(width) | T::FixedUnicode(width) => {
            if *width == 0 {
                return Err(error("zero-width HDF5 header string"));
            }
            let dtype = attr.dtype().map_err(error)?;
            let mut raw = vec![
                0u8;
                attr.size()
                    .checked_mul(*width)
                    .ok_or_else(|| error("HDF5 string size overflow"))?
            ];
            hdf5_backend::sync::sync(|| {
                // SAFETY: exact attribute element count and original fixed-width
                // memory dtype; destination has count*width bytes. The HDF5 mutex
                // protects the live attribute and datatype handles.
                let result = unsafe {
                    hdf5_sys::h5a::H5Aread(attr.id(), dtype.id(), raw.as_mut_ptr().cast())
                };
                if result < 0 {
                    Err(error("cannot read fixed-width HDF5 header attribute"))
                } else {
                    Ok(())
                }
            })?;
            raw.chunks_exact(*width)
                .map(|bytes| {
                    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                    if matches!(descriptor, T::FixedUnicode(_)) {
                        std::str::from_utf8(&bytes[..end])
                            .map(|s| Value::Text(s.to_owned()))
                            .map_err(error)
                    } else {
                        Ok(Value::Bytes(bytes[..end].to_vec()))
                    }
                })
                .collect::<PyResult<_>>()?
        }
        _ => return Ok(None),
    };
    Ok(Some(values))
}

fn reshape(values: Vec<Value>, shape: &[usize]) -> PyResult<Value> {
    fn take(values: &mut std::vec::IntoIter<Value>, shape: &[usize]) -> PyResult<Value> {
        if let Some((&first, rest)) = shape.split_first() {
            Ok(Value::List(
                (0..first)
                    .map(|_| take(values, rest))
                    .collect::<PyResult<_>>()?,
            ))
        } else {
            values
                .next()
                .ok_or_else(|| error("HDF5 attribute shape has no value"))
        }
    }
    if shape.len() > MAX_DEPTH {
        return Err(error("HDF5 header attribute rank exceeds limit"));
    }
    let mut values = values.into_iter();
    let result = take(&mut values, shape)?;
    if values.next().is_some() {
        return Err(error("HDF5 header attribute shape has extra values"));
    }
    Ok(result)
}
