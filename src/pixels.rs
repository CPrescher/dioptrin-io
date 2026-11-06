//! Owned native samples shared by optional format readers.

/// Original scalar sample type, before any consumer conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelType {
    /// Logical boolean with original one-byte storage retained.
    Bool,
    /// IEEE binary16. Buffers retain raw 16-bit patterns, including NaN payloads.
    F16,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    /// Complex sample with IEEE f32 real/imaginary components (NumPy complex64).
    C32,
    /// Complex sample with IEEE f64 components (NumPy complex128).
    C64,
    /// Complex sample stored as an exact signed-i16 real/imaginary pair.
    CI16,
}
impl PixelType {
    /// Bytes per scalar in the returned native buffer.
    #[must_use]
    pub const fn bytes(self) -> usize {
        match self {
            Self::Bool | Self::U8 | Self::I8 => 1,
            Self::F16 | Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 | Self::CI16 => 4,
            Self::U64 | Self::I64 | Self::F64 | Self::C32 => 8,
            Self::C64 => 16,
        }
    }
    /// Width of one endian-converted component; complex pairs keep real before
    /// imaginary. Reversing the entire pair would silently exchange components.
    #[must_use]
    pub const fn component_bytes(self) -> usize {
        match self {
            Self::C32 => 4,
            Self::C64 => 8,
            Self::CI16 => 2,
            _ => self.bytes(),
        }
    }
}

/// Owned, contiguous row-major samples. No scaling, clipping or float conversion.
/// Consumers can match a variant and pass its slice directly to numerical code.
/// Bool/F16 variants are explicit storage-bit containers; callers choose any
/// logical boolean or floating-point widening view separately.
#[derive(Debug, Clone, PartialEq)]
pub enum PixelBuffer {
    /// Original boolean storage bytes. A nonzero byte has logical value true;
    /// noncanonical bytes are not silently normalized to one.
    Bool(Vec<u8>),
    /// Original IEEE binary16 bits, not unsigned integer detector counts.
    F16(Vec<u16>),
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    U64(Vec<u64>),
    I8(Vec<i8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    F32(Vec<f32>),
    F64(Vec<f64>),
    C32(Vec<[f32; 2]>),
    C64(Vec<[f64; 2]>),
    CI16(Vec<[i16; 2]>),
}
impl Default for PixelBuffer {
    fn default() -> Self {
        Self::U8(Vec::new())
    }
}
impl PixelBuffer {
    /// Original sample type.
    #[must_use]
    pub fn pixel_type(&self) -> PixelType {
        match self {
            Self::Bool(_) => PixelType::Bool,
            Self::F16(_) => PixelType::F16,
            Self::U8(_) => PixelType::U8,
            Self::U16(_) => PixelType::U16,
            Self::U32(_) => PixelType::U32,
            Self::U64(_) => PixelType::U64,
            Self::I8(_) => PixelType::I8,
            Self::I16(_) => PixelType::I16,
            Self::I32(_) => PixelType::I32,
            Self::I64(_) => PixelType::I64,
            Self::F32(_) => PixelType::F32,
            Self::F64(_) => PixelType::F64,
            Self::C32(_) => PixelType::C32,
            Self::C64(_) => PixelType::C64,
            Self::CI16(_) => PixelType::CI16,
        }
    }
    /// Number of scalar samples.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Bool(v) => v.len(),
            Self::F16(v) => v.len(),
            Self::U8(v) => v.len(),
            Self::U16(v) => v.len(),
            Self::U32(v) => v.len(),
            Self::U64(v) => v.len(),
            Self::I8(v) => v.len(),
            Self::I16(v) => v.len(),
            Self::I32(v) => v.len(),
            Self::I64(v) => v.len(),
            Self::F32(v) => v.len(),
            Self::F64(v) => v.len(),
            Self::C32(v) => v.len(),
            Self::C64(v) => v.len(),
            Self::CI16(v) => v.len(),
        }
    }
    /// Whether the buffer contains no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Explicit logical view of original boolean storage. Bytes remain available
    /// unchanged in `Bool`; this view treats every nonzero byte as true.
    #[must_use]
    pub fn logical_booleans(&self) -> Option<impl ExactSizeIterator<Item = bool> + '_> {
        match self {
            Self::Bool(values) => Some(values.iter().map(|value| *value != 0)),
            _ => None,
        }
    }

    #[cfg(any(feature = "hdf5", feature = "cbf", feature = "edf", feature = "native"))]
    pub(crate) fn resize(&mut self, ty: PixelType, len: usize) -> Result<(), String> {
        macro_rules! resize {
            ($v:ident, $zero:expr) => {{
                if !matches!(self, Self::$v(_)) {
                    *self = Self::$v(Vec::new());
                }
                if let Self::$v(v) = self {
                    v.try_reserve_exact(len.saturating_sub(v.len()))
                        .map_err(|e| e.to_string())?;
                    v.resize(len, $zero);
                }
            }};
        }
        match ty {
            PixelType::Bool => resize!(Bool, 0),
            PixelType::F16 => resize!(F16, 0),
            PixelType::U8 => resize!(U8, 0),
            PixelType::U16 => resize!(U16, 0),
            PixelType::U32 => resize!(U32, 0),
            PixelType::U64 => resize!(U64, 0),
            PixelType::I8 => resize!(I8, 0),
            PixelType::I16 => resize!(I16, 0),
            PixelType::I32 => resize!(I32, 0),
            PixelType::I64 => resize!(I64, 0),
            PixelType::F32 => resize!(F32, 0.0),
            PixelType::F64 => resize!(F64, 0.0),
            PixelType::C32 => resize!(C32, [0.0; 2]),
            PixelType::C64 => resize!(C64, [0.0; 2]),
            PixelType::CI16 => resize!(CI16, [0; 2]),
        }
        Ok(())
    }
    #[cfg(any(feature = "hdf5", feature = "edf", feature = "native"))]
    pub(crate) fn bytes_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Bool(v) => v,
            Self::F16(v) => bytemuck::cast_slice_mut(v),
            Self::U8(v) => v,
            Self::U16(v) => bytemuck::cast_slice_mut(v),
            Self::U32(v) => bytemuck::cast_slice_mut(v),
            Self::U64(v) => bytemuck::cast_slice_mut(v),
            Self::I8(v) => bytemuck::cast_slice_mut(v),
            Self::I16(v) => bytemuck::cast_slice_mut(v),
            Self::I32(v) => bytemuck::cast_slice_mut(v),
            Self::I64(v) => bytemuck::cast_slice_mut(v),
            Self::F32(v) => bytemuck::cast_slice_mut(v),
            Self::F64(v) => bytemuck::cast_slice_mut(v),
            Self::C32(v) => bytemuck::cast_slice_mut(v),
            Self::C64(v) => bytemuck::cast_slice_mut(v),
            Self::CI16(v) => bytemuck::cast_slice_mut(v),
        }
    }
}
