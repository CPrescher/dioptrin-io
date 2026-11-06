//! Explicit immutable, frame-preserving VDS mosaics. Native HDF5 decodes only
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
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
    path::PathBuf,
};

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
}
/// Original selection records. u64::MAX denotes HDF5's UNLIMITED declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionRecord {
    All {
        shape: Vec<u64>,
        maximum_shape: Vec<u64>,
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
#[derive(Debug, Clone)]
pub struct Mapping {
    pub stored_filename: String,
    pub declared_dataset: String,
    pub source_file: PathBuf,
    pub resolved_dataset: String,
    pub source_shape: [usize; 3],
    pub source_selection: SelectionRecord,
    pub virtual_selection: SelectionRecord,
    pub source_origin: [usize; 3],
    pub virtual_origin: [usize; 3],
    pub mapped_shape: [usize; 3],
    pub filters: Vec<Filter>,
    pub source_pixel_type: PixelType,
    pub source_little_endian: bool,
    pub source_layout: Layout,
    pub source_fill_value: Option<PixelBuffer>,
    pub source_fill_time: FillTime,
    /// Mapped sources require full allocated storage; undefined/missing chunks
    /// cannot become module counts through a fill value.
    pub storage_allocated: bool,
}
#[derive(Debug, Clone)]
pub struct VdsMetadata {
    /// Original mapping dataspace dimensions, independent of native VDS lookup.
    pub declared_shape: [u64; 3],
    /// Frame extent derived solely from explicit concrete source selections.
    pub frozen_shape: [usize; 3],
    /// Original maximum dimensions; u64::MAX denotes HDF5 UNLIMITED.
    pub master_max_shape: [u64; 3],
    pub mappings: Vec<Mapping>,
    pub unmapped_regions: UnmappedRegions,
    pub unmapped_pixels_per_frame: usize,
}
struct SourcePart {
    dataset: Dataset,
    frozen_shape: [usize; 3],
    source_origin: [usize; 3],
    virtual_origin: [usize; 3],
    mapped_shape: [usize; 3],
}
pub(super) struct Mosaic {
    parts: Vec<SourcePart>,
    fill: Option<PixelBuffer>,
}
pub(super) struct Admission {
    pub mosaic: Mosaic,
    pub metadata: VdsMetadata,
    pub storage: Storage,
}
pub(super) struct Bindings<'a> {
    options: VdsOptions<'a>,
    used: BTreeSet<&'a str>,
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
                || binding.stored_filename.contains(['\0', '%'])
                || binding.actual_file.as_os_str().is_empty()
                || !names.insert(binding.stored_filename.as_str())
            {
                return Err(Error(
                    "invalid/duplicate VDS literal filename binding".into(),
                ));
            }
        }
        Ok(Self {
            options,
            used: BTreeSet::new(),
        })
    }
    fn source(&mut self, name: &str, sources: &mut Sources) -> Result<usize, Error> {
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
        if !self.used.is_empty() && self.used.len() != self.options.filenames.len() {
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
        for part in &self.parts {
            if part.dataset.shape() != part.frozen_shape {
                return Err(Error(
                    "VDS concrete source shape changed; reopen snapshot".into(),
                ));
            }
        }
        Ok(())
    }
    pub fn read(
        &self,
        frame: usize,
        shape: [usize; 2],
        pixels: &mut PixelBuffer,
    ) -> Result<(), Error> {
        self.check()?;
        if let Some(fill) = &self.fill {
            fill_pixels(pixels, fill)?;
        }
        let memory_type = typed::memory_type(pixels.pixel_type())?;
        for part in &self.parts {
            let [_, sy, sx] = part.source_origin;
            let [_, dy, dx] = part.virtual_origin;
            let [_, height, width] = part.mapped_shape;
            let source = part
                .dataset
                .space()?
                .select((frame, sy..sy + height, sx..sx + width))?;
            let memory = Dataspace::try_new(shape)?.select((dy..dy + height, dx..dx + width))?;
            let count = height * width;
            if source.selection_size() != count || memory.selection_size() != count {
                return Err(Error(
                    "VDS source/memory rectangle selection mismatch".into(),
                ));
            }
            hdf5_backend::sync::sync(|| {
                // SAFETY: immutable concrete dataset, exact native standard type,
                // bounded source rectangle and selected full-frame memory space.
                // Both select exactly count initialized writable scalar slots.
                if unsafe {
                    H5Dread(
                        part.dataset.id(),
                        memory_type.id(),
                        memory.id(),
                        source.id(),
                        H5P_DEFAULT,
                        pixels.bytes_mut().as_mut_ptr().cast(),
                    )
                } < 0
                {
                    Err(Error("VDS concrete module decoding failed".into()))
                } else {
                    Ok(())
                }
            })?;
        }
        self.check()
    }
}

pub(super) fn admit(
    dataset: &Dataset,
    sources: &mut Sources,
    bindings: &mut Bindings<'_>,
    limits: Limits,
    indexed_datasets: &mut usize,
    pixel_type: PixelType,
) -> Result<Admission, Error> {
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
        let mut out = Vec::new();
        out.try_reserve_exact(count)
            .map_err(|e| Error(e.to_string()))?;
        for index in 0..count {
            let filename = string(plist.id(), index, true, limits.text_bytes)?;
            let path = string(plist.id(), index, false, limits.text_bytes)?;
            if filename.contains('%') || path.contains('%') {
                return Err(Error("VDS printf source mappings are unsupported".into()));
            }
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
                selection(s.0, true)?,
                selection(v.0, false)?,
            ));
        }
        Ok(out)
    })?;
    *indexed_datasets += descriptors.len();
    let mut metadata = Vec::new();
    let mut parts = Vec::new();
    let mut declared = None;
    let mut master_maximum = None;
    let mut frame_count = None;
    for (filename, declared_dataset, source_selection, virtual_selection) in descriptors {
        let source = bindings.source(&filename, sources)?;
        let (source, path) = sources.resolve(source, &declared_dataset)?;
        let concrete = sources.file(source).dataset(&path)?;
        if concrete.layout() == Layout::Virtual {
            return Err(Error(
                "nested/cyclic VDS sources require a separate recursive mapping contract".into(),
            ));
        }
        let frozen_shape: [usize; 3] = concrete
            .shape()
            .try_into()
            .map_err(|_| Error("VDS concrete modules require rank3 stacks".into()))?;
        if frozen_shape.contains(&0)
            || frozen_shape
                .iter()
                .try_fold(pixel_type.bytes(), |n, &d| n.checked_mul(d))
                .is_none()
        {
            return Err(Error(
                "VDS concrete module shape/bytes overflow or empty".into(),
            ));
        }
        let (source_pixel_type, source_little_endian) = typed::standard_type(&concrete.dtype()?)?;
        let same = hdf5_backend::sync::sync(|| {
            // SAFETY: two live datatypes, equality only; no implicit conversion.
            unsafe { H5Tequal(dataset.dtype()?.id(), concrete.dtype()?.id()) }
                .gt(&0)
                .then_some(())
                .ok_or_else(|| {
                    Error("VDS source dtype must exactly match the original virtual dtype".into())
                })
        });
        same?;
        let storage = typed::admit_storage(&concrete, pixel_type, limits)?;
        if !storage.storage_allocated {
            return Err(Error(
                "VDS mapped module storage is not fully allocated; no missing-module/chunk fill"
                    .into(),
            ));
        }
        let (vshape, maximum) = selection_shape(&virtual_selection)?;
        if maximum[1..] != vshape[1..]
            || vshape[1] == 0
            || vshape[2] == 0
            || vshape[1..].contains(&u64::MAX)
            || declared.is_some_and(|d: [u64; 3]| d != vshape)
            || master_maximum.is_some_and(|d: [u64; 3]| d != maximum)
        {
            return Err(Error(
                "VDS spatial extents must be fixed and agree in every mapping".into(),
            ));
        }
        declared = Some(vshape);
        master_maximum = Some(maximum);
        let (source_origin, source_extent) = rectangle(&source_selection, Some(frozen_shape))?;
        let (virtual_origin, virtual_extent) = rectangle(&virtual_selection, None)?;
        if source_origin[0] != 0 || virtual_origin[0] != 0 {
            return Err(Error(
                "VDS mappings must preserve frame numbers starting at zero".into(),
            ));
        }
        if vshape[0] == 0 && (maximum[0] != u64::MAX || virtual_extent[0].is_some()) {
            return Err(Error(
                "zero-extent VDS requires an original unlimited-frame mapping declaration".into(),
            ));
        }
        let frames = match (source_extent[0], virtual_extent[0]) {
            (None, None) => frozen_shape[0],
            (Some(s), Some(v)) if s == v => s,
            (None, Some(v)) | (Some(v), None) if v == frozen_shape[0] => v,
            _ => return Err(Error("VDS source/virtual frame extents disagree".into())),
        };
        if frames == 0
            || frames > frozen_shape[0]
            || frames > limits.frames
            || frame_count.is_some_and(|n| n != frames)
        {
            return Err(Error(
                "VDS concrete source frame extents disagree or exceed limits".into(),
            ));
        }
        frame_count = Some(frames);
        let mut mapped_shape = [frames, 0, 0];
        for axis in 1..3 {
            let length = source_extent[axis].filter(|n| *n > 0).ok_or_else(|| {
                Error("VDS source spatial selections must be finite and nonempty".into())
            })?;
            if virtual_extent[axis] != Some(length)
                || source_origin[axis]
                    .checked_add(length)
                    .is_none_or(|n| n > frozen_shape[axis])
                || virtual_origin[axis]
                    .checked_add(length)
                    .is_none_or(|n| n as u64 > vshape[axis])
            {
                return Err(Error(
                    "VDS mapping shape/selection exceeds concrete or virtual bounds".into(),
                ));
            }
            mapped_shape[axis] = length;
        }
        sources.charge(
            filename
                .len()
                .saturating_mul(2)
                .saturating_add(declared_dataset.len().saturating_mul(2))
                .saturating_add(path.len().saturating_mul(2))
                .saturating_add(sources.path(source).as_os_str().len().saturating_mul(2))
                .saturating_add(storage.filters.len().saturating_mul(256))
                .saturating_add(2304),
        )?;
        metadata.push(Mapping {
            stored_filename: filename,
            declared_dataset,
            source_file: sources.path(source).to_path_buf(),
            resolved_dataset: path,
            source_shape: frozen_shape,
            source_selection,
            virtual_selection,
            source_origin,
            virtual_origin,
            mapped_shape,
            filters: storage.filters,
            source_pixel_type,
            source_little_endian,
            source_layout: concrete.layout(),
            source_fill_value: storage.fill_value,
            source_fill_time: storage.fill_time,
            storage_allocated: true,
        });
        parts.push(SourcePart {
            dataset: concrete,
            frozen_shape,
            source_origin,
            virtual_origin,
            mapped_shape,
        });
    }
    let declared_shape = declared.ok_or_else(|| Error("empty VDS mappings".into()))?;
    let frames = frame_count.ok_or_else(|| Error("empty VDS frame extent".into()))?;
    let master_max_shape =
        master_maximum.ok_or_else(|| Error("empty VDS maximum extent".into()))?;
    if declared_shape[0] != 0 && frames as u64 != declared_shape[0] {
        return Err(Error(
            "VDS explicit source frame extent differs from fixed master extent".into(),
        ));
    }
    let height = usize::try_from(declared_shape[1])
        .map_err(|_| Error("VDS height exceeds platform".into()))?;
    let width = usize::try_from(declared_shape[2])
        .map_err(|_| Error("VDS width exceeds platform".into()))?;
    let pixels = height
        .checked_mul(width)
        .filter(|n| *n > 0)
        .ok_or_else(|| Error("VDS frame shape overflow".into()))?;
    if pixels
        .checked_mul(pixel_type.bytes())
        .is_none_or(|n| n > limits.frame_bytes)
    {
        return Err(Error("VDS output frame exceeds decoded-byte limit".into()));
    }
    frames
        .checked_mul(pixels)
        .and_then(|n| n.checked_mul(pixel_type.bytes()))
        .ok_or_else(|| Error("VDS frozen sequence byte overflow".into()))?;
    let covered = coverage(&parts)?;
    let unmapped = pixels
        .checked_sub(covered)
        .ok_or_else(|| Error("VDS covered area exceeds raster".into()))?;
    let fill = typed::fill(dataset, pixel_type)?;
    let fill_time = plist.get_fill_time()?;
    if unmapped > 0
        && (bindings.options.unmapped_regions != UnmappedRegions::DeclaredFill
            || fill.is_none()
            || fill_time == FillTime::Never)
    {
        return Err(Error(
            "VDS unmapped regions require explicit defined DeclaredFill policy".into(),
        ));
    }
    let mosaic = Mosaic {
        parts,
        fill: if unmapped > 0 { fill.clone() } else { None },
    };
    mosaic.check()?;
    Ok(Admission {
        mosaic,
        metadata: VdsMetadata {
            declared_shape,
            frozen_shape: [frames, height, width],
            master_max_shape,
            mappings: metadata,
            unmapped_regions: bindings.options.unmapped_regions,
            unmapped_pixels_per_frame: unmapped,
        },
        storage: Storage {
            filters: Vec::new(),
            fill_value: fill,
            storage_allocated: false,
            fill_time,
        },
    })
}

fn coverage(parts: &[SourcePart]) -> Result<usize, Error> {
    let mut order: Vec<_> = (0..parts.len()).collect();
    order.sort_unstable_by_key(|&i| parts[i].virtual_origin[1]);
    let mut ends = BinaryHeap::new();
    let mut active = BTreeMap::new();
    let mut covered = 0usize;
    for i in order {
        let p = &parts[i];
        let y = p.virtual_origin[1];
        let x = p.virtual_origin[2];
        let stop = x + p.mapped_shape[2];
        while let Some(&Reverse((end_y, start_x, _))) = ends.peek() {
            if end_y > y {
                break;
            }
            ends.pop();
            active.remove(&start_x);
        }
        if active
            .range(..=x)
            .next_back()
            .is_some_and(|(_, &end)| end > x)
            || active
                .range(x..)
                .next()
                .is_some_and(|(&start, _)| start < stop)
        {
            return Err(Error(
                "overlapping VDS source rectangles are unsupported".into(),
            ));
        }
        active.insert(x, stop);
        ends.push(Reverse((y + p.mapped_shape[1], x, i)));
        covered = covered
            .checked_add(p.mapped_shape[1] * p.mapped_shape[2])
            .ok_or_else(|| Error("VDS coverage area overflow".into()))?;
    }
    Ok(covered)
}
fn fill_pixels(target: &mut PixelBuffer, fill: &PixelBuffer) -> Result<(), Error> {
    macro_rules! values{($($v:ident);*)=>{match(target,fill){$((PixelBuffer::$v(out),PixelBuffer::$v(value))if value.len()==1=>out.fill(value[0]),)*_=>return Err(Error("VDS typed fill does not match native output".into()))}};}
    values!(Bool;F16;U8;I8;U16;I16;U32;I32;U64;I64;F32;F64;C32;C64;CI16);
    Ok(())
}
struct Space(hid_t);
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
fn string(plist: hid_t, index: usize, filename: bool, limit: usize) -> Result<String, Error> {
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
fn selection(id: hid_t, source: bool) -> Result<SelectionRecord, Error> {
    // SAFETY: live owned dataspace ID, fixed-size outputs only after rank check.
    unsafe {
        let rank = H5Sget_simple_extent_ndims(id);
        if rank != 3 && !(source && rank == 0) {
            return Err(Error(
                "VDS selections must be rank3 regular rectangles".into(),
            ));
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
            _ => Err(Error(
                "unsupported VDS point/irregular/empty selection".into(),
            )),
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
        } => Ok((*shape, *maximum_shape)),
    }
}
fn rectangle(
    selection: &SelectionRecord,
    source: Option<[usize; 3]>,
) -> Result<([usize; 3], [Option<usize>; 3]), Error> {
    match selection {
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
