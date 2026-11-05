#![cfg(feature = "tiff")]
//! Independent CC0 byte-level TIFF admission regressions. The inline writer
//! constructs source rows directly and does not use the production decoder.
use dioptrin_io::{
    PixelBuffer,
    tiff::{FrameBuffer, Limits, Reader},
};
use std::io::Write;

fn file(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file
}
fn entry(bytes: &[u8], tag: u16) -> usize {
    let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
    (0..count)
        .map(|i| ifd + 2 + 12 * i)
        .find(|&p| bytes[p..p + 2] == tag.to_le_bytes())
        .unwrap()
}
fn set(bytes: &mut [u8], tag: u16, value: u32) {
    let p = entry(bytes, tag) + 8;
    bytes[p..p + 4].copy_from_slice(&value.to_le_bytes());
}
// All entries inline, one strip or tile. Rows are supplied in stored order.
fn image(width: u32, height: u32, bits: u16, tile: bool, payload: &[u8]) -> Vec<u8> {
    let mut tags = vec![
        (256u16, 4u16, width),
        (257, 4, height),
        (258, 3, u32::from(bits)),
        (259, 3, 1),
        (262, 3, 0),
        (277, 3, 1),
    ];
    if tile {
        tags.extend([
            (322, 4, 4),
            (323, 4, 4),
            (324, 4, 0),
            (325, 4, payload.len() as u32),
        ]);
    } else {
        tags.extend([
            (273, 4, 0),
            (278, 4, height),
            (279, 4, payload.len() as u32),
        ]);
    }
    tags.sort_unstable_by_key(|tag| tag.0);
    let start = 8 + 2 + tags.len() * 12 + 4;
    let mut bytes = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
    bytes.extend_from_slice(&(tags.len() as u16).to_le_bytes());
    for (tag, ty, mut value) in tags {
        if tag == 273 || tag == 324 {
            value = start as u32;
        }
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&ty.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}
fn decode(bytes: &[u8]) -> Result<PixelBuffer, String> {
    let source = file(bytes);
    let mut reader = Reader::open(source.path(), Limits::default()).map_err(|e| e.to_string())?;
    let mut frame = FrameBuffer::default();
    reader
        .read_frame_into(0, &mut frame)
        .map_err(|e| e.to_string())?;
    Ok(frame.pixels().unwrap().clone())
}
#[test]
fn short_declared_window_cannot_read_adapted_header_as_samples() {
    let mut bytes = image(3, 2, 8, false, &[1, 2, 3, 4, 5, 6]);
    let photo = entry(&bytes, 262) + 8;
    // Source byte window ends immediately before the adapted photometric value.
    // The old None codec read six bytes despite this one-byte declaration and
    // silently exposed the virtual BlackIsZero header value as detector counts.
    set(&mut bytes, 273, photo as u32 - 1);
    set(&mut bytes, 279, 1);
    assert!(
        decode(&bytes)
            .unwrap_err()
            .contains("shorter than required source rows")
    );
}
#[test]
fn uncompressed_windows_count_packed_rows_and_source_float16_bytes() {
    let mut bytes = image(1, 2, 1, false, &[0x80, 0]);
    assert_eq!(decode(&bytes).unwrap(), PixelBuffer::U8(vec![1, 0]));
    set(&mut bytes, 279, 1);
    assert!(
        decode(&bytes)
            .unwrap_err()
            .contains("shorter than required source rows")
    );

    let mut bytes = image(2, 1, 16, false, &[0, 0x3c, 0, 0x40]);
    // Add SampleFormat=IEEEFP, keeping all descriptor/payload offsets correct.
    let source = file(&bytes);
    let mut old = Reader::open(source.path(), Limits::default()).unwrap();
    let mut frame = FrameBuffer::default();
    old.read_frame_into(0, &mut frame).unwrap();
    assert_eq!(
        frame.pixels(),
        Some(&PixelBuffer::U16(vec![0x3c00, 0x4000]))
    );
    // Change an existing harmless RowsPerStrip descriptor to SampleFormat;
    // absent RowsPerStrip defaults to the full image height.
    let p = entry(&bytes, 278);
    bytes[p..p + 2].copy_from_slice(&339u16.to_le_bytes());
    bytes[p + 2..p + 4].copy_from_slice(&3u16.to_le_bytes());
    bytes[p + 8..p + 12].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(decode(&bytes).unwrap(), PixelBuffer::F32(vec![1.0, 2.0]));
    set(&mut bytes, 279, 3);
    assert!(
        decode(&bytes)
            .unwrap_err()
            .contains("shorter than required source rows")
    );
}
#[test]
fn tile_windows_include_horizontal_padding_and_actual_bottom_rows() {
    let mut bytes = image(3, 2, 8, true, &[1, 2, 3, 99, 4, 5, 6, 99]);
    assert_eq!(
        decode(&bytes).unwrap(),
        PixelBuffer::U8(vec![1, 2, 3, 4, 5, 6])
    );
    set(&mut bytes, 325, 7);
    assert!(
        decode(&bytes)
            .unwrap_err()
            .contains("shorter than required source rows")
    );
}
#[test]
fn every_codec_stays_inside_declared_windows_and_resets_after_failure() {
    // RFC1950 zlib stream for original bytes [1,2,3,4,5,6], generated once
    // independently with Python's standard-library zlib.compress.
    let stream = [120, 156, 99, 100, 98, 102, 97, 101, 3, 0, 0, 62, 0, 22];
    for compression in [8, 32946] {
        let mut bytes = image(3, 2, 8, false, &stream);
        set(&mut bytes, 259, compression);
        assert_eq!(
            decode(&bytes).unwrap(),
            PixelBuffer::U8(vec![1, 2, 3, 4, 5, 6])
        );
        // Remaining source bytes exist, but are explicitly outside this chunk.
        set(&mut bytes, 279, 3);
        let source = file(&bytes);
        let mut reader = Reader::open(source.path(), Limits::default()).unwrap();
        let mut frame = FrameBuffer::default();
        for _ in 0..2 {
            assert!(reader.read_frame_into(0, &mut frame).is_err());
            assert!(frame.pixels().is_none());
        }
    }
}
#[test]
fn declared_windows_must_fit_source_even_if_samples_would_fit() {
    let mut bytes = image(3, 2, 8, false, &[1, 2, 3, 4, 5, 6]);
    set(&mut bytes, 279, 7);
    assert!(decode(&bytes).unwrap_err().contains("source bounds"));
}
#[test]
fn deflate_pixel_decode_does_not_claim_footer_integrity_validation() {
    // All six source samples precede the final four-byte Adler32 footer. The
    // mature backend requests those samples only; it does not drain the stream
    // to validate the footer. This is a deliberate integrity qualification,
    // separate from the enforced prohibition on reading beyond chunk bounds.
    let stream_without_footer = [120, 156, 99, 100, 98, 102, 97, 101, 3, 0];
    for compression in [8, 32946] {
        let mut bytes = image(3, 2, 8, false, &stream_without_footer);
        set(&mut bytes, 259, compression);
        assert_eq!(
            decode(&bytes).unwrap(),
            PixelBuffer::U8(vec![1, 2, 3, 4, 5, 6])
        );
        // Even an invalid footer physically present outside the declared
        // window cannot enter the bounded codec reader or change these counts.
        bytes.extend_from_slice(&[255; 4]);
        assert_eq!(
            decode(&bytes).unwrap(),
            PixelBuffer::U8(vec![1, 2, 3, 4, 5, 6])
        );
    }
}
