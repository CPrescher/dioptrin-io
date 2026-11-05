//! Bounded MRC/CCP4/MAP/FEI images using the MIT mrc crate's header decoder.
//! Stored axis mappings remain metadata; no spatial reorientation is performed.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelBuffer, PixelType,
    binary::{copy_samples, resize_bytes},
    source::Snapshot,
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ByteOrder {
    #[default]
    MachineStamp,
    /// Caller explicitly supplies the order for a legacy unstamped file.
    Little,
    Big,
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub frames: usize,
    pub pixel_type: PixelType,
    pub mode: i32,
    pub little_endian: bool,
    pub byte_order_policy: ByteOrder,
    /// Original 1024-byte header, including source stamp and labels.
    pub header: Vec<u8>,
    /// Typed header decoded by the permissively licensed existing MRC library.
    pub parsed: mrc_backend::Header,
    pub extended_header: Vec<u8>,
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
    offset: u64,
    frame_bytes: usize,
    limit: usize,
}
impl Reader {
    /// Open frozen stored-order slices. Unknown/wrong machine stamps are not
    /// repaired or retried in another byte order; legacy callers choose explicitly.
    /// # Errors
    /// Invalid shape, source size, stamp/mode, resource limits or unsupported dtype.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        Self::open_with_byte_order(path, limits, ByteOrder::MachineStamp)
    }
    /// Open with an explicit byte-order policy. The original stamp is retained.
    /// # Errors
    /// Same validation as `open`.
    pub fn open_with_byte_order(
        path: impl AsRef<Path>,
        limits: Limits,
        policy: ByteOrder,
    ) -> Result<Self, Error> {
        limits.validate()?;
        if limits.header_bytes < 1024 || limits.metadata_bytes < 2048 {
            return Err(Error("MRC header exceeds metadata limits".into()));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut header = [0u8; 1024];
        file.read_exact(&mut header)?;
        let stamp: [u8; 4] = header[212..216].try_into().unwrap();
        let little = match policy {
            ByteOrder::Little => true,
            ByteOrder::Big => false,
            ByteOrder::MachineStamp => match &stamp[..2] {
                [0x44, 0x44] | [0x44, 0x41] => true,
                [0x11, 0x11] => false,
                _ => {
                    return Err(Error(
                        "unknown MRC machine stamp; choose an explicit byte order".into(),
                    ));
                }
            },
        };
        let mut decode_header = header;
        decode_header[212..216].copy_from_slice(if little {
            &[0x44, 0x44, 0, 0]
        } else {
            &[0x11, 0x11, 0, 0]
        });
        let (mut parsed, fallback) =
            mrc_backend::Header::decode_from_bytes_with_info(&decode_header);
        if fallback.is_some() {
            return Err(Error(
                "MRC mode contradicts selected byte order; no fallback is applied".into(),
            ));
        }
        parsed.machst = stamp;
        let dimension = |n: i32| {
            usize::try_from(n)
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| Error("invalid MRC dimensions".into()))
        };
        let shape = [dimension(parsed.ny)?, dimension(parsed.nx)?];
        let frames = dimension(parsed.nz)?;
        if frames > limits.frames {
            return Err(Error("MRC stack exceeds frame limit".into()));
        }
        let pixel_type = match parsed.mode {
            0 => PixelType::I8,
            1 => PixelType::I16,
            2 => PixelType::F32,
            3 => PixelType::CI16,
            4 => PixelType::C32,
            6 => PixelType::U16,
            _ => return Err(Error(format!("unsupported MRC mode {}", parsed.mode))),
        };
        let frame_bytes = limits.pixels(shape, pixel_type)? * pixel_type.bytes();
        if frame_bytes > limits.encoded_bytes {
            return Err(Error("MRC frame exceeds encoded byte limit".into()));
        }
        let extension = usize::try_from(parsed.nsymbt)
            .map_err(|_| Error("negative MRC extended-header length".into()))?;
        if extension
            .checked_add(1024)
            .is_none_or(|n| n > limits.header_bytes)
            || extension.saturating_add(3072) > limits.metadata_bytes
        {
            return Err(Error("MRC extended header exceeds limits".into()));
        }
        let offset = 1024u64 + extension as u64;
        let end = (frame_bytes as u64)
            .checked_mul(frames as u64)
            .and_then(|n| offset.checked_add(n))
            .ok_or_else(|| Error("MRC stack byte-size overflow".into()))?;
        if end != snapshot.bytes() {
            return Err(Error(
                "MRC source length does not match header (truncation/trailing data)".into(),
            ));
        }
        let mut extended_header = Vec::new();
        resize_bytes(&mut extended_header, extension)?;
        file.read_exact(&mut extended_header)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let metadata = Metadata {
            shape,
            frames,
            pixel_type,
            mode: parsed.mode,
            little_endian: little,
            byte_order_policy: policy,
            header: header.to_vec(),
            parsed,
            extended_header,
        };
        Ok(Self {
            snapshot,
            file,
            metadata,
            offset,
            frame_bytes,
            limit: limits.frame_bytes,
        })
    }
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.metadata.frames
    }
    /// Read one stored section. Mode 3 retains signed-i16 real/imaginary pairs;
    /// mode 4 retains native f32 pairs and all floating bit patterns.
    /// # Errors
    /// Invalid frame, changed source, truncation or allocation failure.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        if index >= self.frames() {
            return Err(Error("MRC frame outside source".into()));
        }
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(
            self.offset + index as u64 * self.frame_bytes as u64,
        ))?;
        resize_bytes(&mut buffer.encoded, self.frame_bytes)?;
        file.read_exact(&mut buffer.encoded)?;
        let shape = self.metadata.shape;
        buffer
            .pixels
            .resize(self.metadata.pixel_type, shape[0] * shape[1])
            .map_err(Error)?;
        copy_samples(
            &buffer.encoded,
            &mut buffer.pixels,
            self.metadata.little_endian,
        )?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        buffer.shape = Some(shape);
        Ok(())
    }
    /// Explicit exact conversion of mode-3 i16 components to complex f32. Source
    /// dtype remains CI16 in metadata. No magnitude/phase/scaling is performed.
    /// # Errors
    /// Any native read error, non-mode3 input or output-byte budget violation.
    pub fn read_frame_as_complex32_into(
        &self,
        index: usize,
        buffer: &mut FrameBuffer,
    ) -> Result<(), Error> {
        buffer.shape = None;
        if self.metadata.mode != 3 {
            return Err(Error(
                "explicit CI16-to-C32 conversion requires MRC mode 3".into(),
            ));
        }
        let count = self.metadata.shape[0] * self.metadata.shape[1];
        if count.checked_mul(8).is_none_or(|n| n > self.limit) {
            return Err(Error(
                "MRC complex conversion exceeds frame byte limit".into(),
            ));
        }
        self.read_frame_into(index, buffer)?;
        buffer.shape = None;
        let PixelBuffer::CI16(values) = std::mem::take(&mut buffer.pixels) else {
            unreachable!()
        };
        buffer.pixels.resize(PixelType::C32, count).map_err(Error)?;
        let PixelBuffer::C32(output) = &mut buffer.pixels else {
            unreachable!()
        };
        for (sample, source) in output.iter_mut().zip(values) {
            *sample = [f32::from(source[0]), f32::from(source[1])];
        }
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        buffer.shape = Some(self.metadata.shape);
        Ok(())
    }
}
