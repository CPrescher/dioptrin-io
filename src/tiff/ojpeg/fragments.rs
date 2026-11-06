//! Declared-window OJPEG fragment assembly. TIFF restart interpretation follows
//! libtiff OJPEGReadHeaderInfo/OJPEGWriteStreamRst; Huffman/IDCT stay in libjpeg.
use super::{
    Fragment, FrameSpec, Partition, Prepared, Reconstruction, SourceMetadata, SourceWindow, io,
    marker, read, scalar, table_header,
};
use crate::{
    source::Snapshot,
    tiff::{
        Error, Limits, Metadata,
        ycbcr::{Chunk, Plane},
    },
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

pub(super) fn prepare(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limits: Limits,
    offsets: Vec<u64>,
    lengths: Vec<u64>,
) -> Result<Prepared, Error> {
    let factors = super::super::ycbcr::subsampling(page)?;
    let tiled = page.tags.contains_key(&324);
    let rows = usize::try_from(scalar(
        page,
        if tiled { 323 } else { 278 },
        page.shape[0] as u64,
    )?)
    .map_err(|_| Error("OJPEG fragment height overflow".into()))?;
    let width = if tiled {
        usize::try_from(scalar(page, 322, 0)?)
            .map_err(|_| Error("OJPEG fragment width overflow".into()))?
    } else {
        page.shape[1]
    };
    if rows == 0
        || width == 0
        || width > 65535
        || rows > 65535
        || !rows.is_multiple_of(factors[1] * 8)
        || tiled && !width.is_multiple_of(factors[0] * 8)
    {
        return Err(Error(
            "fragmented OJPEG requires MCU-aligned rows and one strip/tile across".into(),
        ));
    }
    let across = if tiled {
        page.shape[1].div_ceil(width)
    } else {
        1
    };
    let count = page.shape[0]
        .div_ceil(rows)
        .checked_mul(across)
        .ok_or_else(|| Error("OJPEG fragment count overflow".into()))?;
    if count < 2 || offsets.len() != count || lengths.len() != count {
        return Err(Error(
            "fragmented OJPEG chunk count disagrees with source geometry".into(),
        ));
    }
    let height = if tiled {
        count
            .checked_mul(rows)
            .ok_or_else(|| Error("OJPEG padded height overflow".into()))?
    } else {
        page.shape[0]
    };
    if height > 65535 {
        return Err(Error(
            "OJPEG fragmented frame exceeds JPEG dimensions".into(),
        ));
    }
    let shape = [height, width];
    let restart = u16::try_from(
        width
            .div_ceil(factors[0] * 8)
            .checked_mul(rows / (factors[1] * 8))
            .ok_or_else(|| Error("OJPEG restart interval overflow".into()))?,
    )
    .map_err(|_| Error("OJPEG restart interval exceeds16bits".into()))?;
    let declared = scalar(page, 515, 0)?;
    if declared != 0 && declared != u64::from(restart) {
        return Err(Error(
            "OJPEG restart declaration contradicts fragment MCU geometry".into(),
        ));
    }
    let index_bytes = count
        .checked_mul(192)
        .filter(|&n| n <= limits.metadata_bytes)
        .ok_or_else(|| Error("OJPEG fragment index exceeds metadata limit".into()))?;
    let mut file = File::open(&snapshot.path).map_err(io)?;
    snapshot.check_handle(&file).map_err(Error)?;
    let source_len = file.metadata().map_err(io)?.len();
    let mut windows = Vec::new();
    let mut fragments = Vec::new();
    let mut source_total = 0usize;
    for (index, (&offset, &length)) in offsets.iter().zip(&lengths).enumerate() {
        let bytes =
            usize::try_from(length).map_err(|_| Error("OJPEG fragment length overflow".into()))?;
        if bytes == 0 || offset.checked_add(length).is_none_or(|n| n > source_len) {
            return Err(Error(
                "OJPEG fragment exceeds original source bounds".into(),
            ));
        }
        source_total = source_total
            .checked_add(bytes)
            .filter(|&n| n <= limits.intermediate_bytes / 5)
            .ok_or_else(|| Error("OJPEG source assembly exceeds intermediate limit".into()))?;
        windows.push(SourceWindow {
            offset,
            bytes,
            role: "encoded fragment",
        });
        fragments.push(Fragment {
            index,
            source_offset: offset,
            source_bytes: bytes,
            header_bytes: 0,
            entropy_offset: offset,
            entropy_bytes: bytes,
            origin: [index / across * rows, index % across * width],
            stream_origin: [index * rows, 0],
            image_shape: [
                rows.min(page.shape[0] - index / across * rows),
                width.min(page.shape[1] - index % across * width),
            ],
            encoded_shape: [
                if tiled {
                    rows
                } else {
                    rows.min(page.shape[0] - index * rows)
                },
                width,
            ],
        });
    }
    // Declared fragments must be disjoint, but may be physically out of order.
    let mut ranges: Vec<_> = offsets
        .iter()
        .zip(&lengths)
        .map(|(&o, &n)| (o, o + n))
        .collect();
    ranges.sort_unstable();
    if ranges.windows(2).any(|w| w[0].1 > w[1].0) {
        return Err(Error("overlapping OJPEG fragment windows".into()));
    }
    let mut first = read_fragment(&mut file, &fragments[0])?;
    let prefix = page
        .tags
        .get(&513)
        .map(|v| v.clone().into_u64())
        .transpose()?;
    if prefix.is_none() && page.tags.contains_key(&514) {
        return Err(Error("OJPEG interchange length lacks offset".into()));
    }
    let mut complete = false;
    let mut prefix_bytes = 0usize;
    let mut input = if let Some(offset) = prefix {
        let length = usize::try_from(scalar(page, 514, 0)?)
            .map_err(|_| Error("OJPEG interchange length overflow".into()))?;
        let _ = source_total
            .checked_add(length)
            .filter(|&n| n <= limits.intermediate_bytes / 5)
            .ok_or_else(|| Error("OJPEG source assembly exceeds intermediate limit".into()))?;
        let end = offset
            .checked_add(length as u64)
            .ok_or_else(|| Error("OJPEG interchange range overflow".into()))?;
        complete = ranges
            .iter()
            .all(|&(start, stop)| start >= offset && stop <= end);
        if !complete
            && ranges
                .iter()
                .any(|&(start, stop)| start < end && stop > offset)
        {
            return Err(Error(
                "ambiguous overlapping OJPEG interchange/fragments".into(),
            ));
        }
        let mut data = read(
            &mut file,
            source_len,
            offset,
            length,
            "JPEG interchange fragment",
            limits,
            &mut windows,
        )?;
        prefix_bytes = data.len();
        if !complete {
            data.append(&mut first);
        }
        data
    } else {
        std::mem::take(&mut first)
    };
    drop(first);
    let has_header = prefix.is_some() || input.starts_with(&[255, 216]);
    let mut original_header = Vec::new();
    let mut normalized = false;
    let mut encoded;
    let reconstruction;
    if has_header {
        let (segments, entropy) = if complete {
            crate::jpeg::inspect_dct_bytes(&input, limits.metadata_bytes)
        } else {
            crate::jpeg::inspect_dct_header_bytes(&input, limits.metadata_bytes)
        }
        .map_err(|e| Error(e.to_string()))?;
        let frame = segments
            .iter()
            .find(|s| s.marker == 0xc0)
            .ok_or_else(|| Error("fragmented OJPEG requires baselineSOF0".into()))?;
        if frame.payload.len() != 15
            || frame.payload[0] != 8
            || [
                usize::from(u16::from_be_bytes([frame.payload[1], frame.payload[2]])),
                usize::from(u16::from_be_bytes([frame.payload[3], frame.payload[4]])),
            ] != shape
        {
            return Err(Error(
                "OJPEG fragmented frame contradicts TIFF geometry".into(),
            ));
        }
        let scan = &segments
            .last()
            .ok_or_else(|| Error("OJPEG header lacks SOS".into()))?
            .payload;
        if scan.len() != 10
            || scan[0] != 3
            || scan[7] != 0
            || scan[9] != 0
            || ![0, 63].contains(&scan[8])
        {
            return Err(Error("unsupported fragmented OJPEG scan parameters".into()));
        }
        let declarations: Vec<_> = segments.iter().filter(|s| s.marker == 0xdd).collect();
        if declarations.len() > 1
            || declarations
                .first()
                .is_some_and(|s| s.payload.as_slice() != restart.to_be_bytes())
        {
            return Err(Error(
                "OJPEG header restart interval contradicts fragment geometry".into(),
            ));
        }
        original_header = input[..entropy].to_vec();
        if scan[8] == 0 {
            input[entropy - 2] = 63;
            normalized = true;
        }
        if complete {
            if declarations.is_empty() {
                return Err(Error(
                    "fragmented complete interchange lacks its restart declaration".into(),
                ));
            }
            let base = prefix.unwrap();
            let mut cursor = entropy;
            for fragment in &fragments {
                if fragment.source_offset != base + cursor as u64 {
                    return Err(Error(
                        "OJPEG interchange fragment order/start contradicts entropy boundaries"
                            .into(),
                    ));
                }
                cursor = cursor
                    .checked_add(fragment.source_bytes)
                    .filter(|&n| n <= input.len())
                    .ok_or_else(|| Error("OJPEG interchange fragment extent overflow".into()))?;
                if fragment.index + 1 < count {
                    if input.get(cursor..cursor + 2)
                        != Some(&[255, 0xd0 + (fragment.index % 8) as u8])
                    {
                        return Err(Error(
                            "OJPEG interchange fragment boundary lacks the expected restart".into(),
                        ));
                    }
                    cursor += 2;
                }
            }
            if input.get(cursor..) != Some(&[255, 217]) {
                return Err(Error(
                    "OJPEG interchange fragment windows do not cover exact entropy/EOI".into(),
                ));
            }
            encoded = input;
            reconstruction = Reconstruction::CompleteInterchange;
        } else {
            if entropy < prefix_bytes || entropy - prefix_bytes >= fragments[0].source_bytes {
                return Err(Error(
                    "OJPEG header/entropy fragment boundary is ambiguous".into(),
                ));
            }
            let consumed = entropy - prefix_bytes;
            fragments[0].header_bytes = consumed;
            fragments[0].entropy_offset += consumed as u64;
            fragments[0].entropy_bytes -= consumed;
            encoded = input[..entropy].to_vec();
            if declarations.is_empty() {
                let mut dri = Vec::new();
                marker(&mut dri, 0xdd, &restart.to_be_bytes(), limits)?;
                encoded.splice(2..2, dri);
            }
            reconstruction = Reconstruction::FragmentedInterchange;
            append_entropy(&mut encoded, &input[entropy..], false, limits)?;
            drop(input);
        }
    } else {
        if across > 1 {
            return Err(Error(
                "multi-across OJPEG tiles require explicit source SOF stacked geometry".into(),
            ));
        }
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
        reconstruction = Reconstruction::TableDefinedFragments;
        append_entropy(&mut encoded, &input, false, limits)?;
        drop(input);
    }
    if !complete {
        for (index, fragment) in fragments.iter_mut().enumerate().skip(1) {
            encoded.extend([255, 0xd0 + ((index - 1) % 8) as u8]);
            let source = read_fragment(&mut file, fragment)?;
            if index + 1 == count && source.ends_with(&[255, 217]) {
                fragment.entropy_bytes -= 2;
            }
            append_entropy(&mut encoded, &source, index + 1 == count, limits)?;
        }
        encoded.extend([255, 217]);
    }
    if encoded.len() > limits.intermediate_bytes / 2 {
        return Err(Error(
            "OJPEG reconstructed fragment stream exceeds scratch budget".into(),
        ));
    }
    let retained_bytes = windows
        .len()
        .checked_mul(64)
        .and_then(|n| n.checked_add(index_bytes))
        .and_then(|n| n.checked_add(original_header.len()))
        .filter(|&n| n <= limits.metadata_bytes)
        .ok_or_else(|| Error("OJPEG fragment provenance exceeds metadata budget".into()))?;
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
            restart_interval: restart,
            restart_markers_inserted: if complete { 0 } else { count - 1 },
            fragments,
            partition: Partition::RasterAligned,
            scan_sources: Vec::new(),
            tile_policy: super::super::OjpegTiles::Declared,
        },
    })
}
fn read_fragment(file: &mut File, fragment: &Fragment) -> Result<Vec<u8>, Error> {
    file.seek(SeekFrom::Start(fragment.source_offset))
        .map_err(io)?;
    let mut data = vec![0; fragment.source_bytes];
    file.read_exact(&mut data).map_err(io)?;
    Ok(data)
}
fn append_entropy(
    output: &mut Vec<u8>,
    source: &[u8],
    last: bool,
    limits: Limits,
) -> Result<(), Error> {
    let source = if last && source.ends_with(&[255, 217]) {
        &source[..source.len() - 2]
    } else {
        source
    };
    if source.is_empty() {
        return Err(Error("empty OJPEG entropy fragment".into()));
    }
    let mut i = 0;
    while i < source.len() {
        if source[i] == 255 {
            i += 1;
            if source.get(i) != Some(&0) {
                return Err(Error(
                    "unexpected OJPEG fragment marker/restart/tail".into(),
                ));
            }
        }
        i += 1;
    }
    let size = output
        .len()
        .checked_add(source.len())
        .and_then(|n| n.checked_add(2))
        .filter(|&n| n <= limits.intermediate_bytes / 2)
        .ok_or_else(|| Error("OJPEG fragment stream exceeds intermediate budget".into()))?;
    output
        .try_reserve_exact(size - output.len())
        .map_err(|e| Error(e.to_string()))?;
    output.extend_from_slice(source);
    Ok(())
}

pub(super) fn split(
    global: Chunk,
    fragments: &[Fragment],
    limits: Limits,
) -> Result<Vec<Chunk>, Error> {
    let total = global
        .planes
        .iter()
        .try_fold(0usize, |n, p| n.checked_add(p.samples.len()))
        .ok_or_else(|| Error("OJPEG component size overflow".into()))?;
    if total > limits.frame_bytes
        || total
            .checked_mul(2)
            .is_none_or(|n| n > limits.intermediate_bytes)
    {
        return Err(Error("OJPEG fragment component budget".into()));
    }
    let mut chunks = Vec::new();
    chunks
        .try_reserve_exact(fragments.len())
        .map_err(|e| Error(e.to_string()))?;
    for fragment in fragments {
        let mut result = Vec::new();
        for source in &global.planes {
            let start = fragment.stream_origin[0] / source.sampling[0];
            let rows = if fragment.index + 1 == fragments.len() {
                source.shape[0] - start
            } else {
                fragment.encoded_shape[0] / source.sampling[0]
            };
            let end = start
                .checked_add(rows)
                .filter(|&n| n <= source.shape[0])
                .ok_or_else(|| Error("OJPEG fragment component row bounds".into()))?;
            let samples = source.samples[start * source.shape[1]..end * source.shape[1]].to_vec();
            result.push(Plane {
                component: source.component,
                shape: [rows, source.shape[1]],
                valid_shape: [
                    fragment.image_shape[0].div_ceil(source.sampling[0]),
                    fragment.image_shape[1].div_ceil(source.sampling[1]),
                ],
                sampling: source.sampling,
                samples,
            });
        }
        chunks.push(Chunk {
            origin: fragment.origin,
            image_shape: fragment.image_shape,
            planes: result
                .try_into()
                .map_err(|_| Error("OJPEG component count mismatch".into()))?,
            sampling_units: None,
        });
    }
    Ok(chunks)
}

pub(super) fn check_decode_budget(
    metadata: &SourceMetadata,
    encoded: usize,
    limits: Limits,
) -> Result<(), Error> {
    let [h, v] = metadata.subsampling;
    let [rows, columns] = metadata.encoded_shape;
    let y = rows.div_ceil(v).checked_mul(v).and_then(|r| {
        columns
            .div_ceil(h)
            .checked_mul(h)
            .and_then(|c| r.checked_mul(c))
    });
    let uv = rows.div_ceil(v).checked_mul(columns.div_ceil(h));
    let samples = y
        .and_then(|y| {
            uv.and_then(|uv| uv.checked_mul(2))
                .and_then(|uv| y.checked_add(uv))
        })
        .filter(|&n| n <= limits.frame_bytes)
        .ok_or_else(|| Error("OJPEG fragmented component frame exceeds byte budget".into()))?;
    if samples
        .checked_mul(2)
        .and_then(|n| n.checked_add(encoded))
        .is_none_or(|n| n > limits.intermediate_bytes)
    {
        return Err(Error(
            "OJPEG encoded plus native/split component scratch exceeds intermediate budget".into(),
        ));
    }
    Ok(())
}
