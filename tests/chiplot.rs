#![cfg(feature = "chiplot")]
use dioptrin_io::chiplot::{Exactness, FloatPolicy, Limits, Number, Profile, Provenance, Reader};
use std::io::{self, Read, Write};
const ASC: &[u8] = include_bytes!("data/chiplot/ascending.chi");
fn read(bytes: &[u8]) -> Profile {
    Profile::from_bytes(bytes, Limits::default()).unwrap()
}
#[test]
fn raw_headers_tokens_rows_comments_and_multicurves_survive() {
    let profile = read(include_bytes!("data/chiplot/comments-crlf.chi"));
    assert_eq!(profile.header(0).unwrap(), b" raw title ");
    assert_eq!(profile.provenance, Provenance::Buffer);
    assert_eq!(profile.declared_points, 2);
    assert_eq!(profile.curves, 1);
    assert_eq!(profile.rows[0].line, 6);
    assert_eq!(profile.comments.len(), 5);
    assert_eq!(profile.comments[0].text, b"# declared points");
    for row in &profile.rows {
        for sample in &row.samples {
            assert_eq!(
                &profile.original[sample.offset..sample.offset + sample.lexeme.len()],
                sample.lexeme.as_bytes()
            );
        }
    }
    let multi = read(include_bytes!("data/chiplot/multi-curve.chi"));
    assert_eq!(multi.curves, 2);
    assert_eq!(multi.rows.len(), 3);
    assert_eq!(
        multi.rows[0].samples[2].number,
        Number::I64(9007199254740993)
    );
    assert_eq!(multi.rows[1].samples[2].number, Number::U64(u64::MAX));
    assert_eq!(multi.rows[2].samples[2].number, Number::IntegerText);
    assert_eq!(multi.rows[2].samples[2].lexeme, "18446744073709551616");
    assert_eq!(read(ASC).rows[0].samples[1].lexeme, "00012.3400");
}
#[test]
fn integer_boundaries_never_pass_saturating_roundtrip_or_silent_rounding() {
    let p = read(include_bytes!("data/chiplot/precision.chi"));
    for row in &p.rows[..2] {
        assert!(
            row.samples[1]
                .f64_view(FloatPolicy::RequireExactInteger)
                .is_err()
        );
        assert_eq!(
            row.samples[1]
                .f64_view(FloatPolicy::AllowRoundedInteger)
                .unwrap()
                .exactness,
            Exactness::RoundedInteger
        );
    }
    assert_eq!(
        p.rows[0].samples[0]
            .f64_view(FloatPolicy::RequireExactInteger)
            .unwrap()
            .value
            .to_bits(),
        (-0.0f64).to_bits()
    );
    assert_eq!(p.rows[2].samples[1].number, Number::Decimal);
    assert!(
        p.rows[3].samples[1]
            .f64_view(FloatPolicy::AllowRoundedInteger)
            .is_err()
    );
    let p = read(b"T\nX\nY\n3\n0 9223372036854775808\n1 18446744073709551616\n2 1e999\n");
    assert_eq!(
        p.rows[0].samples[1]
            .f64_view(FloatPolicy::RequireExactInteger)
            .unwrap()
            .exactness,
        Exactness::ExactInteger
    );
    assert!(
        p.rows[1].samples[1]
            .f64_view(FloatPolicy::RequireExactInteger)
            .is_err()
    );
    assert_eq!(
        p.rows[1].samples[1]
            .f64_view(FloatPolicy::AllowRoundedInteger)
            .unwrap()
            .exactness,
        Exactness::UnverifiedIntegerApproximation
    );
    assert!(
        p.rows[2].samples[1]
            .f64_view(FloatPolicy::AllowRoundedInteger)
            .is_err()
    );
}
#[test]
fn explicit_grid_view_preserves_descending_and_nonuniform_x_without_replacing_y() {
    let asc = read(ASC)
        .grid_view(0.0, FloatPolicy::RequireExactInteger)
        .unwrap();
    assert_eq!(asc.uniform_step, Some(1.0));
    let profile = read(include_bytes!("data/chiplot/descending.chi"));
    let grid = profile
        .grid_view(0.0, FloatPolicy::RequireExactInteger)
        .unwrap();
    assert_eq!(grid.uniform_step, Some(-1.0));
    assert_eq!(grid.minimum, Some(1.0));
    assert_eq!(profile.rows[0].samples[1].lexeme, "11");
    assert!(!grid.diagnostics.is_empty());
    let grid = read(include_bytes!("data/chiplot/nonuniform.chi"))
        .grid_view(0.0, FloatPolicy::RequireExactInteger)
        .unwrap();
    assert_eq!(grid.endpoint_average_step, Some(1.0));
    assert!(grid.uniform_step.is_none());
    for bytes in [
        include_bytes!("data/chiplot/zero.chi").as_slice(),
        include_bytes!("data/chiplot/one.chi").as_slice(),
    ] {
        let grid = read(bytes)
            .grid_view(0.0, FloatPolicy::RequireExactInteger)
            .unwrap();
        assert!(grid.uniform_step.is_none());
        assert!(grid.endpoint_average_step.is_none());
    }
    assert!(
        read(ASC)
            .grid_view(-1.0, FloatPolicy::RequireExactInteger)
            .is_err()
    );
    let mut malformed = read(ASC);
    malformed.rows[0].samples.clear();
    assert!(
        malformed
            .grid_view(0.0, FloatPolicy::RequireExactInteger)
            .is_err()
    );
    let mut malformed = read(ASC);
    malformed.rows[0].samples[0].lexeme = "1.5".into();
    assert!(
        malformed.rows[0].samples[0]
            .f64_view(FloatPolicy::RequireExactInteger)
            .is_err()
    );
}
#[test]
fn malformed_counts_widths_uncertainties_and_fortran_d_are_rejected() {
    for bytes in [
        b"T\nX\nY".as_slice(),
        b"T\nX\nY\n2\n0 1\n",
        b"T\nX\nY\n0\n0 1\n",
        b"T\nX\nY\n1\n0 1 2\n",
        b"T\nX\nY\n1\n0\n",
        b"T\nX\nY\n1 0\n0 1\n",
        b"T\nX\nY\n1 1 1\n0 1\n",
        b"T\nX\nY\n1\n0,,1\n",
        b"T\nX\nY\n1\n0 1.23(4)\n",
        b"T\nX\nY\n1\n0 1D+2\n",
        b"T\nX\nY\n18446744073709551616\n",
    ] {
        assert!(
            Profile::from_bytes(bytes, Limits::default()).is_err(),
            "{:?}",
            bytes
        );
    }
}
#[test]
fn limits_and_nonfinite_extensions_are_explicit() {
    for limits in [
        Limits {
            frames: 2,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: ASC.len() - 1,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: ASC.len(),
            ..Limits::default()
        },
        Limits {
            header_bytes: 3,
            ..Limits::default()
        },
        Limits {
            frame_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(Profile::from_bytes(ASC, limits).is_err());
    }
    let huge = b"T\nX\nY\n0 18446744073709551615\n";
    assert!(Profile::from_bytes(huge, Limits::default()).is_err());
    let p = read(include_bytes!("data/chiplot/nonfinite.chi"));
    assert_eq!(p.rows[0].samples[1].number, Number::NonFinite);
    assert!(!p.diagnostics.is_empty());
    assert!(
        p.rows[0].samples[1]
            .f64_view(FloatPolicy::RequireExactInteger)
            .unwrap()
            .value
            .is_nan()
    );
    assert!(
        read(b"T\nX\nY\n1\nnan 1\n")
            .grid_view(0.0, FloatPolicy::RequireExactInteger)
            .is_err()
    );
}
struct Nonseek<'a>(&'a [u8]);
impl Read for Nonseek<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = out.len().min(self.0.len()).min(3);
        out[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}
#[test]
fn bytebuffer_stream_and_file_snapshot_have_distinct_checked_contracts() {
    let p = Profile::from_reader(Nonseek(ASC), Limits::default()).unwrap();
    assert_eq!(p.original, ASC);
    assert_eq!(p.provenance, Provenance::StreamCapture);
    let mut bytes = ASC.to_vec();
    let p = read(&bytes);
    bytes.fill(0);
    assert_eq!(p.original, ASC);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile.chi");
    std::fs::write(&path, ASC).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    assert_eq!(
        reader.metadata().unwrap().provenance,
        Provenance::PathSnapshot
    );
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"# changed")
        .unwrap();
    assert!(reader.metadata().is_err());
    std::fs::write(&path, ASC).unwrap();
    let reader = Reader::open(&path, Limits::default()).unwrap();
    let other = dir.path().join("other");
    std::fs::write(&other, ASC).unwrap();
    std::fs::rename(other, &path).unwrap();
    assert!(reader.metadata().is_err());
    assert!(reader.source_path().is_err());
}
