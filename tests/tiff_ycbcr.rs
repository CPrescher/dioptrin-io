#![cfg(feature = "tiff")]
//! Independent CC0-1.0 TIFF fixture bytes/generator; Rust test code is MIT.
//! Literal LZW writes clear before every symbol, so the 9-bit source codes are
//! independent of both native decoder and upstream TIFF/Pillow writers.
use dioptrin_io::{
    PixelBuffer,
    tiff::{self, FrameBuffer, Limits, Reader, SampleLayout, ycbcr},
};
use std::fs;

fn literal_lzw(samples: &[u8]) -> Vec<u8> {
    let mut bits = Vec::new();
    for &v in samples {
        for code in [256, u16::from(v)] {
            for bit in (0..9).rev() {
                bits.push((code >> bit) & 1);
            }
        }
    }
    for bit in (0..9).rev() {
        bits.push((257 >> bit) & 1);
    }
    let mut out = vec![0; bits.len().div_ceil(8)];
    for (i, bit) in bits.into_iter().enumerate() {
        out[i / 8] |= (bit as u8) << (7 - i % 8);
    }
    out
}
fn fixture(
    width: u32,
    height: u32,
    rows: u32,
    tiled: bool,
    lzw: bool,
    units: &[Vec<u8>],
) -> Vec<u8> {
    let mut file = b"II*\0\0\0\0\0".to_vec();
    let mut offsets = Vec::new();
    let mut lengths = Vec::new();
    for unit in units {
        let data = if lzw { literal_lzw(unit) } else { unit.clone() };
        offsets.push(file.len() as u32);
        lengths.push(data.len() as u32);
        file.extend(data);
    }
    let mut tags = vec![
        (256, 4, vec![width]),
        (257, 4, vec![height]),
        (258, 3, vec![8, 8, 8]),
        (259, 3, vec![if lzw { 5 } else { 1 }]),
        (262, 3, vec![6]),
        (274, 3, vec![8]),
        (277, 3, vec![3]),
        (284, 3, vec![1]),
        (530, 3, vec![2, 2]),
        (531, 3, vec![1]),
    ];
    if tiled {
        tags.extend([
            (322, 4, vec![16]),
            (323, 4, vec![rows]),
            (324, 4, offsets),
            (325, 4, lengths),
        ]);
    } else {
        tags.extend([(273, 4, offsets), (278, 4, vec![rows]), (279, 4, lengths)]);
    }
    tags.sort_by_key(|v| v.0);
    let ifd = file.len();
    file[4..8].copy_from_slice(&(ifd as u32).to_le_bytes());
    file.extend((tags.len() as u16).to_le_bytes());
    file.resize(ifd + 2 + tags.len() * 12 + 4, 0);
    for (i, (tag, ty, values)) in tags.into_iter().enumerate() {
        let pos = ifd + 2 + i * 12;
        file[pos..pos + 2].copy_from_slice(&(tag as u16).to_le_bytes());
        file[pos + 2..pos + 4].copy_from_slice(&(ty as u16).to_le_bytes());
        file[pos + 4..pos + 8].copy_from_slice(&(values.len() as u32).to_le_bytes());
        let bytes: Vec<u8> = values
            .into_iter()
            .flat_map(|v| {
                if ty == 3 {
                    (v as u16).to_le_bytes().to_vec()
                } else {
                    v.to_le_bytes().to_vec()
                }
            })
            .collect();
        if bytes.len() <= 4 {
            file[pos + 8..pos + 8 + bytes.len()].copy_from_slice(&bytes);
        } else {
            let offset = file.len() as u32;
            file[pos + 8..pos + 12].copy_from_slice(&offset.to_le_bytes());
            file.extend(bytes);
        }
    }
    file
}
fn mutate_tag(file: &mut [u8], tag: u16, value: u32) {
    let ifd = u32::from_le_bytes(file[4..8].try_into().unwrap()) as usize;
    let n = u16::from_le_bytes(file[ifd..ifd + 2].try_into().unwrap()) as usize;
    let pos = (0..n)
        .map(|i| ifd + 2 + i * 12)
        .find(|&p| u16::from_le_bytes(file[p..p + 2].try_into().unwrap()) == tag)
        .unwrap();
    file[pos + 8..pos + 12].copy_from_slice(&value.to_le_bytes());
}
fn odd_units() -> Vec<Vec<u8>> {
    vec![
        vec![0, 1, 4, 5, 120, 130, 2, 255, 6, 249, 121, 131],
        vec![10, 11, 230, 231, 122, 132, 12, 248, 232, 233, 123, 133],
    ]
}

#[test]
fn independent_odd_strips_preserve_each_source_unit_plane_and_padding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("units.tif");
    for lzw in [false, true] {
        let units = odd_units();
        fs::write(&path, fixture(3, 3, 2, false, lzw, &units)).unwrap();
        let mut reader = Reader::open(&path, Limits::default()).unwrap();
        let meta = reader.metadata(0).unwrap();
        assert_eq!(
            meta.sample_layout,
            SampleLayout::YCbCrComponents {
                horizontal: 2,
                vertical: 2
            }
        );
        assert_eq!(meta.sample_shape(), vec![3, 3, 3]);
        assert_eq!(meta.uniform_sample_shape(), None);
        assert_eq!(meta.orientation, 8);
        assert_eq!(meta.bits_per_sample, 8);
        assert_eq!(meta.photometric_interpretation, 6);
        let mut ordinary = FrameBuffer::default();
        assert!(
            reader
                .read_frame_into(0, &mut ordinary)
                .unwrap_err()
                .to_string()
                .contains("read_ycbcr_into")
        );
        assert!(ordinary.pixels().is_none());
        let mut buffer = ycbcr::Buffer::default();
        reader.read_ycbcr_into(0, &mut buffer).unwrap();
        assert_eq!(buffer.shape(), Some([3, 3]));
        assert_eq!(buffer.subsampling(), Some([2, 2]));
        assert_eq!(buffer.precision(), Some(8));
        assert_eq!(buffer.storage(), Some(ycbcr::Storage::TiffSamplingUnits));
        let chunks = buffer.chunks().unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].origin, [0, 0]);
        assert_eq!(chunks[1].origin, [2, 0]);
        assert_eq!(chunks[0].image_shape, [2, 3]);
        assert_eq!(chunks[1].image_shape, [1, 3]);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.sampling_units.as_ref(), Some(&units[i]));
            assert_eq!(chunk.planes[0].shape, [2, 4]);
            assert_eq!(chunk.planes[0].valid_shape, chunk.image_shape);
            assert_eq!(chunk.planes[1].shape, [1, 2]);
            assert_eq!(chunk.planes[1].valid_shape, [1, 2]);
            assert_eq!(chunk.planes[1].sampling, [2, 2]);
        }
        assert_eq!(chunks[0].planes[0].samples, [0, 1, 2, 255, 4, 5, 6, 249]);
        assert_eq!(
            chunks[1].planes[0].samples,
            [10, 11, 12, 248, 230, 231, 232, 233]
        );
        assert_eq!(chunks[1].planes[1].samples, [122, 123]);
        assert_eq!(chunks[1].planes[2].samples, [132, 133]);
        let image =
            tiff::view::render_ycbcr_rgb472(reader.metadata(0).unwrap(), &buffer, 1024).unwrap();
        assert_eq!(image.provenance.shape, [3, 3, 3]);
        assert_eq!(image.provenance.source_shape, [3, 3]);
        assert_eq!(
            image.provenance.source_layout,
            reader.metadata(0).unwrap().sample_layout
        );
        assert!(matches!(
            image.pixels,
            tiff::view::Pixels::Scalar(PixelBuffer::U8(_))
        ));
        assert!(
            tiff::view::convert_ycbcr(
                reader.metadata(0).unwrap(),
                &buffer,
                tiff::view::Policy::Pillow13,
                1
            )
            .is_err()
        );
        fs::write(&path, b"changed").unwrap();
        assert!(reader.read_ycbcr_into(0, &mut buffer).is_err());
        assert!(buffer.chunks().is_none());
        assert!(buffer.storage().is_none());
        assert!(buffer.precision().is_none());
    }
}
#[test]
fn independent_edge_tiles_retain_full_native_padded_grids() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tiles.tif");
    let units: Vec<Vec<u8>> = (0..4)
        .map(|i| (0..384).map(|n| (n + i * 43) as u8).collect())
        .collect();
    fs::write(&path, fixture(17, 17, 16, true, true, &units)).unwrap();
    let mut reader = Reader::open(&path, Limits::default()).unwrap();
    let mut out = ycbcr::Buffer::default();
    reader.read_ycbcr_into(0, &mut out).unwrap();
    for (i, chunk) in out.chunks().unwrap().iter().enumerate() {
        assert_eq!(chunk.origin, [(i / 2) * 16, (i % 2) * 16]);
        assert_eq!(chunk.sampling_units.as_ref(), Some(&units[i]));
        assert_eq!(chunk.planes[0].shape, [16, 16]);
        assert_eq!(chunk.planes[1].shape, [8, 8]);
        assert_eq!(chunk.planes[2].shape, [8, 8]);
    }
    assert_eq!(out.chunks().unwrap()[3].image_shape, [1, 1]);
    assert_eq!(out.chunks().unwrap()[3].planes[1].valid_shape, [1, 1]);
}
#[test]
fn full_nominal_last_strip_padding_is_retained_even_when_nonzero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("full-last-strip.tif");
    for lzw in [false, true] {
        let units: Vec<Vec<u8>> = (0..2)
            .map(|i| (0..24).map(|j| (i * 73 + j * 7) as u8).collect())
            .collect();
        fs::write(&path, fixture(3, 5, 4, false, lzw, &units)).unwrap();
        let mut reader = Reader::open(&path, Limits::default()).unwrap();
        let mut out = ycbcr::Buffer::default();
        reader.read_ycbcr_into(0, &mut out).unwrap();
        let last = &out.chunks().unwrap()[1];
        assert_eq!(last.image_shape, [1, 3]);
        assert_eq!(last.planes[0].shape, [4, 4]);
        assert_eq!(last.planes[1].shape, [2, 2]);
        assert_eq!(last.planes[1].valid_shape, [1, 2]);
        assert_eq!(last.sampling_units.as_ref(), Some(&units[1]));
        assert!(last.planes[0].samples[4..].iter().any(|&v| v != 0));
    }
    // A complete codec stream with an intermediate arbitrary unit count is not
    // interpreted as another physical strip geometry.
    let units = vec![vec![7; 24], vec![9; 18]];
    fs::write(&path, fixture(3, 5, 4, false, true, &units)).unwrap();
    let mut reader = Reader::open(&path, Limits::default()).unwrap();
    let mut out = ycbcr::Buffer::default();
    assert!(reader.read_ycbcr_into(0, &mut out).is_err());
    assert!(out.chunks().is_none());
}

#[test]
fn explicit_neutral_chroma_rgb_view_has_known_counts_without_orientation_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gray-units.tif");
    let units = vec![vec![0, 255, 10, 20, 128, 128, 100, 77, 200, 66, 128, 128]];
    fs::write(&path, fixture(3, 2, 2, false, true, &units)).unwrap();
    let mut reader = Reader::open(&path, Limits::default()).unwrap();
    let mut out = ycbcr::Buffer::default();
    reader.read_ycbcr_into(0, &mut out).unwrap();
    let image = tiff::view::render_ycbcr_rgb472(reader.metadata(0).unwrap(), &out, 100).unwrap();
    let tiff::view::Pixels::Scalar(PixelBuffer::U8(rgb)) = image.pixels else {
        panic!("RGB8 required")
    };
    assert_eq!(
        rgb,
        [
            0, 0, 0, 255, 255, 255, 100, 100, 100, 10, 10, 10, 20, 20, 20, 200, 200, 200
        ]
    );
    assert_eq!(image.provenance.source_orientation, 8);
    assert_eq!(image.provenance.shape, [2, 3, 3]);
    assert_eq!(
        out.chunks().unwrap()[0].sampling_units.as_ref(),
        Some(&units[0])
    );
}
#[test]
fn bounded_layouts_and_corrupt_lzw_never_expose_partial_components() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.tif");
    let original = fixture(3, 3, 2, false, true, &odd_units());
    for (tag, value) in [(530, 0), (530, 0x0004_0002), (531, 3), (284, 2), (278, 1)] {
        let mut data = original.clone();
        mutate_tag(&mut data, tag, value);
        fs::write(&path, data).unwrap();
        assert!(Reader::open(&path, Limits::default()).is_err(), "tag{tag}");
    }
    fs::write(&path, &original).unwrap();
    for limits in [
        Limits {
            frame_bytes: 47,
            ..Limits::default()
        },
        Limits {
            intermediate_bytes: 12,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 10,
            ..Limits::default()
        },
    ] {
        assert!(Reader::open(&path, limits).is_err());
    }
    let mut buffer = ycbcr::Buffer::default();
    let mut good = Reader::open(&path, Limits::default()).unwrap();
    good.read_ycbcr_into(0, &mut buffer).unwrap();
    // Structurally complete TIFF; first strip's literal clear/end stream is broken.
    let mut corrupt = original.clone();
    corrupt[8] = 0xff;
    fs::write(&path, corrupt).unwrap();
    let mut reader = Reader::open(&path, Limits::default()).unwrap();
    assert!(reader.read_ycbcr_into(0, &mut buffer).is_err());
    assert!(buffer.chunks().is_none());
    assert!(buffer.shape().is_none());
    assert!(reader.read_ycbcr_into(99, &mut buffer).is_err());
    let ordinary =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/images/rgb-contig.tif");
    let mut reader = Reader::open(ordinary, Limits::default()).unwrap();
    assert!(reader.read_ycbcr_into(0, &mut buffer).is_err());
    assert!(buffer.chunks().is_none());
}

#[test]
fn lzw_requires_real_end_marker_exact_output_and_no_ignored_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stream.tif");
    let mut cases = Vec::new();
    let original = fixture(3, 3, 2, false, true, &odd_units());
    let mut missing = original.clone();
    // StripByteCounts is a two-value LONG payload; shorten the first window by
    // one byte while retaining a structurally complete source and second strip.
    let ifd = u32::from_le_bytes(missing[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(missing[ifd..ifd + 2].try_into().unwrap()) as usize;
    let pos = (0..count)
        .map(|i| ifd + 2 + i * 12)
        .find(|&p| u16::from_le_bytes(missing[p..p + 2].try_into().unwrap()) == 279)
        .unwrap();
    let payload = u32::from_le_bytes(missing[pos + 8..pos + 12].try_into().unwrap()) as usize;
    let length = u32::from_le_bytes(missing[payload..payload + 4].try_into().unwrap());
    missing[payload..payload + 4].copy_from_slice(&(length - 1).to_le_bytes());
    cases.push(missing);
    let mut tail = original.clone();
    tail[payload..payload + 4].copy_from_slice(&(length + 1).to_le_bytes());
    cases.push(tail);
    let mut long = odd_units();
    long[0].push(234);
    cases.push(fixture(3, 3, 2, false, true, &long));
    let mut short = odd_units();
    short[0].pop();
    cases.push(fixture(3, 3, 2, false, true, &short));
    for data in cases {
        fs::write(&path, data).unwrap();
        let mut reader = Reader::open(&path, Limits::default()).unwrap();
        let mut out = ycbcr::Buffer::default();
        assert!(reader.read_ycbcr_into(0, &mut out).is_err());
        assert!(out.chunks().is_none());
    }
}
