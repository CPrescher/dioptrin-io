use super::{BadPoint, Error, Limits, Mask, Point, Polygon};
use crate::PixelType;
use std::collections::BTreeMap;
#[derive(Debug)]
pub struct Decomposition {
    pub shape: [usize; 2],
    pub polygons: Vec<Polygon>,
    pub points: Vec<BadPoint>,
    pub complete: bool,
}
impl Decomposition {
    /// Reconstruct exactly the explicitly selected source mask representation.
    /// # Errors
    /// Output shape/resources exceed limits.
    pub fn build_mask(&self, limits: Limits) -> Result<Mask, Error> {
        limits.validate()?;
        check(self, limits)?;
        if self
            .polygons
            .iter()
            .any(|p| p.kind != 1 || p.x[0] > p.x[1] || p.y[0] > p.y[1])
        {
            return Err(Error("invalid decomposed rectangle".into()));
        }
        let mut mask = Mask::new(self.shape, limits)?;
        for p in &self.polygons {
            for y in usize::from(p.y[0])..=usize::from(p.y[1]) {
                for x in usize::from(p.x[0])..=usize::from(p.x[1]) {
                    if y >= self.shape[0] || x >= self.shape[1] {
                        return Err(Error("decomposed rectangle bounds".into()));
                    }
                    mask.values[y * self.shape[1] + x] = 1;
                }
            }
        }
        for p in &self.points {
            let x = usize::from(p.point.x);
            let y = usize::from(p.point.y);
            if y >= self.shape[0] || x >= self.shape[1] {
                return Err(Error("decomposed point bounds".into()));
            }
            mask.values[y * self.shape[1] + x] = 1;
        }
        Ok(mask)
    }
}
fn polygon(x0: usize, x1: usize, y0: usize, y1: usize) -> Polygon {
    Polygon {
        kind: 1,
        points: 4,
        x: [x0 as u16, x1 as u16, 0, 0, 0, 0],
        y: [y0 as u16, y1 as u16, 0, 0, 0, 0],
    }
}
fn runs(values: impl Iterator<Item = bool>) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut start = None;
    let mut end = 0;
    for (i, value) in values.enumerate() {
        end = i + 1;
        if value && start.is_none() {
            start = Some(i);
        }
        if !value && let Some(start) = start.take() {
            result.push((start, i));
        }
    }
    if let Some(start) = start {
        result.push((start, end));
    }
    result
}
/// Explicit decomposition of a binary mask, never of detector counts. Full mode
/// uses a lossless native row-run cover; rectangle partitions are not unique and
/// need not duplicate pyFAI's optional partition. Coarse mode retains only full
/// row/column gaps, matching FabIO's dependency-free representation.
/// # Errors
/// Nonbinary input, mismatched shape, u16 coordinate or record/resource bounds.
pub fn decompose(
    values: &[u8],
    shape: [usize; 2],
    full: bool,
    limits: Limits,
) -> Result<Decomposition, Error> {
    limits.validate()?;
    let count = limits.pixels(shape, PixelType::U8)?;
    if count != values.len() || shape.iter().any(|&n| n > 65536) || values.iter().any(|&v| v > 1) {
        return Err(Error(
            "Xcalibur decomposition requires bounded binary source geometry".into(),
        ));
    }
    let base_cost = shape[0].saturating_add(shape[1]).saturating_mul(32);
    if base_cost > limits.metadata_bytes {
        return Err(Error("Xcalibur decomposition scratch limit".into()));
    }
    let rows: Vec<_> = (0..shape[0])
        .map(|y| {
            values[y * shape[1]..(y + 1) * shape[1]]
                .iter()
                .all(|&v| v == 1)
        })
        .collect();
    let columns: Vec<_> = (0..shape[1])
        .map(|x| (0..shape[0]).all(|y| values[y * shape[1] + x] == 1))
        .collect();
    let mut result = Decomposition {
        shape,
        polygons: Vec::new(),
        points: Vec::new(),
        complete: full,
    };
    for (a, b) in runs(rows.iter().copied()) {
        ensure_record(&result, limits)?;
        result.polygons.push(polygon(0, shape[1] - 1, a, b - 1));
    }
    for (a, b) in runs(columns.iter().copied()) {
        ensure_record(&result, limits)?;
        result.polygons.push(polygon(a, b - 1, 0, shape[0] - 1));
    }
    let mut active: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    if full {
        for y in 0..shape[0] {
            let mut next = BTreeMap::new();
            if !rows[y] {
                for (x0, stop) in
                    runs((0..shape[1]).map(|x| !columns[x] && values[y * shape[1] + x] == 1))
                {
                    let key = (x0, stop);
                    let index = if let Some(&index) = active.get(&key) {
                        result.polygons[index].y[1] = y as u16;
                        index
                    } else {
                        ensure_record(&result, limits)?;
                        result.polygons.push(polygon(x0, stop - 1, y, y));
                        result.polygons.len() - 1
                    };
                    next.insert(key, index);
                }
            }
            active = next;
            check(&result, limits)?;
        }
        let mut rectangles = Vec::new();
        for poly in result.polygons {
            if poly.x[0] == poly.x[1] && poly.y[0] == poly.y[1] {
                let point = Point {
                    x: poly.x[0],
                    y: poly.y[0],
                };
                result.points.push(BadPoint {
                    point,
                    replacements: [point; 2],
                    treatment: 0,
                });
            } else {
                rectangles.push(poly);
            }
        }
        result.polygons = rectangles;
    }
    check(&result, limits)?;
    Ok(result)
}
fn check(value: &Decomposition, limits: Limits) -> Result<(), Error> {
    let records = value.polygons.len().saturating_add(value.points.len());
    if records > limits.frames
        || records.saturating_mul(128).saturating_add(
            value.shape[0]
                .saturating_add(value.shape[1])
                .saturating_mul(32),
        ) > limits.metadata_bytes
    {
        return Err(Error("Xcalibur decomposition record/metadata limit".into()));
    }
    Ok(())
}

fn ensure_record(value: &Decomposition, limits: Limits) -> Result<(), Error> {
    let records = value
        .polygons
        .len()
        .saturating_add(value.points.len())
        .saturating_add(1);
    if records > limits.frames
        || records.saturating_mul(128).saturating_add(
            value.shape[0]
                .saturating_add(value.shape[1])
                .saturating_mul(32),
        ) > limits.metadata_bytes
    {
        return Err(Error("Xcalibur decomposition record/metadata limit".into()));
    }
    Ok(())
}
