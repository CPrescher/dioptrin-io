use crate::PixelBuffer;
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
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub encoded_bytes: usize,
    pub frame_bytes: usize,
    pub metadata_bytes: usize,
    /// Backend decoded component payload admission, excluding library overhead.
    pub decoder_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            encoded_bytes: 256 * 1024 * 1024,
            frame_bytes: 256 * 1024 * 1024,
            metadata_bytes: 16 * 1024 * 1024,
            decoder_bytes: 512 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub(crate) fn validate(self) -> Result<(), Error> {
        if [
            self.encoded_bytes,
            self.frame_bytes,
            self.metadata_bytes,
            self.decoder_bytes,
        ]
        .contains(&0)
        {
            return Err(Error("raster limits must be positive".into()));
        }
        Ok(())
    }
    pub(crate) fn samples(
        self,
        shape: [usize; 2],
        channels: usize,
        bytes: usize,
    ) -> Result<usize, Error> {
        let n = shape[0]
            .checked_mul(shape[1])
            .and_then(|n| n.checked_mul(channels))
            .filter(|n| *n > 0)
            .ok_or_else(|| Error("raster shape overflow/empty image".into()))?;
        if n.checked_mul(bytes).is_none_or(|n| n > self.frame_bytes) {
            return Err(Error("raster output exceeds decoded-byte limit".into()));
        }
        Ok(n)
    }
}
#[derive(Default, Debug)]
pub struct FrameBuffer {
    pub(crate) pixels: PixelBuffer,
    pub(crate) shape: Option<[usize; 2]>,
    pub(crate) channels: usize,
}
impl FrameBuffer {
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        self.shape.map(|_| &self.pixels)
    }
    #[must_use]
    pub fn shape(&self) -> Option<[usize; 2]> {
        self.shape
    }
    #[must_use]
    pub fn sample_shape(&self) -> Option<Vec<usize>> {
        self.shape.map(|s| {
            if self.channels == 1 {
                s.to_vec()
            } else {
                vec![s[0], s[1], self.channels]
            }
        })
    }
}
