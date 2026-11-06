use crate::{
    decoder::{DecodeStatus, Group4Decoder, StrictRowDecoder},
    maps::{black, mode, white, Mode},
    BitReader,
};
use std::convert::TryFrom;
struct Bits {
    source: String,
    position: usize,
}
impl Bits {
    fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            position: 0,
        }
    }
}
impl BitReader for Bits {
    type Error = ();
    fn peek(&self, n: u8) -> Option<u16> {
        let end = self.position.checked_add(usize::from(n))?;
        if n > 16 || end > self.source.len() {
            return None;
        }
        Some(
            self.source.as_bytes()[self.position..end]
                .iter()
                .fold(0, |n, b| n * 2 + u16::from(*b == b'1')),
        )
    }
    fn consume(&mut self, n: u8) -> Result<(), ()> {
        self.peek(n).ok_or(())?;
        self.position += usize::from(n);
        Ok(())
    }
    fn bits_to_byte_boundary(&self) -> u8 {
        ((8 - self.position % 8) % 8) as u8
    }
}
fn horizontal(a: u16, b: u16) -> String {
    format!(
        "{}{}{}",
        mode::encode(Mode::Horizontal).unwrap(),
        white::encode(a).unwrap(),
        black::encode(b).unwrap()
    )
}
#[test]
fn strict_exact_last_mode_no_virtual_lookahead_or_tail_fill() {
    let mut decoder = StrictRowDecoder::new(5).unwrap();
    let mut bits = Bits::new("1");
    assert_eq!(decoder.decode(&mut bits).unwrap(), []);
    assert_eq!(bits.position, 1);
    decoder.set_reference(&[0, 3]).unwrap();
    let mut bits = Bits::new("111");
    assert_eq!(decoder.decode(&mut bits).unwrap(), [0, 3]);
    assert_eq!(bits.position, 3);
    let mut truncated = Bits::new("11");
    assert!(decoder.decode(&mut truncated).is_err());
    let mut same = Bits::new("111");
    assert_eq!(decoder.decode(&mut same).unwrap(), [0, 3]);
}
#[test]
fn strict_horizontal_width_and_zero_end_sentinel() {
    let mut decoder = StrictRowDecoder::new(5).unwrap();
    let mut bits = Bits::new(horizontal(5, 0));
    assert_eq!(decoder.decode(&mut bits).unwrap(), []);
    let mut overrun = Bits::new(horizontal(6, 0));
    assert!(decoder.decode(&mut overrun).is_err());
    let mut bits = Bits::new(horizontal(0, 5));
    assert_eq!(decoder.decode(&mut bits).unwrap(), [0]);
    let mut zero = Bits::new(horizontal(0, 0));
    assert!(decoder.decode(&mut zero).is_err());
}
#[test]
fn strict_vertical_bounds_premature_eol_and_reference_validation() {
    let mut decoder = StrictRowDecoder::new(5).unwrap();
    assert!(decoder.decode(&mut Bits::new("011")).is_err());
    assert!(decoder
        .decode(&mut Bits::new(mode::encode(Mode::EOF).unwrap().to_string()))
        .is_err());
    assert!(decoder.decode(&mut Bits::new("0000001000")).is_err());
    decoder.set_reference(&[0, 3]).unwrap();
    assert!(decoder.set_reference(&[1, 1]).is_err());
    assert!(decoder.set_reference(&[5]).is_err());
    assert!(StrictRowDecoder::new(0).is_err());
    assert_eq!(decoder.decode(&mut Bits::new("111")).unwrap(), [0, 3]);
}
#[test]
fn legacy_group4_recovery_remains_explicitly_unchanged() {
    let bytes = [0b01100000];
    let mut decoder = Group4Decoder::new(bytes.iter().copied().map(Ok::<_, ()>), 5).unwrap();
    assert_eq!(decoder.advance().unwrap(), DecodeStatus::Incomplete);
    assert_eq!(decoder.transition(), []);
}
#[test]
fn strict_all_vertical_offsets_pass_and_horizontal_share_backend_state_machine() {
    let mut decoder = StrictRowDecoder::new(15).unwrap();
    for delta in -3i8..=3 {
        decoder.set_reference(&[5, 10]).unwrap();
        let code = format!("{}11", mode::encode(Mode::Vertical(delta)).unwrap());
        let mut bits = Bits::new(code);
        assert_eq!(
            decoder.decode(&mut bits).unwrap(),
            [u16::try_from(5i16 + i16::from(delta)).unwrap(), 10]
        );
        assert_eq!(bits.position, bits.source.len());
    }
    decoder.set_reference(&[2, 4, 7]).unwrap();
    assert_eq!(decoder.decode(&mut Bits::new("000111")).unwrap(), [7]);
    decoder.set_reference(&[]).unwrap();
    assert!(decoder.decode(&mut Bits::new("0001")).is_err());
}
