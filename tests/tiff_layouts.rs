#![cfg(feature = "tiff")]
use dioptrin_io::{
    PixelBuffer,
    tiff::{FrameBuffer, Limits, Options, Reader, directories::Catalog},
};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/tiff-layouts")
        .join(name)
}
fn file(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(bytes).unwrap();
    f
}
#[test]
fn independent_libtiff_mixed_group3_rle_source_bits_and_orientation() {
    for compression in [2, 3] {
        for options in if compression == 2 {
            vec![0]
        } else {
            vec![1, 5]
        } {
            for fill in [1, 2] {
                let name = format!("fax-{compression}-options{options}-fill{fill}.tif");
                let original = std::fs::read(fixture(&name)).unwrap();
                let mut reader = Reader::open(fixture(&name), Limits::default()).unwrap();
                let mut buffer = FrameBuffer::default();
                reader
                    .read_frame_into(0, &mut buffer)
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(
                    buffer.pixels(),
                    Some(&PixelBuffer::U8(
                        std::fs::read(fixture(&(name.clone() + ".raw"))).unwrap()
                    )),
                    "{name}"
                );
                let metadata = reader.metadata(0).unwrap();
                assert_eq!(metadata.shape, [17, 93]);
                assert_eq!(metadata.bits_per_sample, 1);
                assert_eq!(metadata.compression, compression);
                assert_eq!(metadata.fill_order, fill);
                assert_eq!(std::fs::read(fixture(&name)).unwrap(), original);
            }
        }
    }
}

#[test]
fn subifds_are_explicit_source_typed_and_never_promoted() {
    for name in ["subifds-classic.tif", "subifds-big-be.tif"] {
        let path = fixture(name);
        let catalog = Catalog::open(&path, Limits::default()).unwrap();
        let nodes = catalog.directories().unwrap();
        assert_eq!(nodes.len(), 4);
        assert_eq!(catalog.primary_offsets().unwrap().len(), 2);
        let parent = nodes.iter().find(|n| n.primary_index == Some(0)).unwrap();
        assert_eq!(parent.subifd_offsets.len(), 2);
        assert_eq!(parent.subifd_entry.as_ref().unwrap().count, 2);
        let mut ordinary = Reader::open(&path, Limits::default()).unwrap();
        assert_eq!(ordinary.frames(), 2);
        let mut buffer = FrameBuffer::default();
        ordinary.read_frame_into(1, &mut buffer).unwrap();
        assert_eq!(
            buffer.pixels(),
            Some(&PixelBuffer::I16(vec![-3, -2, -1, 0, 1, 2]))
        );
        for (index, expected) in [
            PixelBuffer::U16(vec![0, 10001, 20002, 30003, 40004, 50005]),
            PixelBuffer::F32(vec![0.5, -1.5, 7.25, 0.0]),
        ]
        .into_iter()
        .enumerate()
        {
            let offset = parent.subifd_offsets[index];
            let child = nodes.iter().find(|n| n.ifd_offset == offset).unwrap();
            assert_eq!(child.primary_index, None);
            let mut reader =
                Reader::open_directory(&path, offset, Limits::default(), Options::default())
                    .unwrap();
            assert_eq!(reader.frames(), 1);
            reader.read_frame_into(0, &mut buffer).unwrap();
            assert_eq!(buffer.pixels(), Some(&expected));
            assert!(reader.read_frame_into(1, &mut buffer).is_err());
            assert!(buffer.pixels().is_none());
        }
        assert!(Reader::open_directory(&path, 0, Limits::default(), Options::default()).is_err());
        assert!(
            Reader::open_directory(
                &path,
                parent.ifd_offset + 2,
                Limits::default(),
                Options::default()
            )
            .is_err()
        );
        let limited = Limits {
            frames: 2,
            ..Limits::default()
        };
        assert!(Reader::open(&path, limited).is_ok());
        assert!(Catalog::open(&path, limited).is_err());
    }
}
fn tag_at(bytes: &[u8], ifd: usize, tag: u16) -> usize {
    let count = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
    (0..count)
        .map(|n| ifd + 2 + n * 12)
        .find(|&p| bytes[p..p + 2] == tag.to_le_bytes())
        .unwrap()
}
fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn get32(bytes: &[u8], offset: usize) -> usize {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}
fn next_at(bytes: &[u8], ifd: usize) -> usize {
    ifd + 2 + 12 * usize::from(u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()))
}
#[test]
fn supported_child_does_not_require_supported_primary_codec() {
    let mut bytes = std::fs::read(fixture("subifds-classic.tif")).unwrap();
    let root = get32(&bytes, 4);
    let child_table = get32(&bytes, tag_at(&bytes, root, 330) + 8);
    let child = get32(&bytes, child_table);
    let compression = tag_at(&bytes, root, 259) + 8;
    put32(&mut bytes, compression, 65000);
    let f = file(&bytes);
    let c = Catalog::open(f.path(), Limits::default()).unwrap();
    assert_eq!(c.directories().unwrap().len(), 4);
    assert!(Reader::open(f.path(), Limits::default()).is_err());
    let mut reader = Reader::open_directory(
        f.path(),
        child as u64,
        Limits::default(),
        Options::default(),
    )
    .unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::U16(vec![
            0, 10001, 20002, 30003, 40004, 50005
        ]))
    );
}
#[test]
fn graph_aliases_cycles_overlap_bounds_and_source_changes_are_explicit() {
    let original = std::fs::read(fixture("subifds-classic.tif")).unwrap();
    let root = get32(&original, 4);
    let table = get32(&original, tag_at(&original, root, 330) + 8);
    let child = get32(&original, table);
    // Two source slots may explicitly alias the same child without creating a cycle.
    let mut alias = original.clone();
    put32(&mut alias, table + 4, child as u32);
    let f = file(&alias);
    let c = Catalog::open(f.path(), Limits::default()).unwrap();
    assert_eq!(c.directories().unwrap().len(), 3);
    let node = c
        .directories()
        .unwrap()
        .iter()
        .find(|n| n.primary_index == Some(0))
        .unwrap();
    assert_eq!(node.subifd_offsets, [child as u64, child as u64]);
    for (position, value) in [
        (table, root as u32),
        (table, (root + 2) as u32),
        (table, u32::MAX),
        (next_at(&original, child), root as u32),
    ] {
        let mut bad = original.clone();
        put32(&mut bad, position, value);
        let f = file(&bad);
        assert!(
            Catalog::open(f.path(), Limits::default()).is_err(),
            "position{position} value{value}"
        );
        assert!(
            Reader::open_directory(
                f.path(),
                child as u64,
                Limits::default(),
                Options::default()
            )
            .is_err()
        );
    }
    let f = file(&original);
    let c = Catalog::open(f.path(), Limits::default()).unwrap();
    let mut r = Reader::open_directory(
        f.path(),
        child as u64,
        Limits::default(),
        Options::default(),
    )
    .unwrap();
    let mut buffer = FrameBuffer::default();
    r.read_frame_into(0, &mut buffer).unwrap();
    std::fs::write(f.path(), [0; 16]).unwrap();
    assert!(c.directories().is_err());
    assert!(c.primary_offsets().is_err());
    assert!(r.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
fn bilevel(compression: u32, options: u32, height: u32, bits: &str) -> Vec<u8> {
    let tags = [
        (256, 4, 3),
        (257, 4, height),
        (258, 3, 1),
        (259, 3, compression),
        (262, 3, 0),
        (266, 3, 1),
        (273, 4, 8 + 2 + 11 * 12 + 4),
        (277, 3, 1),
        (278, 4, height),
        (279, 4, bits.len().div_ceil(8) as u32),
        (292, 4, options),
    ];
    let mut bytes = b"II*\0\x08\0\0\0".to_vec();
    bytes.extend(11u16.to_le_bytes());
    for (tag, kind, value) in tags {
        bytes.extend((tag as u16).to_le_bytes());
        bytes.extend((kind as u16).to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0u32.to_le_bytes());
    let mut bits = bits.to_string();
    bits.extend(std::iter::repeat_n('0', (8 - bits.len() % 8) % 8));
    for chunk in bits.as_bytes().chunks(8) {
        bytes.push(
            chunk
                .iter()
                .fold(0, |n, &bit| n * 2 + u8::from(bit == b'1')),
        );
    }
    bytes
}
fn decoded(bytes: &[u8]) -> Result<PixelBuffer, String> {
    let f = file(bytes);
    let mut reader = Reader::open(f.path(), Limits::default()).map_err(|e| e.to_string())?;
    let mut output = FrameBuffer::default();
    reader
        .read_frame_into(0, &mut output)
        .map_err(|e| e.to_string())?;
    Ok(output.pixels().unwrap().clone())
}
#[test]
fn strict_mixed_group3_tags_row_modes_eol_rtc_and_truncation() {
    let eol = "000000000001";
    let first = format!("{eol}11000");
    let second = format!("{eol}01");
    assert_eq!(
        decoded(&bilevel(3, 1, 2, &format!("{first}{second}"))).unwrap(),
        PixelBuffer::U8(vec![0; 6])
    );
    assert_eq!(
        decoded(&bilevel(
            3,
            1,
            2,
            &format!("{first}{second}{}", format!("{eol}1").repeat(6))
        ))
        .unwrap(),
        PixelBuffer::U8(vec![0; 6])
    );
    for (rows, bits) in [
        (1, format!("{eol}01")),
        (2, first.clone()),
        (2, format!("{first}{eol}0011")),
        (2, format!("{first}{eol}00000001000")),
        (2, format!("{first}0000000001")),
        (2, format!("{first}{second}1")),
        (2, format!("{first}{second}{eol}0")),
        (2, format!("{first}{second}{}", format!("{eol}1").repeat(2))),
    ] {
        assert!(decoded(&bilevel(3, 1, rows, &bits)).is_err(), "{bits}");
    }
    for options in [2, 3, 6, 7, 8] {
        assert!(decoded(&bilevel(3, options, 1, &first)).is_err());
    }
    let aligned = format!("000{eol}110000000000{eol}01");
    assert_eq!(
        decoded(&bilevel(3, 5, 2, &aligned)).unwrap(),
        PixelBuffer::U8(vec![0; 6])
    );
    assert!(decoded(&bilevel(3, 5, 1, &first)).is_err());
    // A 1D strip cannot acquire a mixed-mode tag by changing only options.
    assert!(decoded(&bilevel(3, 0, 1, &first)).is_err());
}
#[test]
fn strict_ccitt_rle_byte_rows_do_not_read_short_windows_or_tails() {
    let white = "10000000";
    assert_eq!(
        decoded(&bilevel(2, 0, 2, &white.repeat(2))).unwrap(),
        PixelBuffer::U8(vec![0; 6])
    );
    assert_eq!(
        decoded(&bilevel(2, 0, 1, "10001111")).unwrap(),
        PixelBuffer::U8(vec![0; 3])
    );
    for bits in [
        "1000000000000000".to_string(),
        "10110000".to_string(),
        "00000000".to_string(),
    ] {
        assert!(decoded(&bilevel(2, 0, 1, &bits)).is_err(), "{bits}");
    }
    let mut bytes = bilevel(2, 0, 2, &white.repeat(2));
    let pos = tag_at(&bytes, 8, 279) + 8;
    put32(&mut bytes, pos, 1);
    assert!(decoded(&bytes).is_err()); // Extra real source bytes beyond declared strip are not counts.
    assert!(decoded(&bilevel(2, 1, 1, white)).is_err());
}
#[test]
fn mixed_scratch_and_changed_sources_clear_visible_counts() {
    let path = fixture("fax-3-options1-fill1.tif");
    let original = std::fs::read(&path).unwrap();
    let f = file(&original);
    let mut reader = Reader::open(
        f.path(),
        Limits {
            intermediate_bytes: 100,
            ..Limits::default()
        },
    )
    .unwrap();
    let mut buffer = FrameBuffer::default();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    let mut reader = Reader::open(f.path(), Limits::default()).unwrap();
    reader.read_frame_into(0, &mut buffer).unwrap();
    std::fs::write(f.path(), [0; 8]).unwrap();
    assert!(reader.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
#[test]
fn selected_directory_sample_windows_cannot_alias_graph_adaptation() {
    let mut bytes = std::fs::read(fixture("subifds-classic.tif")).unwrap();
    let root = get32(&bytes, 4);
    let children = get32(&bytes, tag_at(&bytes, root, 330) + 8);
    let child = get32(&bytes, children);
    let photo = tag_at(&bytes, root, 262) + 8;
    put32(&mut bytes, photo, 0);
    let strip = tag_at(&bytes, child, 273) + 8;
    put32(&mut bytes, strip, photo as u32);
    let f = file(&bytes);
    assert!(
        Reader::open_directory(
            f.path(),
            child as u64,
            Limits::default(),
            Options::default()
        )
        .is_err()
    );
}
#[test]
fn nested_subifd_and_child_next_chains_reuse_the_same_graph_selection() {
    let original = std::fs::read(fixture("subifds-classic.tif")).unwrap();
    let root = get32(&original, 4);
    let table = get32(&original, tag_at(&original, root, 330) + 8);
    let child = get32(&original, table);
    let grandchild = get32(&original, table + 4);
    for nested in [false, true] {
        let mut bytes = original.clone();
        put32(&mut bytes, table + 4, 0);
        if nested {
            let entry = tag_at(&bytes, child, 305); // Replace descriptive Software with an explicit nested pointer.
            bytes[entry..entry + 2].copy_from_slice(&330u16.to_le_bytes());
            bytes[entry + 2..entry + 4].copy_from_slice(&13u16.to_le_bytes());
            put32(&mut bytes, entry + 4, 1);
            put32(&mut bytes, entry + 8, grandchild as u32);
        } else {
            let pos = next_at(&bytes, child);
            put32(&mut bytes, pos, grandchild as u32);
        }
        let f = file(&bytes);
        let catalog = Catalog::open(f.path(), Limits::default()).unwrap();
        assert_eq!(catalog.directories().unwrap().len(), 4);
        assert_eq!(catalog.primary_offsets().unwrap().len(), 2);
        let mut reader = Reader::open_directory(
            f.path(),
            grandchild as u64,
            Limits::default(),
            Options::default(),
        )
        .unwrap();
        let mut buffer = FrameBuffer::default();
        reader.read_frame_into(0, &mut buffer).unwrap();
        assert_eq!(
            buffer.pixels(),
            Some(&PixelBuffer::F32(vec![0.5, -1.5, 7.25, 0.0]))
        );
    }
}
#[test]
#[cfg(feature = "wrappers")]
fn captured_input_selects_only_the_requested_subifd_and_clears_failed_output() {
    use dioptrin_io::{native, wrapped};
    let bytes = std::fs::read(fixture("subifds-classic.tif")).unwrap();
    let root = get32(&bytes, 4);
    let table = get32(&bytes, tag_at(&bytes, root, 330) + 8);
    let offset = get32(&bytes, table) as u64;
    let input = wrapped::Input::from_bytes(&bytes, None, wrapped::Limits::default()).unwrap();
    let mut reader = input
        .read(
            wrapped::Format::TiffDirectory {
                offset,
                options: Options::default(),
            },
            native::Limits::default(),
            Limits::default(),
        )
        .unwrap();
    assert_eq!(reader.frames(), 1);
    let mut output = wrapped::FrameBuffer::default();
    reader.read_frame_into(0, &mut output).unwrap();
    assert_eq!(
        output.pixels(),
        Some(&PixelBuffer::U16(vec![
            0, 10001, 20002, 30003, 40004, 50005
        ]))
    );
    assert_eq!(output.sample_shape(), Some(vec![2, 3]));
    assert!(reader.read_frame_into(1, &mut output).is_err());
    assert!(output.pixels().is_none());
    let input = wrapped::Input::from_bytes(&bytes, None, wrapped::Limits::default()).unwrap();
    assert!(
        input
            .read(
                wrapped::Format::TiffDirectory {
                    offset: offset + 2,
                    options: Options::default()
                },
                native::Limits::default(),
                Limits::default()
            )
            .is_err()
    );
}
