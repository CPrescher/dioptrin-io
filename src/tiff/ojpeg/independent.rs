//! Explicit caller-selected DC-reset tile interpretation. It uses each declared
//! tile's own geometry, never a guessed global JPEG stack or a failed retry.
use super::{
    Fragment, FrameSpec, Partition, Prepared, Reconstruction, SourceMetadata, SourceWindow, io,
    scalar, table_header, vector,
};
use crate::{
    source::Snapshot,
    tiff::{Error, Limits, Metadata, OjpegTiles, jpeg_components::decode_jpeg, ycbcr::Chunk},
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
fn geometry(page: &Metadata) -> Result<([usize; 2], usize), Error> {
    if !page.tags.contains_key(&324)
        || page.tags.contains_key(&513)
        || page.tags.contains_key(&514)
        || page.planar_configuration != 1
        || scalar(page, 515, 0)? != 0
    {
        return Err(Error("independent-reset OJPEG policy requires table-only contiguous tiles without restart declarations".into()));
    }
    let size = [
        usize::try_from(scalar(page, 323, 0)?)
            .map_err(|_| Error("OJPEG tile height overflow".into()))?,
        usize::try_from(scalar(page, 322, 0)?)
            .map_err(|_| Error("OJPEG tile width overflow".into()))?,
    ];
    let factors = super::super::ycbcr::subsampling(page)?;
    if size.iter().any(|&n| n == 0 || n > 65535)
        || !size[0].is_multiple_of(factors[1] * 8)
        || !size[1].is_multiple_of(factors[0] * 8)
    {
        return Err(Error(
            "independent OJPEG tiles require declared MCU-aligned JPEG dimensions".into(),
        ));
    }
    let across = page.shape[1].div_ceil(size[1]);
    Ok((size, across))
}
fn stream(
    file: &mut File,
    fragment: &Fragment,
    header: &[u8],
    limits: Limits,
) -> Result<Vec<u8>, Error> {
    let size = header
        .len()
        .checked_add(fragment.source_bytes)
        .and_then(|n| n.checked_add(2))
        .filter(|&n| {
            n.checked_mul(2)
                .is_some_and(|n| n <= limits.intermediate_bytes)
        })
        .ok_or_else(|| {
            Error("independent OJPEG encoded staging exceeds intermediate budget".into())
        })?;
    let mut data = Vec::new();
    data.try_reserve_exact(size)
        .map_err(|e| Error(e.to_string()))?;
    data.extend_from_slice(header);
    let start = data.len();
    data.resize(start + fragment.source_bytes, 0);
    file.seek(SeekFrom::Start(fragment.source_offset))
        .map_err(io)?;
    file.read_exact(&mut data[start..]).map_err(io)?;
    if data[start..].ends_with(&[255, 217]) {
        data.truncate(data.len() - 2);
    }
    let mut cursor = start;
    while cursor < data.len() {
        if data[cursor] == 255 {
            cursor += 1;
            if data.get(cursor) != Some(&0) {
                return Err(Error("independent-reset OJPEG tile contains a source marker/restart or truncated escape".into()));
            }
        }
        cursor += 1;
    }
    if data.len() == start {
        return Err(Error("empty independent OJPEG entropy tile".into()));
    }
    data.extend([255, 217]);
    Ok(data)
}
pub(super) fn prepare(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limits: Limits,
) -> Result<Prepared, Error> {
    let (size, across) = geometry(page)?;
    let factors = super::super::ycbcr::subsampling(page)?;
    let offsets = vector(page, 324)?;
    let lengths = vector(page, 325)?;
    let count = page.shape[0]
        .div_ceil(size[0])
        .checked_mul(across)
        .ok_or_else(|| Error("independent OJPEG tile count overflow".into()))?;
    let index_bytes = count
        .checked_mul(256)
        .filter(|&n| n <= limits.metadata_bytes)
        .ok_or_else(|| Error("independent OJPEG tile index exceeds metadata limit".into()))?;
    if count != offsets.len() || lengths.len() != count {
        return Err(Error(
            "independent OJPEG tile tables disagree with geometry".into(),
        ));
    }
    let mut file = File::open(&snapshot.path).map_err(io)?;
    snapshot.check_handle(&file).map_err(Error)?;
    let source_len = file.metadata().map_err(io)?.len();
    let mut windows = Vec::new();
    let header = table_header(
        &mut file,
        source_len,
        page,
        FrameSpec {
            shape: size,
            factors,
            restart: 0,
        },
        limits,
        &mut windows,
    )?;
    let mut fragments = Vec::new();
    let mut total = 0usize;
    let mut ranges = Vec::new();
    for (index, (&offset, &length)) in offsets.iter().zip(&lengths).enumerate() {
        let bytes = usize::try_from(length)
            .map_err(|_| Error("independent OJPEG tile length overflow".into()))?;
        if bytes == 0 || offset.checked_add(length).is_none_or(|n| n > source_len) {
            return Err(Error(
                "independent OJPEG tile exceeds original source window".into(),
            ));
        }
        let origin = [index / across * size[0], index % across * size[1]];
        let mut fragment = Fragment {
            index,
            source_offset: offset,
            source_bytes: bytes,
            header_bytes: 0,
            entropy_offset: offset,
            entropy_bytes: bytes,
            origin,
            stream_origin: [0, 0],
            image_shape: [
                size[0].min(page.shape[0] - origin[0]),
                size[1].min(page.shape[1] - origin[1]),
            ],
            encoded_shape: size,
        };
        let data = stream(&mut file, &fragment, &header, limits)?;
        crate::jpeg::inspect_dct_bytes(&data, limits.metadata_bytes)
            .map_err(|e| Error(e.to_string()))?;
        if bytes >= 2 {
            file.seek(SeekFrom::Start(offset + bytes as u64 - 2))
                .map_err(io)?;
            let mut last = [0; 2];
            file.read_exact(&mut last).map_err(io)?;
            if last == [255, 217] {
                fragment.entropy_bytes -= 2;
            }
        }
        let component_bytes = size[0]
            .checked_mul(size[1])
            .and_then(|y| {
                (size[0] / factors[1])
                    .checked_mul(size[1] / factors[0])
                    .and_then(|uv| uv.checked_mul(2))
                    .and_then(|uv| y.checked_add(uv))
            })
            .ok_or_else(|| Error("independent OJPEG component size overflow".into()))?;
        total = total
            .checked_add(component_bytes)
            .filter(|&n| n <= limits.frame_bytes)
            .ok_or_else(|| Error("independent OJPEG full tile grids exceed frame budget".into()))?;
        if component_bytes
            .checked_mul(2)
            .and_then(|n| n.checked_add(data.len()))
            .is_none_or(|n| n > limits.intermediate_bytes)
        {
            return Err(Error(
                "independent OJPEG native tile scratch exceeds intermediate budget".into(),
            ));
        }
        windows.push(SourceWindow {
            offset,
            bytes,
            role: "independent reset tile",
        });
        ranges.push((offset, offset + length));
        fragments.push(fragment);
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|w| w[0].1 > w[1].0) {
        return Err(Error("overlapping independent OJPEG source tiles".into()));
    }
    let retained = windows
        .len()
        .checked_mul(64)
        .and_then(|n| n.checked_add(index_bytes))
        .filter(|&n| n <= limits.metadata_bytes)
        .ok_or_else(|| Error("independent OJPEG provenance exceeds metadata budget".into()))?;
    snapshot.check_handle(&file).map_err(Error)?;
    snapshot.check_handle(identity).map_err(Error)?;
    Ok(Prepared {
        encoded: Vec::new(),
        metadata: SourceMetadata {
            reconstruction: Reconstruction::TableDefinedIndependentChunks,
            encoded_shape: size,
            subsampling: factors,
            precision: 8,
            source_windows: windows,
            original_jpeg_header: Vec::new(),
            baseline_sos_normalized: false,
            retained_bytes: retained,
            restart_interval: 0,
            restart_markers_inserted: 0,
            fragments,
            partition: Partition::IndependentChunks,
            scan_sources: Vec::new(),
            tile_policy: OjpegTiles::IndependentResetTiles,
        },
    })
}
pub(super) fn decode(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limits: Limits,
    metadata: &SourceMetadata,
) -> Result<Vec<Chunk>, Error> {
    let mut file = File::open(&snapshot.path).map_err(io)?;
    snapshot.check_handle(&file).map_err(Error)?;
    let source_len = file.metadata().map_err(io)?.len();
    let header = table_header(
        &mut file,
        source_len,
        page,
        FrameSpec {
            shape: metadata.encoded_shape,
            factors: metadata.subsampling,
            restart: 0,
        },
        limits,
        &mut Vec::new(),
    )?;
    let mut chunks = Vec::new();
    chunks
        .try_reserve_exact(metadata.fragments.len())
        .map_err(|e| Error(e.to_string()))?;
    for fragment in &metadata.fragments {
        let data = stream(&mut file, fragment, &header, limits)?;
        let chunk = decode_jpeg(
            &data,
            fragment.origin,
            fragment.image_shape,
            fragment.encoded_shape,
            metadata.subsampling,
            limits,
        )?;
        chunks.push(chunk);
    }
    snapshot.check_handle(&file).map_err(Error)?;
    snapshot.check_handle(identity).map_err(Error)?;
    Ok(chunks)
}
