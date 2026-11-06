//! Bounded finite selections using HDF5's own VDS selection-order projection.
use super::*;
use hdf5_sys::h5s::{
    H5S_seloper_t, H5Scombine_select, H5Sget_select_bounds, H5Sget_select_hyper_blocklist,
    H5Sget_select_hyper_nblocks, H5Sget_select_npoints, H5Sselect_adjust, H5Sselect_none,
    H5Sselect_project_intersection, H5Sselect_valid, H5Sset_extent_simple,
};

pub(super) const MAX_BLOCKS: usize = 16_384;
pub(super) const MAX_OPERATIONS: usize = 1_000_000;

pub(super) struct Selection {
    pub(super) source: Dataspace,
    pub(super) virtual_: Dataspace,
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
pub(super) fn complexity(record: &SelectionRecord) -> Result<usize, Error> {
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
pub(super) fn points(space: &Dataspace) -> Result<usize, Error> {
    usize::try_from(unsafe { H5Sget_select_npoints(space.id()) })
        .map_err(|_| Error("invalid/nonfinite VDS selection size".into()))
}
pub(super) fn combination(
    a: &Dataspace,
    b: &Dataspace,
    op: H5S_seloper_t,
) -> Result<Dataspace, Error> {
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
pub(super) fn set_extent(space: &Dataspace, shape: [usize; 3]) -> Result<(), Error> {
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
pub(super) fn bounds(space: &Dataspace) -> Result<([usize; 3], [usize; 3]), Error> {
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
    pub(super) fn intersection(&self, frame: usize) -> Result<Dataspace, Error> {
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
