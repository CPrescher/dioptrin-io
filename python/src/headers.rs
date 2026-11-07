//! Owned snapshots of existing reader fields, converted to Python after unlocking.
use dioptrin_io::wrapped::{Metadata, Reader};
use pyo3::{
    prelude::*,
    types::{PyBytes, PyDict, PyList, PyTuple},
};

#[derive(Clone)]
pub enum Key {
    Text(String),
    Tag(u16),
}

impl From<&str> for Key {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}
impl From<String> for Key {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

#[derive(Clone)]
pub enum Value {
    #[cfg(feature = "hdf5")]
    Null,
    Text(String),
    Bytes(Vec<u8>),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    Bool(bool),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    Dict(Fields),
}

macro_rules! scalar {
    ($variant:ident: $($ty:ty),*) => {$(
        impl From<$ty> for Value {
            fn from(value: $ty) -> Self { Self::$variant(value.into()) }
        }
    )*};
}
scalar!(Signed: i8, i16, i32, i64);
scalar!(Unsigned: u8, u16, u32, u64);
scalar!(Float: f32, f64);
scalar!(Bool: bool);
scalar!(Text: String, &str);

impl Value {
    fn to_python<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        macro_rules! number {
            ($value:expr) => {
                $value.into_pyobject(py)?.to_owned().into_any()
            };
        }
        Ok(match self {
            #[cfg(feature = "hdf5")]
            Self::Null => py.None().into_bound(py),
            Self::Text(v) => number!(v),
            Self::Bytes(v) => PyBytes::new(py, v).into_any(),
            Self::Signed(v) => number!(v),
            Self::Unsigned(v) => number!(v),
            Self::Float(v) => number!(v),
            Self::Bool(v) => number!(v),
            Self::List(v) => PyList::new(
                py,
                v.iter()
                    .map(|v| v.to_python(py))
                    .collect::<PyResult<Vec<_>>>()?,
            )?
            .into_any(),
            Self::Tuple(v) => PyTuple::new(
                py,
                v.iter()
                    .map(|v| v.to_python(py))
                    .collect::<PyResult<Vec<_>>>()?,
            )?
            .into_any(),
            Self::Dict(v) => v.to_dict(py)?.into_any(),
        })
    }
}

#[derive(Clone, Default)]
pub struct Fields(pub Vec<(Key, Value)>);

impl Fields {
    pub fn insert(&mut self, name: impl Into<Key>, value: impl Into<Value>) {
        self.0.push((name.into(), value.into()));
    }
    pub fn strings<'a>(values: impl IntoIterator<Item = (&'a String, &'a String)>) -> Self {
        Self(
            values
                .into_iter()
                .map(|(k, v)| (Key::Text(k.clone()), Value::Text(v.clone())))
                .collect(),
        )
    }
    pub fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.0 {
            let value = value.to_python(py)?;
            match key {
                Key::Text(k) => dict.set_item(k, value)?,
                Key::Tag(k) => dict.set_item(k, value)?,
            }
        }
        Ok(dict)
    }
}

pub fn list<T: Into<Value>>(values: impl IntoIterator<Item = T>) -> Value {
    Value::List(values.into_iter().map(Into::into).collect())
}

#[derive(Default)]
pub struct Snapshot {
    pub fields: Fields,
    pub additional: Fields,
}

pub fn snapshot(reader: &Reader, frame: usize) -> PyResult<Snapshot> {
    let metadata = reader
        .metadata(frame)
        .ok_or_else(|| pyo3::exceptions::PyRuntimeError::new_err("frame has no source metadata"))?;
    let mut additional = Fields::default();
    let fields = match metadata {
        Metadata::Native(m) => {
            let mut fields = Fields::strings(&m.fields);
            if m.format == Some(dioptrin_io::native::Format::Npy) {
                fields.insert(
                    "shape",
                    Value::Tuple(
                        m.source_shape
                            .iter()
                            .map(|&v| Value::Unsigned(v as u64))
                            .collect(),
                    ),
                );
                fields.insert("fortran_order", m.fortran_order);
            }
            fields
        }
        Metadata::Edf(m) => Fields::strings(&m.fields),
        Metadata::Cbf(m) => {
            // Namespace binary/MIME declarations to avoid colliding with CIF tags.
            let mut fields = Fields::default();
            fields.insert("cif", Value::Dict(Fields::strings(&m.cif.fields)));
            fields.insert("binary", Value::Dict(Fields::strings(&m.binary_fields)));
            fields.insert(
                "loops",
                Value::List(
                    m.cif
                        .loops
                        .iter()
                        .map(|group| {
                            let mut fields = Fields::default();
                            fields.insert("columns", list(group.columns.iter().cloned()));
                            fields.insert(
                                "rows",
                                Value::List(
                                    group.rows.iter().map(|r| list(r.iter().cloned())).collect(),
                                ),
                            );
                            Value::Dict(fields)
                        })
                        .collect(),
                ),
            );
            additional.insert("cif_diagnostics", list(m.cif.diagnostics.iter().cloned()));
            additional.insert(
                "cif_entries",
                Value::List(
                    m.cif
                        .entries
                        .iter()
                        .map(|(k, v)| list([k.clone(), v.clone()]))
                        .collect(),
                ),
            );
            additional.insert("cif_data_blocks", list(m.cif.data_blocks.iter().cloned()));
            if let Some(pilatus) = &m.pilatus {
                fields.insert("pilatus", Value::Dict(Fields::strings(&pilatus.fields)));
            }
            fields
        }
        Metadata::Tiff(m) => {
            additional.insert("opaque_tags", Value::Dict(opaque_tiff(m)));
            tiff(m)?
        }
        Metadata::DetectorTiff(m) => {
            let mut fields = tiff(m.image)?;
            additional.insert("opaque_tags", Value::Dict(opaque_tiff(m.image)));
            let acquisition = match m.header {
                dioptrin_io::detector_tiff::Header::Pilatus(h) => Fields::strings(&h.fields),
                dioptrin_io::detector_tiff::Header::MarCcd(h) => {
                    use dioptrin_io::detector_tiff::Value as V;
                    let mut fields = Fields::default();
                    for (k, v) in &h.fields {
                        fields.insert(
                            k.clone(),
                            match v {
                                V::Unsigned(v) => Value::from(*v),
                                V::Signed(v) => Value::from(*v),
                                V::UnsignedArray(v) => list(v.iter().copied()),
                                V::SignedArray(v) => list(v.iter().copied()),
                                V::ShortArray(v) => list(v.iter().copied()),
                                V::Bytes(v) => Value::Bytes(v.clone()),
                            },
                        );
                    }
                    fields
                }
            };
            fields.insert("acquisition", Value::Dict(acquisition));
            fields
        }
        #[cfg(feature = "hdf5")]
        Metadata::Hdf5(m) => {
            let result = hdf5::attributes(m)?;
            additional = result.additional;
            result.fields
        }
        #[cfg(feature = "formats")]
        Metadata::Bruker(m) => Fields::strings(&m.fields),
        #[cfg(feature = "formats")]
        Metadata::Kcd(m) => Fields::strings(&m.fields),
        #[cfg(feature = "formats")]
        Metadata::Esperanto(m) => Fields::strings(&m.fields),
        #[cfg(feature = "formats")]
        Metadata::Mpa(m) => Fields::strings(&m.fields),
        #[cfg(feature = "formats")]
        Metadata::Mar345(m) => {
            let mut fields = Fields::strings(&m.fields);
            fields.insert("binary_words", list(m.binary_words));
            fields
        }
        #[cfg(feature = "formats")]
        Metadata::Oxford(m) => {
            use dioptrin_io::oxford::HeaderValue as V;
            let mut fields = Fields::default();
            for (k, v) in &m.fields {
                fields.insert(
                    k.clone(),
                    match v {
                        V::Unsigned(v) => Value::from(*v),
                        V::Signed(v) => Value::from(*v),
                        V::Float(v) => Value::from(*v),
                        V::Bytes(v) => Value::Bytes(v.clone()),
                    },
                );
            }
            fields
        }
        #[cfg(feature = "formats")]
        Metadata::Raxis(m) => {
            use dioptrin_io::raxis::HeaderValue as V;
            let mut fields = Fields::default();
            for (k, v) in &m.fields {
                fields.insert(
                    k.clone(),
                    match v {
                        V::Integer(v) => Value::from(*v),
                        V::Float(v) => Value::from(*v),
                        V::Bytes(v) => Value::Bytes(v.clone()),
                    },
                );
            }
            fields
        }
        #[cfg(feature = "formats")]
        Metadata::Fit2d(m) => {
            use dioptrin_io::fit2d::HeaderValue as V;
            let mut fields = Fields::default();
            for (k, v) in &m.fields {
                fields.insert(
                    k.clone(),
                    match v {
                        V::Text(v) => Value::from(v.clone()),
                        V::Integer(v) => Value::from(*v),
                        V::Float(v) => Value::from(*v),
                    },
                );
            }
            fields
        }
        #[cfg(feature = "formats")]
        Metadata::Ge(m) => {
            use dioptrin_io::ge::HeaderValue as V;
            let mut fields = Fields::strings(&m.edf_fields);
            for (k, v) in &m.fields {
                fields.insert(
                    *k,
                    match v {
                        V::Bytes(v) => Value::Bytes(v.clone()),
                        V::U16(v) => Value::from(*v),
                        V::U32(v) => Value::from(*v),
                        V::F32(v) => Value::from(*v),
                        V::F64(v) => Value::from(*v),
                    },
                );
            }
            fields
        }
        #[cfg(feature = "formats")]
        Metadata::Pixi(m) => {
            let mut fields = Fields::default();
            // Only the record-size word has an established interpretation.
            fields.insert(
                "record_size",
                i32::from_le_bytes(m.headers[frame][..4].try_into().expect("fixed header")),
            );
            additional.insert("raw_header", Value::Bytes(m.headers[frame].to_vec()));
            fields
        }
        #[cfg(feature = "formats")]
        Metadata::Spe(m) => {
            // Native convenience fields can include defaults and a reformatted
            // timestamp; those remain in metadata diagnostics, not source fields.
            Fields::strings(m.fields.iter().filter(|(name, value)| {
                !matches!(name.as_str(), "version" | "time")
                    && !(name.as_str() == "detector" && value.as_str() == "unspecified")
            }))
        }
        #[cfg(feature = "formats")]
        Metadata::Mrc(m) => {
            let h = &m.parsed;
            let mut fields = Fields::default();
            macro_rules! scalars { ($($name:ident),*) => { $(fields.insert(stringify!($name), h.$name);)* }; }
            scalars!(
                nx, ny, nz, mode, nxstart, nystart, nzstart, mx, my, mz, xlen, ylen, zlen, alpha,
                beta, gamma, mapc, mapr, maps, dmin, dmax, dmean, ispg, nsymbt, rms, nlabl
            );
            fields.insert("origin", list(h.origin));
            fields.insert("extra", Value::Bytes(h.extra.to_vec()));
            fields.insert("map", Value::Bytes(h.map.to_vec()));
            fields.insert("machst", Value::Bytes(h.machst.to_vec()));
            fields.insert("label", Value::Bytes(h.label.to_vec()));
            fields
        }
        #[cfg(feature = "formats")]
        Metadata::Jpeg(m) => {
            let mut fields = Fields::default();
            fields.insert(
                "segments",
                Value::List(
                    m.segments
                        .iter()
                        .map(|s| {
                            let mut fields = Fields::default();
                            fields.insert("marker", s.marker);
                            fields.insert("payload", Value::Bytes(s.payload.clone()));
                            Value::Dict(fields)
                        })
                        .collect(),
                ),
            );
            fields.insert(
                "jfif",
                Value::List(
                    m.jfif_headers()
                        .map(|h| {
                            let mut fields = Fields::default();
                            fields.insert("version", list(h.version));
                            fields.insert("density_unit", h.density_unit);
                            fields.insert("density", list(h.density));
                            fields.insert("thumbnail_shape", list(h.thumbnail_shape));
                            Value::Dict(fields)
                        })
                        .collect(),
                ),
            );
            fields
        }
        #[cfg(feature = "formats")]
        Metadata::Jpeg2000(m) => {
            let mut fields = Fields::default();
            fields.insert(
                "boxes",
                Value::List(
                    m.boxes
                        .iter()
                        .map(|b| {
                            let mut fields = Fields::default();
                            fields.insert("type", Value::Bytes(b.kind.to_vec()));
                            fields.insert("payload", Value::Bytes(b.payload.clone()));
                            Value::Dict(fields)
                        })
                        .collect(),
                ),
            );
            fields.insert(
                "codestream_header",
                Value::Bytes(m.codestream_header.clone()),
            );
            fields
        }
        #[cfg(feature = "formats")]
        Metadata::Dm3(m) => {
            let mut fields = Fields::default();
            fields.insert(
                "tags",
                Value::List(
                    m.tags
                        .iter()
                        .map(|tag| {
                            let mut fields = Fields::default();
                            fields.insert("path", list(tag.path.iter().cloned()));
                            fields.insert("name", Value::Bytes(tag.raw_name.clone()));
                            fields.insert("descriptor", list(tag.descriptor.iter().copied()));
                            // Never load large tag/image arrays merely to expose a header.
                            if let Some(raw) = &tag.raw {
                                fields.insert("raw", Value::Bytes(raw.clone()));
                            }
                            fields.insert("offset", tag.offset);
                            fields.insert("bytes", tag.bytes as u64);
                            Value::Dict(fields)
                        })
                        .collect(),
                ),
            );
            fields
        }
        #[cfg(feature = "formats")]
        Metadata::Xsd(m) => {
            let mut fields = Fields::default();
            fields.insert("dtype", m.dtype.clone());
            fields.insert("shape", list([m.shape[1] as u64, m.shape[0] as u64]));
            fields.insert("size", (m.shape[0] * m.shape[1]) as u64);
            fields.insert(
                "coding",
                match m.coding {
                    dioptrin_io::xsd::Coding::Base16 => "base16",
                    dioptrin_io::xsd::Coding::Base32 => "base32",
                    dioptrin_io::xsd::Coding::Base64 => "base64",
                },
            );
            if let Some(md5) = &m.md5 {
                fields.insert("md5sum", md5.clone());
            }
            fields
        }
    };
    Ok(Snapshot { fields, additional })
}

fn tiff(metadata: &dioptrin_io::tiff::Metadata) -> PyResult<Fields> {
    let mut fields = Fields::default();
    for (key, value) in &metadata.tags {
        fields.insert(Key::Tag(*key), tiff_value(value)?);
    }
    Ok(fields)
}

fn opaque_tiff(metadata: &dioptrin_io::tiff::Metadata) -> Fields {
    let mut fields = Fields::default();
    for (tag, value) in &metadata.opaque_tags {
        let mut record = Fields::default();
        record.insert("field_type", value.field_type);
        record.insert("count", value.count);
        record.insert("ifd_entry", Value::Bytes(value.ifd_entry.clone()));
        record.insert("value_bytes", Value::Bytes(value.value_bytes.clone()));
        record.insert("reason", value.reason.clone());
        fields.insert(Key::Tag(*tag), Value::Dict(record));
    }
    fields
}

#[allow(deprecated)] // Preserve even the backend's deprecated rational variants.
fn tiff_value(value: &dioptrin_io::tiff::TagValue) -> PyResult<Value> {
    use dioptrin_io::tiff::TagValue as V;
    Ok(match value {
        V::Byte(v) => Value::from(*v),
        V::Short(v) => Value::from(*v),
        V::SignedByte(v) => Value::from(*v),
        V::SignedShort(v) => Value::from(*v),
        V::Signed(v) => Value::from(*v),
        V::SignedBig(v) => Value::from(*v),
        V::Unsigned(v) => Value::from(*v),
        V::UnsignedBig(v) => Value::from(*v),
        V::Ifd(v) => Value::from(*v),
        V::IfdBig(v) => Value::from(*v),
        V::Float(v) => Value::from(*v),
        V::Double(v) => Value::from(*v),
        V::Ascii(v) => Value::from(v.clone()),
        V::List(v) => Value::List(v.iter().map(tiff_value).collect::<PyResult<_>>()?),
        V::Rational(a, b) => Value::Tuple(vec![Value::from(*a), Value::from(*b)]),
        V::SRational(a, b) => Value::Tuple(vec![Value::from(*a), Value::from(*b)]),
        V::RationalBig(a, b) => Value::Tuple(vec![Value::from(*a), Value::from(*b)]),
        V::SRationalBig(a, b) => Value::Tuple(vec![Value::from(*a), Value::from(*b)]),
        _ => {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "unsupported TIFF header tag value",
            ));
        }
    })
}

#[cfg(feature = "hdf5")]
mod hdf5;
