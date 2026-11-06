//! Freeze frame-axis unlimited selections from explicit complete source extents.
use super::*;
use hdf5_sys::h5s::{H5S_seloper_t, H5Sselect_hyperslab};

pub(super) fn axis(record: &SelectionRecord) -> Result<Option<usize>, Error> {
    match record {
        SelectionRecord::Regular { count, block, .. } => {
            let axes: Vec<_> = (0..3)
                .filter(|&a| count[a] == u64::MAX || block[a] == u64::MAX)
                .collect();
            if axes.len() > 1 {
                return Err(Error(
                    "VDS supports one unlimited selection axis only".into(),
                ));
            }
            if axes.first().is_some_and(|&a| a != 0) {
                return Err(Error(
                    "non-printf unlimited VDS currently requires the frame axis on both sides"
                        .into(),
                ));
            }
            Ok(axes.first().copied())
        }
        _ => Ok(None),
    }
}
fn product(a: u64, b: u64) -> Result<usize, Error> {
    a.checked_mul(b)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| Error("VDS selected extent overflow".into()))
}
pub(super) fn virtual_end(
    record: &SelectionRecord,
    source_points: usize,
) -> Result<Option<usize>, Error> {
    if axis(record)?.is_none() {
        return Ok(None);
    }
    let SelectionRecord::Regular {
        start,
        stride,
        count,
        block,
        ..
    } = record
    else {
        unreachable!()
    };
    let spatial = product(count[1], block[1])?
        .checked_mul(product(count[2], block[2])?)
        .filter(|&n| n > 0)
        .ok_or_else(|| Error("VDS unlimited spatial selection overflow/empty".into()))?;
    if source_points == 0 || !source_points.is_multiple_of(spatial) {
        return Err(Error("VDS source cannot supply complete selected virtual frames; missing mapped samples cannot become fill".into()));
    }
    let frames = u64::try_from(source_points / spatial)
        .map_err(|_| Error("VDS selected frame count overflow".into()))?;
    let end = if block[0] == u64::MAX {
        if count[0] != 1 {
            return Err(Error("unsupported unlimited VDS block repetition".into()));
        }
        start[0].checked_add(frames)
    } else {
        if count[0] != u64::MAX || block[0] == 0 || stride[0] < block[0] {
            return Err(Error("invalid/overlapping unlimited VDS pattern".into()));
        }
        let last = frames - 1;
        (last / block[0])
            .checked_mul(stride[0])
            .and_then(|n| n.checked_add(start[0]))
            .and_then(|n| n.checked_add(last % block[0] + 1))
    };
    end.and_then(|n| usize::try_from(n).ok())
        .map(Some)
        .ok_or_else(|| Error("VDS frozen unlimited extent overflow".into()))
}
pub(super) fn freeze(
    record: &SelectionRecord,
    space: Space,
    shape: [usize; 3],
) -> Result<Dataspace, Error> {
    hdf5_backend::sync::sync(|| {
        if axis(record)?.is_none() {
            let out = if matches!(record, SelectionRecord::All { .. }) {
                Dataspace::try_new(shape)?
            } else {
                space.into_dataspace()?
            };
            finite::set_extent(&out, shape)?;
            return Ok(out);
        }
        let SelectionRecord::Regular {
            start,
            stride,
            count,
            block,
            ..
        } = record
        else {
            unreachable!()
        };
        if stride.contains(&0) || count.contains(&0) || block.contains(&0) {
            return Err(Error("invalid empty unlimited VDS pattern".into()));
        }
        for a in 1..3 {
            if count[a] == u64::MAX || block[a] == u64::MAX || count[a] > 1 && stride[a] < block[a]
            {
                return Err(Error(
                    "unsupported overlapping/unlimited VDS spatial pattern".into(),
                ));
            }
            let end = (count[a] - 1)
                .checked_mul(stride[a])
                .and_then(|n| n.checked_add(start[a]))
                .and_then(|n| n.checked_add(block[a]))
                .ok_or_else(|| Error("VDS spatial selection overflow".into()))?;
            if end > shape[a] as u64 {
                return Err(Error(
                    "VDS selected spatial coordinates exceed complete bound source".into(),
                ));
            }
        }
        let available = (shape[0] as u64)
            .checked_sub(start[0])
            .filter(|&n| n > 0)
            .ok_or_else(|| {
                Error("unlimited VDS selection has no complete bound source samples".into())
            })?;
        let mut counts = *count;
        let mut blocks = *block;
        let mut tail = None;
        if block[0] == u64::MAX {
            if count[0] != 1 {
                return Err(Error("VDS unlimited block requires one repetition".into()));
            }
            blocks[0] = available;
        } else {
            if count[0] != u64::MAX || stride[0] < block[0] {
                return Err(Error(
                    "invalid overlapping unlimited VDS frame pattern".into(),
                ));
            }
            if stride[0] == block[0] {
                counts[0] = 1;
                blocks[0] = available;
            } else {
                counts[0] = if available >= block[0] {
                    1 + (available - block[0]) / stride[0]
                } else {
                    0
                };
                let next = counts[0]
                    .checked_mul(stride[0])
                    .ok_or_else(|| Error("VDS frame selection overflow".into()))?;
                if next < available {
                    let mut s = *start;
                    let mut b = *block;
                    s[0] = start[0] + next;
                    b[0] = block[0].min(available - next);
                    tail = Some((s, b));
                }
            }
        }
        let finite_record = SelectionRecord::Regular {
            shape: shape.map(|n| n as u64),
            maximum_shape: shape.map(|n| n as u64),
            start: *start,
            stride: *stride,
            count: counts,
            block: blocks,
        };
        if counts[0] > 0 {
            finite::complexity(&finite_record)?;
        }
        let out = Dataspace::try_new(shape)?;
        if counts[0] > 0 {
            if unsafe {
                H5Sselect_hyperslab(
                    out.id(),
                    H5S_seloper_t::H5S_SELECT_SET,
                    start.as_ptr(),
                    stride.as_ptr(),
                    counts.as_ptr(),
                    blocks.as_ptr(),
                )
            } < 0
            {
                return Err(Error(
                    "cannot freeze complete unlimited VDS selection".into(),
                ));
            }
        } else if unsafe { hdf5_sys::h5s::H5Sselect_none(out.id()) } < 0 {
            return Err(Error("cannot initialize frozen VDS selection".into()));
        }
        if let Some((s, b)) = tail {
            let mut c = *count;
            c[0] = 1;
            if unsafe {
                H5Sselect_hyperslab(
                    out.id(),
                    H5S_seloper_t::H5S_SELECT_OR,
                    s.as_ptr(),
                    stride.as_ptr(),
                    c.as_ptr(),
                    b.as_ptr(),
                )
            } < 0
            {
                return Err(Error(
                    "cannot freeze final available VDS source block".into(),
                ));
            }
        }
        finite::set_extent(&out, shape)?;
        Ok(out)
    })
}
