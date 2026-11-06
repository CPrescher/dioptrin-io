//! Immutable typed HDF5 images/stacks, with explicit detector discovery and
//! complete tracking of the local files traversed by admitted external links.
pub use super::sources::{Link, Source};
pub use super::vds::{
    FilenameBinding, Mapping as VdsMapping, SelectionRecord as VdsSelection, UnmappedRegions,
    VdsMetadata, VdsOptions,
};
pub use super::{Error, Filter};
use super::{sources::Sources, vds};
use crate::{PixelBuffer, PixelType};
use hdf5_backend::{Container, Dataset, Dataspace, Datatype, LocationType, types::TypeDescriptor};
use hdf5_sys::{
    h5a::{H5Aexists, H5Aread},
    h5d::{H5D_space_status_t, H5Dget_space_status, H5Dread},
    h5p::H5P_DEFAULT,
    h5s::H5S_ALL,
    h5t::H5Tequal,
};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExternalLinks {
    #[default]
    Reject,
    /// Follow exact link filenames, snapshotting every master/intermediate/data
    /// file. All must be immutable; native external-file search is not used.
    FollowSnapshot,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub frames: usize,
    pub frame_bytes: usize,
    pub chunk_bytes: usize,
    pub metadata_bytes: usize,
    pub text_bytes: usize,
    pub source_files: usize,
    pub datasets: usize,
    pub group_members: usize,
    pub link_traversals: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            frames: 100_000,
            frame_bytes: 256 * 1024 * 1024,
            chunk_bytes: 256 * 1024 * 1024,
            metadata_bytes: 16 * 1024 * 1024,
            text_bytes: 64 * 1024,
            source_files: 128,
            datasets: 1024,
            group_members: 100_000,
            link_traversals: 64,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub limits: Limits,
    pub external_links: ExternalLinks,
}
#[derive(Clone, Copy, Debug)]
pub enum Selection<'a> {
    Dataset(&'a str),
    Eiger,
    Lima,
    Lambda,
}

#[derive(Clone, Debug)]
pub struct Metadata {
    pub source_file: std::path::PathBuf,
    pub dataset: String,
    pub source_shape: Vec<usize>,
    pub shape: [usize; 2],
    pub frames: usize,
    pub first_frame: usize,
    pub pixel_type: PixelType,
    pub little_endian: bool,
    pub layout: hdf5_backend::dataset::Layout,
    pub filters: Vec<Filter>,
    /// HDF5-declared scalar fill, including its default zero. Returning fill
    /// from unwritten dataset regions follows HDF5 storage semantics; it does
    /// not repair missing external files or truncated data.
    pub fill_value: Option<PixelBuffer>,
    pub storage_allocated: bool,
    pub fill_time: hdf5_backend::dataset::FillTime,
    /// Explicit virtual mapping provenance. Source frame extents are derived
    /// from concrete bound modules; missing modules never supply fill.
    pub virtual_dataset: Option<VdsMetadata>,
}
struct Part {
    dataset: Dataset,
    mosaic: Option<vds::Mosaic>,
    metadata: Metadata,
}
pub struct Reader {
    parts: Vec<Part>,
    sources: Sources,
    frames: usize,
    fields: BTreeMap<String, String>,
}
#[derive(Default, Debug)]
pub struct FrameBuffer {
    pixels: PixelBuffer,
    shape: Option<[usize; 2]>,
}
impl FrameBuffer {
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        self.shape.map(|_| &self.pixels)
    }
    #[must_use]
    pub fn shape(&self) -> Option<[usize; 2]> {
        self.shape
    }
}
impl Reader {
    /// Open a frozen image/scan. Acquisitions must have stopped, including for
    /// resizable datasets. Missing links and unsupported parts fail the complete
    /// scan; no partial sequence or neighboring file is substituted.
    /// # Errors
    /// Invalid bounds, source changes, missing/unsupported storage, dtype,
    /// filters, discovery structure or metadata.
    pub fn open_snapshot(
        path: impl AsRef<Path>,
        selection: Selection<'_>,
        options: Options,
    ) -> Result<Self, Error> {
        Self::open_inner(path, selection, options, None)
    }
    /// Explicit immutable VDS mosaics with caller-approved filename bindings.
    /// Regular datasets keep their existing interpretation and ignore unused
    /// global VDS bindings. Virtual datasets require a complete bound graph.
    /// # Errors
    /// Missing/ambiguous bindings, unsupported selections, overlapping modules,
    /// undefined unmapped fill, unavailable/changed concrete sources or limits.
    pub fn open_snapshot_with_vds(
        path: impl AsRef<Path>,
        selection: Selection<'_>,
        options: Options,
        vds: VdsOptions<'_>,
    ) -> Result<Self, Error> {
        Self::open_inner(path, selection, options, Some(vds))
    }
    fn open_inner(
        path: impl AsRef<Path>,
        selection: Selection<'_>,
        options: Options,
        vds_options: Option<VdsOptions<'_>>,
    ) -> Result<Self, Error> {
        let limits = options.limits;
        if [
            limits.frames,
            limits.frame_bytes,
            limits.chunk_bytes,
            limits.metadata_bytes,
            limits.text_bytes,
            limits.source_files,
            limits.datasets,
            limits.group_members,
            limits.link_traversals,
        ]
        .contains(&0)
            || limits.text_bytes == usize::MAX
        {
            return Err(Error(
                "typed HDF5 limits must be positive and finite".into(),
            ));
        }
        super::register_filters()?;
        let mut sources = Sources::new(path, limits, options.external_links)?;
        let mut bindings = None;
        let mut indexed_datasets = 0usize;
        let mut fields = BTreeMap::new();
        let requests = match selection {
            Selection::Dataset(path) => {
                let (source, path) = sources.resolve(0, path)?;
                if sources.file(source).loc_type_by_name(&path)? == LocationType::Group {
                    vec![(source, format!("{}/data", path.trim_end_matches('/')))]
                } else {
                    vec![(source, path)]
                }
            }
            Selection::Eiger => {
                let (source, entry, group) = sources.group(0, "/entry")?;
                let names = sources.names(&group)?;
                if names.iter().any(|n| n == "data") {
                    let (source, path) = sources.resolve(source, &format!("{entry}/data"))?;
                    if sources.file(source).loc_type_by_name(&path)? == LocationType::Group {
                        let group = sources.file(source).group(&path)?;
                        sources
                            .names(&group)?
                            .into_iter()
                            .filter(|n| n.starts_with("data"))
                            .map(|n| (source, format!("{path}/{n}")))
                            .collect()
                    } else {
                        vec![(source, path)]
                    }
                } else {
                    names
                        .into_iter()
                        .filter(|n| n.starts_with("data"))
                        .map(|n| (source, format!("{entry}/{n}")))
                        .collect()
                }
            }
            Selection::Lima => {
                let attr = sources.file(0).attr("default")?;
                let entry = read_text(&attr, limits.text_bytes, true)?;
                sources.charge(entry.len())?;
                let (source, path, group) = sources.group(0, &entry)?;
                let has_default = hdf5_backend::sync::sync(|| {
                    // SAFETY: live group and a static NUL-terminated name.
                    let exists = unsafe { H5Aexists(group.id(), c"default".as_ptr()) };
                    if exists < 0 {
                        Err(Error("cannot inspect LImA default metadata".into()))
                    } else {
                        Ok(exists > 0)
                    }
                })?;
                if has_default {
                    let attr = group.attr("default")?;
                    let default = read_text(&attr, limits.text_bytes, true)?;
                    let detector = default
                        .trim_end_matches('/')
                        .rsplit('/')
                        .nth(1)
                        .ok_or_else(|| Error("invalid LImA detector default path".into()))?;
                    fields.insert("detector".into(), detector.into());
                    sources.charge(default.len())?;
                } else {
                    fields.insert("detector".into(), "detector".into());
                }
                fields.insert("default_entry".into(), entry);
                vec![(source, format!("{path}/measurement/data"))]
            }
            Selection::Lambda => {
                let base = "/entry/instrument/detector";
                let (source, path) = sources.resolve(0, &format!("{base}/description"))?;
                let description_dataset = sources.file(source).dataset(&path)?;
                let description = read_text(&description_dataset, limits.text_bytes, false)?;
                if description != "Lambda" {
                    return Err(Error("HDF5 detector description is not Lambda".into()));
                }
                fields.insert("description".into(), description);
                let (_, _, group) = sources.group(0, base)?;
                let names = sources.names(&group)?;
                if names.iter().any(|n| n == "local_name") {
                    let (source, path) = sources.resolve(0, &format!("{base}/local_name"))?;
                    let name_dataset = sources.file(source).dataset(&path)?;
                    let name = read_text(&name_dataset, limits.text_bytes, false)?;
                    sources.charge(name.len())?;
                    fields.insert("detector".into(), name);
                } else {
                    fields.insert("detector".into(), "detector".into());
                }
                vec![(0, format!("{base}/data"))]
            }
        };
        if requests.is_empty() || requests.len() > limits.datasets {
            return Err(Error(
                "HDF5 discovery found no datasets or exceeds dataset limit".into(),
            ));
        }
        let mut parts = Vec::new();
        let mut frames = 0usize;
        for (source, path) in requests {
            let (source, path) = sources.resolve(source, &path)?;
            let dataset = sources.file(source).dataset(&path)?;
            indexed_datasets = indexed_datasets
                .checked_add(1)
                .filter(|&n| n <= limits.datasets)
                .ok_or_else(|| {
                    Error("HDF5 snapshot exceeds concrete/virtual dataset limit".into())
                })?;
            let (pixel_type, little_endian) = standard_type(&dataset.dtype()?)?;
            let (source_shape, mosaic, virtual_dataset, virtual_storage) = if dataset.layout()
                == hdf5_backend::dataset::Layout::Virtual
            {
                if bindings.is_none() {
                    let options = vds_options.ok_or_else(|| Error("HDF5 virtual datasets/external raw storage require separate source contracts; use explicit VdsOptions".into()))?;
                    bindings = Some(vds::Bindings::new(options, limits)?);
                }
                let bindings = bindings.as_mut().expect("initialized VDS binding policy");
                let admission = vds::admit(
                    &dataset,
                    &mut sources,
                    bindings,
                    limits,
                    &mut indexed_datasets,
                    pixel_type,
                )?;
                (
                    admission.metadata.frozen_shape.to_vec(),
                    Some(admission.mosaic),
                    Some(admission.metadata),
                    Some(admission.storage),
                )
            } else {
                (dataset.shape(), None, None, None)
            };
            let (count, shape) = match source_shape.as_slice() {
                [h, w] => (1, [*h, *w]),
                [n, h, w] => (*n, [*h, *w]),
                _ => return Err(Error("typed HDF5 expects a 2D image or 3D stack".into())),
            };
            let elements = shape[0]
                .checked_mul(shape[1])
                .filter(|&n| n > 0)
                .ok_or_else(|| Error("invalid HDF5 frame shape".into()))?;
            if count == 0
                || elements
                    .checked_mul(pixel_type.bytes())
                    .is_none_or(|n| n > limits.frame_bytes)
            {
                return Err(Error("HDF5 frame exceeds decoded byte limit".into()));
            }
            // Check the entire dataset arithmetic before any HDF5 selection.
            count
                .checked_mul(elements)
                .and_then(|n| n.checked_mul(pixel_type.bytes()))
                .ok_or_else(|| Error("HDF5 dataset shape/byte overflow".into()))?;
            let next = frames
                .checked_add(count)
                .filter(|&n| n <= limits.frames)
                .ok_or_else(|| Error("HDF5 sequence exceeds frame limit".into()))?;
            let Storage {
                filters,
                fill_value,
                fill_time,
                storage_allocated,
            } = if let Some(storage) = virtual_storage {
                storage
            } else {
                admit_storage(&dataset, pixel_type, limits)?
            };
            sources.charge(
                path.len()
                    .saturating_mul(2)
                    .saturating_add(filters.len().saturating_mul(256))
                    .saturating_add(sources.path(source).as_os_str().len())
                    .saturating_add(512),
            )?;
            let metadata = Metadata {
                source_file: sources.path(source).to_path_buf(),
                dataset: path,
                source_shape,
                shape,
                frames: count,
                first_frame: frames,
                pixel_type,
                little_endian,
                layout: dataset.layout(),
                filters,
                fill_value,
                storage_allocated,
                fill_time,
                virtual_dataset,
            };
            parts.push(Part {
                dataset,
                metadata,
                mosaic,
            });
            frames = next;
        }
        if let Some(bindings) = bindings {
            bindings.finish()?;
        }
        sources.check()?;
        Ok(Self {
            parts,
            sources,
            frames,
            fields,
        })
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.frames
    }
    #[must_use]
    pub fn sources(&self) -> &[Source] {
        &self.sources.records
    }
    #[must_use]
    pub fn links(&self) -> &[Link] {
        &self.sources.links
    }
    #[must_use]
    pub fn fields(&self) -> &BTreeMap<String, String> {
        &self.fields
    }
    #[must_use]
    pub fn metadata(&self, index: usize) -> Option<&Metadata> {
        self.part(index).map(|p| &p.metadata)
    }
    fn part(&self, index: usize) -> Option<&Part> {
        if index >= self.frames {
            return None;
        }
        self.parts.get(
            self.parts
                .partition_point(|p| p.metadata.first_frame <= index)
                .checked_sub(1)?,
        )
    }
    /// Read exact native samples into reusable output storage through the
    /// existing HDF5 decoder. No integration, orientation or dtype promotion.
    /// # Errors
    /// Changed sources, out-of-range frame, decode or allocation failure.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.sources.check()?;
        let part = self
            .part(index)
            .ok_or_else(|| Error("HDF5 frame outside snapshot".into()))?;
        let meta = &part.metadata;
        if part.mosaic.is_none() && part.dataset.shape() != meta.source_shape {
            return Err(Error("HDF5 dataset changed; reopen snapshot".into()));
        }
        let count = meta.shape[0] * meta.shape[1];
        buffer
            .pixels
            .resize(meta.pixel_type, count)
            .map_err(Error)?;
        if let Some(mosaic) = &part.mosaic {
            mosaic.read(index - meta.first_frame, meta.shape, &mut buffer.pixels)?;
            self.sources.check()?;
            buffer.shape = Some(meta.shape);
            return Ok(());
        }
        let memory_type = memory_type(meta.pixel_type)?;
        let space = part.dataset.space()?;
        let space = if meta.source_shape.len() == 3 {
            space.select((index - meta.first_frame, .., ..))?
        } else {
            space
        };
        let memory = Dataspace::try_new(meta.shape)?;
        if space.selection_size() != count || memory.size() != count {
            return Err(Error("HDF5 selection shape mismatch".into()));
        }
        hdf5_backend::sync::sync(|| {
            // SAFETY: both live dataspaces select exactly count scalars. The
            // native memory datatype has precisely PixelType's byte width and
            // the typed output vector owns that many initialized writable
            // samples. Standard file dtype equality was admitted at open.
            let result = unsafe {
                H5Dread(
                    part.dataset.id(),
                    memory_type.id(),
                    memory.id(),
                    space.id(),
                    H5P_DEFAULT,
                    buffer.pixels.bytes_mut().as_mut_ptr().cast(),
                )
            };
            if result < 0 {
                Err(Error("HDF5 native frame decoding failed".into()))
            } else {
                Ok(())
            }
        })?;
        self.sources.check()?;
        buffer.shape = Some(meta.shape);
        Ok(())
    }
}

/// Shared storage admission for typed images and explicitly selected sparse arrays.
pub(super) struct Storage {
    pub filters: Vec<Filter>,
    pub fill_value: Option<PixelBuffer>,
    pub storage_allocated: bool,
    pub fill_time: hdf5_backend::dataset::FillTime,
}
pub(super) fn admit_storage(
    dataset: &Dataset,
    pixel_type: PixelType,
    limits: Limits,
) -> Result<Storage, Error> {
    let filters = admit_filters(dataset, pixel_type.bytes(), limits)?;
    let plist = dataset.dcpl()?;
    let fill_value = fill(dataset, pixel_type)?;
    let fill_time = plist.get_fill_time()?;
    let storage_allocated = storage_allocated(dataset)?;
    if !storage_allocated
        && (fill_value.is_none() || fill_time == hdf5_backend::dataset::FillTime::Never)
    {
        return Err(Error(
            "unallocated HDF5 regions have undefined fill; refusing to invent counts".into(),
        ));
    }

    Ok(Storage {
        filters,
        fill_value,
        fill_time,
        storage_allocated,
    })
}

pub(super) fn admit_filters(
    dataset: &Dataset,
    element_bytes: usize,
    limits: Limits,
) -> Result<Vec<Filter>, Error> {
    let plist = dataset.dcpl()?;
    if dataset.layout() == hdf5_backend::dataset::Layout::Virtual
        || !plist.get_external()?.is_empty()
    {
        return Err(Error(
            "HDF5 virtual datasets/external raw storage require separate source contracts".into(),
        ));
    }
    if let Some(chunk) = dataset.chunk()
        && chunk
            .into_iter()
            .try_fold(element_bytes, usize::checked_mul)
            .is_none_or(|n| n > limits.chunk_bytes)
    {
        return Err(Error("HDF5 decoded chunk exceeds byte limit".into()));
    }
    let mut filters = Vec::new();
    for filter in dataset.filters() {
        let id = filter.id();
        if ![1, 2, 3, 32000, 32008].contains(&id) || !filter.decode_enabled() {
            return Err(Error(format!(
                "unsupported/unavailable typed HDF5 filter {id}"
            )));
        }
        let parameters = if let hdf5_backend::filters::Filter::User(_, p) = &filter {
            p.clone()
        } else {
            Vec::new()
        };
        filters.push(Filter {
            id,
            parameters,
            decode_enabled: true,
        });
    }
    Ok(filters)
}
pub(super) fn storage_allocated(dataset: &Dataset) -> Result<bool, Error> {
    hdf5_backend::sync::sync(|| {
        let mut status = H5D_space_status_t::H5D_SPACE_STATUS_ERROR;
        // SAFETY: live dataset ID and writable native status enum.
        if unsafe { H5Dget_space_status(dataset.id(), &mut status) } < 0 {
            return Err(Error("cannot determine HDF5 storage allocation".into()));
        }
        Ok(status == H5D_space_status_t::H5D_SPACE_STATUS_ALLOCATED)
    })
}
pub(super) fn standard_type(dtype: &Datatype) -> Result<(PixelType, bool), Error> {
    use hdf5_backend::globals::*;
    hdf5_backend::sync::sync(|| {
        macro_rules! admit {($($v:ident,$le:ident,$be:ident);*)=>{$(
            // SAFETY: predefined standard type IDs have process lifetime and
            // dtype is a live datatype; native comparison never modifies it.
            if unsafe {H5Tequal(dtype.id(),*$le)}>0 {return Ok((PixelType::$v,true));}
            if unsafe {H5Tequal(dtype.id(),*$be)}>0 {return Ok((PixelType::$v,false));}
        )*};}
        admit!(U8,H5T_STD_U8LE,H5T_STD_U8BE;I8,H5T_STD_I8LE,H5T_STD_I8BE;
            U16,H5T_STD_U16LE,H5T_STD_U16BE;I16,H5T_STD_I16LE,H5T_STD_I16BE;
            U32,H5T_STD_U32LE,H5T_STD_U32BE;I32,H5T_STD_I32LE,H5T_STD_I32BE;
            U64,H5T_STD_U64LE,H5T_STD_U64BE;I64,H5T_STD_I64LE,H5T_STD_I64BE;
            F32,H5T_IEEE_F32LE,H5T_IEEE_F32BE;F64,H5T_IEEE_F64LE,H5T_IEEE_F64BE);
        Err(Error("HDF5 samples must use a standard 8/16/32/64-bit integer or IEEE f32/f64 type; custom precision, enums, complex and compound types are unsupported".into()))
    })
}
pub(super) fn memory_type(ty: PixelType) -> Result<Datatype, Error> {
    macro_rules! types {($($v:ident,$t:ty);*)=>{match ty {$(PixelType::$v=>Datatype::from_type::<$t>()?),*,_=>return Err(Error("complex HDF5 samples need a separate dtype contract".into()))}};}
    Ok(types!(U8,u8;I8,i8;U16,u16;I16,i16;U32,u32;I32,i32;U64,u64;I64,i64;F32,f32;F64,f64))
}
pub(super) fn fill(dataset: &Dataset, ty: PixelType) -> Result<Option<PixelBuffer>, Error> {
    let plist = dataset.dcpl()?;
    macro_rules! types {($($v:ident,$t:ty);*)=>{match ty {$(PixelType::$v=>plist.get_fill_value_as::<$t>()?.map(|v|PixelBuffer::$v(vec![v]))),*,_=>return Err(Error("complex HDF5 fill needs a separate dtype contract".into()))}};}
    Ok(types!(U8,u8;I8,i8;U16,u16;I16,i16;U32,u32;I32,i32;U64,u64;I64,i64;F32,f32;F64,f64))
}
fn read_text(container: &Container, limit: usize, attribute: bool) -> Result<String, Error> {
    if !attribute {
        let dataset = container.as_dataset()?;
        if dataset.layout() == hdf5_backend::dataset::Layout::Virtual
            || !dataset.dcpl()?.get_external()?.is_empty()
        {
            return Err(Error(
                "HDF5 discovery text cannot use VDS or raw external storage".into(),
            ));
        }
    }
    if container.size() != 1 {
        return Err(Error("HDF5 discovery text must contain one string".into()));
    }
    let descriptor = container.dtype()?.to_descriptor()?;
    let bytes = match descriptor {
        TypeDescriptor::VarLenAscii => {
            let values = container.read_raw::<hdf5_backend::types::VarLenAscii>()?;
            let bytes = values[0].as_bytes();
            if bytes.len() > limit {
                return Err(Error("HDF5 discovery text exceeds limit".into()));
            }
            bytes.to_vec()
        }
        TypeDescriptor::VarLenUnicode => {
            let values = container.read_raw::<hdf5_backend::types::VarLenUnicode>()?;
            let bytes = values[0].as_bytes();
            if bytes.len() > limit {
                return Err(Error("HDF5 discovery text exceeds limit".into()));
            }
            bytes.to_vec()
        }
        TypeDescriptor::FixedAscii(n) | TypeDescriptor::FixedUnicode(n) => {
            if n > limit {
                return Err(Error("HDF5 discovery text exceeds limit".into()));
            }
            let memory = Datatype::from_descriptor(&descriptor)?;
            let mut bytes = vec![0u8; n];
            hdf5_backend::sync::sync(|| {
                // SAFETY: one fixed-width string. The memory datatype and the
                // writable destination both have exactly n bytes. All native
                // calls hold the upstream HDF5 mutex.
                let result = unsafe {
                    if attribute {
                        H5Aread(container.id(), memory.id(), bytes.as_mut_ptr().cast())
                    } else {
                        H5Dread(
                            container.id(),
                            memory.id(),
                            H5S_ALL,
                            H5S_ALL,
                            H5P_DEFAULT,
                            bytes.as_mut_ptr().cast(),
                        )
                    }
                };
                if result < 0 {
                    return Err(Error("cannot decode HDF5 discovery text".into()));
                }
                Ok(())
            })?;
            bytes
        }
        _ => return Err(Error("HDF5 discovery requires text metadata".into())),
    };
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec())
        .map_err(|_| Error("non-UTF8 HDF5 discovery text is unsupported".into()))
}
