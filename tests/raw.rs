#![cfg(feature = "hdf5")]
use dioptrin_io::{
    PixelBuffer,
    hdf5::{
        Limits, Reader,
        raw::{FrameBuffer, PipelineOptions, RawReader},
    },
};
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "dioptrin-io-raw-{}-{}.h5",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn independent() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/counts-bitshuffle-lz4.h5")
}

#[test]
fn independent_counts_are_native_reusable_and_parallel_without_orientation_changes() {
    let raw = RawReader::open_snapshot(independent(), "detector/data", Limits::default()).unwrap();
    let fallback = Reader::open(independent(), "detector/data", Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    for i in [0, 1, 0] {
        raw.read_frame_into(i, &mut buffer).unwrap();
        assert_eq!(buffer.shape(), Some([3, 4]));
        assert_eq!(
            buffer.pixels(),
            Some(&PixelBuffer::U32(fallback.read_frame(i).unwrap().pixels))
        );
    }
    assert!(raw.read_frame_into(2, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
    let seen = Mutex::new(Vec::new());
    raw.for_each_frame(
        0..2,
        PipelineOptions {
            workers: 2,
            memory_bytes: 1024 * 1024,
        },
        |i, shape, pixels| {
            assert_eq!(shape, [3, 4]);
            assert_eq!(
                pixels,
                &PixelBuffer::U32(fallback.read_frame(i).unwrap().pixels)
            );
            seen.lock().unwrap().push(i);
            Ok(())
        },
    )
    .unwrap();
    seen.lock().unwrap().sort();
    assert_eq!(*seen.lock().unwrap(), [0, 1]);
    assert!(
        raw.for_each_frame(
            0..2,
            PipelineOptions {
                workers: 2,
                memory_bytes: 1
            },
            |_, _, _| Ok(())
        )
        .is_err()
    );
    assert!(
        raw.for_each_frame(
            0..2,
            PipelineOptions {
                workers: 0,
                memory_bytes: 1024
            },
            |_, _, _| Ok(())
        )
        .is_err()
    );
}

#[test]
fn u16_raw_and_bitshuffle_blocks_tails_and_sentinels_match_library() {
    hdf5_backend::sync::sync(hdf5_bitshuffle::register_bitshuffle_plugin);
    for count in [1, 7, 8, 9, 2047, 2048, 2049, 8193] {
        for filtered in [false, true] {
            let fixture = Fixture::new();
            let values: Vec<u16> = (0..2 * count).map(|i| (i * 257) as u16).collect();
            {
                let f = hdf5_backend::File::create(&fixture.0).unwrap();
                let mut builder = f
                    .new_dataset::<u16>()
                    .shape((2, 1, count))
                    .chunk((1, 1, count));
                if filtered {
                    builder =
                        builder.set_filters(&[hdf5_backend::filters::Filter::user(32008, &[0, 2])]);
                }
                builder.create("data").unwrap().write_raw(&values).unwrap();
            }
            let reader = RawReader::open_snapshot(&fixture.0, "data", Limits::default()).unwrap();
            let mut buffer = FrameBuffer::default();
            for i in 0..2 {
                reader.read_frame_into(i, &mut buffer).unwrap();
                assert_eq!(
                    buffer.pixels(),
                    Some(&PixelBuffer::U16(
                        values[i * count..(i + 1) * count].to_vec()
                    ))
                );
            }
        }
    }
}

#[test]
fn unsupported_layouts_filters_missing_chunks_and_external_sources_fail_explicitly() {
    let fixture = Fixture::new();
    {
        let f = hdf5_backend::File::create(&fixture.0).unwrap();
        for (name, chunk) in [
            ("multiframe", (2, 2, 3)),
            ("partial", (1, 1, 3)),
            ("missing", (1, 2, 3)),
        ] {
            f.new_dataset::<u32>()
                .shape((2, 2, 3))
                .chunk(chunk)
                .create(name)
                .unwrap();
        }
        f.new_dataset::<u32>()
            .shape((2, 2, 3))
            .chunk((1, 2, 3))
            .deflate(1)
            .create("gzip")
            .unwrap()
            .write_raw(&[1; 12])
            .unwrap();
        f.link_external(independent().to_str().unwrap(), "/detector/data", "foreign")
            .unwrap();
    }
    for dataset in ["multiframe", "partial", "missing", "gzip", "foreign"] {
        assert!(
            RawReader::open_snapshot(&fixture.0, dataset, Limits::default()).is_err(),
            "{dataset}"
        );
    }
}

#[test]
fn source_mutation_and_truncation_invalidate_reused_output() {
    use std::io::Write;
    let fixture = Fixture::new();
    std::fs::copy(independent(), &fixture.0).unwrap();
    let reader = RawReader::open_snapshot(&fixture.0, "detector/data", Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    reader.read_frame_into(0, &mut buffer).unwrap();
    // Close no HDF5 handles are retained by the raw reader. Appending is allowed
    // by OS sharing modes and must never return stale indexed pixels.
    std::fs::OpenOptions::new()
        .append(true)
        .open(&fixture.0)
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    assert!(
        reader
            .read_frame_into(0, &mut buffer)
            .unwrap_err()
            .to_string()
            .contains("source")
    );
    assert!(buffer.pixels().is_none());
}

#[test]
fn independent_compressed_payload_with_default_block_and_skipped_filter_is_exact() {
    // This test reads through HDF5 directly before constructing our reader.
    // Register first; neither test order nor installed external plugins may
    // supply the filter implicitly.
    hdf5_backend::sync::sync(hdf5_bitshuffle::register_bitshuffle_plugin);
    use std::ffi::c_void;
    use std::io::{Read, Seek, SeekFrom};
    unsafe extern "C" {
        fn H5Dwrite_chunk(
            dataset: i64,
            dxpl: i64,
            filter_mask: u32,
            offset: *const u64,
            size: usize,
            data: *const c_void,
        ) -> i32;
    }
    let source = hdf5_backend::File::open(independent()).unwrap();
    let data = source.dataset("detector/data").unwrap();
    let info = data.chunk_info(0).unwrap();
    let counts = data.read_raw::<u32>().unwrap();
    let mut file = std::fs::File::open(independent()).unwrap();
    file.seek(SeekFrom::Start(info.addr)).unwrap();
    let mut compressed = vec![0; info.size as usize];
    file.read_exact(&mut compressed).unwrap();
    // Keep independent compressed payload; a zero header selects the same stable
    // codec default as the original explicit 8192-byte block field.
    compressed[8..12].fill(0);
    let fixture = Fixture::new();
    {
        let f = hdf5_backend::File::create(&fixture.0).unwrap();
        let ds = f
            .new_dataset::<u32>()
            .shape((2, 3, 4))
            .chunk((1, 3, 4))
            .set_filters(&[hdf5_backend::filters::Filter::user(32008, &[0, 2])])
            .create("data")
            .unwrap();
        let raw: Vec<u8> = counts[12..].iter().flat_map(|x| x.to_ne_bytes()).collect();
        for (i, mask, bytes) in [(0, 0, &compressed), (1, 1, &raw)] {
            let offset = [i, 0, 0];
            // SAFETY: live dataset, default property list, valid rank-3 offset
            // and disjoint slices with exactly the declared byte lengths. Same
            // upstream HDF5 mutex as all high-level calls; no unbounded decoder.
            let status = hdf5_backend::sync::sync(|| unsafe {
                H5Dwrite_chunk(
                    ds.id(),
                    0,
                    mask,
                    offset.as_ptr(),
                    bytes.len(),
                    bytes.as_ptr().cast(),
                )
            });
            assert_eq!(status, 0);
        }
    }
    let raw = RawReader::open_snapshot(&fixture.0, "data", Limits::default()).unwrap();
    let general = Reader::open(&fixture.0, "data", Limits::default()).unwrap();
    let mut buffer = FrameBuffer::default();
    for i in 0..2 {
        raw.read_frame_into(i, &mut buffer).unwrap();
        assert_eq!(
            buffer.pixels(),
            Some(&PixelBuffer::U32(counts[i * 12..(i + 1) * 12].to_vec()))
        );
        assert_eq!(
            general.read_frame(i).unwrap().pixels,
            counts[i * 12..(i + 1) * 12]
        );
    }
}
