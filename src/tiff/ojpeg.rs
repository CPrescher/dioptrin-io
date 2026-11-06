//! Bounded OJPEG header/table adaptation following libtiff.
use super::{Error, Limits, Metadata, jpeg_components::decode_jpeg, ycbcr::Chunk};
use crate::source::Snapshot;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
mod advanced;
mod fragments;
mod independent;
/// Original declared legacy JPEG source window. Tables have intrinsic lengths
/// (64 quantizers or 16 Huffman counts plus their exact number of symbols).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceWindow {
    pub offset: u64,
    pub bytes: usize,
    pub role: &'static str,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconstruction {
    CompleteInterchange,
    TableDefinedSingleChunk,
    TableDefinedFragments,
    FragmentedInterchange,
    GlobalStream,
    PlanarScans,
    TableDefinedIndependentChunks,
}
/// Whether source byte windows correspond directly to logical raster chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Partition {
    RasterAligned,
    GlobalStream,
    PlanarScans,
    IndependentChunks,
}
/// Original byte ranges for a separately coded source component scan.
#[derive(Debug, Clone)]
pub struct ScanSource {
    pub component: super::ycbcr::Component,
    pub header_windows: Vec<SourceWindow>,
    pub entropy_windows: Vec<SourceWindow>,
}
/// One original declared encoded chunk and its contribution to the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub index: usize,
    pub source_offset: u64,
    pub source_bytes: usize,
    pub header_bytes: usize,
    pub entropy_offset: u64,
    pub entropy_bytes: usize,
    pub origin: [usize; 2],
    /// Position in the encoded stacked JPEG frame, distinct from logical tile origin.
    pub stream_origin: [usize; 2],
    pub image_shape: [usize; 2],
    pub encoded_shape: [usize; 2],
}
/// Legacy container interpretation, separate from decoded component buffers.
#[derive(Debug, Clone)]
pub struct SourceMetadata {
    pub reconstruction: Reconstruction,
    pub encoded_shape: [usize; 2],
    /// TIFF tag order: horizontal, vertical.
    pub subsampling: [usize; 2],
    pub precision: u8,
    pub source_windows: Vec<SourceWindow>,
    /// Complete original interchange header through SOS. Empty when source
    /// header/table declarations live exclusively in TIFF tags.
    pub original_jpeg_header: Vec<u8>,
    /// libtiff's OJPEG baseline-header operation: obsolete Se=0 is interpreted
    /// as the sequential baseline scan Se=63; original source bytes stay above.
    pub baseline_sos_normalized: bool,
    pub retained_bytes: usize,
    pub restart_interval: u16,
    pub restart_markers_inserted: usize,
    pub fragments: Vec<Fragment>,
    pub partition: Partition,
    pub scan_sources: Vec<ScanSource>,
    pub tile_policy: super::OjpegTiles,
}
struct Prepared {
    encoded: Vec<u8>,
    metadata: SourceMetadata,
}
/// Index the exact declared legacy windows without decoding or retaining pixels.
pub(super) fn inspect(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limits: Limits,
    options: super::Options,
) -> Result<SourceMetadata, Error> {
    Ok(prepare(snapshot, identity, page, limits, options)?.metadata)
}
pub(super) fn decode(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limits: Limits,
    options: super::Options,
) -> Result<Vec<Chunk>, Error> {
    let prepared = prepare(snapshot, identity, page, limits, options)?;
    if prepared.metadata.partition == Partition::IndependentChunks {
        return independent::decode(snapshot, identity, page, limits, &prepared.metadata);
    }
    if !prepared.metadata.fragments.is_empty()
        || prepared.metadata.partition == Partition::PlanarScans
    {
        fragments::check_decode_budget(&prepared.metadata, prepared.encoded.len(), limits)?;
    }
    let chunk = decode_jpeg(
        &prepared.encoded,
        [0, 0],
        if !prepared.metadata.fragments.is_empty() {
            prepared.metadata.encoded_shape
        } else {
            page.shape
        },
        prepared.metadata.encoded_shape,
        prepared.metadata.subsampling,
        limits,
    )?;
    snapshot.check_handle(identity).map_err(Error)?;
    if !prepared.metadata.fragments.is_empty() {
        fragments::split(chunk, &prepared.metadata.fragments, limits)
    } else {
        Ok(vec![chunk])
    }
}
fn prepare(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limits: Limits,
    options: super::Options,
) -> Result<Prepared, Error> {
    snapshot.check_handle(identity).map_err(Error)?;
    if page.compression != 6
        || page.photometric_interpretation != 6
        || page.bits_per_sample != 8
        || page.channels != 3
        || ![1, 2].contains(&page.planar_configuration)
        || scalar(page, 512, 1)? != 1
    {
        return Err(Error(
            "old JPEG TIFF requires supported8-bit sequentialYCbCr source declarations".into(),
        ));
    }
    if options.ojpeg_tiles == super::OjpegTiles::IndependentResetTiles {
        return independent::prepare(snapshot, identity, page, limits);
    }
    let tiled = page.tags.contains_key(&324);
    let offsets = vector(page, if tiled { 324 } else { 273 })?;
    let lengths = vector(page, if tiled { 325 } else { 279 })?;
    if let Some(prepared) =
        advanced::prepare_if_recognized(snapshot, identity, page, limits, &offsets, &lengths)?
    {
        return Ok(prepared);
    }
    if page.planar_configuration != 1 {
        return Err(Error("unsupported OJPEG planar source profile".into()));
    }
    if offsets.len() > 1 {
        return fragments::prepare(snapshot, identity, page, limits, offsets, lengths);
    }
    if offsets.len() != 1 || lengths.len() != 1 || scalar(page, 515, 0)? != 0 {
        return Err(Error(
            "old JPEG TIFF currently requires exactly one complete strip/tile".into(),
        ));
    }
    let shape = if tiled {
        [
            scalar(page, 323, 0)? as usize,
            scalar(page, 322, 0)? as usize,
        ]
    } else {
        page.shape
    };
    if shape
        .iter()
        .zip(page.shape)
        .any(|(&stored, valid)| stored < valid || stored == 0 || stored > 65535)
    {
        return Err(Error(
            "old JPEG TIFF encoded dimensions exceed supported bounds".into(),
        ));
    }
    if !tiled && scalar(page, 278, page.shape[0] as u64)? < page.shape[0] as u64 {
        return Err(Error(
            "old JPEG TIFF row geometry requires additional strips".into(),
        ));
    }
    let factors = super::ycbcr::subsampling(page)?;
    let mut file = File::open(&snapshot.path).map_err(io)?;
    snapshot.check_handle(&file).map_err(Error)?;
    let mut windows = Vec::new();
    let source_len = file.metadata().map_err(io)?.len();
    let chunk_len = usize::try_from(lengths[0])
        .map_err(|_| Error("old JPEG chunk length exceeds platform".into()))?;
    let chunk_end = offsets[0]
        .checked_add(lengths[0])
        .filter(|&n| n <= source_len)
        .ok_or_else(|| Error("old JPEG chunk exceeds source bounds".into()))?;
    if chunk_len == 0 {
        return Err(Error("empty old JPEG source chunk".into()));
    }
    windows.push(SourceWindow {
        offset: offsets[0],
        bytes: chunk_len,
        role: "encoded chunk",
    });
    let interchange = page
        .tags
        .get(&513)
        .map(|v| v.clone().into_u64())
        .transpose()?;
    let mut normalized = false;
    let (encoded, original_header, reconstruction) = if let Some(offset) = interchange {
        let length = usize::try_from(scalar(page, 514, 0)?)
            .map_err(|_| Error("old JPEG interchange size exceeds platform".into()))?;
        let mut data = read(
            &mut file,
            source_len,
            offset,
            length,
            "JPEG interchange",
            limits,
            &mut windows,
        )?;
        if offsets[0] < offset || chunk_end > offset.checked_add(length as u64).unwrap_or(0) {
            return Err(Error(
                "old JPEG fragmented interchange/chunk layout is unsupported".into(),
            ));
        }
        let (segments, entropy) = crate::jpeg::inspect_dct_bytes(&data, limits.metadata_bytes)
            .map_err(|e| Error(e.to_string()))?;
        let frame = segments
            .iter()
            .find(|s| s.marker == 0xc0)
            .ok_or_else(|| Error("old JPEG interchange requires baselineSOF0".into()))?;
        if frame.payload.len() != 15
            || frame.payload[0] != 8
            || [
                usize::from(u16::from_be_bytes([frame.payload[1], frame.payload[2]])),
                usize::from(u16::from_be_bytes([frame.payload[3], frame.payload[4]])),
            ] != shape
        {
            return Err(Error(
                "old JPEG interchange frame contradicts TIFF geometry/precision".into(),
            ));
        }
        let scan = &segments
            .last()
            .ok_or_else(|| Error("missing old JPEG SOS".into()))?
            .payload;
        if scan.len() != 10
            || scan[0] != 3
            || scan[7] != 0
            || scan[9] != 0
            || ![0, 63].contains(&scan[8])
        {
            return Err(Error(
                "unsupported old JPEG baseline scan parameters".into(),
            ));
        }
        let original = data[..entropy].to_vec();
        if scan[8] == 0 {
            data[entropy - 2] = 63;
            normalized = true;
        }
        (data, original, Reconstruction::CompleteInterchange)
    } else {
        if page.tags.contains_key(&514) {
            return Err(Error(
                "old JPEG interchange size lacks its source offset".into(),
            ));
        }
        let mut data = table_header(
            &mut file,
            source_len,
            page,
            FrameSpec {
                shape,
                factors,
                restart: 0,
            },
            limits,
            &mut windows,
        )?;
        let mut entropy = read(
            &mut file,
            source_len,
            offsets[0],
            chunk_len,
            "entropy chunk",
            limits,
            &mut Vec::new(),
        )?;
        // Raw entropy has byte stuffing/restarts, never source marker headers.
        let mut cursor = 0;
        while cursor < entropy.len() {
            if entropy[cursor] == 255 {
                cursor += 1;
                if cursor == entropy.len() || entropy[cursor] != 0 {
                    return Err(Error(
                        "old JPEG entropy contains unsupported source marker/tail".into(),
                    ));
                }
            }
            cursor += 1;
        }
        let total = data
            .len()
            .checked_add(entropy.len())
            .and_then(|n| n.checked_add(2))
            .filter(|&n| n <= limits.intermediate_bytes)
            .ok_or_else(|| {
                Error("old JPEG reconstructed stream exceeds intermediate limit".into())
            })?;
        data.try_reserve_exact(total - data.len())
            .map_err(|e| Error(e.to_string()))?;
        data.append(&mut entropy);
        data.extend([255, 217]);
        (data, Vec::new(), Reconstruction::TableDefinedSingleChunk)
    };
    let retained_bytes = windows
        .len()
        .checked_mul(64)
        .and_then(|n| n.checked_add(original_header.len()))
        .filter(|&n| n <= limits.metadata_bytes)
        .ok_or_else(|| Error("old JPEG retained headers exceed metadata budget".into()))?;
    snapshot.check_handle(&file).map_err(Error)?;
    snapshot.check_handle(identity).map_err(Error)?;
    Ok(Prepared {
        encoded,
        metadata: SourceMetadata {
            reconstruction,
            encoded_shape: shape,
            subsampling: factors,
            precision: 8,
            source_windows: windows,
            original_jpeg_header: original_header,
            baseline_sos_normalized: normalized,
            retained_bytes,
            restart_interval: 0,
            restart_markers_inserted: 0,
            fragments: Vec::new(),
            partition: Partition::RasterAligned,
            scan_sources: Vec::new(),
            tile_policy: options.ojpeg_tiles,
        },
    })
}
fn read(
    file: &mut File,
    source_len: u64,
    offset: u64,
    length: usize,
    role: &'static str,
    limits: Limits,
    windows: &mut Vec<SourceWindow>,
) -> Result<Vec<u8>, Error> {
    if length == 0
        || length > limits.intermediate_bytes
        || offset
            .checked_add(length as u64)
            .is_none_or(|n| n > source_len)
    {
        return Err(Error(
            "old JPEG declared source window exceeds source/resource bounds".into(),
        ));
    }
    file.seek(SeekFrom::Start(offset)).map_err(io)?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes).map_err(io)?;
    windows.push(SourceWindow {
        offset,
        bytes: length,
        role,
    });
    Ok(bytes)
}
fn marker(output: &mut Vec<u8>, tag: u8, payload: &[u8], limits: Limits) -> Result<(), Error> {
    if output
        .len()
        .checked_add(payload.len() + 4)
        .is_none_or(|n| n > limits.intermediate_bytes)
    {
        return Err(Error("old JPEG synthesized header exceeds limit".into()));
    }
    let length = u16::try_from(payload.len() + 2)
        .map_err(|_| Error("old JPEG marker size overflow".into()))?;
    output.extend([255, tag]);
    output.extend(length.to_be_bytes());
    output.extend(payload);
    Ok(())
}
fn vector(page: &Metadata, tag: u16) -> Result<Vec<u64>, Error> {
    page.tags
        .get(&tag)
        .ok_or_else(|| Error(format!("old JPEG missing source tag{tag}")))?
        .clone()
        .into_u64_vec()
        .map_err(Error::from)
}
fn scalar(page: &Metadata, tag: u16, default: u64) -> Result<u64, Error> {
    let Some(value) = page.tags.get(&tag) else {
        return Ok(default);
    };
    let values = value.clone().into_u64_vec()?;
    if values.len() != 1 {
        return Err(Error("old JPEG source tag is not scalar".into()));
    }
    Ok(values[0])
}
fn io(error: std::io::Error) -> Error {
    Error(error.to_string())
}

struct FrameSpec {
    shape: [usize; 2],
    factors: [usize; 2],
    restart: u16,
}
fn table_header(
    file: &mut File,
    source_len: u64,
    page: &Metadata,
    spec: FrameSpec,
    limits: Limits,
    windows: &mut Vec<SourceWindow>,
) -> Result<Vec<u8>, Error> {
    let FrameSpec {
        shape,
        factors,
        restart,
    } = spec;
    let mut data = vec![255, 216];
    let mut selectors = [[0u8; 3]; 3];
    for (kind, tag) in [(0, 519), (1, 520), (2, 521)] {
        let locations = vector(page, tag)?;
        if locations.len() != 3 || locations[0] == 0 {
            return Err(Error(
                "old JPEG requires three declared table selectors".into(),
            ));
        }
        for component in 0..3 {
            if component > 0
                && (locations[component] == 0 || locations[component] == locations[component - 1])
            {
                selectors[kind][component] = selectors[kind][component - 1];
                continue;
            }
            if locations[..component].contains(&locations[component]) {
                return Err(Error("old JPEG table selector order is unsupported".into()));
            }
            selectors[kind][component] = component as u8;
            let role = match kind {
                0 => "quantization table",
                1 => "DC Huffman table",
                _ => "AC Huffman table",
            };
            let table = if kind == 0 {
                read(
                    file,
                    source_len,
                    locations[component],
                    64,
                    role,
                    limits,
                    windows,
                )?
            } else {
                let counts = read(
                    file,
                    source_len,
                    locations[component],
                    16,
                    role,
                    limits,
                    &mut Vec::new(),
                )?;
                let symbols: usize = counts.iter().map(|&n| usize::from(n)).sum();
                if symbols == 0 || symbols > 256 {
                    return Err(Error("invalid old JPEG Huffman symbol count".into()));
                }
                read(
                    file,
                    source_len,
                    locations[component],
                    16 + symbols,
                    role,
                    limits,
                    windows,
                )?
            };
            let mut payload = vec![(if kind == 2 { 16 } else { 0 }) | component as u8];
            payload.extend(table);
            marker(
                &mut data,
                if kind == 0 { 0xdb } else { 0xc4 },
                &payload,
                limits,
            )?;
        }
    }
    let mut frame = vec![8];
    frame.extend((shape[0] as u16).to_be_bytes());
    frame.extend((shape[1] as u16).to_be_bytes());
    frame.push(3);
    let mut scan = vec![3];
    for (c, &q_selector) in selectors[0].iter().enumerate() {
        frame.extend([
            c as u8,
            if c == 0 {
                ((factors[0] as u8) << 4) | factors[1] as u8
            } else {
                0x11
            },
            q_selector,
        ]);
        scan.extend([c as u8, selectors[1][c] << 4 | selectors[2][c]]);
    }
    scan.extend([0, 63, 0]);
    if restart != 0 {
        marker(&mut data, 0xdd, &restart.to_be_bytes(), limits)?;
    }
    marker(&mut data, 0xc0, &frame, limits)?;
    marker(&mut data, 0xda, &scan, limits)?;
    Ok(data)
}
