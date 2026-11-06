//! Local bounded adapter for the pre-TIFF-6 LSB/late-change LZW dialect.
//! Pixel prediction, sample conversion and strip/tile placement stay upstream.
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Match the first nine-bit clear code in the original compressed bytes.
/// This disjoint signature is also used by libtiff's LZWPreDecode; it is never
/// selected by trying another decoder after an MSB/early-change decode failure.
pub(super) fn recognized(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0 && bytes[1] & 1 == 1
}

/// Validate terminal code boundaries without decoding a dictionary or pixels.
/// Public weezl APIs report consumed bytes, including bit-reader lookahead, but
/// not the exact bit where EOI ends. Track only code widths/clear resets here so
/// trailing bytes or nonzero padding cannot disappear inside that lookahead.
fn validate_end(encoded: &[u8], legacy: bool) -> io::Result<()> {
    let bits = encoded
        .len()
        .checked_mul(8)
        .ok_or_else(|| invalid("legacy LZW bit count overflow"))?;
    let mut position = 0usize;
    let mut width = 9usize;
    let mut next = 258u16;
    let mut previous = false;
    loop {
        let stop = position
            .checked_add(width)
            .filter(|&n| n <= bits)
            .ok_or_else(|| invalid("legacy LZW lacks a complete end code"))?;
        let mut code = 0u16;
        for (shift, bit) in (position..stop).enumerate() {
            let source_bit = if legacy { bit % 8 } else { 7 - bit % 8 };
            let code_bit = if legacy { shift } else { width - 1 - shift };
            code |= u16::from((encoded[bit / 8] >> source_bit) & 1) << code_bit;
        }
        position = stop;
        match code {
            256 => {
                width = 9;
                next = 258;
                previous = false;
            }
            257 => {
                // The pinned imagecodecs writer flushes one zero byte when
                // modern EOI is already byte aligned. Eight zero bits cannot
                // contain another nine-bit code; longer/nonzero tails fail.
                if bits - position > if legacy { 7 } else { 8 }
                    || (position..bits).any(|bit| {
                        encoded[bit / 8] & (1 << if legacy { bit % 8 } else { 7 - bit % 8 }) != 0
                    })
                {
                    return Err(invalid("legacy LZW contains data after end code"));
                }
                return Ok(());
            }
            _ => {
                if code > next || code == next && !previous {
                    return Err(invalid("invalid legacy LZW code boundary"));
                }
                if previous && next < 4096 {
                    next += 1;
                    if next == (1 << width) - u16::from(!legacy) && width < 12 {
                        width += 1;
                    }
                }
                previous = true;
            }
        }
    }
}

/// Eagerly validate the complete legacy chunk, including padded tile rows,
/// before upstream raster code consumes any decoded source bytes. The extra
/// output byte detects expansion beyond the exact admitted source byte count.
pub(super) fn decode(encoded: &[u8], expected: usize, legacy: bool) -> io::Result<Vec<u8>> {
    if legacy && !recognized(encoded)
        || !legacy && (encoded.len() < 2 || encoded[0] != 128 || encoded[1] & 128 != 0)
    {
        return Err(invalid("missing legacy LZW initial clear code"));
    }
    validate_end(encoded, legacy)?;
    let capacity = expected
        .checked_add(1)
        .ok_or_else(|| invalid("legacy LZW output length overflow"))?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|e| io::Error::other(e.to_string()))?;
    output.resize(capacity, 0);
    let mut decoder = if legacy {
        weezl::decode::Decoder::new(weezl::BitOrder::Lsb, 8)
    } else {
        weezl::decode::Decoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
    };
    let (mut input, mut written) = (0usize, 0usize);
    loop {
        let result = decoder.decode_bytes(&encoded[input..], &mut output[written..]);
        input += result.consumed_in;
        written += result.consumed_out;
        if written > expected {
            return Err(invalid("legacy LZW expands beyond declared source rows"));
        }
        match result.status {
            Ok(weezl::LzwStatus::Done) => {
                if written != expected {
                    return Err(invalid("legacy LZW decoded source byte count mismatch"));
                }
                output.truncate(expected);
                return Ok(output);
            }
            Ok(weezl::LzwStatus::NoProgress) => {
                return Err(invalid("legacy LZW stream did not reach end code"))
            }
            Ok(weezl::LzwStatus::Ok) if result.consumed_in != 0 || result.consumed_out != 0 => {}
            Ok(_) => return Err(invalid("legacy LZW decoder made no progress")),
            Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
        }
    }
}
