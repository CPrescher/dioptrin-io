#![cfg(feature = "xcalibur")]
use dioptrin_io::xcalibur::{
    Limits,
    run::{HEADER_BYTES, Provenance, Reader, RunDescription, SWEEP_BYTES, Sweep},
};
use std::io::{self, Read, Write};
const RECORDS: &[u8] = include_bytes!("data/xcalibur/records.run");
const ZERO: &[u8] = include_bytes!("data/xcalibur/zero-sweeps.run");
const RAW: &[u8] = include_bytes!("data/xcalibur/raw-text-floats.run");

#[test]
fn independent_records_preserve_all_fields_bits_and_trailer() {
    let run = RunDescription::from_bytes(RECORDS, Limits::default()).unwrap();
    assert_eq!(run.provenance, Provenance::Buffer);
    assert_eq!(run.prefix.text().unwrap(), "run_ä\0embedded");
    assert_eq!(run.folder.text().unwrap(), "/unavailable/original/output");
    assert_eq!(run.declared_sweeps, 2);
    assert_eq!(run.unknown.to_bits(), 0x8000000000000000);
    assert_eq!(run.parsed_bytes, HEADER_BYTES + 2 * SWEEP_BYTES);
    assert_eq!(run.trailing_bytes(), b"opaque run trailer\0\xff");
    assert_eq!(run.original, RECORDS);
    let s = &run.sweeps[0];
    assert_eq!(
        (s.scan_index, s.scan_type, s.unknown1),
        (7, 0xabcd, 0xfedcba98)
    );
    assert_eq!(
        [
            s.omega.to_bits(),
            s.theta.to_bits(),
            s.kappa.to_bits(),
            s.phi.to_bits(),
            s.start.to_bits(),
            s.end.to_bits(),
            s.width.to_bits(),
            s.unknown2.to_bits(),
            s.exposure.to_bits()
        ],
        [
            0x8000000000000000,
            0x3ff4000000000000,
            0xc004000000000000,
            0x400c000000000000,
            0x4012000000000000,
            0x4016000000000000,
            0x3fb999999999999a,
            0xbfd0000000000000,
            0x4025000000000000
        ]
    );
    assert_eq!(s.unknown, [0, 0xffff, 0x1234, 0x8000]);
    assert_eq!(s.original, RECORDS[528..616]);
    assert_eq!(run.sweeps[1].scan_index, 0xffff);
    let empty = RunDescription::from_bytes(ZERO, Limits::default()).unwrap();
    assert_eq!(empty.declared_sweeps, 0);
    assert!(empty.sweeps.is_empty() && empty.trailing_bytes().is_empty());
}
#[test]
fn invalid_text_and_nonfinite_values_remain_original_source_bits() {
    let run = RunDescription::from_bytes(RAW, Limits::default()).unwrap();
    assert!(run.prefix.text().is_err());
    assert_eq!(run.prefix.0[0], 0xff);
    assert_eq!(run.unknown.to_bits(), 0x7ff8000000000042);
    let s = &run.sweeps[0];
    assert_eq!(
        [
            s.omega.to_bits(),
            s.theta.to_bits(),
            s.kappa.to_bits(),
            s.phi.to_bits(),
            s.start.to_bits()
        ],
        [
            1,
            0x7ff0000000000000,
            0xfff0000000000000,
            0x7ff8000000000042,
            0x7ff0000000000001
        ]
    );
    assert_eq!(run.original, RAW);
}
#[test]
fn all_counted_truncations_reject_without_fabricating_records() {
    for end in 0..704 {
        assert!(
            RunDescription::from_bytes(&RECORDS[..end], Limits::default()).is_err(),
            "end={end}"
        );
    }
    assert!(RunDescription::from_bytes(&RECORDS[..704], Limits::default()).is_ok());
    for length in [0, 87, 89] {
        assert!(Sweep::from_bytes(&vec![0; length]).is_err());
    }
    assert!(Sweep::from_bytes(&RECORDS[528..616]).is_ok());
}
#[test]
fn record_byte_header_and_aggregate_limits_are_checked() {
    for limits in [
        Limits {
            frames: 1,
            ..Limits::default()
        },
        Limits {
            header_bytes: 527,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: RECORDS.len() - 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: RECORDS.len(),
            ..Limits::default()
        },
        Limits {
            frames: 0,
            ..Limits::default()
        },
    ] {
        assert!(RunDescription::from_bytes(RECORDS, limits).is_err());
    }
    let mut huge = ZERO.to_vec();
    huge[512..520].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(
        RunDescription::from_bytes(
            &huge,
            Limits {
                frames: usize::MAX,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        RunDescription::from_reader(
            ZERO,
            Limits {
                encoded_bytes: usize::MAX,
                metadata_bytes: usize::MAX,
                ..Limits::default()
            }
        )
        .is_err()
    );
}
struct Nonseekable<'a>(&'a [u8]);
impl Read for Nonseekable<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = output.len().min(self.0.len()).min(3);
        output[..count].copy_from_slice(&self.0[..count]);
        self.0 = &self.0[count..];
        Ok(count)
    }
}
struct Counting<'a> {
    source: &'a [u8],
    consumed: usize,
}
impl Read for Counting<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let length = output.len().min(self.source.len());
        output[..length].copy_from_slice(&self.source[..length]);
        self.source = &self.source[length..];
        self.consumed += length;
        Ok(length)
    }
}
#[test]
fn stream_capture_is_owned_bounded_and_not_a_file_identity_claim() {
    let run = RunDescription::from_reader(Nonseekable(RECORDS), Limits::default()).unwrap();
    assert_eq!(run.provenance, Provenance::StreamCapture);
    assert_eq!(run.original, RECORDS);
    assert!(
        RunDescription::from_reader(
            Nonseekable(RECORDS),
            Limits {
                encoded_bytes: RECORDS.len() - 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let mut source = RECORDS.to_vec();
    let run = RunDescription::from_bytes(&source, Limits::default()).unwrap();
    source.fill(0);
    assert_eq!(run.original, RECORDS);
    let mut counted = Counting {
        source: RECORDS,
        consumed: 0,
    };
    assert!(
        RunDescription::from_reader(
            &mut counted,
            Limits {
                encoded_bytes: 528,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert_eq!(counted.consumed, 529);
}
#[test]
fn path_access_rejects_growth_and_same_size_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.run");
    std::fs::write(&path, RECORDS).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(
        reader.metadata().unwrap().provenance,
        Provenance::PathSnapshot
    );
    assert_eq!(
        reader.source_path().unwrap(),
        std::fs::canonicalize(&path).unwrap()
    );
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&[0])
        .unwrap();
    assert!(reader.metadata().is_err());
    std::fs::write(&path, RECORDS).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let replacement = dir.path().join("new.run");
    std::fs::write(&replacement, RECORDS).unwrap();
    std::fs::rename(replacement, &path).unwrap();
    assert!(reader.metadata().is_err());
    assert!(reader.source_path().is_err());
}
