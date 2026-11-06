//! Bounded finite selections using HDF5's own VDS selection-order projection.
use super::*;
use hdf5_sys::h5s::{
    H5S_seloper_t, H5Scombine_select, H5Sget_select_bounds, H5Sget_select_hyper_blocklist,
    H5Sget_select_hyper_nblocks, H5Sget_select_npoints, H5Sselect_adjust, H5Sselect_none,
    H5Sselect_project_intersection, H5Sselect_valid, H5Sset_extent_simple,
};

const MAX_BLOCKS: usize = 16_384;
const MAX_OPERATIONS: usize = 1_000_000;

pub(super) struct Selection {
    source: Dataspace,
    virtual_: Dataspace,
}
pub(super) fn record_is_finite(record: &SelectionRecord) -> bool {
    match record {
        SelectionRecord::All { shape, .. } => !shape.contains(&u64::MAX),
        SelectionRecord::Regular { count, block, .. } => {
            !count.contains(&u64::MAX) && !block.contains(&u64::MAX)
        }
        SelectionRecord::Irregular { .. } => true,
    }
}
pub(super) fn blocks(
    id: hid_t,
    sources: &mut Sources,
    limits: Limits,
) -> Result<Vec<[[u64; 3]; 2]>, Error> {
    // SAFETY: live rank3 finite hyperslab dataspace, native count only.
    let count = usize::try_from(unsafe { H5Sget_select_hyper_nblocks(id) })
        .ok()
        .filter(|&n| n > 0 && n <= MAX_BLOCKS && n <= limits.group_members)
        .ok_or_else(|| Error("VDS irregular selection exceeds bounded block count".into()))?;
    sources.charge(
        count
            .checked_mul(96)
            .ok_or_else(|| Error("VDS block metadata overflow".into()))?,
    )?;
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|e| Error(e.to_string()))?;
    out.resize(count, [[0; 3]; 2]);
    // SAFETY: count owned entries each contain two contiguous rank3 u64 bounds.
    if unsafe { H5Sget_select_hyper_blocklist(id, 0, count as u64, out.as_mut_ptr().cast()) } < 0 {
        return Err(Error(
            "cannot inspect complete irregular VDS block list".into(),
        ));
    }
    Ok(out)
}
fn complexity(record: &SelectionRecord) -> Result<usize, Error> {
    let count = match record {
        SelectionRecord::All { .. } => 1,
        SelectionRecord::Irregular { blocks, .. } => blocks.len(),
        SelectionRecord::Regular {
            count,
            stride,
            block,
            ..
        } => (0..3)
            .try_fold(1usize, |n, axis| {
                let repeats = if stride[axis] == block[axis] {
                    1
                } else {
                    usize::try_from(count[axis]).ok()?
                };
                n.checked_mul(repeats)
            })
            .ok_or_else(|| Error("VDS regular selection complexity overflow".into()))?,
    };
    if count == 0 || count > MAX_BLOCKS {
        return Err(Error(
            "VDS finite selection exceeds bounded block complexity".into(),
        ));
    }
    Ok(count)
}
fn points(space: &Dataspace) -> Result<usize, Error> {
    usize::try_from(unsafe { H5Sget_select_npoints(space.id()) })
        .map_err(|_| Error("invalid/nonfinite VDS selection size".into()))
}
fn combination(a: &Dataspace, b: &Dataspace, op: H5S_seloper_t) -> Result<Dataspace, Error> {
    let ak = unsafe { H5Sget_select_type(a.id()) };
    let bk = unsafe { H5Sget_select_type(b.id()) };
    use H5S_sel_type::{H5S_SEL_ALL, H5S_SEL_NONE};
    let selected = match op {
        H5S_seloper_t::H5S_SELECT_AND if ak == H5S_SEL_NONE || bk == H5S_SEL_NONE => {
            let out = Space(unsafe { hdf5_sys::h5s::H5Scopy(a.id()) }).into_dataspace()?;
            if unsafe { H5Sselect_none(out.id()) } < 0 {
                return Err(Error("cannot initialize empty VDS intersection".into()));
            }
            return Ok(out);
        }
        H5S_seloper_t::H5S_SELECT_AND if ak == H5S_SEL_ALL => Some(b),
        H5S_seloper_t::H5S_SELECT_AND if bk == H5S_SEL_ALL => Some(a),
        H5S_seloper_t::H5S_SELECT_OR if ak == H5S_SEL_NONE || bk == H5S_SEL_ALL => Some(b),
        H5S_seloper_t::H5S_SELECT_OR if bk == H5S_SEL_NONE || ak == H5S_SEL_ALL => Some(a),
        _ => None,
    };
    if let Some(selected) = selected {
        return Space(unsafe { hdf5_sys::h5s::H5Scopy(selected.id()) }).into_dataspace();
    }
    // H5Scombine_select accepts hyperslabs only; ALL/NONE are handled above
    // with exact set identities and native copies, not synthetic sample data.
    Space(unsafe { H5Scombine_select(a.id(), op, b.id()) }).into_dataspace()
}
fn set_extent(space: &Dataspace, shape: [usize; 3]) -> Result<(), Error> {
    let shape = shape.map(|n| n as u64);
    // SAFETY: fixed rank3 extent, complete initialized shape array. Selection
    // bounds and counts are explicitly checked before any H5Dread.
    if unsafe { H5Sset_extent_simple(space.id(), 3, shape.as_ptr(), shape.as_ptr()) } < 0 {
        return Err(Error(
            "cannot bind VDS selection to frozen source extent".into(),
        ));
    }
    if unsafe { H5Sselect_valid(space.id()) } <= 0 {
        return Err(Error(
            "VDS selection exceeds complete frozen source/virtual extent".into(),
        ));
    }
    Ok(())
}
fn bounds(space: &Dataspace) -> Result<([usize; 3], [usize; 3]), Error> {
    let (mut start, mut stop) = ([0u64; 3], [0u64; 3]);
    if unsafe { H5Sget_select_bounds(space.id(), start.as_mut_ptr(), stop.as_mut_ptr()) } < 0 {
        return Err(Error("cannot inspect finite VDS selection bounds".into()));
    }
    let mut origin = [0; 3];
    let mut shape = [0; 3];
    for axis in 0..3 {
        origin[axis] = usize::try_from(start[axis])
            .map_err(|_| Error("VDS offset exceeds platform".into()))?;
        shape[axis] = stop[axis]
            .checked_sub(start[axis])
            .and_then(|n| n.checked_add(1))
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| Error("invalid VDS selected bounds".into()))?;
    }
    Ok((origin, shape))
}
impl Selection {
    fn intersection(&self, frame: usize) -> Result<Dataspace, Error> {
        let frame_plane = Dataspace::try_new(self.virtual_.shape())?.select((frame, .., ..))?;
        combination(&self.virtual_, &frame_plane, H5S_seloper_t::H5S_SELECT_AND)
    }
    pub fn read(
        &self,
        dataset: &Dataset,
        frame: usize,
        shape: [usize; 2],
        memory_type: &hdf5_backend::Datatype,
        pixels: &mut PixelBuffer,
    ) -> Result<(), Error> {
        hdf5_backend::sync::sync(|| {
            let memory = self.intersection(frame)?;
            let count = points(&memory)?;
            if count == 0 {
                return Ok(());
            }
            let frame_plane = Dataspace::try_new(self.virtual_.shape())?.select((frame, .., ..))?;
            // Native projection follows HDF5 iteration order even when source
            // and destination block shapes/counts differ or cross frame axes.
            let source = Space(unsafe {
                H5Sselect_project_intersection(
                    self.virtual_.id(),
                    self.source.id(),
                    frame_plane.id(),
                )
            })
            .into_dataspace()?;
            if points(&source)? != count {
                return Err(Error(
                    "VDS projected source/output point counts differ".into(),
                ));
            }
            let offset = [
                i64::try_from(frame)
                    .map_err(|_| Error("VDS frame offset exceeds native signed range".into()))?,
                0,
                0,
            ];
            if unsafe { H5Sselect_adjust(memory.id(), offset.as_ptr()) } < 0 {
                return Err(Error(
                    "cannot translate VDS output selection to one frame".into(),
                ));
            }
            set_extent(&memory, [1, shape[0], shape[1]])?;
            if count > shape[0] * shape[1] {
                return Err(Error("VDS projected selection exceeds output frame".into()));
            }
            // SAFETY: admitted immutable file dtype and equal valid source and
            // one-frame memory selections into exactly sized initialized output.
            if unsafe {
                H5Dread(
                    dataset.id(),
                    memory_type.id(),
                    memory.id(),
                    source.id(),
                    H5P_DEFAULT,
                    pixels.bytes_mut().as_mut_ptr().cast(),
                )
            } < 0
            {
                return Err(Error(
                    "finite VDS concrete selection decoding failed".into(),
                ));
            }
            Ok(())
        })
    }
}

pub(super) fn admit(
    dataset: &Dataset,
    sources: &mut Sources,
    bindings: &mut Bindings<'_>,
    limits: Limits,
    pixel_type: PixelType,
    descriptors: Vec<Descriptor>,
) -> Result<Admission, Error> {
    let mut declared = None;
    let mut maximum = None;
    let mut parts = Vec::new();
    let mut metadata = Vec::new();
    let mut operations = 0usize;
    for (
        filename,
        declared_dataset,
        source_selection,
        virtual_selection,
        source_space,
        virtual_space,
    ) in descriptors
    {
        let (vshape, vmax) = selection_shape(&virtual_selection)?;
        if vshape.contains(&0)
            || vshape.contains(&u64::MAX)
            || vmax[1..] != vshape[1..]
            || declared.is_some_and(|d| d != vshape)
            || maximum.is_some_and(|d| d != vmax)
        {
            return Err(Error(
                "finite VDS mapping extents must agree and have fixed nonempty spatial axes".into(),
            ));
        }
        declared = Some(vshape);
        maximum = Some(vmax);
        let mut frozen = [0usize; 3];
        for axis in 0..3 {
            frozen[axis] = usize::try_from(vshape[axis])
                .map_err(|_| Error("VDS virtual shape exceeds platform".into()))?;
        }
        let frame_pixels = frozen[1]
            .checked_mul(frozen[2])
            .filter(|&n| n > 0)
            .ok_or_else(|| Error("VDS frame shape overflow".into()))?;
        if frozen[0] > limits.frames
            || frame_pixels
                .checked_mul(pixel_type.bytes())
                .is_none_or(|n| n > limits.frame_bytes)
        {
            return Err(Error("finite VDS output frame/count exceeds limit".into()));
        }
        frozen
            .iter()
            .try_fold(pixel_type.bytes(), |n, &d| n.checked_mul(d))
            .ok_or_else(|| Error("finite VDS output byte overflow".into()))?;
        let weight = complexity(&source_selection)?
            .checked_add(complexity(&virtual_selection)?)
            .ok_or_else(|| Error("VDS selection operation overflow".into()))?;
        operations = operations
            .checked_add(
                frozen[0]
                    .checked_mul(weight)
                    .ok_or_else(|| Error("VDS frame selection operation overflow".into()))?,
            )
            .filter(|&n| n <= MAX_OPERATIONS)
            .ok_or_else(|| Error("finite VDS frame/selection operation limit exceeded".into()))?;
        let source = bindings.source(&filename, sources)?;
        let (source, path) = sources.resolve(source, &declared_dataset)?;
        let concrete = crate::hdf5::sources::hard_dataset(sources.file(source), &path)?;
        if concrete.layout() == Layout::Virtual {
            return Err(Error(
                "nested/cyclic VDS sources require a separate recursive mapping contract".into(),
            ));
        }
        let frozen_shape: [usize; 3] = concrete
            .shape()
            .try_into()
            .map_err(|_| Error("VDS concrete sources require rank3 stacks".into()))?;
        if frozen_shape.contains(&0)
            || frozen_shape
                .iter()
                .try_fold(pixel_type.bytes(), |n, &d| n.checked_mul(d))
                .is_none()
        {
            return Err(Error(
                "VDS concrete source shape/bytes overflow or empty".into(),
            ));
        }
        let (source_pixel_type, source_little_endian) = typed::standard_type(&concrete.dtype()?)?;
        if hdf5_backend::sync::sync(|| {
            unsafe { H5Tequal(dataset.dtype()?.id(), concrete.dtype()?.id()) }
                .gt(&0)
                .then_some(())
                .ok_or_else(|| {
                    Error("VDS concrete dtype must exactly match original virtual dtype".into())
                })
        })
        .is_err()
        {
            return Err(Error(
                "VDS concrete dtype must exactly match original virtual dtype".into(),
            ));
        }
        let storage = typed::admit_storage(&concrete, pixel_type, limits)?;
        if !storage.storage_allocated {
            return Err(Error(
                "VDS source storage must be fully allocated; missing chunks never become fill"
                    .into(),
            ));
        }
        let source_space = if matches!(source_selection, SelectionRecord::All { .. }) {
            concrete.space()?
        } else {
            source_space.into_dataspace()?
        };
        let virtual_space = virtual_space.into_dataspace()?;
        let (selected, (source_origin, source_bounds), (virtual_origin, virtual_bounds)) =
            hdf5_backend::sync::sync(|| {
                set_extent(&source_space, frozen_shape)?;
                set_extent(&virtual_space, frozen)?;
                let selected = points(&source_space)?;
                if selected == 0 || selected != points(&virtual_space)? {
                    return Err(Error(
                        "finite VDS source/virtual selection element counts differ or empty".into(),
                    ));
                }
                Ok((selected, bounds(&source_space)?, bounds(&virtual_space)?))
            })?;
        sources.charge(
            filename
                .len()
                .saturating_mul(2)
                .saturating_add(declared_dataset.len().saturating_mul(2))
                .saturating_add(path.len().saturating_mul(3))
                .saturating_add(sources.path(source).as_os_str().len().saturating_mul(2))
                .saturating_add(storage.filters.len().saturating_mul(256))
                .saturating_add(3072),
        )?;
        let layout = if source_bounds == virtual_bounds
            && rectangle(&source_selection, Some(frozen_shape)).is_ok()
            && rectangle(&virtual_selection, None).is_ok()
        {
            SelectionLayout::ContiguousRectangle
        } else {
            SelectionLayout::FiniteSelection
        };
        metadata.push(Mapping {
            stored_filename: filename,
            declared_dataset,
            source_file: sources.path(source).to_path_buf(),
            resolved_dataset: path.clone(),
            source_shape: frozen_shape,
            source_selection,
            virtual_selection,
            source_origin,
            virtual_origin,
            mapped_shape: virtual_bounds,
            selection_layout: layout,
            source_bounding_shape: source_bounds,
            virtual_bounding_shape: virtual_bounds,
            selected_elements: selected as u64,
            filters: storage.filters,
            source_pixel_type,
            source_little_endian,
            source_layout: concrete.layout(),
            source_fill_value: storage.fill_value,
            source_fill_time: storage.fill_time,
            storage_allocated: true,
        });
        parts.push(SourcePart {
            file: sources.file(source).clone(),
            path,
            dataset: concrete,
            finite: Some(Selection {
                source: source_space,
                virtual_: virtual_space,
            }),
            frozen_shape,
            source_origin,
            virtual_origin,
            mapped_shape: virtual_bounds,
        });
    }
    let declared_shape = declared.ok_or_else(|| Error("empty finite VDS mappings".into()))?;
    let master_max_shape = maximum.expect("nonempty checked");
    let frozen_shape = declared_shape.map(|n| n as usize);
    let [frames, height, width] = frozen_shape;
    sources.charge(
        frames
            .checked_mul(std::mem::size_of::<usize>())
            .ok_or_else(|| Error("VDS frame coverage metadata overflow".into()))?,
    )?;
    let mut unmapped = Vec::new();
    unmapped
        .try_reserve_exact(frames)
        .map_err(|e| Error(e.to_string()))?;
    hdf5_backend::sync::sync(|| {
        for frame in 0..frames {
            let coverage = Dataspace::try_new(frozen_shape)?;
            if unsafe { H5Sselect_none(coverage.id()) } < 0 {
                return Err(Error("cannot initialize VDS coverage selection".into()));
            }
            let mut coverage = coverage;
            for part in &parts {
                let selected = part
                    .finite
                    .as_ref()
                    .expect("finite path")
                    .intersection(frame)?;
                if points(&combination(
                    &coverage,
                    &selected,
                    H5S_seloper_t::H5S_SELECT_AND,
                )?)? > 0
                {
                    return Err(Error(
                        "overlapping finite VDS mappings are unsupported".into(),
                    ));
                }
                coverage = combination(&coverage, &selected, H5S_seloper_t::H5S_SELECT_OR)?;
            }
            unmapped.push(
                (height * width)
                    .checked_sub(points(&coverage)?)
                    .ok_or_else(|| Error("VDS coverage exceeds output raster".into()))?,
            );
        }
        Ok(())
    })?;
    let any_unmapped = unmapped.iter().any(|&n| n > 0);
    let fill = typed::fill(dataset, pixel_type)?;
    let fill_time = dataset.dcpl()?.get_fill_time()?;
    if any_unmapped
        && (bindings.options.unmapped_regions != UnmappedRegions::DeclaredFill
            || fill.is_none()
            || fill_time == FillTime::Never)
    {
        return Err(Error(
            "VDS unmapped regions require explicit defined DeclaredFill policy".into(),
        ));
    }
    let constant = unmapped
        .first()
        .copied()
        .filter(|n| unmapped.iter().all(|v| v == n));
    let mosaic = Mosaic {
        master: dataset.clone(),
        master_file: dataset.file()?,
        master_path: dataset.name(),
        master_plist: dataset.dcpl()?,
        finite_extent: Some(dataset.shape()),
        parts,
        fill: if any_unmapped { fill.clone() } else { None },
    };
    mosaic.check()?;
    Ok(Admission {
        mosaic,
        metadata: VdsMetadata {
            declared_shape,
            frozen_shape,
            master_max_shape,
            mappings: metadata,
            unmapped_regions: bindings.options.unmapped_regions,
            unmapped_pixels_per_frame: constant,
            unmapped_pixels_by_frame: unmapped,
        },
        storage: Storage {
            filters: Vec::new(),
            fill_value: fill,
            storage_allocated: false,
            fill_time,
        },
    })
}
