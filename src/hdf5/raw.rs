//! Opt-in immutable raw-chunk snapshots for full-frame uint16/uint32 chunks.
//!
//! HDF5 discovers metadata/chunk addresses once. Reads use positional file I/O
//! and bounded LZ4 decoding outside its mutex. Bit unshuffle reuses the bundled
//! native codec (MIT notice in `licenses/`); compressed input is never passed to
//! its unbounded decompression API. Unsupported sources must use `Reader`.
use super::{Error, Limits, PixelType as Hdf5Type, Reader};
use crate::{PixelBuffer, PixelType, source::Snapshot};
use std::{
    ffi::c_void,
    fs::File,
    ops::Range,
    path::Path,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

unsafe extern "C" {
    fn bshuf_default_block_size(element_size: usize) -> usize;
    fn bshuf_using_NEON() -> i32;
    fn bshuf_using_SSE2() -> i32;
    #[cfg(target_arch = "x86_64")]
    fn dioptrin_bshuf_avx2_compiled() -> i32;
    fn dioptrin_bshuf_unshuffle_block(
        input: *const c_void,
        output: *mut c_void,
        scratch: *mut c_void,
        scratch_bytes: usize,
        elements: usize,
        element_size: usize,
        backend: i32,
    ) -> i64;
}

/// Native unshuffle kernel. Auto selects only instructions supported by the
/// current CPU and operating system; explicit unsupported choices are rejected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum UnshuffleBackend {
    #[default]
    Auto = 0,
    Scalar = 1,
    Sse2 = 2,
    Avx2 = 3,
    Neon = 4,
}
impl UnshuffleBackend {
    /// Whether this build and CPU can execute this backend safely.
    #[must_use]
    pub fn is_available(self) -> bool {
        match self {
            Self::Auto | Self::Scalar => true,
            // SAFETY: immutable, argument-free linked codec capability queries.
            Self::Sse2 => unsafe { bshuf_using_SSE2() != 0 },
            Self::Neon => unsafe { bshuf_using_NEON() != 0 },
            Self::Avx2 => {
                #[cfg(target_arch = "x86_64")]
                // Rust's feature detector also verifies OS support for AVX state.
                {
                    std::arch::is_x86_feature_detected!("avx2")
                        && unsafe { dioptrin_bshuf_avx2_compiled() != 0 }
                }
                #[cfg(not(target_arch = "x86_64"))]
                false
            }
        }
    }
    fn resolve(self) -> Result<Self, Error> {
        if self == Self::Auto {
            return Ok([Self::Avx2, Self::Neon, Self::Sse2, Self::Scalar]
                .into_iter()
                .find(|b| b.is_available())
                .unwrap());
        }
        if self.is_available() {
            Ok(self)
        } else {
            Err(Error(format!(
                "unavailable native unshuffle backend: {self:?}"
            )))
        }
    }
}

#[derive(Debug)]
struct Chunk {
    address: u64,
    bytes: usize,
    filtered: bool,
}

/// Reusable worker-owned storage. Failed reads invalidate access to partial data.
#[derive(Debug, Default)]
pub struct FrameBuffer {
    pixels: PixelBuffer,
    compressed: Vec<u8>,
    block_scratch: BlockScratch,
    shape: Option<[usize; 2]>,
}

#[derive(Debug, Default)]
struct BlockScratch {
    shuffled: Vec<u8>,
    unshuffle: Vec<u8>,
}
impl FrameBuffer {
    /// Complete original pixels from the last successful read, or `None`.
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        self.shape.map(|_| &self.pixels)
    }
    /// Stored detector height and width; invalidated before every attempted read.
    #[must_use]
    pub fn shape(&self) -> Option<[usize; 2]> {
        self.shape
    }
}

/// Bounded parallel reading. Each worker owns exactly one reusable frame buffer.
#[derive(Clone, Copy, Debug)]
pub struct PipelineOptions {
    /// Between 1 and 256; idle workers beyond the frame range are not spawned.
    pub workers: usize,
    /// Maximum combined decoder payload (not process RSS or consumer storage).
    pub memory_bytes: usize,
}
impl Default for PipelineOptions {
    fn default() -> Self {
        Self {
            workers: 1,
            memory_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Indexed self-contained file snapshot with one allocated chunk per frame.
///
/// Caller asserts acquisition has stopped and the file is immutable for this
/// handle's lifetime. Resizable datasets are allowed as frozen snapshots; no
/// refresh or live acquisition is supported. Metadata identity is checked before
/// and after indexing/reads, including the open file handle. This detects normal
/// file changes, not adversarial writes preserving all filesystem metadata.
#[derive(Debug)]
pub struct RawReader {
    snapshot: Snapshot,
    file: File,
    shape: [usize; 3],
    pixel_type: PixelType,
    little_endian: bool,
    chunks: Vec<Chunk>,
    max_encoded: usize,
    chunk_limit: usize,
    configured_block: usize,
    backend: UnshuffleBackend,
}
impl RawReader {
    /// Index exactly the requested dataset as a frozen immutable snapshot.
    ///
    /// Supports full-frame chunks with no filters or only Bitshuffle/LZ4 32008,
    /// including chunks with that filter skipped. No implicit fallback occurs.
    /// # Errors
    /// Rejects virtual/external storage, nonzero userblocks, other filters,
    /// partial/multiframe chunks, unallocated chunks, changed sources or limits.
    pub fn open_snapshot(
        path: impl AsRef<Path>,
        dataset: &str,
        limits: Limits,
    ) -> Result<Self, Error> {
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let reader = Reader::open(&snapshot.path, dataset, limits)?;
        let actual =
            std::fs::canonicalize(reader.dataset.filename()).map_err(|e| Error(e.to_string()))?;
        if actual != snapshot.path || !reader.dataset.dcpl()?.get_external()?.is_empty() {
            return Err(Error(
                "raw snapshot requires a self-contained source dataset".into(),
            ));
        }
        if reader.file.userblock() != 0 {
            return Err(Error(
                "raw snapshot does not support HDF5 userblocks; use Reader".into(),
            ));
        }
        let shape = reader.shape();
        if reader.dataset.chunk().as_deref() != Some(&[1, shape[1], shape[2]]) {
            return Err(Error(
                "raw snapshot requires one full-frame chunk per frame; use Reader".into(),
            ));
        }
        let pixel_type = match reader.pixel_type() {
            Hdf5Type::U16 => PixelType::U16,
            Hdf5Type::U32 => PixelType::U32,
        };
        let mut configured_block = 0;
        let has_filter = match reader.filters() {
            [] => false,
            [filter] if filter.id == 32008 => {
                let p = &filter.parameters;
                if p.len() < 5 || p[2] as usize != pixel_type.bytes() || p[4] != 2 {
                    return Err(Error(
                        "raw snapshot supports only Bitshuffle/LZ4 filter configuration".into(),
                    ));
                }
                configured_block = p[3] as usize;
                true
            }
            _ => {
                return Err(Error(
                    "raw snapshot does not support this filter pipeline; use Reader".into(),
                ));
            }
        };
        let little_endian = match reader.dataset.dtype()?.byte_order() {
            hdf5_backend::datatype::ByteOrder::LittleEndian => true,
            hdf5_backend::datatype::ByteOrder::BigEndian => false,
            _ => {
                return Err(Error(
                    "raw snapshot requires ordinary little/big-endian samples".into(),
                ));
            }
        };
        if reader.dataset.num_chunks() != Some(shape[0]) {
            return Err(Error(
                "raw snapshot requires all frame chunks allocated; use Reader for fill values"
                    .into(),
            ));
        }
        let mut chunks: Vec<Option<Chunk>> = Vec::new();
        chunks
            .try_reserve_exact(shape[0])
            .map_err(|e| Error(e.to_string()))?;
        chunks.resize_with(shape[0], || None);
        let mut max_encoded = 0;
        for i in 0..shape[0] {
            let info = reader
                .dataset
                .chunk_info(i)
                .ok_or_else(|| Error("failed to index HDF5 chunk".into()))?;
            let frame = usize::try_from(info.offset[0]).map_err(|e| Error(e.to_string()))?;
            let bytes = usize::try_from(info.size).map_err(|e| Error(e.to_string()))?;
            let allowed_mask = u32::from(has_filter);
            if frame >= shape[0]
                || info.offset[1..] != [0, 0]
                || chunks[frame].is_some()
                || info.filter_mask & !allowed_mask != 0
                || bytes == 0
                || bytes > limits.chunk_bytes
                || info
                    .addr
                    .checked_add(info.size)
                    .is_none_or(|end| end > snapshot.bytes())
            {
                return Err(Error("invalid or oversized raw HDF5 chunk index".into()));
            }
            max_encoded = max_encoded.max(bytes);
            chunks[frame] = Some(Chunk {
                address: info.addr,
                bytes,
                filtered: has_filter && info.filter_mask == 0,
            });
        }
        let chunks = chunks
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| Error("missing raw chunk".into()))?;
        let file = File::open(&snapshot.path).map_err(|e| Error(e.to_string()))?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            shape,
            pixel_type,
            little_endian,
            chunks,
            max_encoded,
            chunk_limit: limits.chunk_bytes,
            configured_block,
            backend: UnshuffleBackend::Auto.resolve()?,
        })
    }
    /// Override the kernel while retaining identical IO and decoding semantics.
    /// # Errors
    /// The requested kernel is not compiled or unsupported by this CPU/OS.
    pub fn with_unshuffle_backend(mut self, backend: UnshuffleBackend) -> Result<Self, Error> {
        self.backend = backend.resolve()?;
        Ok(self)
    }
    /// Actual selected kernel (never Auto).
    #[must_use]
    pub fn unshuffle_backend(&self) -> UnshuffleBackend {
        self.backend
    }
    /// Frozen `(frames,height,width)` dimensions.
    #[must_use]
    pub fn shape(&self) -> [usize; 3] {
        self.shape
    }
    /// Original sample storage; uint16 stays uint16.
    #[must_use]
    pub fn pixel_type(&self) -> PixelType {
        self.pixel_type
    }
    /// Conservative maximum decoder payload per worker (excluding native codec
    /// bookkeeping, allocator overhead, index and caller-retained output).
    #[must_use]
    pub fn worker_payload_bytes(&self) -> usize {
        // The shuffled scratch is bounded by the decoded chunk, even if the
        // stored block size is larger. Both reusable block buffers are covered
        // by the two decoded-frame allowances, including retained capacity.
        self.shape[1]
            .saturating_mul(self.shape[2])
            .saturating_mul(self.pixel_type.bytes())
            .saturating_mul(3)
            .saturating_add(self.max_encoded)
    }
    /// Read into reusable native storage. No HDF5 calls occur here.
    /// # Errors
    /// Invalid frame, changed source, allocation, truncated/corrupt codec input.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        let chunk = self
            .chunks
            .get(index)
            .ok_or_else(|| Error("HDF5 frame index outside snapshot".into()))?;
        resize_bytes(&mut buffer.compressed, chunk.bytes)?;
        read_exact_at(&self.file, &mut buffer.compressed, chunk.address)
            .map_err(|e| Error(e.to_string()))?;
        buffer
            .pixels
            .resize(self.pixel_type, self.shape[1] * self.shape[2])
            .map_err(Error)?;
        let output = buffer.pixels.bytes_mut();
        if chunk.filtered {
            decode_bitshuffle_lz4(
                &buffer.compressed,
                output,
                self.pixel_type.bytes(),
                self.chunk_limit,
                self.configured_block,
                &mut buffer.block_scratch,
                self.backend,
            )?;
        } else {
            if buffer.compressed.len() != output.len() {
                return Err(Error("unfiltered raw chunk size mismatch".into()));
            }
            output.copy_from_slice(&buffer.compressed);
        }
        if self.little_endian != cfg!(target_endian = "little") {
            for sample in output.chunks_exact_mut(self.pixel_type.bytes()) {
                sample.reverse();
            }
        }
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        buffer.shape = Some([self.shape[1], self.shape[2]]);
        Ok(())
    }
    /// Process a range with bounded worker-owned storage. Callbacks run on worker
    /// threads in unspecified order and borrow pixels only until they return.
    /// No full-scan decoded queue or full-frame float conversion is created.
    /// # Errors
    /// Invalid options/range, budget admission, decode or consumer error. On
    /// failure, active callbacks may finish; unstarted work is canceled.
    pub fn for_each_frame<F>(
        &self,
        range: Range<usize>,
        options: PipelineOptions,
        consume: F,
    ) -> Result<(), Error>
    where
        F: Fn(usize, [usize; 2], &PixelBuffer) -> Result<(), Error> + Sync,
    {
        if range.start > range.end
            || range.end > self.shape[0]
            || options.workers == 0
            || options.workers > 256
        {
            return Err(Error("invalid raw pipeline range or worker count".into()));
        }
        let workers = options.workers.min(range.len());
        if self
            .worker_payload_bytes()
            .checked_mul(workers)
            .is_none_or(|n| n > options.memory_bytes)
        {
            return Err(Error("raw pipeline exceeds decoder payload budget".into()));
        }
        let next = AtomicUsize::new(range.start);
        let stop = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..workers {
                let (next, stop, consume) = (&next, &stop, &consume);
                handles.push(scope.spawn(move || {
                    let mut buffer = FrameBuffer::default();
                    while !stop.load(Ordering::Relaxed) {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= range.end {
                            break;
                        }
                        let result = self
                            .read_frame_into(i, &mut buffer)
                            .and_then(|()| consume(i, buffer.shape.unwrap(), &buffer.pixels));
                        if result.is_err() {
                            stop.store(true, Ordering::Relaxed);
                            return result;
                        }
                    }
                    Ok(())
                }));
            }
            // Join every worker, including on error; scoped threads never escape.
            let mut result = Ok(());
            for handle in handles {
                let current = handle
                    .join()
                    .unwrap_or_else(|_| Err(Error("raw pipeline worker panicked".into())));
                if result.is_ok() {
                    result = current;
                }
            }
            result
        })
    }
}

fn resize_bytes(buffer: &mut Vec<u8>, bytes: usize) -> Result<(), Error> {
    buffer
        .try_reserve_exact(bytes.saturating_sub(buffer.len()))
        .map_err(|e| Error(e.to_string()))?;
    buffer.resize(bytes, 0);
    Ok(())
}

fn decode_bitshuffle_lz4(
    input: &[u8],
    output: &mut [u8],
    element: usize,
    limit: usize,
    configured_block: usize,
    scratch: &mut BlockScratch,
    backend: UnshuffleBackend,
) -> Result<(), Error> {
    let bad = || Error("malformed or truncated Bitshuffle/LZ4 chunk".into());
    let header = input.get(..12).ok_or_else(bad)?;
    let decoded = u64::from_be_bytes(header[..8].try_into().unwrap());
    let stored_block = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    // A zero field explicitly selects the codec's stable default. Real Eiger
    // files use this encoding as well as explicit block byte counts.
    let block_bytes = if stored_block == 0 {
        // SAFETY: the API accepts an element size, here admitted as 2 or 4.
        (unsafe { bshuf_default_block_size(element) }) * element
    } else {
        stored_block
    };
    if decoded != output.len() as u64
        || block_bytes == 0
        || block_bytes > limit
        || block_bytes % (8 * element) != 0
        || (configured_block != 0 && configured_block.checked_mul(element) != Some(block_bytes))
    {
        return Err(bad());
    }
    let mut source = 12_usize;
    let mut dest = 0;
    let shuffled_bytes = output.len() / (8 * element) * (8 * element);
    while dest < shuffled_bytes {
        let bytes = block_bytes.min(shuffled_bytes - dest);
        let length = input
            .get(source..source.checked_add(4).ok_or_else(bad)?)
            .ok_or_else(bad)?;
        let compressed = u32::from_be_bytes(length.try_into().unwrap()) as usize;
        source += 4;
        let end = source.checked_add(compressed).ok_or_else(bad)?;
        let block = input.get(source..end).ok_or_else(bad)?;
        resize_bytes(&mut scratch.shuffled, bytes)?;
        resize_bytes(&mut scratch.unshuffle, bytes)?;
        let actual = lz4_flex::block::decompress_into(block, &mut scratch.shuffled)
            .map_err(|e| Error(format!("invalid LZ4 block: {e}")))?;
        if actual != bytes {
            return Err(bad());
        }
        // SAFETY: all three disjoint slices contain elements*element bytes.
        // All lengths derive from admitted output, the LZ4 decoder is bounded,
        // and block size is a positive multiple of eight elements. This API
        // reads already decompressed bytes, never untrusted length prefixes.
        let result = unsafe {
            dioptrin_bshuf_unshuffle_block(
                scratch.shuffled.as_ptr().cast(),
                output[dest..dest + bytes].as_mut_ptr().cast(),
                scratch.unshuffle.as_mut_ptr().cast(),
                scratch.unshuffle.len(),
                bytes / element,
                element,
                backend as i32,
            )
        };
        if result != bytes as i64 {
            return Err(Error(format!("native bit-unshuffle failed: {result}")));
        }
        source = end;
        dest += bytes;
    }
    let tail = output.len() - dest;
    if input.len().checked_sub(source) != Some(tail) {
        return Err(bad());
    }
    output[dest..].copy_from_slice(&input[source..]);
    Ok(())
}

fn read_exact_at(file: &File, mut output: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    while !output.is_empty() {
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(file, output, offset)?;
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(file, output, offset)?;
        #[cfg(not(any(unix, windows)))]
        compile_error!("raw HDF5 positional reads require Unix or Windows");
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        offset += n as u64;
        output = &mut output[n..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_reads_retain_both_block_scratch_allocations() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/counts-bitshuffle-lz4.h5");
        let reader = RawReader::open_snapshot(path, "detector/data", Limits::default()).unwrap();
        let mut buffer = FrameBuffer::default();
        reader.read_frame_into(0, &mut buffer).unwrap();
        let pointers = (
            buffer.block_scratch.shuffled.as_ptr(),
            buffer.block_scratch.unshuffle.as_ptr(),
        );
        assert!(!buffer.block_scratch.unshuffle.is_empty());
        for index in [1, 0, 1] {
            reader.read_frame_into(index, &mut buffer).unwrap();
            assert_eq!(
                pointers,
                (
                    buffer.block_scratch.shuffled.as_ptr(),
                    buffer.block_scratch.unshuffle.as_ptr(),
                )
            );
            assert!(buffer.pixels().is_some());
        }
    }
    #[test]
    fn corrupt_chunks_are_bounded_and_rejected() {
        let mut scratch = BlockScratch::default();
        let mut output = [0; 32];
        for length in 0..100 {
            assert!(
                decode_bitshuffle_lz4(
                    &vec![0; length],
                    &mut output,
                    4,
                    1024,
                    0,
                    &mut scratch,
                    UnshuffleBackend::Scalar
                )
                .is_err()
            );
        }
        let mut input = Vec::from(32_u64.to_be_bytes());
        input.extend_from_slice(&32_u32.to_be_bytes());
        for payload in [vec![], vec![255; 4], vec![0, 0, 0, 1, 0]] {
            let mut bad = input.clone();
            bad.extend(payload);
            assert!(
                decode_bitshuffle_lz4(
                    &bad,
                    &mut output,
                    4,
                    1024,
                    0,
                    &mut scratch,
                    UnshuffleBackend::Scalar
                )
                .is_err()
            );
        }
    }
}
