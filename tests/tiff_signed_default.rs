#![cfg(feature = "tiff")]
//! Varex ngMultiXRD writes signed, dark-subtracted 32-bit counts without a
//! SampleFormat tag. Only that writer is read as signed; other untagged 32-bit
//! TIFFs keep the TIFF/FabIO default (unsigned), and a declared format wins.
use dioptrin_io::{
    PixelBuffer, PixelType,
    tiff::{FrameBuffer, Limits, Reader},
};
use std::io::Write;

/// Uncompressed little-endian 2×2 grayscale TIFF with 32-bit samples.
fn tiff(
    samples: [u32; 4],
    sample_format: Option<u16>,
    software: Option<&str>,
) -> tempfile::NamedTempFile {
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    const ASCII: u16 = 2;
    let mut entries: Vec<(u16, u16, u32)> = vec![
        (256, LONG, 2),
        (257, LONG, 2),
        (258, SHORT, 32),
        (259, SHORT, 1),
        (262, SHORT, 1),
        (273, LONG, 0), // strip offset, patched below
        (277, SHORT, 1),
        (278, LONG, 2),
        (279, LONG, 16),
    ];
    // Software (ASCII, tag 305) sorts between 279 and 339; its bytes follow the IFD.
    let software = software.map(|s| format!("{s}\0").into_bytes());
    if let Some(text) = &software {
        entries.push((305, ASCII, text.len() as u32));
    }
    if let Some(format) = sample_format {
        entries.push((339, SHORT, u32::from(format)));
    }
    let ifd_end = 8 + 2 + 12 * entries.len() as u32 + 4;
    let text_len = software.as_ref().map_or(0, |t| t.len() as u32);
    let data_offset = ifd_end + text_len;
    let mut bytes = b"II*\0".to_vec();
    bytes.extend(8u32.to_le_bytes());
    bytes.extend((entries.len() as u16).to_le_bytes());
    for (tag, kind, value) in entries {
        let value = if tag == 273 { data_offset } else { value };
        bytes.extend(tag.to_le_bytes());
        bytes.extend(kind.to_le_bytes());
        if kind == ASCII {
            // Count is the string length; the value field points past the IFD.
            bytes.extend(value.to_le_bytes());
            bytes.extend(ifd_end.to_le_bytes());
            continue;
        }
        bytes.extend(1u32.to_le_bytes());
        if kind == SHORT {
            bytes.extend((value as u16).to_le_bytes());
            bytes.extend([0, 0]);
        } else {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes.extend(0u32.to_le_bytes());
    if let Some(text) = &software {
        bytes.extend(text);
    }
    for sample in samples {
        bytes.extend(sample.to_le_bytes());
    }
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(&bytes).unwrap();
    file
}

fn read(file: &tempfile::NamedTempFile) -> (PixelType, PixelBuffer) {
    let mut reader = Reader::open(file.path(), Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    (
        reader.metadata(0).unwrap().pixel_type,
        buffer.pixels().unwrap().clone(),
    )
}

const STORED: [u32; 4] = [1272, (-12672i32) as u32, u32::MAX, 118_475];
const SIGNED: [i32; 4] = [1272, -12672, -1, 118_475];
const VAREX: &str = "ngMultiXRD4343 V0.8.24-beta";

#[test]
fn untagged_varex_32_bit_integers_are_signed() {
    let (pixel_type, pixels) = read(&tiff(STORED, None, Some(VAREX)));
    assert_eq!(pixel_type, PixelType::I32);
    assert_eq!(pixels, PixelBuffer::I32(SIGNED.to_vec()));
}

#[test]
fn other_untagged_32_bit_integers_stay_unsigned() {
    for software in [None, Some("tifffile.py"), Some("ImageJ")] {
        let (pixel_type, pixels) = read(&tiff(STORED, None, software));
        assert_eq!(pixel_type, PixelType::U32, "{software:?}");
        assert_eq!(pixels, PixelBuffer::U32(STORED.to_vec()));
    }
}

#[test]
fn declared_sample_formats_win_for_every_writer() {
    for software in [None, Some(VAREX)] {
        let (pixel_type, pixels) = read(&tiff(STORED, Some(1), software));
        assert_eq!(pixel_type, PixelType::U32);
        assert_eq!(pixels, PixelBuffer::U32(STORED.to_vec()));
        let (pixel_type, pixels) = read(&tiff(STORED, Some(2), software));
        assert_eq!(pixel_type, PixelType::I32);
        assert_eq!(pixels, PixelBuffer::I32(SIGNED.to_vec()));
    }
}
