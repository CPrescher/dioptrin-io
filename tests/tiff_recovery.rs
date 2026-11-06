#![cfg(feature = "tiff")]
use dioptrin_io::{
    PixelBuffer,
    tiff::{
        FrameBuffer, Limits, Options, Reader, TileTables, UncompressedWindows,
        compatibility::{ChunkKind, Operation},
        thunderscan::{AvailableFrame, Integrity},
    },
};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/tiff-recovery")
        .join(name)
}
fn options() -> Options {
    Options {
        uncompressed_windows: UncompressedWindows::FromShape,
        tile_tables: TileTables::FromStripTags,
        ..Default::default()
    }
}
fn file(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(bytes).unwrap();
    f
}
fn bytes(p: &PixelBuffer) -> Vec<u8> {
    match p {
        PixelBuffer::U8(v) => v.clone(),
        PixelBuffer::U16(v) => v.iter().flat_map(|v| v.to_le_bytes()).collect(),
        PixelBuffer::F32(v) => v.iter().flat_map(|v| v.to_bits().to_le_bytes()).collect(),
        _ => panic!("fixture dtype"),
    }
}
fn tag(data: &[u8], wanted: u16) -> usize {
    let n = u16::from_le_bytes(data[8..10].try_into().unwrap()) as usize;
    (0..n)
        .map(|i| 10 + 12 * i)
        .find(|&p| data[p..p + 2] == wanted.to_le_bytes())
        .unwrap()
}
fn get(data: &[u8], wanted: u16) -> u32 {
    let p = tag(data, wanted) + 8;
    u32::from_le_bytes(data[p..p + 4].try_into().unwrap())
}
fn set(data: &mut [u8], wanted: u16, value: u32) {
    let p = tag(data, wanted) + 8;
    data[p..p + 4].copy_from_slice(&value.to_le_bytes());
}
#[test]
fn explicit_interpretations_keep_source_tags_descriptors_bits_and_counts() {
    for name in [
        "short-u16.tif",
        "missing-count-be-f32.tif",
        "short-packed4.tif",
        "mixed-modern-lzw-tiles.tif",
    ] {
        let original = std::fs::read(fixture(name)).unwrap();
        assert!(
            Reader::open(fixture(name), Limits::default()).is_err(),
            "strict {name}"
        );
        let mut r = Reader::open_with_options(fixture(name), Limits::default(), options())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut frame = FrameBuffer::default();
        r.read_frame_into(0, &mut frame)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            bytes(frame.pixels().unwrap()),
            std::fs::read(fixture(&format!("{name}.raw"))).unwrap()
        );
        let m = r.metadata(0).unwrap();
        let provenance = m.compatibility.as_ref().unwrap();
        for e in &provenance.source_entries {
            assert_eq!(
                e.ifd_entry,
                &original[e.entry_offset as usize..e.entry_offset as usize + e.ifd_entry.len()]
            );
        }
        if name.starts_with("missing") {
            assert!(!m.tags.contains_key(&279));
            assert!(!provenance.source_entries.iter().any(|e| e.tag == 279));
            assert!(m.tags.contains_key(&65001));
        }
        if name.starts_with("short-packed") {
            assert_eq!(m.bits_per_sample, 4);
            assert_eq!(m.tags[&279].clone().into_u64().unwrap(), 5);
            assert_eq!(provenance.windows[0].bytes, 6);
        }
        if name.starts_with("mixed") {
            assert_eq!(provenance.operations, [Operation::TileTablesFromStripTags]);
            assert_eq!(provenance.kind, ChunkKind::Tiles);
            assert!(!m.tags.contains_key(&324));
            assert!(m.tags.contains_key(&273));
        }
        assert_eq!(std::fs::read(fixture(name)).unwrap(), original);
    }
}
#[test]
fn effective_windows_reject_bounds_aliases_overlaps_and_ambiguous_geometry() {
    let source = std::fs::read(fixture("short-packed4.tif")).unwrap();
    let reject = |data: &[u8]| {
        let f = file(data);
        assert!(Reader::open_with_options(f.path(), Limits::default(), options()).is_err());
    };
    let mut short = source.clone();
    short.pop();
    reject(&short);
    let mut metadata = source.clone();
    set(&mut metadata, 273, 8);
    reject(&metadata);
    let mut overflow = source.clone();
    set(&mut overflow, 256, u32::MAX);
    reject(&overflow);
    let mut metadata = source.clone();
    let p = tag(&metadata, 279);
    metadata[p + 2..p + 4].copy_from_slice(&12u16.to_le_bytes());
    reject(&metadata);
    let mut overlap = std::fs::read(fixture("short-u16.tif")).unwrap();
    let offsets = get(&overlap, 273) as usize;
    let first = u32::from_le_bytes(overlap[offsets..offsets + 4].try_into().unwrap());
    overlap[offsets + 4..offsets + 8].copy_from_slice(&(first + 1).to_le_bytes());
    reject(&overlap);
    let mut ambiguous = std::fs::read(fixture("mixed-modern-lzw-tiles.tif")).unwrap();
    set(&mut ambiguous, 257, 64);
    let p = tag(&ambiguous, 273);
    let n = u32::from_le_bytes(ambiguous[p + 4..p + 8].try_into().unwrap());
    assert_eq!(n, 4); // Four full-height strip entries as well as four tiles.
    // A mismatching/ambiguous geometry cannot be promoted by the tile policy.
    set(&mut ambiguous, 322, 19);
    set(&mut ambiguous, 323, 16);
    let rows = tag(&ambiguous, 296);
    ambiguous[rows..rows + 2].copy_from_slice(&278u16.to_le_bytes());
    set(&mut ambiguous, 278, 16);
    reject(&ambiguous);
    let f = file(&source);
    let limits = Limits {
        intermediate_bytes: 5,
        ..Limits::default()
    };
    assert!(Reader::open_with_options(f.path(), limits, options()).is_err());
}
#[test]
fn recovered_lzw_requires_complete_source_tile_counts_and_end_code() {
    let source = std::fs::read(fixture("mixed-modern-lzw-tiles.tif")).unwrap();
    let p = tag(&source, 279);
    let counts = get(&source, 279) as usize;
    assert_eq!(
        u32::from_le_bytes(source[p + 4..p + 8].try_into().unwrap()),
        4
    );
    let mut missing = source.clone();
    let n = u16::from_le_bytes(missing[counts..counts + 2].try_into().unwrap());
    missing[counts..counts + 2].copy_from_slice(&(n - 1).to_le_bytes());
    let f = file(&missing);
    let mut r = Reader::open_with_options(f.path(), Limits::default(), options()).unwrap();
    let mut frame = FrameBuffer::default();
    assert!(r.read_frame_into(0, &mut frame).is_err());
    assert!(frame.pixels().is_none());
    // Pinned imagecodecs writer emits an aligned EOI + exactly eight zero
    // flush bits in tile2. That independently encoded positive fixture reads.
    let offsets = get(&source, 273) as usize;
    let offset = u32::from_le_bytes(source[offsets + 8..offsets + 12].try_into().unwrap()) as usize;
    let length = u16::from_le_bytes(source[counts + 4..counts + 6].try_into().unwrap()) as usize;
    assert_eq!(source[offset + length - 1], 0);
    let mut bad = source.clone();
    bad[offset + length - 1] = 0x80;
    let reject_decode = |data: &[u8]| {
        let f = file(data);
        let mut r = Reader::open_with_options(f.path(), Limits::default(), options()).unwrap();
        let mut b = FrameBuffer::default();
        assert!(r.read_frame_into(0, &mut b).is_err());
        assert!(b.pixels().is_none());
    };
    reject_decode(&bad);
    let mut extra = source.clone();
    extra.insert(offset + length, 0);
    extra[counts + 4..counts + 6].copy_from_slice(&((length + 1) as u16).to_le_bytes());
    let next = u32::from_le_bytes(source[offsets + 12..offsets + 16].try_into().unwrap());
    extra[offsets + 12..offsets + 16].copy_from_slice(&(next + 1).to_le_bytes());
    reject_decode(&extra);
}
#[test]
fn thunder_complete_rows_partial_discard_and_other_page_are_explicit() {
    for name in ["thunder-complete.tif", "thunder-all-modes.tif"] {
        let mut r = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut f = FrameBuffer::default();
        r.read_frame_into(0, &mut f).unwrap();
        assert_eq!(
            bytes(f.pixels().unwrap()),
            std::fs::read(fixture(&format!("{name}.raw"))).unwrap()
        );
        assert_eq!(r.metadata(0).unwrap().bits_per_sample, 4);
    }
    let mut r = Reader::open(fixture("thunder-partial-stack.tif"), Limits::default()).unwrap();
    let mut dense = FrameBuffer::default();
    assert!(r.read_frame_into(0, &mut dense).is_err());
    assert!(dense.pixels().is_none());
    let mut available = AvailableFrame::default();
    r.read_thunderscan_available_into(0, &mut available)
        .unwrap();
    assert_eq!(available.shape(), Some([5, 7]));
    assert!(!available.complete());
    assert_eq!(available.pixels().unwrap().len(), 28);
    assert_eq!(
        bytes(available.pixels().unwrap()),
        std::fs::read(fixture("thunder-partial-stack.tif.raw")).unwrap()
    );
    let spans = available.available_rows().unwrap();
    assert_eq!(
        (spans[0].start_row, spans[0].rows, spans[0].sample_offset),
        (0, 3, 0)
    );
    assert_eq!(
        (spans[1].start_row, spans[1].rows, spans[1].sample_offset),
        (4, 1, 21)
    );
    let missing = &available.missing_rows().unwrap()[0];
    assert_eq!((missing.start_row, missing.rows, missing.strip), (3, 1, 1));
    let status = &available.strips().unwrap()[1];
    assert_eq!(status.complete_rows, 1);
    assert_eq!(status.consumed_bytes, status.declared_bytes);
    r.read_frame_into(1, &mut dense).unwrap();
    assert_eq!(
        bytes(dense.pixels().unwrap()),
        std::fs::read(fixture("thunder-partial-stack.tif.page1.raw")).unwrap()
    );
    assert!(
        r.read_thunderscan_available_into(1, &mut available)
            .is_err()
    );
    assert!(available.pixels().is_none());
    assert!(available.available_rows().is_none());
    assert!(available.missing_rows().is_none());
    assert!(available.strips().is_none());
}
#[test]
fn thunder_overruns_tails_limits_and_source_changes_never_expose_filler() {
    for (name, present, integrity) in [
        ("thunder-run-overflow.tif", 0, Integrity::InvalidRow),
        ("thunder-coded-tail.tif", 3, Integrity::TrailingCodedData),
        ("thunder-noop-tail.tif", 3, Integrity::Complete),
    ] {
        let mut r = Reader::open(fixture(name), Limits::default()).unwrap();
        let mut available = AvailableFrame::default();
        r.read_thunderscan_available_into(0, &mut available)
            .unwrap();
        assert_eq!(available.pixels().unwrap().len(), present);
        assert_eq!(available.strips().unwrap()[0].integrity, integrity);
        let mut dense = FrameBuffer::default();
        assert_eq!(
            r.read_frame_into(0, &mut dense).is_ok(),
            integrity == Integrity::Complete
        );
    }
    let source = std::fs::read(fixture("thunder-complete.tif")).unwrap();
    let f = file(&source);
    let mut r = Reader::open(f.path(), Limits::default()).unwrap();
    let mut available = AvailableFrame::default();
    r.read_thunderscan_available_into(0, &mut available)
        .unwrap();
    std::fs::write(f.path(), [0; 8]).unwrap();
    assert!(
        r.read_thunderscan_available_into(0, &mut available)
            .is_err()
    );
    assert!(available.pixels().is_none());
    assert!(available.shape().is_none());
    assert!(!available.complete());
    let f = file(&source);
    let mut r = Reader::open(
        f.path(),
        Limits {
            intermediate_bytes: 4,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(
        r.read_thunderscan_available_into(0, &mut available)
            .is_err()
    );
    assert!(available.pixels().is_none());
    // Truncated declared windows cannot consume physically adjacent encoded bytes.
    let mut short = source.clone();
    let counts = get(&short, 279) as usize;
    short[counts..counts + 4].copy_from_slice(&1u32.to_le_bytes());
    let f = file(&short);
    let mut r = Reader::open(f.path(), Limits::default()).unwrap();
    r.read_thunderscan_available_into(0, &mut available)
        .unwrap();
    assert_eq!(available.strips().unwrap()[0].consumed_bytes, 1);
    assert_eq!(available.strips().unwrap()[0].complete_rows, 0);
}

#[test]
fn thunder_native_byte_modes_and_huge_rows_are_bounded_before_ffi() {
    let source = std::fs::read(fixture("thunder-run-overflow.tif")).unwrap();
    let mut available = AvailableFrame::default();
    for value in 0..=255u8 {
        let mut data = source.clone();
        *data.last_mut().unwrap() = value;
        let f = file(&data);
        let mut r = Reader::open(f.path(), Limits::default()).unwrap();
        r.read_thunderscan_available_into(0, &mut available)
            .unwrap();
        let PixelBuffer::U8(values) = available.pixels().unwrap() else {
            panic!("source dtype")
        };
        assert!(values.len() <= 1);
        assert!(values.iter().all(|&v| v <= 15));
    }
    let mut huge = source;
    set(&mut huge, 256, u32::MAX);
    let f = file(&huge);
    let mut r = Reader::open(
        f.path(),
        Limits {
            frame_bytes: usize::MAX,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(
        r.read_thunderscan_available_into(0, &mut available)
            .is_err()
    );
    assert!(available.pixels().is_none());
    assert!(available.available_rows().is_none());
}
