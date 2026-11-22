#![cfg(feature = "tiff")]
//! 32-bit integer TIFFs without SampleFormat (Varex/PerkinElmer flat-panel
//! software) hold signed, dark-subtracted counts. PIL reads them as int32; an
//! explicit SampleFormat keeps the declared interpretation.
use dioptrin_io::{
    PixelBuffer, PixelType,
    tiff::{FrameBuffer, Limits, Reader},
};
use std::io::Write;

/// Uncompressed little-endian 2×2 grayscale TIFF with 32-bit samples.
fn tiff(samples: [u32; 4], sample_format: Option<u16>) -> tempfile::NamedTempFile {
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
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
    if let Some(format) = sample_format {
        entries.push((339, SHORT, u32::from(format)));
    }
    let data_offset = 8 + 2 + 12 * entries.len() as u32 + 4;
    let mut bytes = b"II*\0".to_vec();
    bytes.extend(8u32.to_le_bytes());
    bytes.extend((entries.len() as u16).to_le_bytes());
    for (tag, kind, value) in entries {
        let value = if tag == 273 { data_offset } else { value };
        bytes.extend(tag.to_le_bytes());
        bytes.extend(kind.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        if kind == SHORT {
            bytes.extend((value as u16).to_le_bytes());
            bytes.extend([0, 0]);
        } else {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes.extend(0u32.to_le_bytes());
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

#[test]
fn untagged_32_bit_integers_are_signed_like_pil() {
    let stored = [1272, (-12672i32) as u32, u32::MAX, 118_475];
    let (pixel_type, pixels) = read(&tiff(stored, None));
    assert_eq!(pixel_type, PixelType::I32);
    assert_eq!(pixels, PixelBuffer::I32(vec![1272, -12672, -1, 118_475]));
}

#[test]
fn declared_sample_formats_are_kept() {
    let stored = [1272, (-12672i32) as u32, u32::MAX, 118_475];
    let (pixel_type, pixels) = read(&tiff(stored, Some(1)));
    assert_eq!(pixel_type, PixelType::U32);
    assert_eq!(pixels, PixelBuffer::U32(stored.to_vec()));
    let (pixel_type, pixels) = read(&tiff(stored, Some(2)));
    assert_eq!(pixel_type, PixelType::I32);
    assert_eq!(pixels, PixelBuffer::I32(vec![1272, -12672, -1, 118_475]));
}
