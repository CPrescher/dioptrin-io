//! Bounded Group3 one-dimensional strips through the existing permissive fax
//! backend Huffman tables. This adapter owns strict TIFF row/EOL framing and
//! emits only complete terminating runs within the declared source window.
use super::{Error, Metadata, TagValue};
use crate::{PixelBuffer, source::Snapshot};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
use tiff_fax_backend::{
    BitReader,
    maps::{black, white},
};
pub(super) fn decode(
    snapshot: &Snapshot,
    identity: &File,
    page: &Metadata,
    limit: usize,
    target: &mut PixelBuffer,
) -> Result<(), Error> {
    snapshot.check_handle(identity).map_err(Error)?;
    let offsets = values(page, 273)?;
    let lengths = values(page, 279)?;
    let rows = page
        .tags
        .get(&278)
        .map(|v| v.clone().into_u64())
        .transpose()?
        .unwrap_or(page.shape[0] as u64)
        .min(page.shape[0] as u64) as usize;
    if rows == 0 || offsets.len() != page.shape[0].div_ceil(rows) || lengths.len() != offsets.len()
    {
        return Err(Error("invalid Group3 TIFF strip table".into()));
    }
    let width = u16::try_from(page.shape[1])
        .map_err(|_| Error("Group3 TIFF exceeds decoder width limit".into()))?;
    if !matches!(target, PixelBuffer::U8(_)) {
        *target = PixelBuffer::U8(Vec::new());
    }
    let PixelBuffer::U8(output) = target else {
        unreachable!()
    };
    output.clear();
    output
        .try_reserve_exact(page.shape[0] * page.shape[1])
        .map_err(|e| Error(e.to_string()))?;
    let mut file = File::open(&snapshot.path).map_err(io)?;
    snapshot.check_handle(&file).map_err(Error)?;
    let file_bytes = file.metadata().map_err(io)?.len();
    let mut encoded = Vec::new();
    for (strip, (offset, length)) in offsets.iter().zip(lengths).enumerate() {
        let offset = offset.clone().into_u64()?;
        let length = usize::try_from(length.clone().into_u64()?)
            .map_err(|_| Error("Group3 strip length overflow".into()))?;
        if length > limit || length.checked_mul(8).is_none() {
            return Err(Error(
                "Group3 encoded scratch exceeds intermediate limit".into(),
            ));
        }
        if offset
            .checked_add(length as u64)
            .is_none_or(|n| n > file_bytes)
        {
            return Err(Error("Group3 strip exceeds source bounds".into()));
        }
        file.seek(SeekFrom::Start(offset)).map_err(io)?;
        encoded.clear();
        encoded
            .try_reserve_exact(length)
            .map_err(|e| Error(e.to_string()))?;
        encoded.resize(length, 0);
        file.read_exact(&mut encoded).map_err(io)?;
        if page.fill_order == 2 {
            encoded
                .iter_mut()
                .for_each(|byte| *byte = byte.reverse_bits());
        }
        let height = rows.min(page.shape[0] - strip * rows);
        let byte_aligned = page
            .tags
            .get(&292)
            .map(|v| v.clone().into_u32())
            .transpose()?
            .unwrap_or(0)
            & 4
            != 0;
        decode_strip(&encoded, width, height, byte_aligned, output)?;
    }
    if output.len() != page.shape[0] * page.shape[1] {
        return Err(Error("Group3 decoded sample count mismatch".into()));
    }
    snapshot.check_handle(&file).map_err(Error)?;
    snapshot.check_handle(identity).map_err(Error)?;
    Ok(())
}
fn values(page: &Metadata, tag: u16) -> Result<&[TagValue], Error> {
    match page.tags.get(&tag) {
        Some(TagValue::List(values)) => Ok(values),
        Some(value) => Ok(std::slice::from_ref(value)),
        None => Err(Error(format!("missing Group3 strip tag{tag}"))),
    }
}
fn io(error: std::io::Error) -> Error {
    Error(error.to_string())
}

/// The backend's Huffman tables decode runs; this bounded adapter owns TIFF
/// row framing. In particular, a short zero prefix is never accepted as EOL.
struct Bits<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl BitReader for Bits<'_> {
    type Error = ();
    fn peek(&self, count: u8) -> Option<u16> {
        let end = self.position.checked_add(usize::from(count))?;
        if count > 16 || end > self.bytes.len() * 8 {
            return None;
        }
        let mut value = 0;
        for pos in self.position..end {
            value = (value << 1) | u16::from((self.bytes[pos / 8] >> (7 - pos % 8)) & 1);
        }
        Some(value)
    }
    fn consume(&mut self, count: u8) -> Result<(), ()> {
        self.peek(count).ok_or(())?;
        self.position += usize::from(count);
        Ok(())
    }
    fn bits_to_byte_boundary(&self) -> u8 {
        ((8 - self.position % 8) % 8) as u8
    }
}
impl Bits<'_> {
    fn eol(&mut self, byte_aligned: bool) -> Result<(), Error> {
        let mut zeros = 0;
        while self.peek(1) == Some(0) {
            self.position += 1;
            zeros += 1;
            if zeros > 18 {
                return Err(Error("Group3 EOL exceeds seven fill bits".into()));
            }
        }
        if zeros < 11 || self.peek(1) != Some(1) {
            return Err(Error("Group3 requires an eleven-zero EOL marker".into()));
        }
        self.position += 1;
        if byte_aligned && !self.position.is_multiple_of(8) || !byte_aligned && zeros != 11 {
            return Err(Error(
                "Group3 EOL fill contradicts declared T4Options".into(),
            ));
        }
        Ok(())
    }
    fn only_byte_padding(&self) -> bool {
        let remaining = self.bytes.len() * 8 - self.position;
        remaining < 8 && self.peek(remaining as u8) == Some(0)
    }
}
fn decode_strip(
    encoded: &[u8],
    width: u16,
    height: usize,
    byte_aligned: bool,
    output: &mut Vec<u8>,
) -> Result<(), Error> {
    let mut bits = Bits {
        bytes: encoded,
        position: 0,
    };
    for _ in 0..height {
        bits.eol(byte_aligned)?;
        let mut column = 0u16;
        let mut black_run = false;
        while column < width {
            let mut run = 0u16;
            loop {
                let count = decode_run(&mut bits, black_run)
                    .ok_or_else(|| Error("invalid/truncated Group3 Huffman run".into()))?;
                run = run
                    .checked_add(count)
                    .filter(|&n| n <= width - column)
                    .ok_or_else(|| Error("Group3 run exceeds declared row width".into()))?;
                if count < 64 {
                    break;
                }
            }
            output.extend(std::iter::repeat_n(u8::from(black_run), usize::from(run)));
            column += run;
            black_run = !black_run;
        }
    }
    // Class-F strips may end after the last complete row with byte padding.
    // Otherwise admit one trailing EOL, or the complete six-EOL RTC sequence.
    if bits.only_byte_padding() {
        return Ok(());
    }
    bits.eol(byte_aligned)?;
    if bits.only_byte_padding() {
        return Ok(());
    }
    for _ in 0..5 {
        bits.eol(byte_aligned)?;
    }
    if !bits.only_byte_padding() {
        return Err(Error("extra coded data after Group3 rows/RTC".into()));
    }
    Ok(())
}

fn decode_run(bits: &mut Bits<'_>, black_run: bool) -> Option<u16> {
    let start = bits.position;
    let decoded = if black_run {
        black::decode(bits)
    } else {
        white::decode(bits)
    };
    if decoded.is_some() {
        return decoded;
    }
    bits.position = start;
    // Some backend LUT leaves peek beyond a complete final codeword. Match
    // the backend's public codewords against only the remaining source bits;
    // no virtual lookahead, byte padding or consumption is supplied.
    if bits.bytes.len() * 8 - start >= 13 {
        return None;
    }
    for value in (0..64).chain((64..=2560).step_by(64)) {
        let code = if black_run {
            black::encode(value)
        } else {
            white::encode(value)
        }?;
        if bits.peek(code.len) == Some(code.data) {
            bits.consume(code.len).ok()?;
            return Some(value);
        }
    }
    None
}
