#![cfg(feature = "wrappers")]
use dioptrin_io::{
    PixelBuffer, native,
    navigation::{Cursor, FrameSource, Position},
    tiff,
    wrapped::{self, Format, FrameBuffer, Input, Reader},
};
use std::{cell::Cell, fs, path::Path};

fn edf(shape: [usize; 2], ty: &str, order: &str, values: &[u8]) -> Vec<u8> {
    let text = format!(
        "{{\nHeaderID = EH:000001:000000:000000 ;\nEDF_HeaderSize = 512 ;\nDim_1 = {} ;\nDim_2 = {} ;\nDataType = {ty} ;\nByteOrder = {order} ;\nSize = {} ;\n",
        shape[1],
        shape[0],
        values.len()
    );
    let mut data = text.into_bytes();
    data.resize(510, b' ');
    data.extend(b"}\n");
    data.extend(values);
    data
}
fn stack() -> Vec<u8> {
    let mut data = edf(
        [1, 3],
        "UnsignedShort",
        "HighByteFirst",
        &[0, 0, 255, 255, 128, 1],
    );
    let ints: Vec<_> = [i32::MIN, i32::MAX]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect();
    data.extend(edf([2, 1], "SignedInteger", "LowByteFirst", &ints));
    let floats: Vec<_> = [-0.0f64, f64::INFINITY]
        .into_iter()
        .flat_map(f64::to_be_bytes)
        .collect();
    data.extend(edf([1, 2], "DoubleValue", "HighByteFirst", &floats));
    data
}
fn from_bytes(bytes: &[u8]) -> Reader {
    Input::from_bytes(bytes, None, wrapped::Limits::default())
        .unwrap()
        .read(
            Format::Edf,
            native::Limits::default(),
            tiff::Limits::default(),
        )
        .unwrap()
}
fn from_path(path: &Path) -> Reader {
    Reader::open(
        path,
        Format::Edf,
        wrapped::Limits::default(),
        native::Limits::default(),
        tiff::Limits::default(),
    )
    .unwrap()
}
fn assert_hidden<S: FrameSource>(cursor: &Cursor<S>) {
    assert_eq!(cursor.current_index(), None);
    assert!(cursor.buffer().is_none());
}
fn assert_frame(cursor: &Cursor<Reader>, index: usize) {
    assert_eq!(cursor.current_index(), Some(index));
    assert_eq!(cursor.position(), Position::Frame(index));
    let frame = cursor.buffer().unwrap();
    match index {
        0 => {
            assert_eq!(frame.sample_shape(), Some(vec![1, 3]));
            assert_eq!(
                frame.pixels(),
                Some(&PixelBuffer::U16(vec![0, 65535, 32769]))
            );
        }
        1 => {
            assert_eq!(frame.sample_shape(), Some(vec![2, 1]));
            assert_eq!(
                frame.pixels(),
                Some(&PixelBuffer::I32(vec![i32::MIN, i32::MAX]))
            );
        }
        2 => {
            assert_eq!(frame.sample_shape(), Some(vec![1, 2]));
            let Some(PixelBuffer::F64(values)) = frame.pixels() else {
                panic!("wrong float dtype")
            };
            assert_eq!(
                values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                vec![(-0.0f64).to_bits(), f64::INFINITY.to_bits()]
            );
        }
        _ => unreachable!(),
    }
}
#[test]
fn heterogeneous_stack_controls_preserve_exact_counts_types_and_shapes() {
    let mut cursor = Cursor::new(from_bytes(&stack()));
    assert_eq!(cursor.frames(), 3);
    assert_eq!(cursor.position(), Position::BeforeFirst);
    assert_hidden(&cursor);
    cursor.next().unwrap();
    assert_frame(&cursor, 0);
    cursor.next().unwrap();
    assert_frame(&cursor, 1);
    cursor.last().unwrap();
    assert_frame(&cursor, 2);
    cursor.previous().unwrap();
    assert_frame(&cursor, 1);
    cursor.first().unwrap();
    assert_frame(&cursor, 0);
    cursor.seek_relative(2).unwrap();
    assert_frame(&cursor, 2);
    cursor.seek_relative(-1).unwrap();
    assert_frame(&cursor, 1);
    cursor.seek_relative(0).unwrap();
    assert_frame(&cursor, 1);
    for index in [2, 0, 1, 0, 2] {
        cursor.read_at(index).unwrap();
        assert_frame(&cursor, index);
    }
    assert_eq!(cursor.source().frames(), 3);
    assert_eq!(cursor.into_inner().frames(), 3);
}
#[test]
fn both_boundaries_are_explicit_reversible_and_never_wrap() {
    let mut cursor = Cursor::new(from_bytes(&stack()));
    for _ in 0..2 {
        assert!(cursor.previous().is_err());
        assert_eq!(cursor.position(), Position::BeforeFirst);
        assert_hidden(&cursor);
    }
    cursor.next().unwrap();
    assert_frame(&cursor, 0);
    assert!(cursor.seek_relative(-1).is_err());
    assert_eq!(cursor.position(), Position::BeforeFirst);
    assert_hidden(&cursor);
    assert!(cursor.seek_relative(0).is_err());
    cursor.seek_relative(3).unwrap();
    assert_frame(&cursor, 2);
    assert!(cursor.next().is_err());
    assert_eq!(cursor.position(), Position::AfterLast);
    assert_hidden(&cursor);
    for offset in [0, 1, i64::MAX] {
        assert!(cursor.seek_relative(offset).is_err());
        assert_eq!(cursor.position(), Position::AfterLast);
        assert_hidden(&cursor);
    }
    cursor.previous().unwrap();
    assert_frame(&cursor, 2);
    assert!(cursor.read_at(3).is_err());
    assert_hidden(&cursor);
    cursor.seek_relative(-3).unwrap();
    assert_frame(&cursor, 0);
    assert!(cursor.read_at(usize::MAX).is_err());
    assert_hidden(&cursor);
    cursor.first().unwrap();
    assert!(cursor.seek_relative(i64::MIN).is_err());
    assert_eq!(cursor.position(), Position::BeforeFirst);
    assert_hidden(&cursor);
    assert!(cursor.seek_relative(i64::MAX).is_err());
    assert_eq!(cursor.position(), Position::AfterLast);
    assert_hidden(&cursor);
    cursor.previous().unwrap();
    assert_frame(&cursor, 2);
}
#[test]
fn signed_absolute_indices_reject_negatives_without_wrapping_or_stale_output() {
    let mut cursor = Cursor::new(from_bytes(&stack()));
    cursor.last().unwrap();
    for index in [-1, i64::MIN] {
        assert!(cursor.read_at_signed(index).is_err());
        assert_eq!(cursor.position(), Position::BeforeFirst);
        assert_hidden(&cursor);
        cursor.next().unwrap();
        assert_frame(&cursor, 0);
    }
    assert!(cursor.read_at_signed(i64::MAX).is_err());
    assert_eq!(cursor.position(), Position::AfterLast);
    assert_hidden(&cursor);
    cursor.read_at_signed(1).unwrap();
    assert_frame(&cursor, 1);
}
struct Empty;
impl FrameSource for Empty {
    fn frames(&self) -> usize {
        0
    }
    fn read_frame_into(&mut self, _: usize, _: &mut FrameBuffer) -> Result<(), wrapped::Error> {
        panic!("empty source must not be read")
    }
}
#[test]
fn empty_sources_never_call_decoder_and_keep_distinct_end_positions() {
    let mut cursor = Cursor::new(Empty);
    assert_eq!(cursor.frames(), 0);
    assert!(cursor.first().is_err());
    assert_eq!(cursor.position(), Position::AfterLast);
    assert!(cursor.previous().is_err());
    assert_eq!(cursor.position(), Position::BeforeFirst);
    assert!(cursor.next().is_err());
    assert_eq!(cursor.position(), Position::AfterLast);
    assert!(cursor.last().is_err());
    assert_eq!(cursor.position(), Position::BeforeFirst);
    assert!(cursor.read_at(0).is_err());
    assert_eq!(cursor.position(), Position::AfterLast);
    assert_hidden(&cursor);
}
// A synthetic repeated-frame source tests arithmetic near usize::MAX without
// allocating a frame manifest or claiming such a detector corpus exists.
struct Wide {
    reader: Reader,
    calls: usize,
}
impl FrameSource for Wide {
    fn frames(&self) -> usize {
        usize::MAX
    }
    fn read_frame_into(
        &mut self,
        _: usize,
        output: &mut FrameBuffer,
    ) -> Result<(), wrapped::Error> {
        self.calls += 1;
        self.reader.read_frame_into(0, output)
    }
}
#[test]
fn index_arithmetic_overflow_cannot_wrap_to_an_unrelated_frame() {
    let mut cursor = Cursor::new(Wide {
        reader: from_bytes(&stack()),
        calls: 0,
    });
    cursor.read_at(usize::MAX - 2).unwrap();
    assert_eq!(cursor.source().calls, 1);
    assert!(cursor.seek_relative(3).is_err());
    assert_eq!(cursor.position(), Position::AfterLast);
    assert_eq!(cursor.source().calls, 1);
    assert_hidden(&cursor);
    cursor.previous().unwrap();
    assert_eq!(cursor.current_index(), Some(usize::MAX - 1));
    assert_eq!(cursor.source().calls, 2);
    assert!(cursor.next().is_err());
    assert_eq!(cursor.position(), Position::AfterLast);
    assert_eq!(cursor.source().calls, 2);
    assert_hidden(&cursor);
}
struct Fault {
    reader: Reader,
    fail: bool,
    count: Cell<usize>,
    change_after_read: bool,
}
impl FrameSource for Fault {
    fn frames(&self) -> usize {
        self.count.get()
    }
    fn read_frame_into(
        &mut self,
        index: usize,
        output: &mut FrameBuffer,
    ) -> Result<(), wrapped::Error> {
        self.reader.read_frame_into(index, output)?;
        if self.change_after_read {
            self.count.set(4);
        }
        if self.fail {
            // Produce a real backend error after leaving the caller's output
            // valid, exercising the cursor's independent invalidation.
            return self
                .reader
                .read_frame_into(usize::MAX, &mut FrameBuffer::default());
        }
        Ok(())
    }
}
fn fault() -> Fault {
    Fault {
        reader: from_bytes(&stack()),
        fail: false,
        count: Cell::new(3),
        change_after_read: false,
    }
}
#[test]
fn failures_after_output_and_mutable_access_never_reveal_stale_pixels() {
    let mut cursor = Cursor::new(fault());
    cursor.first().unwrap();
    cursor.source_mut().fail = true;
    assert_hidden(&cursor);
    assert!(cursor.next().is_err());
    assert_eq!(cursor.position(), Position::Frame(0));
    assert_hidden(&cursor);
    assert!(cursor.next().is_err());
    assert_hidden(&cursor);
    cursor.source_mut().fail = false;
    cursor.next().unwrap();
    assert_eq!(cursor.current_index(), Some(1));
    assert_eq!(
        cursor.buffer().unwrap().pixels(),
        Some(&PixelBuffer::I32(vec![i32::MIN, i32::MAX]))
    );
}
#[test]
fn changed_frame_counts_fail_before_or_after_decode_without_visible_output() {
    let mut cursor = Cursor::new(fault());
    cursor.first().unwrap();
    cursor.source().count.set(4);
    assert!(cursor.next().is_err());
    assert_hidden(&cursor);
    assert!(cursor.read_at_signed(-1).is_err());
    assert_eq!(cursor.position(), Position::Frame(0));
    assert_hidden(&cursor);
    cursor.source().count.set(3);
    cursor.source_mut().change_after_read = true;
    assert!(cursor.next().is_err());
    assert_hidden(&cursor);
    assert_eq!(cursor.frames(), 3);
    assert_eq!(cursor.position(), Position::Frame(0));
}
#[test]
fn replacing_an_identical_source_invalidates_navigation_and_requires_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stack.edf");
    fs::write(&path, stack()).unwrap();
    let mut cursor = Cursor::new(from_path(&path));
    cursor.last().unwrap();
    assert_frame(&cursor, 2);
    let original = dir.path().join("original.edf");
    fs::rename(&path, &original).unwrap();
    fs::copy(&original, &path).unwrap();
    assert!(cursor.previous().is_err());
    assert_hidden(&cursor);
    assert_eq!(cursor.position(), Position::Frame(2));
    assert!(cursor.read_at(2).is_err());
    assert_hidden(&cursor);
    let mut reopened = Cursor::new(from_path(&path));
    reopened.last().unwrap();
    assert_frame(&reopened, 2);
}
