//! Shared bounded byte and scalar utilities for CBF/EDF readers.
#[cfg(any(feature = "cbf", feature = "edf", feature = "native"))]
use crate::{PixelBuffer, PixelType};
use std::fmt;
#[derive(Debug)]
pub struct Error(pub(crate) String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub frames: usize,
    pub header_bytes: usize,
    pub metadata_bytes: usize,
    pub frame_bytes: usize,
    pub encoded_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            frames: 100_000,
            header_bytes: 1024 * 1024,
            metadata_bytes: 16 * 1024 * 1024,
            frame_bytes: 256 * 1024 * 1024,
            encoded_bytes: 256 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub(crate) fn validate(self) -> Result<(), Error> {
        if [
            self.frames,
            self.header_bytes,
            self.metadata_bytes,
            self.frame_bytes,
            self.encoded_bytes,
        ]
        .contains(&0)
        {
            return Err(Error("detector read limits must be positive".into()));
        }
        Ok(())
    }
    #[cfg(any(feature = "cbf", feature = "edf", feature = "native"))]
    pub(crate) fn pixels(self, shape: [usize; 2], ty: PixelType) -> Result<usize, Error> {
        let n = shape[0]
            .checked_mul(shape[1])
            .filter(|&n| n > 0)
            .ok_or_else(|| Error("invalid detector dimensions".into()))?;
        if n.checked_mul(ty.bytes())
            .is_none_or(|n| n > self.frame_bytes)
        {
            return Err(Error("detector frame exceeds byte limit".into()));
        }
        Ok(n)
    }
}
#[derive(Debug, Default)]
#[cfg(any(feature = "cbf", feature = "edf", feature = "native"))]
pub struct FrameBuffer {
    pub(crate) pixels: PixelBuffer,
    pub(crate) encoded: Vec<u8>,
    #[cfg(feature = "edf")]
    pub(crate) decoded: Vec<u8>,
    pub(crate) shape: Option<[usize; 2]>,
}
#[cfg(any(feature = "cbf", feature = "edf", feature = "native"))]
impl FrameBuffer {
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        self.shape.map(|_| &self.pixels)
    }
    #[must_use]
    pub fn shape(&self) -> Option<[usize; 2]> {
        self.shape
    }
}
#[cfg(any(feature = "cbf", feature = "edf", feature = "native"))]
pub(crate) fn resize_bytes(v: &mut Vec<u8>, len: usize) -> Result<(), Error> {
    v.try_reserve_exact(len.saturating_sub(v.len()))
        .map_err(|e| Error(e.to_string()))?;
    v.resize(len, 0);
    Ok(())
}
#[cfg(any(feature = "edf", feature = "native"))]
pub(crate) fn copy_samples(
    input: &[u8],
    output: &mut PixelBuffer,
    little: bool,
) -> Result<(), Error> {
    let size = output.pixel_type().component_bytes();
    let bytes = output.bytes_mut();
    if input.len() != bytes.len() {
        return Err(Error("decoded detector payload size mismatch".into()));
    }
    bytes.copy_from_slice(input);
    if little != cfg!(target_endian = "little") {
        for sample in bytes.chunks_exact_mut(size) {
            sample.reverse();
        }
    }
    Ok(())
}

#[cfg(all(test, any(feature = "native", feature = "edf")))]
mod complex_tests {
    use super::*;
    #[test]
    fn complex_endian_conversion_preserves_components_bits_and_integer_storage() {
        macro_rules! check {
            ($variant:ident,$ty:ty,$a:expr,$b:expr) => {{
                let expected: [$ty; 2] = [$a, $b];
                for little in [true, false] {
                    let mut bytes = Vec::new();
                    for value in expected {
                        bytes.extend(if little {
                            value.to_le_bytes()
                        } else {
                            value.to_be_bytes()
                        });
                    }
                    let mut output = PixelBuffer::default();
                    output.resize(PixelType::$variant, 1).unwrap();
                    copy_samples(&bytes, &mut output, little).unwrap();
                    let PixelBuffer::$variant(values) = output else {
                        panic!("type")
                    };
                    assert_eq!(values.len(), 1);
                    for (actual, expected) in values[0].iter().zip(expected) {
                        assert_eq!(actual.to_le_bytes(), expected.to_le_bytes());
                    }
                }
            }};
        }
        check!(C32, f32, f32::from_bits(0x7fc12345), -0.0f32);
        check!(C64, f64, f64::from_bits(0x7ff8123456789abc), -0.0f64);
        check!(CI16, i16, i16::MIN, i16::MAX);
    }
}

#[cfg(feature = "cbf")]
pub(crate) fn byte_offset(
    input: &[u8],
    output: &mut PixelBuffer,
    extended: bool,
) -> Result<(), Error> {
    let mut offset = 0;
    let mut value = 0_i128;
    fn take<const N: usize>(input: &[u8], offset: &mut usize) -> Result<[u8; N], Error> {
        let end = offset
            .checked_add(N)
            .ok_or_else(|| Error("byte-offset position overflow".into()))?;
        let bytes = input
            .get(*offset..end)
            .ok_or_else(|| Error("truncated CBF byte-offset escape".into()))?;
        *offset = end;
        Ok(bytes.try_into().unwrap())
    }
    macro_rules! decode { ($($variant:ident,$ty:ty);*)=> {match output { $(PixelBuffer::$variant(v)=>{
        for sample in v {
            let mut delta=i8::from_le_bytes(take(input,&mut offset)?) as i64;
            if delta==i8::MIN as i64 {delta=i16::from_le_bytes(take(input,&mut offset)?) as i64;
                if delta==i16::MIN as i64 {delta=i32::from_le_bytes(take(input,&mut offset)?) as i64;
                    if extended && delta==i32::MIN as i64 {delta=i64::from_le_bytes(take(input,&mut offset)?);}
                }
            }
            value+=i128::from(delta);
            if !extended { value=i128::from(value as i32); }
            *sample=<$ty>::try_from(value).map_err(|_|Error("CBF count overflow for declared sample type".into()))?;
        }
    }),*, _=>return Err(Error("CBF byte-offset requires integer samples".into()))}}; }
    decode!(U8,u8;U16,u16;U32,u32;U64,u64;I8,i8;I16,i16;I32,i32;I64,i64);
    if offset != input.len() {
        return Err(Error("extra CBF byte-offset samples/payload".into()));
    }
    Ok(())
}

#[cfg(all(test, feature = "cbf"))]
mod tests {
    use super::*;
    #[test]
    fn byte_offset_escapes_truncation_count_mismatch_and_overflow_are_rejected() {
        for input in [
            &[128][..],
            &[128, 0][..],
            &[128, 0, 128][..],
            &[128, 0, 128, 0, 0, 0, 128][..],
        ] {
            assert!(byte_offset(input, &mut PixelBuffer::I64(vec![0]), true).is_err());
        }
        assert!(byte_offset(&[1, 2], &mut PixelBuffer::I8(vec![0]), true).is_err());
        assert!(byte_offset(&[127, 127], &mut PixelBuffer::I8(vec![0; 2]), true).is_err());
        let legacy = [128, 0, 128, 0, 0, 0, 128];
        let mut counts = PixelBuffer::I32(vec![0]);
        assert!(byte_offset(&legacy, &mut counts, true).is_err());
        byte_offset(&legacy, &mut counts, false).unwrap();
        assert_eq!(counts, PixelBuffer::I32(vec![i32::MIN]));
    }
}
