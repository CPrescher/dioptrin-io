//! Source-rank geometry and explicitly shaped EDF array output.
use super::{Error, FrameBuffer, Limits};
use crate::{PixelBuffer, PixelType};
use std::collections::BTreeMap;

pub const MAX_RANK: usize = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameLayout {
    Native2D,
    /// Existing frame API compatibility, without discarding the source shape.
    SingletonLeadingAxes,
    ArrayOrPlaneRequired,
}
/// One complete source image record, not a stack of invented EDF records.
/// The buffer owns original typed samples in source C-order axes. Errors hide
/// both shape and samples; decoder workspace is reused under existing limits.
#[derive(Debug, Default)]
pub struct ArrayBuffer {
    pub(super) data: FrameBuffer,
    pub(super) shape: Vec<usize>,
    pub(super) valid: bool,
}
impl ArrayBuffer {
    #[must_use]
    pub fn shape(&self) -> Option<&[usize]> {
        self.valid.then_some(self.shape.as_slice())
    }
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        self.valid.then_some(&self.data.pixels)
    }
    pub(super) fn invalidate(&mut self) {
        self.valid = false;
        self.data.shape = None;
    }
}
pub(super) struct Geometry {
    pub shape: Vec<usize>,
    pub implicit: Vec<usize>,
    pub plane: [usize; 2],
    pub planes: usize,
    pub samples: usize,
    pub layout: FrameLayout,
}
impl Geometry {
    pub fn metadata_bytes(&self) -> usize {
        self.shape
            .len()
            .saturating_add(self.implicit.len())
            .saturating_mul(std::mem::size_of::<usize>())
            .saturating_add(128)
    }
}
pub(super) enum GeometryError {
    Invalid(Error),
    Bound(Error),
}
impl GeometryError {
    pub fn into_error(self) -> Error {
        match self {
            Self::Invalid(error) | Self::Bound(error) => error,
        }
    }
}
pub(super) fn geometry(
    fields: &BTreeMap<String, String>,
    ty: PixelType,
    limits: Limits,
) -> Result<Geometry, GeometryError> {
    let invalid = |message: &str| GeometryError::Invalid(Error(message.into()));
    let bound = |message: &str| GeometryError::Bound(Error(message.into()));
    let mut rank = 0;
    for key in fields.keys() {
        if let Some(suffix) = key.strip_prefix("DIM_")
            && !suffix.is_empty()
            && suffix.bytes().all(|b| b.is_ascii_digit())
        {
            let index = suffix
                .parse::<usize>()
                .map_err(|_| bound("EDF source rank exceeds platform bound"))?;
            if index > MAX_RANK {
                return Err(bound("EDF source rank exceeds 32-axis limit"));
            }
            rank = rank.max(index);
        }
    }
    let mut shape = Vec::new();
    let mut implicit = Vec::new();
    for axis in (1..=rank).rev() {
        let key = format!("DIM_{axis}");
        let size = match fields.get(&key) {
            Some(value) => value
                .parse::<usize>()
                .map_err(|_| invalid("invalid EDF dimension integer"))?,
            None if axis == 1 => 0,
            None => {
                implicit.push(axis);
                1
            }
        };
        if size == 0 {
            return Err(invalid(
                "EDF empty/missing-fast-axis arrays are unsupported",
            ));
        }
        shape.push(size);
    }
    let samples = shape
        .iter()
        .try_fold(1usize, |n, &axis| n.checked_mul(axis))
        .ok_or_else(|| bound("EDF source shape product overflows platform"))?;
    samples
        .checked_mul(ty.bytes())
        .filter(|&n| n <= limits.frame_bytes)
        .ok_or_else(|| bound("EDF complete source array exceeds decoded-byte limit"))?;
    let plane = match shape.as_slice() {
        [] => [1, 1],
        [width] => [1, *width],
        dims => [dims[dims.len() - 2], dims[dims.len() - 1]],
    };
    limits.pixels(plane, ty).map_err(GeometryError::Bound)?;
    let planes = shape[..shape.len().saturating_sub(2)].iter().product();
    implicit.sort_unstable();
    let layout = if rank == 2 {
        FrameLayout::Native2D
    } else if rank > 2 && planes == 1 {
        FrameLayout::SingletonLeadingAxes
    } else {
        FrameLayout::ArrayOrPlaneRequired
    };
    Ok(Geometry {
        shape,
        implicit,
        plane,
        planes,
        samples,
        layout,
    })
}
pub(super) fn select_plane(
    pixels: &mut PixelBuffer,
    start: usize,
    end: usize,
) -> Result<(), Error> {
    if start > end || end > pixels.len() {
        return Err(Error("EDF plane sample window exceeds source array".into()));
    }
    macro_rules! select{($($kind:ident);*)=>{match pixels{$(PixelBuffer::$kind(values)=>{values.copy_within(start..end,0);values.truncate(end-start);}),*,_=>return Err(Error("unsupported EDF plane scalar representation".into()))}};}
    select!(U8;U16;U32;U64;I8;I16;I32;I64;F32;F64);
    Ok(())
}
