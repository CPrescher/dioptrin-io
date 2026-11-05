//! Explicit consumer interpretation of Bruker86's LINEAR floating-image encoding.
//! Stored counts, source dtype and coefficients remain available from the reader.
//! This operation applies only the file's encoding; it performs no detector,
//! geometric, intensity-normalization or other acquisition correction.
use super::{Error, FrameBuffer, Reader, Version};
use crate::PixelBuffer;

/// Read a float-encoded Bruker86 image as the pinned FabIO reader does: expand
/// storage tables, multiply in f64, add the offset in a separate f64 operation,
/// then round once to f32. Absent/identity LINEAR keeps the original integer type.
/// IEEE nonfinite results from an explicit precision conversion remain results
/// of this derived view, never replacements for inaccessible source words.
///
/// # Errors
/// Wrong format, output budget, source mutation, malformed storage or allocation.
pub fn read_linear_into(reader: &Reader, buffer: &mut FrameBuffer) -> Result<(), Error> {
    buffer.shape = None;
    if reader.metadata.version != Version::V86 {
        return Err(Error("Bruker LINEAR view requires format 86".into()));
    }
    let Some([slope, offset]) = reader.metadata.linear else {
        return reader.read_frame_into(buffer);
    };
    if slope == 1.0 && offset == 0.0 {
        return reader.read_frame_into(buffer);
    }
    let count = reader.metadata.shape[0] * reader.metadata.shape[1];
    if count.checked_mul(4).is_none_or(|n| n > reader.frame_bytes) {
        return Err(Error(
            "Bruker LINEAR f32 output exceeds frame budget".into(),
        ));
    }
    reader.read_frame_into(buffer)?;
    // Conversion errors must invalidate even the successful stored-read result.
    buffer.shape = None;
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|e| Error(e.to_string()))?;
    macro_rules! decode {
        ($values:expr) => {
            for &value in $values {
                let scaled = f64::from(value) * slope;
                out.push((scaled + offset) as f32);
            }
        };
    }
    match &buffer.pixels {
        PixelBuffer::U8(v) => decode!(v),
        PixelBuffer::U16(v) => decode!(v),
        PixelBuffer::U32(v) => decode!(v),
        _ => {
            return Err(Error(
                "unsupported stored Bruker86 type for LINEAR view".into(),
            ));
        }
    }
    reader.source.check()?;
    buffer.pixels = PixelBuffer::F32(out);
    buffer.shape = Some(reader.metadata.shape);
    Ok(())
}
