//! Explicit native Y/Cb/Cr components. No chroma expansion or color conversion.
use super::{Error, Limits, Metadata, TagValue};
use crate::source::Snapshot;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    TiffSamplingUnits,
    JpegDecodedComponents,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Y,
    Cb,
    Cr,
}
/// Native component grid, including codec/storage padding. Row/column sampling
/// steps are relative to the logical image raster; samples are never expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plane {
    pub component: Component,
    pub shape: [usize; 2],
    pub valid_shape: [usize; 2],
    pub sampling: [usize; 2],
    pub samples: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub origin: [usize; 2],
    pub image_shape: [usize; 2],
    pub planes: [Plane; 3],
    /// Losslessly decoded source order: h*v luma samples, then Cb and Cr for
    /// each sampling unit. Present only for TIFF sampling-unit storage.
    pub sampling_units: Option<Vec<u8>>,
}
/// Worker-owned component output. Failed reads invalidate every accessor.
#[derive(Debug, Default)]
pub struct Buffer {
    shape: Option<[usize; 2]>,
    subsampling: [usize; 2],
    storage: Option<Storage>,
    chunks: Vec<Chunk>,
}
impl Buffer {
    #[must_use]
    pub fn shape(&self) -> Option<[usize; 2]> {
        self.shape
    }
    /// Horizontal, vertical source sampling factors (TIFF tag order).
    #[must_use]
    pub fn subsampling(&self) -> Option<[usize; 2]> {
        self.shape.map(|_| self.subsampling)
    }
    #[must_use]
    pub fn precision(&self) -> Option<u8> {
        self.shape.map(|_| 8)
    }
    #[must_use]
    pub fn storage(&self) -> Option<Storage> {
        self.storage
    }
    #[must_use]
    pub fn chunks(&self) -> Option<&[Chunk]> {
        self.shape.map(|_| self.chunks.as_slice())
    }
    pub(super) fn invalidate(&mut self) {
        self.shape = None;
        self.storage = None;
        self.chunks.clear();
    }
    pub(super) fn set(
        &mut self,
        shape: [usize; 2],
        subsampling: [usize; 2],
        storage: Storage,
        chunks: Vec<Chunk>,
    ) {
        self.chunks = chunks;
        self.subsampling = subsampling;
        self.storage = Some(storage);
        self.shape = Some(shape);
    }
}

pub(super) fn subsampling(page: &Metadata) -> Result<[usize; 2], Error> {
    let values = page
        .tags
        .get(&530)
        .map(|v| v.clone().into_u16_vec())
        .transpose()?
        .unwrap_or_else(|| vec![2, 2]);
    let [h, v] = values.as_slice() else {
        return Err(Error("YCbCrSubSampling requires two factors".into()));
    };
    if ![1, 2, 4].contains(h) || ![1, 2, 4].contains(v) || v > h {
        return Err(Error("unsupported YCbCr sampling factors".into()));
    }
    Ok([usize::from(*h), usize::from(*v)])
}
fn number(page: &Metadata, tag: u16, default: usize) -> Result<usize, Error> {
    let Some(value) = page.tags.get(&tag) else {
        return Ok(default);
    };
    let values = value.clone().into_u64_vec()?;
    if values.len() != 1 {
        return Err(Error(format!("YCbCr tag {tag} must be scalar")));
    }
    usize::try_from(values[0]).map_err(|_| Error("YCbCr integer exceeds platform".into()))
}
fn vector(page: &Metadata, tag: u16) -> Result<Vec<u64>, Error> {
    page.tags
        .get(&tag)
        .ok_or_else(|| Error(format!("YCbCr missing tag {tag}")))?
        .clone()
        .into_u64_vec()
        .map_err(Error::from)
}
#[derive(Debug)]
struct Geometry {
    origin: [usize; 2],
    image: [usize; 2],
    stored: [usize; 2],
    grid: [usize; 2],
    samples: usize,
}
fn geometries(page: &Metadata) -> Result<Vec<Geometry>, Error> {
    let [h, v] = subsampling(page)?;
    let tiled = page.tags.contains_key(&324);
    let width = if tiled {
        number(page, 322, 0)?
    } else {
        page.shape[1]
    };
    let rows = if tiled {
        number(page, 323, 0)?
    } else {
        number(page, 278, page.shape[0])?.min(page.shape[0])
    };
    if width == 0
        || rows == 0
        || tiled && (!width.is_multiple_of(h) || !rows.is_multiple_of(v))
        || !tiled && rows < page.shape[0] && !rows.is_multiple_of(v)
    {
        return Err(Error(
            "YCbCr chunk geometry disagrees with sampling factors".into(),
        ));
    }
    let across = page.shape[1].div_ceil(width);
    let down = page.shape[0].div_ceil(rows);
    let count = across
        .checked_mul(down)
        .ok_or_else(|| Error("YCbCr chunk count overflow".into()))?;
    let offsets = vector(page, if tiled { 324 } else { 273 })?;
    let lengths = vector(page, if tiled { 325 } else { 279 })?;
    if offsets.len() != count || lengths.len() != count {
        return Err(Error("YCbCr chunk table disagrees with image shape".into()));
    }
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|e| Error(e.to_string()))?;
    for i in 0..count {
        let origin = [(i / across) * rows, (i % across) * width];
        let image = [
            rows.min(page.shape[0] - origin[0]),
            width.min(page.shape[1] - origin[1]),
        ];
        let physical_rows = if tiled { rows } else { image[0] };
        let grid = [physical_rows.div_ceil(v), width.div_ceil(h)];
        let stored = [
            grid[0]
                .checked_mul(v)
                .ok_or_else(|| Error("YCbCr rows overflow".into()))?,
            grid[1]
                .checked_mul(h)
                .ok_or_else(|| Error("YCbCr columns overflow".into()))?,
        ];
        let samples = grid[0]
            .checked_mul(grid[1])
            .and_then(|n| n.checked_mul(h * v + 2))
            .ok_or_else(|| Error("YCbCr sample count overflow".into()))?;
        out.push(Geometry {
            origin,
            image,
            stored,
            grid,
            samples,
        });
    }
    Ok(out)
}
pub(super) fn validate(page: &Metadata, limits: Limits) -> Result<(), Error> {
    if page.photometric_interpretation != 6
        || page.bits_per_sample != 8
        || page.pixel_type != crate::PixelType::U8
        || page.channels != 3
        || page.planar_configuration != 1
        || page.fill_order != 1
        || ![1, 5].contains(&page.compression)
        || number(page, 317, 1)? != 1
        || page.tags.contains_key(&338)
    {
        return Err(Error("YCbCr sampling units require contiguous unsigned8, predictor1, fillorder1, no extras and none/LZW compression".into()));
    }
    let _ = colorimetry(page)?;
    let geometries = geometries(page)?;
    let total = geometries
        .iter()
        .try_fold(0usize, |n, g| n.checked_add(g.samples))
        .and_then(|n| n.checked_mul(2));
    if total.is_none_or(|n| n > limits.frame_bytes) {
        return Err(Error(
            "YCbCr units plus component grids exceed decoded-byte budget".into(),
        ));
    }
    let lengths = vector(
        page,
        if page.tags.contains_key(&324) {
            325
        } else {
            279
        },
    )?;
    for (g, bytes) in geometries.iter().zip(lengths) {
        if bytes > limits.intermediate_bytes as u64
            || page.compression == 5
                && usize::try_from(bytes)
                    .ok()
                    .and_then(|n| n.checked_add(g.samples))
                    .and_then(|n| n.checked_add(1))
                    .is_none_or(|n| n > limits.intermediate_bytes)
            || g.samples
                .checked_add(1)
                .is_none_or(|n| n > limits.intermediate_bytes)
            || page.compression == 1 && bytes != g.samples as u64
        {
            return Err(Error(
                "YCbCr chunk exceeds scratch limit or exact uncompressed size".into(),
            ));
        }
    }
    Ok(())
}

/// Effective colorimetry; exact original rational words remain in Metadata.tags.
#[derive(Debug, Clone, PartialEq)]
pub struct Colorimetry {
    pub coefficients: [f64; 3],
    pub reference_black_white: [f64; 6],
    pub positioning: u16,
}
pub fn colorimetry(page: &Metadata) -> Result<Colorimetry, Error> {
    fn rational<const N: usize>(
        tag: Option<&TagValue>,
        default: [f64; N],
    ) -> Result<[f64; N], Error> {
        let Some(tag) = tag else { return Ok(default) };
        let items = match tag {
            TagValue::List(values) => values.as_slice(),
            value => std::slice::from_ref(value),
        };
        if items.len() != N {
            return Err(Error("YCbCr rational tag count mismatch".into()));
        }
        let mut out = [0.0; N];
        for (dst, src) in out.iter_mut().zip(items) {
            let TagValue::Rational(n, d) = src else {
                return Err(Error(
                    "YCbCr colorimetry requires source unsigned rationals".into(),
                ));
            };
            if *d == 0 {
                return Err(Error("YCbCr rational denominator is zero".into()));
            }
            *dst = f64::from(*n) / f64::from(*d);
        }
        Ok(out)
    }
    let coefficients = rational(page.tags.get(&529), [0.299, 0.587, 0.114])?;
    let reference_black_white = rational(
        page.tags.get(&532),
        [0.0, 255.0, 128.0, 255.0, 128.0, 255.0],
    )?;
    let positioning = u16::try_from(number(page, 531, 1)?)
        .map_err(|_| Error("YCbCr positioning exceeds u16".into()))?;
    if ![1, 2].contains(&positioning)
        || coefficients.iter().any(|&n| n <= 0.0 || n >= 1.0)
        || reference_black_white
            .chunks_exact(2)
            .any(|pair| pair[1] <= pair[0])
    {
        return Err(Error(
            "invalid YCbCr coefficients/reference range/positioning".into(),
        ));
    }
    Ok(Colorimetry {
        coefficients,
        reference_black_white,
        positioning,
    })
}

pub(super) fn decode(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limit: usize,
) -> Result<Vec<Chunk>, Error> {
    snapshot.check_handle(identity).map_err(Error)?;
    let [h, v] = subsampling(page)?;
    let geometries = geometries(page)?;
    let tiled = page.tags.contains_key(&324);
    let offsets = vector(page, if tiled { 324 } else { 273 })?;
    let lengths = vector(page, if tiled { 325 } else { 279 })?;
    let mut file = File::open(&snapshot.path).map_err(io)?;
    snapshot.check_handle(&file).map_err(Error)?;
    let source_bytes = file.metadata().map_err(io)?.len();
    let mut chunks = Vec::new();
    chunks
        .try_reserve_exact(geometries.len())
        .map_err(|e| Error(e.to_string()))?;
    for ((geometry, offset), length) in geometries.into_iter().zip(offsets).zip(lengths) {
        let length = usize::try_from(length)
            .map_err(|_| Error("YCbCr encoded length exceeds platform".into()))?;
        if length > limit
            || offset
                .checked_add(length as u64)
                .is_none_or(|n| n > source_bytes)
        {
            return Err(Error("YCbCr chunk exceeds source or scratch bounds".into()));
        }
        let mut encoded = allocate(length)?;
        file.seek(SeekFrom::Start(offset)).map_err(io)?;
        file.read_exact(&mut encoded).map_err(io)?;
        let units = if page.compression == 1 {
            encoded
        } else {
            decode_lzw(&encoded, geometry.samples)?
        };
        if units.len() != geometry.samples {
            return Err(Error("YCbCr source sampling-unit count mismatch".into()));
        }
        let mut y = allocate(geometry.stored[0] * geometry.stored[1])?;
        let mut cb = allocate(geometry.grid[0] * geometry.grid[1])?;
        let mut cr = allocate(cb.len())?;
        for (index, unit) in units.chunks_exact(h * v + 2).enumerate() {
            let row = index / geometry.grid[1];
            let col = index % geometry.grid[1];
            for dy in 0..v {
                let start = (row * v + dy) * geometry.stored[1] + col * h;
                y[start..start + h].copy_from_slice(&unit[dy * h..(dy + 1) * h]);
            }
            cb[index] = unit[h * v];
            cr[index] = unit[h * v + 1];
        }
        chunks.push(Chunk {
            origin: geometry.origin,
            image_shape: geometry.image,
            planes: [
                Plane {
                    component: Component::Y,
                    shape: geometry.stored,
                    valid_shape: geometry.image,
                    sampling: [1, 1],
                    samples: y,
                },
                Plane {
                    component: Component::Cb,
                    shape: geometry.grid,
                    valid_shape: [geometry.image[0].div_ceil(v), geometry.image[1].div_ceil(h)],
                    sampling: [v, h],
                    samples: cb,
                },
                Plane {
                    component: Component::Cr,
                    shape: geometry.grid,
                    valid_shape: [geometry.image[0].div_ceil(v), geometry.image[1].div_ceil(h)],
                    sampling: [v, h],
                    samples: cr,
                },
            ],
            sampling_units: Some(units),
        });
    }
    snapshot.check_handle(&file).map_err(Error)?;
    snapshot.check_handle(identity).map_err(Error)?;
    Ok(chunks)
}
fn allocate(n: usize) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).map_err(|e| Error(e.to_string()))?;
    out.resize(n, 0);
    Ok(out)
}
fn io(error: std::io::Error) -> Error {
    Error(error.to_string())
}
fn decode_lzw(encoded: &[u8], expected: usize) -> Result<Vec<u8>, Error> {
    let mut output = allocate(
        expected
            .checked_add(1)
            .ok_or_else(|| Error("YCbCr output size overflow".into()))?,
    )?;
    let mut decoder = weezl::decode::Decoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8);
    let mut input = 0;
    let mut written = 0;
    loop {
        let result = decoder.decode_bytes(&encoded[input..], &mut output[written..]);
        input += result.consumed_in;
        written += result.consumed_out;
        match result.status.map_err(|e| Error(e.to_string()))? {
            weezl::LzwStatus::Done => break,
            weezl::LzwStatus::NoProgress => {
                return Err(Error("incomplete YCbCr LZW stream".into()));
            }
            weezl::LzwStatus::Ok => {
                if written > expected || result.consumed_in == 0 && result.consumed_out == 0 {
                    return Err(Error(
                        "YCbCr LZW output exceeds declared units or stalled".into(),
                    ));
                }
            }
        }
    }
    if written != expected || input != encoded.len() {
        return Err(Error(
            "YCbCr LZW exact output/end-marker/window mismatch".into(),
        ));
    }
    output.truncate(expected);
    Ok(output)
}
