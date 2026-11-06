use std::convert::Infallible;

use crate::maps::{black, mode, white, Mode};
use crate::{BitReader, ByteReader, Color, Transitions};

fn with_markup<D, R>(decoder: D, reader: &mut R) -> Option<u16>
where
    D: Fn(&mut R) -> Option<u16>,
{
    let mut sum: u16 = 0;
    while let Some(n) = decoder(reader) {
        //print!("{} ", n);
        sum = sum.checked_add(n)?;
        if n < 64 {
            //debug!("= {}", sum);
            return Some(sum);
        }
    }
    None
}

fn colored(current: Color, reader: &mut impl BitReader) -> Option<u16> {
    //debug!("{:?}", current);
    match current {
        Color::Black => with_markup(black::decode, reader),
        Color::White => with_markup(white::decode, reader),
    }
}

/// Turn a list of color changing position into an iterator of pixel colors
///
/// The width of the line/image has to be given in `width`.
/// The iterator will produce exactly that many items.
pub fn pels(line: &[u16], width: u16) -> impl Iterator<Item = Color> + '_ {
    use std::iter::repeat;
    let mut color = Color::White;
    let mut last = 0;
    let pad_color = if line.len() & 1 == 1 { !color } else { color };
    line.iter()
        .flat_map(move |&p| {
            let c = color;
            color = !color;
            let n = p.saturating_sub(last);
            last = p;
            repeat(c).take(n as usize)
        })
        .chain(repeat(pad_color))
        .take(width as usize)
}

/// Decode a Group 3 encoded image.
///
/// The callback `line_cb` is called for each decoded line.
/// The argument is the list of positions of color change, starting with white.
///
/// To obtain an iterator over the pixel colors, the `pels` function is provided.
pub fn decode_g3(input: impl Iterator<Item = u8>, mut line_cb: impl FnMut(&[u16])) -> Option<()> {
    let reader = input.map(Result::<u8, Infallible>::Ok);
    let mut decoder = Group3Decoder::new(reader).ok()?;

    while let Ok(status) = decoder.advance() {
        // Always emit the decoded line before checking for end-of-document.
        // The last line before the RTC (Return To Control) marker contains
        // valid data that should not be dropped.
        line_cb(decoder.transitions());
        if status == DecodeStatus::End {
            return Some(());
        }
    }
    None
}

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum DecodeStatus {
    Incomplete,
    End,
}

pub struct Group3Decoder<R> {
    reader: ByteReader<R>,
    current: Vec<u16>,
}
impl<E: std::fmt::Debug, R: Iterator<Item = Result<u8, E>>> Group3Decoder<R> {
    pub fn new(reader: R) -> Result<Self, DecodeError<E>> {
        let mut reader = ByteReader::new(reader).map_err(DecodeError::Reader)?;
        // Skip any fill bits (zeros) then consume the initial EOL marker.
        skip_to_eol(&mut reader).map_err(|_| DecodeError::Invalid)?;

        Ok(Group3Decoder {
            reader,
            current: vec![],
        })
    }
    pub fn advance(&mut self) -> Result<DecodeStatus, DecodeError<E>> {
        self.current.clear();
        let mut a0: u16 = 0;
        let mut color = Color::White;
        loop {
            // Check for EOL before attempting to parse a run-length code.
            // This prevents the prefix tree from destructively consuming
            // EOL bits that it can't match as a valid code.
            if is_eol_ahead(&self.reader) {
                break;
            }
            match colored(color, &mut self.reader) {
                Some(p) => {
                    a0 = a0.checked_add(p).ok_or(DecodeError::Invalid)?;
                    self.current.push(a0);
                    color = !color;
                }
                None => break,
            }
        }
        // Skip any fill bits and consume the EOL.
        skip_to_eol(&mut self.reader).map_err(|_| DecodeError::Invalid)?;

        // Check for end-of-document: 6 consecutive EOLs (5 more after the one above).
        for _ in 0..5 {
            if is_eol_ahead(&self.reader) {
                skip_to_eol(&mut self.reader).map_err(|_| DecodeError::Invalid)?;
            } else {
                return Ok(DecodeStatus::Incomplete);
            }
        }

        Ok(DecodeStatus::End)
    }
    pub fn transitions(&self) -> &[u16] {
        &self.current
    }
}

/// Check if the next bits form an EOL marker (possibly with fill bits).
///
/// An EOL is `000000000001` (11 zeros + 1). Fill bits add extra leading
/// zeros for byte alignment (up to 7). No valid run-length code has more
/// than 7 leading zeros, so 8+ leading zeros guarantees fill + EOL.
///
/// We peek at 9 bits: if all zero, this is definitely fill+EOL or bare EOL
/// (the EOL itself starts with 11 zeros). This handles any fill count
/// without exceeding the 16-bit peek window.
fn is_eol_ahead<E, R: Iterator<Item = Result<u8, E>>>(reader: &ByteReader<R>) -> bool {
    // 9 zero bits cannot be the start of any valid run-length code
    // (max leading zeros in any code is 7). Must be fill + EOL.
    // This also matches bare EOL (000000000001) since its first 9 bits are zero.
    reader.peek(9) == Some(0)
}

/// Skip zero fill bits and consume the EOL marker (000000000001).
/// Returns Err if no valid EOL is found.
fn skip_to_eol<E: std::fmt::Debug, R: Iterator<Item = Result<u8, E>>>(
    reader: &mut ByteReader<R>,
) -> Result<(), DecodeError<E>> {
    // Skip zero fill bits (used for byte alignment in Group3Options bit 2).
    while reader.peek(1) == Some(0) {
        reader.consume(1).map_err(DecodeError::Reader)?;
    }
    // The next bit should be the '1' that terminates the EOL.
    if reader.peek(1) == Some(1) {
        reader.consume(1).map_err(DecodeError::Reader)?;
        Ok(())
    } else {
        Err(DecodeError::Invalid)
    }
}

/// Decode a Group 4 Image
///
/// - `width` is the width of the image.
/// - The callback `line_cb` is called for each decoded line.
///   The argument is the list of positions of color change, starting with white.
///
///   If `height` is specified, at most that many lines will be decoded,
///   otherwise data is decoded until the end-of-block marker (or end of data).
///
/// To obtain an iterator over the pixel colors, the `pels` function is provided.
pub fn decode_g4(
    input: impl Iterator<Item = u8>,
    width: u16,
    height: Option<u16>,
    mut line_cb: impl FnMut(&[u16]),
) -> Option<()> {
    let reader = input.map(Result::<u8, Infallible>::Ok);
    let mut decoder = Group4Decoder::new(reader, width).ok()?;

    let max_lines = height.unwrap_or(u16::MAX);
    let mut lines_emitted: u16 = 0;

    while lines_emitted < max_lines {
        let status = decoder.advance().ok()?;
        if status == DecodeStatus::End {
            break;
        }
        line_cb(decoder.transition());
        lines_emitted += 1;
    }

    // Some encoders omit trailing all-white lines before the EOFB,
    // expecting the receiver to pad to the known height.
    // Empty transitions = all-white line (pels handles this correctly).
    if let Some(h) = height {
        while lines_emitted < h {
            line_cb(&[]);
            lines_emitted += 1;
        }
    }

    Some(())
}

#[derive(Debug)]
pub enum DecodeError<E> {
    Reader(E),
    Invalid,
    Unsupported,
}
impl<E> std::fmt::Display for DecodeError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Decode Error")
    }
}
impl<E: std::error::Error> std::error::Error for DecodeError<E> {}

pub struct Group4Decoder<R> {
    reader: ByteReader<R>,
    reference: Vec<u16>,
    current: Vec<u16>,
    width: u16,
}
impl<E, R: Iterator<Item = Result<u8, E>>> Group4Decoder<R> {
    pub fn new(reader: R, width: u16) -> Result<Self, E> {
        Ok(Group4Decoder {
            reader: ByteReader::new(reader)?,
            reference: Vec::new(),
            current: Vec::new(),
            width,
        })
    }
    // when Complete::Complete is returned, there is no useful data in .transitions() or .line()
    pub fn advance(&mut self) -> Result<DecodeStatus, DecodeError<E>> {
        let status = decode_2d_row(
            &mut self.reader,
            &self.reference,
            &mut self.current,
            self.width,
            false,
        )?;
        if status == DecodeStatus::End {
            return Ok(status);
        }
        std::mem::swap(&mut self.reference, &mut self.current);
        self.current.clear();
        Ok(status)
    }

    pub fn transition(&self) -> &[u16] {
        &self.reference
    }

    pub fn line(&self) -> Line<'_> {
        Line {
            transitions: &self.reference,
            width: self.width,
        }
    }
}

// The compatibility Group4 API and strict TIFF mixed-mode row API share this
// mature state machine. Strict mode adds bounds/progress checks without
// changing legacy Group4 recovery behavior.
fn decode_2d_row<B: crate::BitReader>(
    reader: &mut B,
    reference: &[u16],
    current: &mut Vec<u16>,
    width: u16,
    strict: bool,
) -> Result<DecodeStatus, DecodeError<B::Error>> {
    let mut transitions = Transitions::new(reference);
    let mut a0 = 0;
    let mut color = Color::White;
    let mut start_of_row = true;
    //debug!("\n\nline {}", y);

    loop {
        //reader.print_peek();
        let mode = match if strict {
            strict_mode(reader)?
        } else {
            mode::decode(reader)
        } {
            Some(mode) => mode,
            None => return Err(DecodeError::Invalid),
        };
        //debug!("  {:?}, color={:?}, a0={}", mode, color, a0);

        match mode {
            Mode::Pass => {
                if start_of_row && color == Color::White {
                    transitions.pos += 1;
                } else {
                    transitions
                        .next_color(a0, !color, false)
                        .ok_or(DecodeError::Invalid)?;
                }
                //debug!("b1={}", b1);
                if let Some(b2) = transitions.next() {
                    if strict && (b2 <= a0 || b2 >= width) {
                        return Err(DecodeError::Invalid);
                    }
                    //debug!("b2={}", b2);
                    a0 = b2;
                } else if strict {
                    return Err(DecodeError::Invalid);
                }
            }
            Mode::Vertical(delta) => {
                let b1 = transitions
                    .next_color(a0, !color, start_of_row)
                    .unwrap_or(width);
                let a1_i32 = b1 as i32 + delta as i32;
                if a1_i32 < 0 || a1_i32 > width as i32 {
                    if strict {
                        return Err(DecodeError::Invalid);
                    }
                    break;
                }
                let a1 = a1_i32 as u16;
                if strict && (a1 < a0 || a1 == a0 && !(start_of_row && color == Color::White)) {
                    return Err(DecodeError::Invalid);
                }
                //debug!("transition to {:?} at {}", !color, a1);
                // Canonical form: only store transitions strictly less
                // than width. A transition at width is the implicit
                // end-of-line and is not a color change. This matches
                // the encoder's `current` representation (see
                // encoder.rs — it only pushes values yielded by pels,
                // which are always in [0, width-1]).
                if a1 < width {
                    current.push(a1);
                }
                color = !color;
                a0 = a1;
                if delta < 0 {
                    transitions.seek_back(a0);
                }
            }
            Mode::Horizontal => {
                let a0a1 = if strict {
                    strict_colored(color, reader, width - a0)?
                } else {
                    colored(color, reader).ok_or(DecodeError::Invalid)?
                };
                let a1a2 = if strict {
                    None
                } else {
                    Some(colored(!color, reader).ok_or(DecodeError::Invalid)?)
                };
                let a1 = a0.checked_add(a0a1).ok_or(DecodeError::Invalid)?;
                let a1a2 = match a1a2 {
                    Some(n) => n,
                    None => strict_colored(!color, reader, width - a1)?,
                };
                let a2 = a1.checked_add(a1a2).ok_or(DecodeError::Invalid)?;
                if strict
                    && (a2 > width
                        || a1 == a0 && !(start_of_row && color == Color::White)
                        || a2 == a1 && a1 != width)
                {
                    return Err(DecodeError::Invalid);
                }
                //debug!("a0a1={}, a1a2={}, a1={}, a2={}", a0a1, a1a2, a1, a2);

                // Same canonical form rule: never store a transition
                // at width (it's the end-of-line sentinel, not a flip).
                if a1 < width {
                    current.push(a1);
                }
                if a2 >= width {
                    break;
                }
                current.push(a2);
                a0 = a2;
            }
            Mode::Extension => {
                let _ext = reader.peek(3).ok_or(DecodeError::Invalid)?;
                let _ = reader.consume(3);
                return Err(DecodeError::Unsupported);
            }
            Mode::EOF => {
                return if strict {
                    Err(DecodeError::Invalid)
                } else {
                    Ok(DecodeStatus::End)
                }
            }
        }
        start_of_row = false;

        if a0 >= width {
            break;
        }
    }
    //debug!("{:?}", current);

    Ok(DecodeStatus::Incomplete)
}

/// One strict T.4/T.6 two-dimensional row at a time, driven by the caller's
/// bounded bit reader. No EOL/RTC/EOFB is consumed or inferred here. TIFF owns
/// those delimiters and must supply/reset the reference for one-dimensional rows.
/// The Group4 compatibility API above uses the same state machine with its
/// historical recovery behavior; this API never returns an incomplete row.
pub struct StrictRowDecoder {
    reference: Vec<u16>,
    current: Vec<u16>,
    width: u16,
}
impl StrictRowDecoder {
    pub fn new(width: u16) -> Result<Self, DecodeError<()>> {
        if width == 0 {
            return Err(DecodeError::Invalid);
        }
        let mut reference = Vec::new();
        let mut current = Vec::new();
        reference
            .try_reserve_exact(usize::from(width))
            .map_err(|_| DecodeError::Invalid)?;
        current
            .try_reserve_exact(usize::from(width))
            .map_err(|_| DecodeError::Invalid)?;
        Ok(Self {
            reference,
            current,
            width,
        })
    }
    /// Canonical changing elements: strictly increasing positions below width,
    /// with optional position zero for a row starting black. Empty means white.
    pub fn set_reference(&mut self, transitions: &[u16]) -> Result<(), DecodeError<()>> {
        if transitions.len() > usize::from(self.width)
            || transitions.iter().any(|v| *v >= self.width)
            || transitions.windows(2).any(|w| w[0] >= w[1])
        {
            return Err(DecodeError::Invalid);
        }
        self.reference.clear();
        self.reference.extend_from_slice(transitions);
        Ok(())
    }
    /// Successful output is a complete row. On failure the old reference stays
    /// intact and the incomplete current row is discarded.
    pub fn decode<B: crate::BitReader>(
        &mut self,
        reader: &mut B,
    ) -> Result<&[u16], DecodeError<B::Error>> {
        self.current.clear();
        if let Err(error) =
            decode_2d_row(reader, &self.reference, &mut self.current, self.width, true)
        {
            self.current.clear();
            return Err(error);
        }
        std::mem::swap(&mut self.reference, &mut self.current);
        self.current.clear();
        Ok(&self.reference)
    }
}

// Table decoders may consume a prefix before a failed lookahead. Stage their
// peeks/consumes without changing the caller's source position until a complete
// codeword is known. The fallback uses the same backend's public codewords;
// no zero extension or virtual source bits are supplied at strip EOF.
struct Staged<'a, B> {
    reader: &'a B,
    consumed: u8,
}
impl<B: crate::BitReader> crate::BitReader for Staged<'_, B> {
    type Error = ();
    fn peek(&self, n: u8) -> Option<u16> {
        let total = self.consumed.checked_add(n)?;
        if total > 16 {
            return None;
        }
        let mask = if n == 16 { u16::MAX } else { (1u16 << n) - 1 };
        Some(self.reader.peek(total)? & mask)
    }
    fn consume(&mut self, n: u8) -> Result<(), ()> {
        self.peek(n).ok_or(())?;
        self.consumed += n;
        Ok(())
    }
    fn bits_to_byte_boundary(&self) -> u8 {
        (self.reader.bits_to_byte_boundary() + 8 - self.consumed % 8) % 8
    }
}
fn strict_mode<B: crate::BitReader>(reader: &mut B) -> Result<Option<Mode>, DecodeError<B::Error>> {
    let mut staged = Staged {
        reader: &*reader,
        consumed: 0,
    };
    if let Some(value) = mode::decode(&mut staged) {
        let consumed = staged.consumed;
        reader.consume(consumed).map_err(DecodeError::Reader)?;
        return Ok(Some(value));
    }
    for value in [
        Mode::Pass,
        Mode::Horizontal,
        Mode::Vertical(0),
        Mode::Vertical(1),
        Mode::Vertical(2),
        Mode::Vertical(3),
        Mode::Vertical(-1),
        Mode::Vertical(-2),
        Mode::Vertical(-3),
        Mode::Extension,
        Mode::EOF,
    ] {
        let Some(code) = mode::encode(value) else {
            continue;
        };
        if reader.peek(code.len) == Some(code.data) {
            reader.consume(code.len).map_err(DecodeError::Reader)?;
            return Ok(Some(value));
        }
    }
    Ok(None)
}
fn strict_colored<B: crate::BitReader>(
    color: Color,
    reader: &mut B,
    maximum: u16,
) -> Result<u16, DecodeError<B::Error>> {
    let mut total = 0u16;
    loop {
        let mut staged = Staged {
            reader: &*reader,
            consumed: 0,
        };
        let decoded = match color {
            Color::White => white::decode(&mut staged),
            Color::Black => black::decode(&mut staged),
        };
        let (value, length) = if let Some(value) = decoded {
            (value, staged.consumed)
        } else {
            let mut found = None;
            for value in (0..64).chain((64..=2560).step_by(64)) {
                let code = match color {
                    Color::White => white::encode(value),
                    Color::Black => black::encode(value),
                };
                if let Some(code) = code {
                    if reader.peek(code.len) == Some(code.data) {
                        found = Some((value, code.len));
                        break;
                    }
                }
            }
            found.ok_or(DecodeError::Invalid)?
        };
        reader.consume(length).map_err(DecodeError::Reader)?;
        total = total
            .checked_add(value)
            .filter(|n| *n <= maximum)
            .ok_or(DecodeError::Invalid)?;
        if value < 64 {
            return Ok(total);
        }
    }
}

pub struct Line<'a> {
    pub transitions: &'a [u16],
    pub width: u16,
}
impl<'a> Line<'a> {
    pub fn pels(&self) -> impl Iterator<Item = Color> + 'a {
        pels(&self.transitions, self.width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fuzz artifact: 5 bytes that triggered checked_add overflow in G4
    /// horizontal mode before the fix. The overflow is now caught by
    /// checked_add and the decoder recovers, producing partial output.
    #[test]
    fn g4_fuzz_crash_horizontal_overflow() {
        let data: Vec<u8> = vec![0xe8, 0x05, 0x00, 0x00, 0x00];
        let mut lines = 0u32;
        let result = decode_g4(data.into_iter(), 100, Some(10), |_| {
            lines += 1;
        });
        // Decoder recovers from the overflow and produces some lines.
        // The key assertion: no panic. Before the fix this was an
        // "attempt to add with overflow" panic.
        assert!(
            result.is_some(),
            "decoder should recover from caught overflow"
        );
        assert!(lines <= 10, "should not exceed requested height");
    }

    /// Fuzz artifact: 119 bytes that triggered G3 run-length overflow.
    /// After the fix, checked_add returns DecodeError::Invalid and the
    /// decoder returns None.
    #[test]
    fn g3_fuzz_crash_run_length_overflow() {
        let mut data = vec![
            0x10, 0x10, 0x00, 0x04, 0x00, 0x10, 0x00, 0xb3, 0x00, 0x00, 0x10, 0x00, 0xb3, 0x00,
            0x10, 0x10,
        ];
        data.extend_from_slice(&[0xce; 103]);
        let result = decode_g3(data.into_iter(), |_| {});
        assert_eq!(result, None, "corrupt G3 data should return None");
    }

    /// Width > 32767 used to overflow i16 in vertical mode delta.
    /// Now uses i32 — must not panic.
    #[test]
    fn g4_large_width_no_overflow() {
        let data: Vec<u8> = vec![0x00; 512];
        let result = decode_g4(data.into_iter(), 40000, Some(1), |_| {});
        let _ = result; // must not panic
    }

    /// Zero-width image: degenerate, must not loop forever or panic.
    #[test]
    fn g4_zero_width_no_panic() {
        let data: Vec<u8> = vec![0x00; 64];
        let result = decode_g4(data.into_iter(), 0, Some(1), |_| {});
        let _ = result; // must not panic
    }

    /// Random bytes fed to G3 decoder — must not panic regardless of content.
    #[test]
    fn g3_random_bytes_no_panic() {
        let data: Vec<u8> = (0..512).map(|i| (i * 37 + 13) as u8).collect();
        let result = decode_g3(data.into_iter(), |_| {});
        let _ = result; // must not panic
    }

    /// Roundtrip: a line with a color change at width-1 should produce
    /// the same pels after encode→decode. Note that transition lists are
    /// NOT a canonical representation — e.g., `[3]` and `[3, 4]` both
    /// represent "3 white + 1 black" at width=4. We compare pels (the
    /// semantic form) rather than transition lists.
    #[test]
    fn g4_roundtrip_width_boundary_transition() {
        let transitions = vec![3u16, 4];
        let width = 4u16;
        let input_pels: Vec<_> = super::pels(&transitions, width).collect();
        let writer = crate::VecWriter::new();
        let mut encoder = crate::encoder::Encoder::new(writer);
        let _ = encoder.encode_line(input_pels.iter().copied(), width);
        let encoded = encoder.finish().unwrap().finish();
        let mut decoded = Vec::new();
        let _ = decode_g4(encoded.into_iter(), width, Some(1), |line| {
            decoded.push(line.to_vec());
        });
        let decoded_line = decoded.first().expect("decoded one line");
        let output_pels: Vec<_> = super::pels(decoded_line, width).collect();
        assert_eq!(
            input_pels, output_pels,
            "pels must roundtrip (decoded transitions: {:?})",
            decoded_line
        );
    }

    /// Single transition at arbitrary position should roundtrip cleanly.
    /// Regression for the 23 "crash" artifacts surfaced by cargo fuzz cmin:
    /// the decoder was producing non-canonical transition lists (appending
    /// width sentinel), which the fuzz assertion flagged as mismatches.
    #[test]
    fn g4_roundtrip_canonical_form() {
        for &(width, ref transitions) in &[
            (10u16, vec![5]),
            (2000, vec![10]),
            (2000, vec![3, 51]),
            (4, vec![3]),
            (100, vec![50]),
            (100, vec![1]),
            (100, vec![99]),
        ] {
            let input_pels: Vec<_> = super::pels(transitions, width).collect();
            let writer = crate::VecWriter::new();
            let mut encoder = crate::encoder::Encoder::new(writer);
            let _ = encoder.encode_line(input_pels.iter().copied(), width);
            let encoded = encoder.finish().unwrap().finish();
            let mut decoded = Vec::new();
            let _ = decode_g4(encoded.into_iter(), width, Some(1), |line| {
                decoded.push(line.to_vec());
            });
            let decoded_line = decoded.first().expect("decoded one line");
            let output_pels: Vec<_> = super::pels(decoded_line, width).collect();
            assert_eq!(
                input_pels, output_pels,
                "pels must roundtrip for width={width} transitions={transitions:?}, \
                 got decoded transitions {decoded_line:?}"
            );
            // Canonical form: decoder must not append the width sentinel.
            assert!(
                decoded_line.iter().all(|&t| t < width),
                "decoder produced non-canonical transition list {decoded_line:?} \
                 (contains width={width}); transitions should all be < width",
                decoded_line = decoded_line,
                width = width
            );
        }
    }
}
