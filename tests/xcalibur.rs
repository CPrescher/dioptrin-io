#![cfg(feature = "xcalibur")]
use dioptrin_io::xcalibur::{Characteristics, Limits, Projection, Reader, decompose};
const BYTES: &[u8] = include_bytes!("data/xcalibur/records.ccd");
fn parse() -> Characteristics {
    Characteristics::from_bytes(BYTES, Limits::default()).unwrap()
}
#[test]
fn independent_records_preserve_every_family_and_original_bytes() {
    let m = parse();
    assert_eq!(m.version, 0x1000c);
    assert_eq!(m.dark_current.to_bits(), (-0.0f64).to_bits());
    assert_eq!(m.read_noise, 3.25);
    assert_eq!(m.text[6].text().unwrap(), "尾");
    assert_eq!(m.fip_origin, [9, 10, 11]);
    assert_eq!(m.corner_count, 1);
    assert_eq!(m.corner_y, [5, 6, 7, 8]);
    assert_eq!(m.glowing_count, 0);
    assert_eq!(m.glowing_x, [8, 7, 6, 5]);
    let p = &m.polygons.as_ref().unwrap()[0];
    assert_eq!((p.kind, p.points), (1, 4));
    assert_eq!(p.x, [1, 2, 91, 92, 93, 94]);
    assert_eq!(p.y, [0, 1, 95, 96, 97, 98]);
    let p = &m.points.as_ref().unwrap()[0];
    assert_eq!((p.point.x, p.point.y), (3, 2));
    assert_eq!(
        (p.replacements[1].x, p.replacements[1].y, p.treatment),
        (6, 7, 0xabcd)
    );
    assert_eq!(
        m.columns.each_ref().map(|v| v.as_ref().unwrap().len()),
        [1, 0, 1, 0]
    );
    let col = &m.columns[2].as_ref().unwrap()[0];
    assert_eq!(
        (
            col.row.start.x,
            col.row.treatment,
            col.low_limit,
            col.high_limit
        ),
        (10, 18, 111, 222)
    );
    assert_eq!(
        m.rows.each_ref().map(|v| v.as_ref().unwrap().len()),
        [1, 1, 0, 2]
    );
    assert_eq!(m.rows[3].as_ref().unwrap()[1].treatment, 68);
    let e = m.electronics.as_ref().unwrap();
    assert_eq!((e.scintillator, e.gain_mo, e.gain_cu), (7, 1.5, 2.5));
    assert_eq!(
        (
            e.machine.as_ref().unwrap().enabled,
            e.machine.as_ref().unwrap().a,
            e.machine.as_ref().unwrap().b
        ),
        (1, 3.5, 4.5)
    );
    assert_eq!(&m.original[m.parsed_bytes..], b"opaque trailer\0\xff");
    assert_eq!(m.original, BYTES);
}
#[test]
fn absent_sections_remain_absent_and_partial_sections_fail() {
    let m = Characteristics::from_bytes(&BYTES[..1854], Limits::default()).unwrap();
    assert!(m.polygons.is_none() && m.points.is_none() && m.electronics.is_none());
    assert_eq!(m.dark_current.to_bits(), (-0.0f64).to_bits());
    // Every truncation except a complete section boundary must fail.
    let boundaries = [
        1854, 1884, 1900, 1924, 1926, 1950, 1952, 1972, 2008, 2028, 2030, 2068,
    ];
    assert_eq!(parse().parsed_bytes, 2068);
    for end in 0..2068 {
        let result = Characteristics::from_bytes(&BYTES[..end], Limits::default());
        assert_eq!(result.is_ok(), boundaries.contains(&end), "end={end}");
    }
}
#[test]
fn bounds_versions_and_invalid_text_are_explicit() {
    let mut bytes = BYTES.to_vec();
    bytes[..4].copy_from_slice(&0x20001u32.to_le_bytes());
    assert!(Characteristics::from_bytes(&bytes, Limits::default()).is_err());
    bytes[..4].copy_from_slice(&0x10001u32.to_le_bytes());
    bytes[20] = 255;
    let m = Characteristics::from_bytes(&bytes, Limits::default()).unwrap();
    assert!(m.text[0].text().is_err());
    assert_eq!(m.text[0].0[0], 255);
    for limits in [
        Limits {
            frames: 1,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: 100,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 100,
            ..Limits::default()
        },
    ] {
        assert!(Characteristics::from_bytes(BYTES, limits).is_err());
    }
}
#[test]
fn mask_projection_is_explicit_and_source_orientation_is_retained() {
    let mut m = parse();
    assert!(
        m.build_mask([3, 4], Projection::Strict, Limits::default())
            .is_err()
    );
    let mask = m
        .build_mask([3, 4], Projection::Fabio2026, Limits::default())
        .unwrap();
    assert_eq!(mask.values, [0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1]);
    assert_eq!(mask.ignored_records, 7); // two columns, four rows, one corner
    m.columns = std::array::from_fn(|_| None);
    m.rows = std::array::from_fn(|_| None);
    m.corner_count = 0;
    assert_eq!(
        m.build_mask([3, 4], Projection::Strict, Limits::default())
            .unwrap()
            .values,
        mask.values
    );
    assert!(
        m.build_mask([3, 3], Projection::Fabio2026, Limits::default())
            .is_err()
    );
    m.points = None;
    m.polygons.as_mut().unwrap()[0].x[1] = 10;
    assert!(
        m.build_mask([3, 4], Projection::Strict, Limits::default())
            .is_err()
    );
    assert_eq!(
        m.build_mask([3, 4], Projection::Fabio2026, Limits::default())
            .unwrap()
            .values,
        [0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0]
    );
    m.polygons.as_mut().unwrap()[0].x[0] = 11;
    assert_eq!(
        m.build_mask([3, 4], Projection::Fabio2026, Limits::default())
            .unwrap()
            .values,
        [0; 12]
    );
    m.polygons.as_mut().unwrap()[0].kind = 2;
    assert!(
        m.build_mask([3, 4], Projection::Strict, Limits::default())
            .is_err()
    );
    assert_eq!(
        m.build_mask([3, 4], Projection::Fabio2026, Limits::default())
            .unwrap()
            .ignored_records,
        1
    );
}
#[test]
fn changed_source_invalidates_metadata_and_projection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.ccd");
    std::fs::write(&path, BYTES).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert!(reader.metadata().is_ok());
    std::fs::rename(&path, dir.path().join("original.ccd")).unwrap();
    std::fs::write(&path, BYTES).unwrap();
    assert!(reader.metadata().is_err());
    assert!(
        reader
            .build_mask([3, 4], Projection::Fabio2026, Limits::default())
            .is_err()
    );
}
#[test]
fn full_decomposition_exhaustively_reconstructs_small_binary_masks() {
    for bits in 0..4096usize {
        let values: Vec<u8> = (0..12).map(|i| ((bits >> i) & 1) as u8).collect();
        let result = decompose(&values, [3, 4], true, Limits::default()).unwrap();
        assert!(result.complete);
        assert_eq!(
            result.build_mask(Limits::default()).unwrap().values,
            values,
            "bits={bits}"
        );
        let coarse = decompose(&values, [3, 4], false, Limits::default()).unwrap();
        let expected: Vec<u8> = (0..12)
            .map(|i| {
                u8::from(
                    (0..4).all(|x| values[i / 4 * 4 + x] == 1)
                        || (0..3).all(|y| values[y * 4 + i % 4] == 1),
                )
            })
            .collect();
        assert!(!coarse.complete);
        assert_eq!(
            coarse.build_mask(Limits::default()).unwrap().values,
            expected
        );
    }
}
#[test]
fn decomposition_rejects_invalid_inputs_limits_and_mutated_geometry() {
    for (values, shape) in [
        (vec![], [0, 4]),
        (vec![1, 2], [1, 2]),
        (vec![1], [1, 2]),
        (vec![0; 65537], [1, 65537]),
    ] {
        assert!(decompose(&values, shape, true, Limits::default()).is_err());
    }
    for limits in [
        Limits {
            frame_bytes: 3,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 64,
            ..Limits::default()
        },
        Limits {
            frames: 1,
            ..Limits::default()
        },
    ] {
        assert!(decompose(&[1, 0, 1, 0, 1, 0], [2, 3], true, limits).is_err());
    }
    let mut d = decompose(&[1, 1], [1, 2], true, Limits::default()).unwrap();
    d.polygons[0].x[0] = 3;
    assert!(d.build_mask(Limits::default()).is_err());
    d.polygons[0].x[0] = 0;
    d.polygons[0].kind = 2;
    assert!(d.build_mask(Limits::default()).is_err());
    let d = decompose(&vec![0; 65536], [1, 65536], true, Limits::default()).unwrap();
    assert_eq!(d.build_mask(Limits::default()).unwrap().values.len(), 65536);
}

#[test]
fn explicit_terminal_machine_policy_retains_partial_bytes_without_defaults() {
    use dioptrin_io::xcalibur::ReadPolicy;
    for end in 1990..2008 {
        // after complete gains, before complete machine function
        let bytes = &BYTES[..end];
        assert!(Characteristics::from_bytes(bytes, Limits::default()).is_err());
        let m = Characteristics::from_bytes_with_policy(
            bytes,
            Limits::default(),
            ReadPolicy::RetainTruncatedMachineFunction,
        )
        .unwrap();
        let diagnostic = m.incomplete_machine_function.as_ref().unwrap();
        assert_eq!(diagnostic.offset, 1990);
        assert_eq!(diagnostic.expected_bytes, 18);
        assert_eq!(diagnostic.available_bytes, end - 1990);
        assert_eq!(m.parsed_bytes, 1990);
        assert_eq!(&m.original[1990..], &BYTES[1990..end]);
        assert!(m.electronics.unwrap().machine.is_none());
        assert!(m.rows[1..].iter().all(Option::is_none));
    }
    // The policy never admits truncated gain fields or counted records.
    for end in [1855, 1883, 1973, 1989] {
        assert!(
            Characteristics::from_bytes_with_policy(
                &BYTES[..end],
                Limits::default(),
                ReadPolicy::RetainTruncatedMachineFunction
            )
            .is_err()
        );
    }
}
