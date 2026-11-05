//! Pixirad PiXI fixed-size records with exact u16 samples and original headers.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelType,
    binary::{copy_samples, resize_bytes},
    source::Snapshot,
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
const SHAPE: [usize; 2] = [512, 476];
const IMAGE_BYTES: usize = 512 * 476 * 2;
const FRAME_BYTES: usize = 24 + IMAGE_BYTES;
const WORDS: i32 = ((FRAME_BYTES - 4) / 2) as i32;
#[derive(Clone, Debug)]
pub struct Metadata {
    pub shape: [usize; 2],
    pub frames: usize,
    pub pixel_type: PixelType,
    pub frame_bytes: usize,
    /// One exact 24-byte header per source record. Opaque words are not discarded.
    pub headers: Vec<[u8; 24]>,
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
}
impl Reader {
    /// Open all records in this exact file. Filename-series navigation is not
    /// implicit: select/reopen each source path explicitly.
    /// # Errors
    /// Nonintegral source length, invalid record sizes, truncation or resource limits.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let count = limits.pixels(SHAPE, PixelType::U16)?;
        if count * 2 > limits.encoded_bytes || limits.header_bytes < 24 {
            return Err(Error("Pixi record exceeds encoded/header limits".into()));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if snapshot.bytes() == 0 || !snapshot.bytes().is_multiple_of(FRAME_BYTES as u64) {
            return Err(Error(
                "Pixi source has truncated/trailing record bytes".into(),
            ));
        }
        let frames = usize::try_from(snapshot.bytes() / FRAME_BYTES as u64)
            .map_err(|_| Error("Pixi frame count overflows usize".into()))?;
        if frames > limits.frames
            || frames
                .checked_mul(24)
                .is_none_or(|n| n > limits.metadata_bytes)
        {
            return Err(Error("Pixi index exceeds frame/metadata limits".into()));
        }
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut headers = Vec::new();
        headers
            .try_reserve_exact(frames)
            .map_err(|e| Error(e.to_string()))?;
        for index in 0..frames {
            file.seek(SeekFrom::Start(index as u64 * FRAME_BYTES as u64))?;
            let mut header = [0u8; 24];
            file.read_exact(&mut header)?;
            if i32::from_le_bytes(header[..4].try_into().unwrap()) != WORDS {
                return Err(Error(format!("invalid Pixi record size at frame {index}")));
            }
            headers.push(header);
        }
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            metadata: Metadata {
                shape: SHAPE,
                frames,
                pixel_type: PixelType::U16,
                frame_bytes: FRAME_BYTES,
                headers,
            },
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
    /// Read stored rows from one checked record into reusable native output.
    /// # Errors
    /// Invalid frame index, changed source, truncation or allocation failure.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        if index >= self.frames() {
            return Err(Error("Pixi frame outside source".into()));
        }
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(index as u64 * FRAME_BYTES as u64 + 24))?;
        resize_bytes(&mut buffer.encoded, IMAGE_BYTES)?;
        file.read_exact(&mut buffer.encoded)?;
        buffer
            .pixels
            .resize(PixelType::U16, SHAPE[0] * SHAPE[1])
            .map_err(Error)?;
        copy_samples(&buffer.encoded, &mut buffer.pixels, true)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        buffer.shape = Some(SHAPE);
        Ok(())
    }
}
