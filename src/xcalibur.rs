//! CrysAlis Xcalibur CCD characteristics and explicitly requested mask geometry.
//! Binary field schema adapted from MIT FabIO xcaliburimage.py (ESRF, Jérôme
//! Kieffer); notice in licenses/native-readers/FabIO-Xcalibur-MIT.txt. Source
//! orientation, treatment flags, reserved coordinates and original bytes survive.
pub use crate::binary::{Error, Limits};
use crate::{PixelType, source::Snapshot};
use std::{fs::File, io::Read, path::Path};
mod decomposition;
pub mod run;
pub use decomposition::{Decomposition, decompose};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadPoint {
    pub point: Point,
    pub replacements: [Point; 2],
    pub treatment: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadRow {
    pub start: Point,
    pub end: Point,
    pub replacements: [Point; 2],
    pub treatment: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadColumn {
    pub row: BadRow,
    pub low_limit: u16,
    pub high_limit: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Polygon {
    pub kind: u16,
    pub points: u16,
    pub x: [u16; 6],
    pub y: [u16; 6],
}
#[derive(Clone, Debug, PartialEq)]
pub struct Electronics {
    pub scintillator: u16,
    pub gain_mo: f64,
    pub gain_cu: f64,
    pub machine: Option<MachineFunction>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct MachineFunction {
    pub enabled: u16,
    pub a: f64,
    pub b: f64,
}
/// Only the known terminal machine-function truncation may be retained. Counted
/// sections and fixed source headers must always be complete.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadPolicy {
    #[default]
    Strict,
    RetainTruncatedMachineFunction,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncompleteMachineFunction {
    pub offset: usize,
    pub expected_bytes: usize,
    pub available_bytes: usize,
}
/// Fixed source text octets; invalid UTF-8 and all zero padding remain readable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text(pub [u8; 256]);
impl Text {
    /// Source-compatible UTF-8 view, trimming only the surrounding NUL bytes.
    /// # Errors
    /// Original bytes are not UTF-8; Text.0 remains lossless.
    pub fn text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0).map(|s| s.trim_matches('\0'))
    }
}
#[derive(Clone, Debug)]
pub struct Characteristics {
    pub version: u32,
    pub dark_current: f64,
    pub read_noise: f64,
    /// Characteristics filename, CCD producer/type/serial, taper producer/type/serial.
    pub text: [Text; 7],
    pub fip_origin: [u16; 3],
    pub corner_count: u16,
    pub corner_x: [u16; 4],
    pub corner_y: [u16; 4],
    pub glowing_count: u16,
    pub glowing_x: [u16; 4],
    pub glowing_y: [u16; 4],
    /// None denotes an absent complete optional section, not an invented zero count.
    pub polygons: Option<Vec<Polygon>>,
    pub points: Option<Vec<BadPoint>>,
    /// General, 1x1, 2x2, 4x4 source columns, in that order.
    pub columns: [Option<Vec<BadColumn>>; 4],
    /// General, 1x1, 2x2, 4x4 source rows, in that order.
    pub rows: [Option<Vec<BadRow>>; 4],
    pub electronics: Option<Electronics>,
    pub incomplete_machine_function: Option<IncompleteMachineFunction>,
    pub parsed_bytes: usize,
    /// Full original source, including absent-section boundaries and trailing bytes.
    pub original: Vec<u8>,
}
struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    records: usize,
    limits: Limits,
}
impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self
            .position
            .checked_add(n)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| Error("truncated Xcalibur record".into()))?;
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn point(&mut self) -> Result<Point, Error> {
        Ok(Point {
            x: self.u16()?,
            y: self.u16()?,
        })
    }
    fn row(&mut self) -> Result<BadRow, Error> {
        Ok(BadRow {
            start: self.point()?,
            end: self.point()?,
            replacements: [self.point()?, self.point()?],
            treatment: self.u16()?,
        })
    }
    fn section<T>(
        &mut self,
        size: usize,
        read: impl Fn(&mut Self) -> Result<T, Error>,
    ) -> Result<Option<Vec<T>>, Error> {
        if self.position == self.bytes.len() {
            return Ok(None);
        }
        let count = usize::from(self.u16()?);
        self.records = self
            .records
            .checked_add(count)
            .filter(|&n| n <= self.limits.frames)
            .ok_or_else(|| Error("Xcalibur record limit".into()))?;
        count
            .checked_mul(size)
            .and_then(|n| self.position.checked_add(n))
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| Error("truncated Xcalibur counted section".into()))?;
        let cost = self
            .bytes
            .len()
            .saturating_mul(3)
            .saturating_add(self.records.saturating_mul(64));
        if cost > self.limits.metadata_bytes {
            return Err(Error("Xcalibur retained metadata limit".into()));
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|e| Error(e.to_string()))?;
        for _ in 0..count {
            values.push(read(self)?);
        }
        Ok(Some(values))
    }
}
impl Characteristics {
    /// Decode little-endian stored records. Complete older section prefixes are
    /// retained as such; partial counts/records fail, independently of FabIO's
    /// permissive defaults. Unknown versions fail explicitly.
    /// # Errors
    /// Unknown version, truncated structures or configured resource bounds.
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self, Error> {
        Self::from_bytes_with_policy(bytes, limits, ReadPolicy::Strict)
    }
    /// Retain an explicitly admitted incomplete terminal machine function as raw
    /// bytes plus a diagnostic; no missing fields or row counts are invented.
    /// # Errors
    /// Malformed headers/counts, unsupported versions, limits or unadmitted truncation.
    pub fn from_bytes_with_policy(
        bytes: &[u8],
        limits: Limits,
        policy: ReadPolicy,
    ) -> Result<Self, Error> {
        limits.validate()?;
        if bytes.len() < 1854
            || bytes.len() > limits.encoded_bytes
            || bytes.len().saturating_mul(3) > limits.metadata_bytes
        {
            return Err(Error("Xcalibur source/metadata bounds".into()));
        }
        let mut c = Cursor {
            bytes,
            position: 0,
            records: 0,
            limits,
        };
        let version = c.u32()?;
        if !(0x10001..=0x1000c).contains(&version) {
            return Err(Error(
                "unsupported Xcalibur CCD version (explicit little endian)".into(),
            ));
        }
        let dark_current = c.f64()?;
        let read_noise = c.f64()?;
        let text: Vec<_> = (0..7)
            .map(|_| c.take(256).map(|s| Text(s.try_into().unwrap())))
            .collect::<Result<_, _>>()?;
        let fip_origin = [c.u16()?, c.u16()?, c.u16()?];
        let corner_count = c.u16()?;
        let mut corner_x = [0; 4];
        let mut corner_y = [0; 4];
        for v in &mut corner_x {
            *v = c.u16()?;
        }
        for v in &mut corner_y {
            *v = c.u16()?;
        }
        let glowing_count = c.u16()?;
        let mut glowing_x = [0; 4];
        let mut glowing_y = [0; 4];
        for v in &mut glowing_x {
            *v = c.u16()?;
        }
        for v in &mut glowing_y {
            *v = c.u16()?;
        }
        let polygons = c.section(28, |c| {
            let kind = c.u16()?;
            let points = c.u16()?;
            let mut x = [0; 6];
            let mut y = [0; 6];
            for v in &mut x {
                *v = c.u16()?;
            }
            for v in &mut y {
                *v = c.u16()?;
            }
            Ok(Polygon { kind, points, x, y })
        })?;
        let points = c.section(14, |c| {
            Ok(BadPoint {
                point: c.point()?,
                replacements: [c.point()?, c.point()?],
                treatment: c.u16()?,
            })
        })?;
        let mut columns = std::array::from_fn(|_| None);
        for values in &mut columns {
            *values = c.section(22, |c| {
                Ok(BadColumn {
                    row: c.row()?,
                    low_limit: c.u16()?,
                    high_limit: c.u16()?,
                })
            })?;
        }
        let mut rows = std::array::from_fn(|_| None);
        rows[0] = c.section(18, |c| c.row())?;
        let mut incomplete_machine_function = None;
        let electronics = if c.position == bytes.len() {
            None
        } else {
            let scintillator = c.u16()?;
            let gain_mo = c.f64()?;
            let gain_cu = c.f64()?;
            let available = bytes.len() - c.position;
            let machine = if available < 18 && policy == ReadPolicy::RetainTruncatedMachineFunction
            {
                incomplete_machine_function = Some(IncompleteMachineFunction {
                    offset: c.position,
                    expected_bytes: 18,
                    available_bytes: available,
                });
                None
            } else {
                Some(MachineFunction {
                    enabled: c.u16()?,
                    a: c.f64()?,
                    b: c.f64()?,
                })
            };
            Some(Electronics {
                scintillator,
                gain_mo,
                gain_cu,
                machine,
            })
        };
        if incomplete_machine_function.is_none() {
            for values in rows.iter_mut().skip(1) {
                *values = c.section(18, |c| c.row())?;
            }
        }
        Ok(Self {
            version,
            dark_current,
            read_noise,
            text: text.try_into().unwrap(),
            fip_origin,
            corner_count,
            corner_x,
            corner_y,
            glowing_count,
            glowing_x,
            glowing_y,
            polygons,
            points,
            columns,
            rows,
            electronics,
            incomplete_machine_function,
            parsed_bytes: c.position,
            original: bytes.to_vec(),
        })
    }
    /// Explicit derived bitmap; geometry is never applied to detector counts.
    /// # Errors
    /// Unsupported strict geometry, invalid bounds, shape or output limits.
    pub fn build_mask(
        &self,
        shape: [usize; 2],
        projection: Projection,
        limits: Limits,
    ) -> Result<Mask, Error> {
        limits.validate()?;
        let records = self
            .columns
            .iter()
            .flatten()
            .map(Vec::len)
            .chain(self.rows.iter().flatten().map(Vec::len))
            .fold(0usize, usize::saturating_add)
            .saturating_add(self.polygons.as_ref().map_or(0, Vec::len))
            .saturating_add(self.points.as_ref().map_or(0, Vec::len));
        if records > limits.frames || records.saturating_mul(64) > limits.metadata_bytes {
            return Err(Error("Xcalibur projection record/metadata limit".into()));
        }
        let mut mask = Mask::new(shape, limits)?;
        for poly in self.polygons.as_deref().unwrap_or_default() {
            if poly.kind != 1 {
                if projection == Projection::Strict {
                    return Err(Error("unsupported Xcalibur polygon projection".into()));
                }
                mask.ignored_records += 1;
                continue;
            }
            let [x0, x1] = [usize::from(poly.x[0]), usize::from(poly.x[1])];
            let [y0, y1] = [usize::from(poly.y[0]), usize::from(poly.y[1])];
            if projection == Projection::Strict
                && (x0 > x1 || y0 > y1 || x1 >= shape[1] || y1 >= shape[0])
            {
                return Err(Error("Xcalibur rectangle outside requested shape".into()));
            }
            for y in y0.min(shape[0])..(y1 + 1).min(shape[0]) {
                for x in x0.min(shape[1])..(x1 + 1).min(shape[1]) {
                    mask.values[y * shape[1] + x] = 1;
                }
            }
        }
        for point in self.points.as_deref().unwrap_or_default() {
            let [x, y] = [usize::from(point.point.x), usize::from(point.point.y)];
            if x >= shape[1] || y >= shape[0] {
                return Err(Error("Xcalibur point outside requested shape".into()));
            }
            mask.values[y * shape[1] + x] = 1;
        }
        let omitted = self.columns.iter().flatten().map(Vec::len).sum::<usize>()
            + self.rows.iter().flatten().map(Vec::len).sum::<usize>()
            + usize::from(self.corner_count)
            + usize::from(self.glowing_count);
        if omitted > 0 && projection == Projection::Strict {
            return Err(Error("row/column/corner projection requires an explicit source treatment model; Fabio projection ignores them".into()));
        }
        mask.ignored_records += omitted;
        Ok(mask)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Strict,
    Fabio2026,
}
#[derive(Clone, Debug)]
pub struct Mask {
    pub shape: [usize; 2],
    pub values: Vec<u8>,
    pub ignored_records: usize,
}
impl Mask {
    fn new(shape: [usize; 2], limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let count = limits.pixels(shape, PixelType::U8)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|e| Error(e.to_string()))?;
        values.resize(count, 0);
        Ok(Self {
            shape,
            values,
            ignored_records: 0,
        })
    }
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Characteristics,
}
impl Reader {
    /// Index immutable source metadata under explicit byte/record bounds.
    /// # Errors
    /// Unsupported/malformed source, allocation bounds or source mutation.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        Self::open_with_policy(path, limits, ReadPolicy::Strict)
    }
    /// # Errors
    /// Source mutation, limits or malformed source outside the explicit policy.
    pub fn open_with_policy(
        path: impl AsRef<Path>,
        limits: Limits,
        policy: ReadPolicy,
    ) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if snapshot.bytes() > limits.encoded_bytes.min(limits.metadata_bytes / 3) as u64 {
            return Err(Error("Xcalibur source byte limit".into()));
        }
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take((limits.encoded_bytes.min(limits.metadata_bytes / 3) as u64).saturating_add(1))
            .read_to_end(&mut bytes)?;
        let metadata = Characteristics::from_bytes_with_policy(&bytes, limits, policy)?;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            metadata,
        })
    }
    /// # Errors
    /// Original source changed after indexing.
    pub fn metadata(&self) -> Result<&Characteristics, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(&self.metadata)
    }
    /// # Errors
    /// Changed source, unsupported projection or mask bounds.
    pub fn build_mask(
        &self,
        shape: [usize; 2],
        projection: Projection,
        limits: Limits,
    ) -> Result<Mask, Error> {
        let mask = self.metadata()?.build_mask(shape, projection, limits)?;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        Ok(mask)
    }
}
