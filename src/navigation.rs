//! Explicit bounded navigation over a frozen source of native frames.
use crate::wrapped::{Error, FrameBuffer, Reader};

/// A frozen sequence whose successful reads preserve each frame's source dtype,
/// shape and counts. Implementations must check their source graph on each read,
/// and invalidate output on errors. Navigation performs no filename discovery.
pub trait FrameSource {
    fn frames(&self) -> usize;
    /// # Errors
    /// Invalid index, source mutation, unsupported layout or decoding failure.
    fn read_frame_into(&mut self, index: usize, output: &mut FrameBuffer) -> Result<(), Error>;
}
impl FrameSource for Reader {
    fn frames(&self) -> usize {
        Self::frames(self)
    }
    fn read_frame_into(&mut self, index: usize, output: &mut FrameBuffer) -> Result<(), Error> {
        Self::read_frame_into(self, index, output)
    }
}

/// Logical navigation anchor, distinct from whether decoded output is available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    BeforeFirst,
    Frame(usize),
    AfterLast,
}

/// Owns one reusable output frame and its source. Starts before the first frame.
/// Indexing is zero based; moves never wrap. Every operation clears visible
/// output before validation or I/O. A backend failure leaves the logical anchor
/// unchanged, allowing a retry; boundary failures select the corresponding end.
/// Only successful reads set `current_index` and expose `buffer`.
pub struct Cursor<S> {
    source: S,
    frames: usize,
    position: Position,
    current: Option<usize>,
    output: FrameBuffer,
}
impl<S: FrameSource> Cursor<S> {
    #[must_use]
    pub fn new(source: S) -> Self {
        let frames = source.frames();
        Self {
            source,
            frames,
            position: Position::BeforeFirst,
            current: None,
            output: FrameBuffer::default(),
        }
    }
    /// Frozen frame count; a later count change causes subsequent moves to fail.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.frames
    }
    #[must_use]
    pub fn position(&self) -> Position {
        self.position
    }
    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }
    /// Available only after the latest navigation operation succeeded.
    #[must_use]
    pub fn buffer(&self) -> Option<&FrameBuffer> {
        self.current.map(|_| &self.output)
    }
    #[must_use]
    pub fn source(&self) -> &S {
        &self.source
    }
    /// Clears visible output before allowing mutable access to the source.
    pub fn source_mut(&mut self) -> &mut S {
        self.clear();
        &mut self.source
    }
    #[must_use]
    pub fn into_inner(self) -> S {
        self.source
    }
    fn clear(&mut self) {
        self.current = None;
        self.output.invalidate();
    }
    fn check_count(&self) -> Result<(), Error> {
        if self.source.frames() != self.frames {
            return Err(Error(
                "navigation source frame count changed; reopen source".into(),
            ));
        }
        Ok(())
    }
    fn boundary(&mut self, position: Position) -> Result<&FrameBuffer, Error> {
        self.clear();
        self.check_count()?;
        self.position = position;
        Err(Error(match position {
            Position::BeforeFirst => "navigation before first frame".into(),
            _ => "navigation after last frame".into(),
        }))
    }
    /// Read an absolute zero-based frame index. Out-of-range indices select
    /// AfterLast; a decoder failure retains the preceding logical anchor.
    /// # Errors
    /// Invalid index, changed source/count or backend failure.
    pub fn read_at(&mut self, index: usize) -> Result<&FrameBuffer, Error> {
        self.clear();
        self.check_count()?;
        if index >= self.frames {
            return self.boundary(Position::AfterLast);
        }
        let result = self.source.read_frame_into(index, &mut self.output);
        if let Err(error) = result {
            self.clear();
            return Err(error);
        }
        if let Err(error) = self.check_count() {
            self.clear();
            return Err(error);
        }
        self.current = Some(index);
        self.position = Position::Frame(index);
        Ok(&self.output)
    }
    /// Read an absolute signed index without Python-style negative wrapping.
    /// Negative indices select BeforeFirst; an index too large for this platform
    /// selects AfterLast. Both clear visible output and return an error.
    /// # Errors
    /// Negative/out-of-range index, changed source/count or backend failure.
    pub fn read_at_signed(&mut self, index: i64) -> Result<&FrameBuffer, Error> {
        if index < 0 {
            return self.boundary(Position::BeforeFirst);
        }
        match usize::try_from(index) {
            Ok(index) => self.read_at(index),
            Err(_) => self.boundary(Position::AfterLast),
        }
    }
    /// Move forward one frame, including BeforeFirst to frame zero.
    /// # Errors
    /// End of source, source change or decoding failure.
    #[allow(clippy::should_implement_trait)] // Borrowed output and reversible boundaries are not Iterator semantics.
    pub fn next(&mut self) -> Result<&FrameBuffer, Error> {
        self.seek_relative(1)
    }
    /// Move backward one frame, including AfterLast to the final frame.
    /// # Errors
    /// Before source, source change or decoding failure.
    pub fn previous(&mut self) -> Result<&FrameBuffer, Error> {
        self.seek_relative(-1)
    }
    /// Read frame zero; an empty source selects AfterLast.
    /// # Errors
    /// Empty source, source change or decoding failure.
    pub fn first(&mut self) -> Result<&FrameBuffer, Error> {
        self.read_at(0)
    }
    /// Read the final frame; an empty source selects BeforeFirst.
    /// # Errors
    /// Empty source, source change or decoding failure.
    pub fn last(&mut self) -> Result<&FrameBuffer, Error> {
        match self.frames.checked_sub(1) {
            Some(index) => self.read_at(index),
            None => self.boundary(Position::BeforeFirst),
        }
    }
    /// Offset from the logical anchor. BeforeFirst acts as -1 and AfterLast as
    /// `frames`. Zero rereads a frame but cannot read either boundary. Overshoot
    /// selects a boundary without reading; integer overflow cannot wrap indices.
    /// # Errors
    /// Boundary/overflow, source change or decoding failure.
    pub fn seek_relative(&mut self, offset: i64) -> Result<&FrameBuffer, Error> {
        self.clear();
        self.check_count()?;
        let positive = offset >= 0;
        let Ok(distance) = usize::try_from(offset.unsigned_abs()) else {
            return self.boundary(if positive {
                Position::AfterLast
            } else {
                Position::BeforeFirst
            });
        };
        let target = match (self.position, positive) {
            (Position::BeforeFirst, true) => distance.checked_sub(1),
            (Position::BeforeFirst, false) => None,
            (Position::AfterLast, true) => return self.boundary(Position::AfterLast),
            (Position::AfterLast, false) => self.frames.checked_sub(distance),
            (Position::Frame(index), true) => match index.checked_add(distance) {
                Some(index) => Some(index),
                None => return self.boundary(Position::AfterLast),
            },
            (Position::Frame(index), false) => index.checked_sub(distance),
        };
        match target {
            Some(index) => self.read_at(index),
            None => self.boundary(Position::BeforeFirst),
        }
    }
}
