//! Recognized original-JPEG stream profiles which do not have one entropy
//! window per raster chunk. Source byte contributions and logical geometry are
//! deliberately separate; decoding still uses the existing native JPEG path.
use super::{
    Fragment, FrameSpec, Partition, Prepared, Reconstruction, ScanSource, SourceMetadata,
    SourceWindow, io, marker, read, scalar, table_header,
};
use crate::{
    source::Snapshot,
    tiff::{Error, Limits, Metadata, ycbcr::Component},
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
struct Part {
    offset: u64,
    bytes: usize,
    start: usize,
}
fn bytes(file: &mut File, offset: u64, length: usize) -> Result<Vec<u8>, Error> {
    file.seek(SeekFrom::Start(offset)).map_err(io)?;
    let mut v = vec![0; length];
    file.read_exact(&mut v).map_err(io)?;
    Ok(v)
}
pub(super) fn prepare_if_recognized(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limits: Limits,
    offsets: &[u64],
    lengths: &[u64],
) -> Result<Option<Prepared>, Error> {
    if offsets.is_empty() || offsets.len() != lengths.len() {
        return Err(Error("invalid OJPEG source chunk table".into()));
    }
    let planar = page.planar_configuration == 2;
    let prefix = page
        .tags
        .get(&513)
        .map(|v| v.clone().into_u64())
        .transpose()?;
    let declared = scalar(page, 515, 0)?;
    if !planar && prefix.is_none() && declared == 0 {
        // Table-only ordinary fragments stay on the established path. A source
        // SOI, however, explicitly identifies a complete/header-led stream.
        let mut file = File::open(&snapshot.path).map_err(io)?;
        snapshot.check_handle(&file).map_err(Error)?;
        if lengths[0] < 2 {
            return Ok(None);
        }
        let first = bytes(&mut file, offsets[0], 2)?;
        if first != [255, 216] {
            return Ok(None);
        }
    }
    let factors = super::super::ycbcr::subsampling(page)?;
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
    if width == 0 || rows == 0 || width > 65535 || rows > 65535 {
        return Err(Error("OJPEG stream dimensions exceed JPEG bounds".into()));
    }
    let across = page.shape[1].div_ceil(width);
    let spatial = page.shape[0]
        .div_ceil(rows)
        .checked_mul(across)
        .ok_or_else(|| Error("OJPEG stream chunk count overflow".into()))?;
    let expected = spatial
        .checked_mul(if planar { 3 } else { 1 })
        .ok_or_else(|| Error("OJPEG planar chunk count overflow".into()))?;
    if expected != offsets.len() {
        return Err(Error(
            "OJPEG stream windows disagree with TIFF geometry".into(),
        ));
    }
    if planar && (tiled || spatial != 1) {
        return Err(Error(
            "OJPEG planar source profile requires one complete strip per component".into(),
        ));
    }
    let shape = if tiled {
        [
            spatial
                .checked_mul(rows)
                .ok_or_else(|| Error("OJPEG stacked frame size overflow".into()))?,
            width,
        ]
    } else {
        page.shape
    };
    if shape.iter().any(|&n| n == 0 || n > 65535) {
        return Err(Error(
            "OJPEG stream frame dimensions exceed JPEG bounds".into(),
        ));
    }
    let retained_base = offsets
        .len()
        .checked_mul(256)
        .filter(|&n| n <= limits.metadata_bytes)
        .ok_or_else(|| Error("OJPEG stream provenance exceeds metadata budget".into()))?;
    let mut file = File::open(&snapshot.path).map_err(io)?;
    snapshot.check_handle(&file).map_err(Error)?;
    let source_len = file.metadata().map_err(io)?.len();
    let mut total = 0usize;
    let mut windows = Vec::new();
    let mut parts = Vec::new();
    for (&offset, &length) in offsets.iter().zip(lengths) {
        let size = usize::try_from(length)
            .map_err(|_| Error("OJPEG stream window size overflow".into()))?;
        if size == 0 || offset.checked_add(length).is_none_or(|n| n > source_len) {
            return Err(Error("OJPEG stream window exceeds original source".into()));
        }
        total = total
            .checked_add(size)
            .filter(|&n| n <= limits.intermediate_bytes / 5)
            .ok_or_else(|| Error("OJPEG stream assembly exceeds intermediate budget".into()))?;
        windows.push(SourceWindow {
            offset,
            bytes: size,
            role: "encoded stream part",
        });
        parts.push(Part {
            offset,
            bytes: size,
            start: 0,
        });
    }
    let mut ordered: Vec<_> = parts
        .iter()
        .map(|p| (p.offset, p.offset + p.bytes as u64))
        .collect();
    ordered.sort_unstable();
    if ordered.windows(2).any(|w| w[0].1 > w[1].0) {
        return Err(Error("overlapping OJPEG stream parts".into()));
    }
    let mut complete = false;
    let mut prefix_bytes = 0usize;
    let mut source_parts = Vec::new();
    let mut input = if let Some(offset) = prefix {
        let length = number(page, 514, 0)?;
        let _ = total
            .checked_add(length)
            .filter(|&n| n <= limits.intermediate_bytes / 5)
            .ok_or_else(|| {
                Error("OJPEG interchange assembly exceeds intermediate budget".into())
            })?;
        let end = offset
            .checked_add(length as u64)
            .ok_or_else(|| Error("OJPEG interchange extent overflow".into()))?;
        complete = ordered.iter().all(|&(a, b)| a >= offset && b <= end);
        if !complete && ordered.iter().any(|&(a, b)| a < end && b > offset) {
            return Err(Error("ambiguous OJPEG interchange/stream overlap".into()));
        }
        let v = read(
            &mut file,
            source_len,
            offset,
            length,
            "JPEG interchange stream",
            limits,
            &mut windows,
        )?;
        prefix_bytes = length;
        source_parts.push(Part {
            offset,
            bytes: length,
            start: 0,
        });
        v
    } else {
        Vec::new()
    };
    for part in &mut parts {
        if complete {
            part.start = usize::try_from(part.offset - prefix.unwrap())
                .map_err(|_| Error("OJPEG source position overflow".into()))?;
        } else {
            part.start = input.len();
            input.extend(bytes(&mut file, part.offset, part.bytes)?);
            source_parts.push(Part {
                offset: part.offset,
                bytes: part.bytes,
                start: part.start,
            });
        }
    }
    if planar && factors != [1, 1] && !complete {
        return Err(Error(
            "subsampled OJPEG planar scans require complete original interchange authority".into(),
        ));
    }
    let source_header = input.starts_with(&[255, 216]);
    let (segments, entropy) = if source_header {
        let (h, e) = crate::jpeg::inspect_dct_header_bytes(&input, limits.metadata_bytes)
            .map_err(|e| Error(e.to_string()))?;
        (Some(h), e)
    } else {
        (None, 0)
    };
    let has_new_header = segments
        .as_ref()
        .is_some_and(|s| s.iter().any(|s| s.marker == 0xc1));
    let beyond_first = source_header && !complete && entropy > prefix_bytes + parts[0].bytes;
    let single_new = parts.len() == 1 && (!complete || declared != 0);
    let header_restart = segments
        .as_ref()
        .and_then(|s| s.iter().find(|s| s.marker == 0xdd))
        .map(|s| {
            if s.payload.len() == 2 {
                u16::from_be_bytes([s.payload[0], s.payload[1]])
            } else {
                0
            }
        });
    let nominal = width
        .div_ceil(factors[0] * 8)
        .checked_mul(rows.div_ceil(factors[1] * 8))
        .ok_or_else(|| Error("OJPEG nominal MCU count overflow".into()))?;
    let complete_intra =
        complete && header_restart.is_some_and(|r| r != 0 && usize::from(r) != nominal);
    let stored_restarts = source_header
        && !complete
        && input[entropy..]
            .windows(2)
            .any(|w| w[0] == 255 && (0xd0..=0xd7).contains(&w[1]));
    if !planar
        && !has_new_header
        && !beyond_first
        && !single_new
        && !complete_intra
        && !stored_restarts
    {
        return Ok(None);
    }
    let mut original_header = Vec::new();
    let mut normalized = false;
    let mut encoded;
    let restart;
    let mut scan_sources = Vec::new();
    if let Some(segments) = segments {
        let frame = segments
            .iter()
            .find(|s| [0xc0, 0xc1].contains(&s.marker))
            .ok_or_else(|| {
                Error("OJPEG source stream requires8-bit sequential SOF0/SOF1".into())
            })?;
        if frame.payload.len() != 15
            || frame.payload[0] != 8
            || [
                usize::from(u16::from_be_bytes([frame.payload[1], frame.payload[2]])),
                usize::from(u16::from_be_bytes([frame.payload[3], frame.payload[4]])),
            ] != shape
        {
            return Err(Error(
                "OJPEG source frame contradicts TIFF stream geometry".into(),
            ));
        }
        let dris: Vec<_> = segments.iter().filter(|s| s.marker == 0xdd).collect();
        if dris.len() > 1 || dris.first().is_some_and(|s| s.payload.len() != 2) {
            return Err(Error(
                "invalid/duplicate OJPEG source restart declaration".into(),
            ));
        }
        restart = if let Some(dri) = dris.first() {
            u16::from_be_bytes([dri.payload[0], dri.payload[1]])
        } else {
            u16::try_from(declared)
                .map_err(|_| Error("OJPEG restart interval exceeds16bits".into()))?
        };
        if declared != 0 && declared != u64::from(restart) {
            return Err(Error(
                "OJPEG source/tag restart declarations disagree".into(),
            ));
        }
        original_header = input[..entropy].to_vec();
        let scan = &segments
            .last()
            .ok_or_else(|| Error("missing OJPEG SOS".into()))?
            .payload;
        if planar {
            if restart != 0 {
                return Err(Error(
                    "OJPEG planar restart profile remains unsupported".into(),
                ));
            }
            if scan.len() != 6 {
                return Err(Error(
                    "OJPEG planar source requires one component per sequential scan".into(),
                ));
            }
            ensure_eoi(&mut input)?;
            scan_sources =
                planar_scans(&input, entropy, scan, &frame.payload, &source_parts, limits)?;
            if complete {
                for (part, scan) in parts.iter().zip(&scan_sources) {
                    let ent = scan
                        .entropy_windows
                        .first()
                        .ok_or_else(|| Error("OJPEG planar scan lacks source entropy".into()))?;
                    let hdr = scan
                        .header_windows
                        .first()
                        .ok_or_else(|| Error("OJPEG planar scan lacks source SOS".into()))?;
                    if scan.entropy_windows.len() != 1
                        || scan.header_windows.len() != 1
                        || part.offset + part.bytes as u64 != ent.offset + ent.bytes as u64
                        || ![ent.offset, hdr.offset].contains(&part.offset)
                    {
                        return Err(Error(
                            "OJPEG complete planar windows do not match exact component scans"
                                .into(),
                        ));
                    }
                }
            }
        } else {
            if scan.len() != 10
                || scan[0] != 3
                || scan[7] != 0
                || scan[9] != 0
                || ![0, 63].contains(&scan[8])
            {
                return Err(Error(
                    "OJPEG stream requires interleaved sequential scan".into(),
                ));
            }
            if scan[8] == 0 {
                input[entropy - 2] = 63;
                normalized = true;
            }
            let actual = entropy_end(&input, entropy, restart)?;
            let mcus = shape[1].div_ceil(factors[0] * 8) * shape[0].div_ceil(factors[1] * 8);
            check_restart_count(actual, mcus, restart)?;
            if beyond_first && parts.iter().filter(|p| p.start + p.bytes > entropy).count() != 1 {
                return Err(Error(
                    "header-continuation OJPEG requires one unambiguous full entropy part".into(),
                ));
            }
            if complete {
                validate_partition(&input, entropy, &parts, nominal, restart)?;
            }
            ensure_eoi(&mut input)?;
        }
        encoded = input;
        if dris.is_empty() && restart != 0 {
            let mut dri = Vec::new();
            marker(&mut dri, 0xdd, &restart.to_be_bytes(), limits)?;
            encoded.splice(2..2, dri);
        }
    } else {
        if parts.len() != 1 || declared == 0 {
            return Err(Error(
                "unsupported OJPEG stream without explicit source header".into(),
            ));
        }
        restart = u16::try_from(declared)
            .map_err(|_| Error("OJPEG restart interval exceeds16bits".into()))?;
        let actual = entropy_end(&input, 0, restart)?;
        let mcus = shape[1].div_ceil(factors[0] * 8) * shape[0].div_ceil(factors[1] * 8);
        check_restart_count(actual, mcus, restart)?;
        encoded = table_header(
            &mut file,
            source_len,
            page,
            FrameSpec {
                shape,
                factors,
                restart,
            },
            limits,
            &mut windows,
        )?;
        if input.ends_with(&[255, 217]) {
            input.truncate(input.len() - 2);
        }
        encoded.extend(input);
        encoded.extend([255, 217]);
    }
    let mut fragments = Vec::new();
    if !planar {
        for (index, part) in parts.iter().enumerate() {
            let header_bytes = entropy.saturating_sub(part.start).min(part.bytes);
            let mut entropy_bytes = part.bytes - header_bytes;
            if part.start + part.bytes
                == if complete {
                    prefix_bytes
                } else {
                    source_parts.last().map_or(0, |p| p.start + p.bytes)
                }
                && entropy_bytes >= 2
            {
                let last = bytes(&mut file, part.offset + part.bytes as u64 - 2, 2)?;
                if last == [255, 217] {
                    entropy_bytes -= 2;
                }
            }
            let row = index / across * rows;
            let col = index % across * width;
            fragments.push(Fragment {
                index,
                source_offset: part.offset,
                source_bytes: part.bytes,
                header_bytes,
                entropy_offset: part.offset + header_bytes as u64,
                entropy_bytes,
                origin: [row, col],
                stream_origin: [index * rows, 0],
                image_shape: [
                    rows.min(page.shape[0] - row),
                    width.min(page.shape[1] - col),
                ],
                encoded_shape: [
                    if tiled {
                        rows
                    } else {
                        rows.min(page.shape[0] - row)
                    },
                    width,
                ],
            });
        }
    }
    let retained = windows
        .len()
        .checked_mul(64)
        .and_then(|n| n.checked_add(retained_base))
        .and_then(|n| n.checked_add(original_header.len()))
        .and_then(|n| {
            scan_sources.iter().try_fold(n, |n, s| {
                n.checked_add((s.header_windows.len() + s.entropy_windows.len()) * 64 + 64)
            })
        })
        .filter(|&n| n <= limits.metadata_bytes)
        .ok_or_else(|| Error("OJPEG advanced provenance exceeds metadata budget".into()))?;
    if encoded.len() > limits.intermediate_bytes / 2 {
        return Err(Error(
            "OJPEG advanced encoded stream exceeds intermediate budget".into(),
        ));
    }
    crate::jpeg::inspect_dct_bytes(&encoded, limits.metadata_bytes)
        .map_err(|e| Error(e.to_string()))?;
    snapshot.check_handle(&file).map_err(Error)?;
    snapshot.check_handle(identity).map_err(Error)?;
    Ok(Some(Prepared {
        encoded,
        metadata: SourceMetadata {
            reconstruction: if planar {
                Reconstruction::PlanarScans
            } else {
                Reconstruction::GlobalStream
            },
            encoded_shape: shape,
            subsampling: factors,
            precision: 8,
            source_windows: windows,
            original_jpeg_header: original_header,
            baseline_sos_normalized: normalized,
            retained_bytes: retained,
            restart_interval: restart,
            restart_markers_inserted: 0,
            fragments,
            partition: if planar {
                Partition::PlanarScans
            } else {
                Partition::GlobalStream
            },
            scan_sources,
            tile_policy: super::super::OjpegTiles::Declared,
        },
    }))
}
fn number(page: &Metadata, tag: u16, default: usize) -> Result<usize, Error> {
    usize::try_from(scalar(page, tag, default as u64)?)
        .map_err(|_| Error("OJPEG source integer exceeds platform".into()))
}
fn ensure_eoi(input: &mut Vec<u8>) -> Result<(), Error> {
    if !input.ends_with(&[255, 217]) {
        if input.last() == Some(&255) {
            return Err(Error("truncated OJPEG terminal marker".into()));
        }
        input.extend([255, 217]);
    }
    Ok(())
}
fn check_restart_count(actual: usize, mcus: usize, restart: u16) -> Result<(), Error> {
    let expected = if restart == 0 {
        0
    } else {
        (mcus - 1) / usize::from(restart)
    };
    if actual != expected {
        return Err(Error(
            "OJPEG restart count contradicts complete source MCU count".into(),
        ));
    }
    Ok(())
}
fn entropy_end(input: &[u8], start: usize, restart: u16) -> Result<usize, Error> {
    let mut cursor = start;
    let mut count = 0;
    while cursor < input.len() {
        if input[cursor] == 255 {
            let next = *input
                .get(cursor + 1)
                .ok_or_else(|| Error("truncated OJPEG entropy escape".into()))?;
            if next == 217 && cursor + 2 == input.len() {
                break;
            }
            if next != 0 {
                if restart == 0 || next != 0xd0 + (count % 8) as u8 {
                    return Err(Error(
                        "invalid OJPEG internal restart sequence/marker".into(),
                    ));
                }
                count += 1;
            }
            cursor += 1;
        }
        cursor += 1;
    }
    Ok(count)
}
fn validate_partition(
    input: &[u8],
    entropy: usize,
    parts: &[Part],
    nominal: usize,
    restart: u16,
) -> Result<(), Error> {
    let base = parts[0].start;
    let mut cursor = entropy;
    if base != entropy {
        return Err(Error(
            "OJPEG complete interchange parts alias headers or omit entropy".into(),
        ));
    }
    for (index, part) in parts.iter().enumerate() {
        if part.start != cursor {
            return Err(Error(
                "OJPEG complete interchange part order/start mismatch".into(),
            ));
        }
        cursor = cursor
            .checked_add(part.bytes)
            .filter(|&n| n <= input.len())
            .ok_or_else(|| Error("OJPEG part extent overflow".into()))?;
        if index + 1 < parts.len() {
            if restart == 0 || !nominal.is_multiple_of(usize::from(restart)) {
                return Err(Error(
                    "OJPEG source restart interval does not align TIFF boundaries".into(),
                ));
            }
            let marker = (nominal / usize::from(restart) * (index + 1) - 1) % 8;
            if input.get(cursor..cursor + 2) != Some(&[255, 0xd0 + marker as u8]) {
                return Err(Error(
                    "OJPEG declared boundary lacks its exact restart marker".into(),
                ));
            }
            cursor += 2;
        }
    }
    if input.get(cursor..) != Some(&[255, 217]) {
        return Err(Error(
            "OJPEG declared parts do not cover exact entropy/EOI".into(),
        ));
    }
    Ok(())
}
fn mapped(parts: &[Part], start: usize, end: usize, role: &'static str) -> Vec<SourceWindow> {
    parts
        .iter()
        .filter_map(|p| {
            let a = start.max(p.start);
            let b = end.min(p.start + p.bytes);
            (a < b).then(|| SourceWindow {
                offset: p.offset + (a - p.start) as u64,
                bytes: b - a,
                role,
            })
        })
        .collect()
}
fn planar_scans(
    input: &[u8],
    first_entropy: usize,
    first_scan: &[u8],
    frame: &[u8],
    parts: &[Part],
    limits: Limits,
) -> Result<Vec<ScanSource>, Error> {
    let ids = [frame[6], frame[9], frame[12]];
    let mut result = Vec::new();
    let mut scan = first_scan.to_vec();
    let mut start = first_entropy;
    let mut header_start = first_entropy - first_scan.len() - 4;
    let mut metadata = 0usize;
    loop {
        let index = result.len();
        if index >= 3
            || scan.len() != 6
            || scan[0] != 1
            || scan[1] != ids[index]
            || scan[3..] != [0, 63, 0]
        {
            return Err(Error(
                "OJPEG planar stream requires exactlyY,Cb,Cr sequential scans".into(),
            ));
        }
        let mut cursor = start;
        while cursor < input.len() {
            if input[cursor] == 255 {
                let next = *input
                    .get(cursor + 1)
                    .ok_or_else(|| Error("truncated OJPEG planar entropy marker".into()))?;
                if (0xd0..=0xd7).contains(&next) {
                    return Err(Error(
                        "OJPEG planar restart profile remains unsupported".into(),
                    ));
                }
                if next != 0 {
                    break;
                }
                cursor += 1;
            }
            cursor += 1;
        }
        if cursor >= input.len() {
            return Err(Error("OJPEG planar stream lacks EOI".into()));
        }
        result.push(ScanSource {
            component: [Component::Y, Component::Cb, Component::Cr][index],
            header_windows: mapped(parts, header_start, start, "component SOS"),
            entropy_windows: mapped(parts, start, cursor, "component entropy"),
        });
        loop {
            if input.get(cursor..cursor + 2) == Some(&[255, 217]) {
                if cursor + 2 != input.len() || result.len() != 3 {
                    return Err(Error("OJPEG planar scan cardinality/tail mismatch".into()));
                }
                return Ok(result);
            }
            if input.get(cursor) != Some(&255) {
                return Err(Error("OJPEG planar marker boundary".into()));
            }
            let marker = *input
                .get(cursor + 1)
                .ok_or_else(|| Error("truncated OJPEG planar marker".into()))?;
            if ![0xc4, 0xdb, 0xfe, 0xda].contains(&marker) && !(0xe0..=0xef).contains(&marker) {
                return Err(Error("unsupported OJPEG planar inter-scan marker".into()));
            }
            let size = input
                .get(cursor + 2..cursor + 4)
                .ok_or_else(|| Error("truncated OJPEG planar marker size".into()))?;
            let size = usize::from(u16::from_be_bytes([size[0], size[1]]));
            let end = cursor
                .checked_add(size + 2)
                .filter(|&n| size >= 2 && n <= input.len())
                .ok_or_else(|| Error("OJPEG planar marker extent".into()))?;
            metadata = metadata
                .checked_add(size + 66)
                .filter(|&n| n <= limits.metadata_bytes)
                .ok_or_else(|| Error("OJPEG planar headers exceed metadata limit".into()))?;
            if marker == 0xda {
                scan = input[cursor + 4..end].to_vec();
                header_start = cursor;
                start = end;
                break;
            }
            cursor = end;
        }
    }
}
