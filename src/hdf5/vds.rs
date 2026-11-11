//! Explicit immutable finite/unlimited VDS mappings. Native HDF5 decodes only
//! explicitly resolved concrete sources; it never resolves or reads a VDS.
use super::{
    Error, Filter,
    sources::Sources,
    typed::{self, Limits, Storage},
};
use crate::{PixelBuffer, PixelType};
use hdf5_backend::{
    Dataset, Dataspace,
    dataset::{FillTime, Layout},
};
use hdf5_sys::{
    h5d::H5Dread,
    h5i::hid_t,
    h5p::{
        H5P_DEFAULT, H5Pget_virtual_count, H5Pget_virtual_dsetname, H5Pget_virtual_filename,
        H5Pget_virtual_srcspace, H5Pget_virtual_vspace,
    },
    h5s::{
        H5S_sel_type, H5Sclose, H5Sget_regular_hyperslab, H5Sget_select_type,
        H5Sget_simple_extent_dims, H5Sget_simple_extent_ndims, H5Sis_regular_hyperslab,
    },
    h5t::H5Tequal,
};
#[path = "vds_finite.rs"]
mod finite;
#[path = "vds_graph.rs"]
mod graph;
#[path = "vds_printf.rs"]
pub(super) mod printf;
#[path = "vds_unlimited.rs"]
mod unlimited;
pub use printf::{PrintfDeclaration, PrintfExpansion, PrintfMapping};
use std::{collections::BTreeSet, path::PathBuf};

/// Exact original filename declaration and caller-approved actual local file.
/// No basename search or filename inference is performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilenameBinding {
    pub stored_filename: String,
    pub actual_file: PathBuf,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UnmappedRegions {
    /// Every logical raster sample must belong to a concrete mapped source.
    #[default]
    Reject,
    /// Unmapped raster regions use the VDS's defined original scalar fill.
    /// Missing or unallocated mapped sources never use this policy.
    DeclaredFill,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct VdsOptions<'a> {
    pub filenames: &'a [FilenameBinding],
    pub unmapped_regions: UnmappedRegions,
    pub printf_mappings: &'a [PrintfMapping],
}
/// Original selection records. u64::MAX denotes HDF5's UNLIMITED declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionRecord {
    All {
        shape: Vec<u64>,
        maximum_shape: Vec<u64>,
    },
    /// Canonical inclusive native HDF5 hyperslab blocks; iteration pairing is
    /// still performed by HDF5, never by matching block indices.
    Irregular {
        shape: [u64; 3],
        maximum_shape: [u64; 3],
        blocks: Vec<[[u64; 3]; 2]>,
    },
    Regular {
        shape: [u64; 3],
        maximum_shape: [u64; 3],
        start: [u64; 3],
        stride: [u64; 3],
        count: [u64; 3],
        block: [u64; 3],
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionLayout {
    ContiguousRectangle,
    FiniteSelection,
}
#[derive(Debug, Clone)]
pub struct Mapping {
    pub stored_filename: String,
    pub declared_dataset: String,
    pub source_file: PathBuf,
    pub resolved_dataset: String,
    pub source_shape: [usize; 3],
    pub source_selection: SelectionRecord,
    pub virtual_selection: SelectionRecord,
    pub frozen_source_selection: SelectionRecord,
    pub frozen_virtual_selection: SelectionRecord,
    pub nested_node: Option<usize>,
    pub printf_expansion: Option<PrintfExpansion>,
    pub source_origin: [usize; 3],
    pub virtual_origin: [usize; 3],
    /// Compatibility alias of virtual_bounding_shape; holes may exist for
    /// FiniteSelection. selected_elements gives actual mapped sample count.
    pub mapped_shape: [usize; 3],
    pub selection_layout: SelectionLayout,
    pub source_bounding_shape: [usize; 3],
    pub virtual_bounding_shape: [usize; 3],
    pub selected_elements: u64,
    pub filters: Vec<Filter>,
    pub source_pixel_type: PixelType,
    pub source_little_endian: bool,
    pub source_layout: Layout,
    pub source_fill_value: Option<PixelBuffer>,
    pub source_fill_time: FillTime,
    /// Concrete leaves require full allocation. Virtual nodes report false;
    /// their individually checked leaves and defined-fill provenance are separate.
    pub storage_allocated: bool,
}
#[derive(Debug, Clone)]
pub struct VdsMetadata {
    /// Original mapping dataspace dimensions, independent of native VDS lookup.
    pub declared_shape: [u64; 3],
    /// Finite: fixed declared master extent; unlimited: bound source-derived extent.
    pub frozen_shape: [usize; 3],
    /// Original maximum dimensions; u64::MAX denotes HDF5 UNLIMITED.
    pub master_max_shape: [u64; 3],
    pub mappings: Vec<Mapping>,
    pub unmapped_regions: UnmappedRegions,
    pub unmapped_pixels_per_frame: Option<usize>,
    pub unmapped_pixels_by_frame: Vec<usize>,
    pub node_id: usize,
    pub source_file: PathBuf,
    pub dataset: String,
    pub fill_value: Option<PixelBuffer>,
    pub nested_nodes: Vec<VdsNodeMetadata>,
    pub effective_fill_regions: Vec<FillRegion>,
}
#[derive(Debug, Clone)]
pub struct FillRegion {
    pub origin_node: usize,
    pub source_file: PathBuf,
    pub dataset: String,
    pub origin_selection: SelectionRecord,
    pub virtual_selection: SelectionRecord,
    pub value: PixelBuffer,
}
#[derive(Debug, Clone)]
pub struct VdsNodeMetadata {
    pub node_id: usize,
    pub source_file: PathBuf,
    pub dataset: String,
    pub declared_shape: [u64; 3],
    pub frozen_shape: [usize; 3],
    pub master_max_shape: [u64; 3],
    pub mappings: Vec<Mapping>,
    pub fill_value: Option<PixelBuffer>,
    pub fill_time: FillTime,
    pub unmapped_pixels_per_frame: Option<usize>,
    pub unmapped_pixels_by_frame: Vec<usize>,
}
pub(super) struct Mosaic {
    graph: graph::Graph,
}
pub(super) struct Admission {
    pub mosaic: Mosaic,
    pub metadata: VdsMetadata,
    pub storage: Storage,
}
pub(super) struct Bindings<'a> {
    options: VdsOptions<'a>,
    used: BTreeSet<&'a str>,
    printf_used: BTreeSet<usize>,
}
impl<'a> Bindings<'a> {
    pub fn new(options: VdsOptions<'a>, limits: Limits) -> Result<Self, Error> {
        if options.filenames.len() > limits.source_files {
            return Err(Error(
                "VDS filename bindings exceed source-file limit".into(),
            ));
        }
        let mut names = BTreeSet::new();
        for binding in options.filenames {
            if binding.stored_filename.is_empty()
                || binding.stored_filename.len() > limits.text_bytes
                || binding.stored_filename.contains('\0')
                || binding.actual_file.as_os_str().is_empty()
                || !names.insert(binding.stored_filename.as_str())
            {
                return Err(Error(
                    "invalid/duplicate VDS literal filename binding".into(),
                ));
            }
        }
        for plan in options.printf_mappings {
            printf::validate_plan(plan, limits)?;
        }
        if options.printf_mappings.len() > limits.datasets {
            return Err(Error("VDS printf plan count exceeds dataset limit".into()));
        }
        Ok(Self {
            options,
            used: BTreeSet::new(),
            printf_used: BTreeSet::new(),
        })
    }
    fn source(&mut self, name: &str, owner: usize, sources: &mut Sources) -> Result<usize, Error> {
        if name == "." {
            return Ok(owner);
        }
        let binding = self
            .options
            .filenames
            .iter()
            .find(|b| b.stored_filename == name)
            .ok_or_else(|| {
                Error(format!(
                    "VDS source requires explicit filename binding: {name}"
                ))
            })?;
        self.used.insert(&binding.stored_filename);
        sources.add(&binding.actual_file)
    }
    pub fn finish(&self) -> Result<(), Error> {
        if self.used.len() != self.options.filenames.len()
            || self.printf_used.len() != self.options.printf_mappings.len()
        {
            return Err(Error(
                "unused VDS filename binding; refusing an incomplete/wrong mapping declaration"
                    .into(),
            ));
        }
        Ok(())
    }
}
impl Mosaic {
    pub fn check(&self) -> Result<(), Error> {
        self.graph.check()
    }
    pub fn read(
        &self,
        frame: usize,
        shape: [usize; 2],
        pixels: &mut PixelBuffer,
    ) -> Result<(), Error> {
        self.graph.read(frame, shape, pixels)
    }
}
pub(super) fn admit(
    dataset: &Dataset,
    owner: (usize, &str),
    sources: &mut Sources,
    bindings: &mut Bindings<'_>,
    limits: Limits,
    indexed_datasets: &mut usize,
    pixel_type: PixelType,
) -> Result<Admission, Error> {
    graph::admit(
        dataset,
        owner,
        sources,
        bindings,
        limits,
        indexed_datasets,
        pixel_type,
    )
}
fn descriptors(
    dataset: &Dataset,
    sources: &mut Sources,
    limits: Limits,
    indexed_datasets: &mut usize,
) -> Result<Vec<Descriptor>, Error> {
    let plist = dataset.dcpl()?;
    if !plist.get_external()?.is_empty() || !dataset.filters().is_empty() {
        return Err(Error(
            "VDS master cannot contain raw external storage or filters".into(),
        ));
    }
    let descriptors = hdf5_backend::sync::sync(|| -> Result<_, Error> {
        let mut count = 0;
        // SAFETY: live creation property list and writable scalar count.
        if unsafe { H5Pget_virtual_count(plist.id(), &mut count) } < 0
            || count == 0
            || count > limits.datasets.saturating_sub(*indexed_datasets)
        {
            return Err(Error(
                "VDS mapping count exceeds dataset limit or is empty".into(),
            ));
        }
        sources.charge(
            count
                .checked_mul(512)
                .ok_or_else(|| Error("VDS descriptor metadata overflow".into()))?,
        )?;
        let mut out = Vec::new();
        out.try_reserve_exact(count)
            .map_err(|e| Error(e.to_string()))?;
        for index in 0..count {
            let filename = string(plist.id(), index, true, limits.text_bytes, sources)?;
            let path = string(plist.id(), index, false, limits.text_bytes, sources)?;
            // SAFETY: live property list/index; successful calls return newly
            // owned dataspaces, released by Space on every return path.
            let v = Space(unsafe { H5Pget_virtual_vspace(plist.id(), index) });
            let s = Space(unsafe { H5Pget_virtual_srcspace(plist.id(), index) });
            if v.0 < 0 || s.0 < 0 {
                return Err(Error("cannot inspect VDS mapping spaces".into()));
            }
            out.push((
                filename,
                path,
                selection(s.0, true, sources, limits)?,
                selection(v.0, false, sources, limits)?,
                s,
                v,
            ));
        }
        Ok(out)
    })?;
    *indexed_datasets += descriptors.len();
    Ok(descriptors)
}
type Descriptor = (
    String,
    String,
    SelectionRecord,
    SelectionRecord,
    Space,
    Space,
);
struct Space(hid_t);
impl Space {
    fn into_dataspace(mut self) -> Result<Dataspace, Error> {
        let id = self.0;
        self.0 = -1;
        // SAFETY: H5P/H5S returned an owned dataspace; ownership transfers once.
        Ok(unsafe { hdf5_backend::from_id(id)? })
    }
}

impl Drop for Space {
    fn drop(&mut self) {
        if self.0 >= 0 {
            hdf5_backend::sync::sync(|| {
                // SAFETY: uniquely owned H5Pget_virtual_* dataspace ID.
                unsafe { H5Sclose(self.0) };
            });
        }
    }
}
fn string(
    plist: hid_t,
    index: usize,
    filename: bool,
    limit: usize,
    sources: &mut Sources,
) -> Result<String, Error> {
    // SAFETY: live plist/index; null requests only length, without allocation.
    let length = unsafe {
        if filename {
            H5Pget_virtual_filename(plist, index, std::ptr::null_mut(), 0)
        } else {
            H5Pget_virtual_dsetname(plist, index, std::ptr::null_mut(), 0)
        }
    };
    let length = usize::try_from(length)
        .ok()
        .filter(|&n| n > 0 && n <= limit && n < usize::MAX)
        .ok_or_else(|| {
            Error("VDS source filename/dataset exceeds text limit or is empty".into())
        })?;
    sources.charge(
        length
            .checked_mul(2)
            .and_then(|n| n.checked_add(64))
            .ok_or_else(|| Error("VDS name metadata overflow".into()))?,
    )?;
    let mut bytes = vec![0; length + 1];
    // SAFETY: buffer has the admitted full byte count plus required NUL slot.
    let read = unsafe {
        if filename {
            H5Pget_virtual_filename(plist, index, bytes.as_mut_ptr().cast(), bytes.len())
        } else {
            H5Pget_virtual_dsetname(plist, index, bytes.as_mut_ptr().cast(), bytes.len())
        }
    };
    if read != length as isize || bytes[length] != 0 || bytes[..length].contains(&0) {
        return Err(Error("unstable/malformed VDS source name".into()));
    }
    bytes.truncate(length);
    String::from_utf8(bytes).map_err(|_| {
        Error("non-UTF8 VDS source names require a separate byte-path contract".into())
    })
}
fn selection(
    id: hid_t,
    source: bool,
    sources: &mut Sources,
    limits: Limits,
) -> Result<SelectionRecord, Error> {
    // SAFETY: live owned dataspace ID, fixed-size outputs only after rank check.
    unsafe {
        let rank = H5Sget_simple_extent_ndims(id);
        if rank != 3 && !(source && rank == 0) {
            return Err(Error("VDS selections require rank3 hyperslabs".into()));
        }
        let mut shape = [0; 3];
        let mut maximum_shape = [0; 3];
        if rank == 3
            && H5Sget_simple_extent_dims(id, shape.as_mut_ptr(), maximum_shape.as_mut_ptr()) != 3
        {
            return Err(Error("cannot inspect VDS selection extent".into()));
        }
        match H5Sget_select_type(id) {
            H5S_sel_type::H5S_SEL_ALL => Ok(SelectionRecord::All {
                shape: if rank == 0 {
                    Vec::new()
                } else {
                    shape.to_vec()
                },
                maximum_shape: if rank == 0 {
                    Vec::new()
                } else {
                    maximum_shape.to_vec()
                },
            }),
            H5S_sel_type::H5S_SEL_HYPERSLABS if rank == 3 && H5Sis_regular_hyperslab(id) > 0 => {
                let (mut start, mut stride, mut count, mut block) =
                    ([0; 3], [0; 3], [0; 3], [0; 3]);
                if H5Sget_regular_hyperslab(
                    id,
                    start.as_mut_ptr(),
                    stride.as_mut_ptr(),
                    count.as_mut_ptr(),
                    block.as_mut_ptr(),
                ) < 0
                {
                    return Err(Error("cannot inspect VDS regular selection".into()));
                }
                Ok(SelectionRecord::Regular {
                    shape,
                    maximum_shape,
                    start,
                    stride,
                    count,
                    block,
                })
            }
            H5S_sel_type::H5S_SEL_HYPERSLABS if rank == 3 => {
                let blocks = finite::blocks(id, sources, limits)?;
                Ok(SelectionRecord::Irregular {
                    shape,
                    maximum_shape,
                    blocks,
                })
            }
            _ => Err(Error("unsupported VDS point/empty selection".into())),
        }
    }
}
fn selection_shape(selection: &SelectionRecord) -> Result<([u64; 3], [u64; 3]), Error> {
    match selection {
        SelectionRecord::All {
            shape,
            maximum_shape,
        } => Ok((
            shape
                .as_slice()
                .try_into()
                .map_err(|_| Error("VDS virtual extent must have rank3".into()))?,
            maximum_shape
                .as_slice()
                .try_into()
                .map_err(|_| Error("VDS virtual maximum extent must have rank3".into()))?,
        )),
        SelectionRecord::Regular {
            shape,
            maximum_shape,
            ..
        }
        | SelectionRecord::Irregular {
            shape,
            maximum_shape,
            ..
        } => Ok((*shape, *maximum_shape)),
    }
}
fn rectangle(
    selection: &SelectionRecord,
    source: Option<[usize; 3]>,
) -> Result<([usize; 3], [Option<usize>; 3]), Error> {
    match selection {
        SelectionRecord::Irregular { .. } => Err(Error(
            "irregular VDS selection requires finite projection".into(),
        )),
        SelectionRecord::All { shape, .. } => {
            let extents = if let Some(source) = source {
                source
            } else {
                let shape: [u64; 3] = shape
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error("VDS ALL virtual extent must have rank3".into()))?;
                let mut sizes = [0; 3];
                for i in 0..3 {
                    sizes[i] = usize::try_from(shape[i])
                        .map_err(|_| Error("VDS ALL extent exceeds platform".into()))?;
                }
                sizes
            };
            Ok(([0; 3], extents.map(Some)))
        }
        SelectionRecord::Regular {
            start,
            stride,
            count,
            block,
            ..
        } => {
            let mut origin = [0; 3];
            let mut extents = [None; 3];
            for axis in 0..3 {
                origin[axis] = usize::try_from(start[axis])
                    .map_err(|_| Error("VDS selection offset exceeds platform".into()))?;
                if stride[axis] == 0
                    || count[axis] == 0
                    || block[axis] == 0
                    || count[axis] > 1 && stride[axis] != block[axis]
                {
                    return Err(Error(
                        "VDS selections must be contiguous without strides/gaps/overlap".into(),
                    ));
                }
                if count[axis] == u64::MAX || block[axis] == u64::MAX {
                    if axis != 0 || count[axis] == u64::MAX && block[axis] == u64::MAX {
                        return Err(Error("VDS unlimited selections are supported only for contiguous frame extent".into()));
                    }
                } else {
                    extents[axis] = count[axis]
                        .checked_mul(block[axis])
                        .and_then(|n| usize::try_from(n).ok());
                    if extents[axis].is_none() {
                        return Err(Error("VDS selection size overflow".into()));
                    }
                }
            }
            Ok((origin, extents))
        }
    }
}
