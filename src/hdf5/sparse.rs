//! Frozen pyFAI sparse frames. Stored index/intensity samples are the default;
//! background reconstruction is an explicitly selected derived representation.
mod numeric;
mod reconstruction;
pub use super::{Error, Filter};
use super::{sources::Sources, typed};
pub use crate::binary::FrameBuffer;
use crate::{PixelBuffer, PixelType};
use hdf5_backend::{Dataset, Location, LocationType, types::TypeDescriptor};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
pub use typed::{ExternalLinks, Link, Source};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Reconstruction {
    /// Expose source events and component arrays. Dense reads require selection.
    #[default]
    StoredSparse,
    /// Explicit derived view matching the pinned native FabIO NOISY=False
    /// endpoint-grid interpolation, background normalization and integer rounding.
    DeterministicBackground,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub hdf5: typed::Limits,
    /// Maximum number of stored events across all frames; arrays are not cached.
    pub events: usize,
    /// Maximum bytes for an explicitly requested complete numeric component.
    pub component_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            hdf5: typed::Limits::default(),
            events: 100_000_000,
            component_bytes: 256 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub limits: Limits,
    pub external_links: ExternalLinks,
    pub reconstruction: Reconstruction,
}
#[derive(Debug, Clone, Copy, Default)]
pub enum Selection<'a> {
    #[default]
    Default,
    Group(&'a str),
}
#[derive(Debug, Clone)]
pub enum AttributeValue {
    Numeric {
        shape: Vec<usize>,
        pixel_type: PixelType,
        little_endian: bool,
        values: PixelBuffer,
    },
    Text(Vec<String>),
}
#[derive(Debug, Clone)]
pub struct AttributeStorage {
    pub shape: Vec<usize>,
    pub type_descriptor: String,
}
#[derive(Debug, Clone)]
pub struct Attributes {
    pub source_file: PathBuf,
    pub path: String,
    pub values: BTreeMap<String, AttributeValue>,
    /// Original shape/character representation for text and numeric attributes.
    pub declarations: BTreeMap<String, AttributeStorage>,
}
#[derive(Debug, Clone)]
pub struct Component {
    /// Exact source group-relative name. Optional entry metadata uses entry/.
    pub name: String,
    pub source_file: PathBuf,
    pub dataset: String,
    pub shape: Vec<usize>,
    pub pixel_type: Option<PixelType>,
    pub little_endian: Option<bool>,
    pub type_descriptor: String,
    pub layout: hdf5_backend::dataset::Layout,
    pub filters: Vec<Filter>,
    pub fill_value: Option<PixelBuffer>,
    pub storage_allocated: bool,
    pub fill_time: hdf5_backend::dataset::FillTime,
    /// Text values are retained verbatim; numeric values use bounded read APIs.
    pub text: Option<Vec<String>>,
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub source_file: PathBuf,
    pub group: String,
    pub shape: [usize; 2],
    pub frames: usize,
    pub intensity_type: PixelType,
    pub index_type: PixelType,
    pub reconstruction: Reconstruction,
    /// Reconstructed pixels include a model background and are not measured counts.
    pub dense_is_derived: bool,
    pub components: Vec<Component>,
    pub attributes: Vec<Attributes>,
}
struct Stored {
    dataset: Dataset,
    metadata: Component,
}
pub struct Reader {
    sources: Sources,
    metadata: Metadata,
    stored: Vec<Stored>,
    names: BTreeMap<String, usize>,
    frame_ptr: Vec<usize>,
    index: usize,
    intensity: usize,
    mask: usize,
    pointer: usize,
    options: Options,
}
#[derive(Debug, Default)]
pub struct ArrayBuffer {
    pixels: PixelBuffer,
    shape: Option<Vec<usize>>,
}
impl ArrayBuffer {
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        self.shape.as_ref().map(|_| &self.pixels)
    }
    #[must_use]
    pub fn shape(&self) -> Option<&[usize]> {
        self.shape.as_deref()
    }
}
#[derive(Debug, Default)]
pub struct SparseFrameBuffer {
    index: ArrayBuffer,
    intensity: ArrayBuffer,
    shape: Option<[usize; 2]>,
    range: Option<[usize; 2]>,
}
impl SparseFrameBuffer {
    #[must_use]
    pub fn index(&self) -> Option<&PixelBuffer> {
        self.shape.map(|_| &self.index.pixels)
    }
    #[must_use]
    pub fn intensity(&self) -> Option<&PixelBuffer> {
        self.shape.map(|_| &self.intensity.pixels)
    }
    #[must_use]
    pub fn shape(&self) -> Option<[usize; 2]> {
        self.shape
    }
    #[must_use]
    pub fn source_range(&self) -> Option<[usize; 2]> {
        self.range
    }
}
impl Reader {
    /// Open immutable sparse data without constructing a dense background.
    /// # Errors
    /// Unsupported storage/metadata, inconsistent sparse arrays, links or bounds.
    pub fn open(path: impl AsRef<Path>, options: Options) -> Result<Self, Error> {
        Self::open_snapshot(path, Selection::Default, options)
    }
    /// Select the source group explicitly or follow exact declared NeXus defaults.
    /// # Errors
    /// Unsupported filters/layouts, malformed indices/pointers or source mutation.
    pub fn open_snapshot(
        path: impl AsRef<Path>,
        selection: Selection<'_>,
        options: Options,
    ) -> Result<Self, Error> {
        Self::open_snapshot_with_origin(path, selection, options, None)
    }
    /// Capture/relocate the root with an explicitly supplied relative-link directory.
    /// # Errors
    /// Same immutable storage, bounds and source validation as open_snapshot.
    pub fn open_snapshot_with_origin(
        path: impl AsRef<Path>,
        selection: Selection<'_>,
        options: Options,
        origin: Option<&Path>,
    ) -> Result<Self, Error> {
        let limits = options.limits;
        numeric::validate_limits(limits)?;
        super::register_filters()?;
        let mut sources =
            Sources::new_with_origin(path, limits.hdf5, options.external_links, origin)?;
        let mut attributes = Vec::new();
        let mut entry = None;
        let (source, group_path, group) = match selection {
            Selection::Default => {
                let (_, _, root) = sources.group(0, "/")?;
                let rootattrs = numeric::attributes(&mut sources, 0, &root, limits.hdf5)?;
                let name = numeric::text_attribute(&rootattrs, "default")?;
                attributes.push(rootattrs);
                let (source, path, entry_group) = sources.group(0, &name)?;
                let attrs = numeric::attributes(&mut sources, source, &entry_group, limits.hdf5)?;
                let default = if attrs.values.contains_key("pyFAI_sparse_frames") {
                    numeric::text_attribute(&attrs, "pyFAI_sparse_frames")?
                } else {
                    numeric::text_attribute(&attrs, "default")?
                };
                attributes.push(attrs);
                entry = Some((source, path.clone()));
                let requested = if default.starts_with('/') {
                    default
                } else {
                    format!("{}/{default}", path.trim_end_matches('/'))
                };
                sources.group(source, &requested)?
            }
            Selection::Group(path) => sources.group(0, path)?,
        };
        let main_file = sources.path(source).to_path_buf();
        let mut stored = Vec::new();
        let mut names = BTreeMap::new();
        discover(
            &mut sources,
            source,
            &group_path,
            "",
            &mut stored,
            &mut names,
            &mut attributes,
            limits,
            0,
        )?;
        if let Some(attrs) = attributes
            .iter()
            .find(|a| a.source_file == main_file && a.path == group.name())
            && attrs.values.contains_key("dataformat")
            && numeric::text_attribute(attrs, "dataformat")? != "sparsify-Bragg"
        {
            return Err(Error(
                "HDF5 group declares a different sparse data format".into(),
            ));
        }
        if let Some((entry_source, entry_path)) = entry {
            let (_, _, entry_group) = sources.group(entry_source, &entry_path)?;
            for name in sources.names(&entry_group)? {
                if name == "peaks" || name == "sparsify" {
                    discover(
                        &mut sources,
                        entry_source,
                        &format!("{entry_path}/{name}"),
                        &format!("entry/{name}"),
                        &mut stored,
                        &mut names,
                        &mut attributes,
                        limits,
                        0,
                    )?;
                }
            }
        }
        let required = |name: &str| {
            names
                .get(name)
                .copied()
                .ok_or_else(|| Error(format!("missing sparse component {name}")))
        };
        let mask = required("mask")?;
        let index = required("index")?;
        let intensity = required("intensity")?;
        let pointer = required("frame_ptr")?;
        let shape: [usize; 2] = stored[mask]
            .metadata
            .shape
            .clone()
            .try_into()
            .map_err(|_| Error("sparse mask must be a 2D radius image".into()))?;
        if shape.contains(&0)
            || !matches!(
                stored[mask].metadata.pixel_type,
                Some(PixelType::F32 | PixelType::F64)
            )
        {
            return Err(Error(
                "sparse mask requires nonempty IEEE float radii".into(),
            ));
        }
        let pixels = shape[0]
            .checked_mul(shape[1])
            .ok_or_else(|| Error("sparse shape overflow".into()))?;
        let intensity_type = stored[intensity]
            .metadata
            .pixel_type
            .ok_or_else(|| Error("sparse intensity must be native numeric samples".into()))?;
        let index_type = stored[index]
            .metadata
            .pixel_type
            .ok_or_else(|| Error("sparse index must be integers".into()))?;
        if !numeric::is_integer(index_type)
            || !stored[pointer]
                .metadata
                .pixel_type
                .is_some_and(numeric::is_integer)
        {
            return Err(Error("sparse frame_ptr/index require integer types".into()));
        }
        let event_count = match stored[intensity].metadata.shape.as_slice() {
            [n] => *n,
            _ => return Err(Error("sparse intensity must be 1D".into())),
        };
        if stored[index].metadata.shape != [event_count] || event_count > limits.events {
            return Err(Error(
                "sparse index/intensity lengths disagree or exceed event limit".into(),
            ));
        }
        if pixels
            .checked_mul(intensity_type.bytes())
            .is_none_or(|n| n > limits.hdf5.frame_bytes)
        {
            return Err(Error("sparse image exceeds decoded frame limit".into()));
        }
        let frames = match stored[pointer].metadata.shape.as_slice() {
            [n] if *n >= 2 => n - 1,
            _ => {
                return Err(Error(
                    "sparse frame_ptr requires at least two boundaries".into(),
                ));
            }
        };
        if frames > limits.hdf5.frames {
            return Err(Error("sparse frame count exceeds limit".into()));
        }
        let charge = (frames + 1)
            .checked_mul(
                std::mem::size_of::<usize>() + stored[pointer].metadata.pixel_type.unwrap().bytes(),
            )
            .ok_or_else(|| Error("sparse pointer allocation overflow".into()))?;
        sources.charge(charge)?;
        let mut buffer = ArrayBuffer::default();
        numeric::read(
            &stored[pointer],
            None,
            &mut buffer,
            limits.hdf5.metadata_bytes,
        )?;
        let frame_ptr = numeric::indices(&buffer.pixels)?;
        if frame_ptr.first() != Some(&0)
            || frame_ptr.last() != Some(&event_count)
            || frame_ptr.windows(2).any(|w| w[0] > w[1])
        {
            return Err(Error(
                "sparse frame_ptr must span all events monotonically from zero".into(),
            ));
        }
        validate_auxiliary(&stored, &names, shape, frames, limits)?;
        let mut seen = Vec::new();
        for range in frame_ptr.windows(2) {
            let events = range[1] - range[0];
            if events > pixels
                || events
                    .checked_mul(index_type.bytes() + intensity_type.bytes())
                    .is_none_or(|n| n > limits.hdf5.frame_bytes)
                || events
                    .checked_mul(std::mem::size_of::<usize>())
                    .is_none_or(|n| n > limits.hdf5.frame_bytes)
            {
                return Err(Error(
                    "sparse frame event count exceeds shape/byte limits".into(),
                ));
            }
            if events == 0 {
                continue;
            }
            numeric::read(
                &stored[index],
                Some((range[0], range[1])),
                &mut buffer,
                limits.hdf5.frame_bytes,
            )?;
            validate_indices(&buffer.pixels, pixels, &mut seen)?;
        }
        sources.check()?;
        let components = stored.iter().map(|s| s.metadata.clone()).collect();
        Ok(Self {
            sources,
            metadata: Metadata {
                source_file: main_file,
                group: group_path,
                shape,
                frames,
                intensity_type,
                index_type,
                reconstruction: options.reconstruction,
                dense_is_derived: options.reconstruction != Reconstruction::StoredSparse,
                components,
                attributes,
            },
            stored,
            names,
            frame_ptr,
            index,
            intensity,
            mask,
            pointer,
            options,
        })
    }
    fn check(&self) -> Result<(), Error> {
        self.sources.check()?;
        if self
            .stored
            .iter()
            .any(|stored| stored.dataset.shape() != stored.metadata.shape)
        {
            return Err(Error(
                "sparse dataset shape changed; reopen immutable snapshot".into(),
            ));
        }
        Ok(())
    }
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.metadata.frames
    }
    #[must_use]
    pub fn source_bindings(&self) -> &[crate::SourceBinding] {
        &self.sources.bindings
    }
    /// # Errors
    /// Any source, alias or frozen sparse layout changed.
    pub fn check_sources(&self) -> Result<(), Error> {
        self.check()
    }
    #[must_use]
    pub fn sources(&self) -> &[Source] {
        &self.sources.records
    }
    #[must_use]
    pub fn links(&self) -> &[Link] {
        &self.sources.links
    }
    /// Read one original numeric component. No dtype conversion or reconstruction.
    /// # Errors
    /// Unknown/text component, source change, numeric bounds or native HDF5 failure.
    pub fn read_component_into(&self, name: &str, buffer: &mut ArrayBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.check()?;
        let &index = self
            .names
            .get(name)
            .ok_or_else(|| Error("sparse component outside source".into()))?;
        numeric::read(
            &self.stored[index],
            None,
            buffer,
            self.options.limits.component_bytes,
        )?;
        let shape = buffer.shape.take();
        self.check()?;
        buffer.shape = shape;
        Ok(())
    }
    /// Return original index/intensity arrays for one frame. Event order, source
    /// dtype, signedness and all reserved intensity values remain unchanged.
    /// # Errors
    /// Invalid frame, source mutation or native decoding failure.
    pub fn read_sparse_frame_into(
        &self,
        index: usize,
        buffer: &mut SparseFrameBuffer,
    ) -> Result<(), Error> {
        buffer.shape = None;
        buffer.range = None;
        self.check()?;
        let range = self
            .frame_ptr
            .get(index..index.saturating_add(2))
            .filter(|r| r.len() == 2)
            .ok_or_else(|| Error("sparse frame outside source".into()))?;
        let mut control = ArrayBuffer::default();
        numeric::read(
            &self.stored[self.pointer],
            Some((index, index + 2)),
            &mut control,
            self.options.limits.hdf5.metadata_bytes,
        )?;
        if numeric::index(&control.pixels, 0)? != range[0]
            || numeric::index(&control.pixels, 1)? != range[1]
        {
            return Err(Error(
                "sparse frame_ptr changed; reopen immutable snapshot".into(),
            ));
        }
        numeric::read(
            &self.stored[self.index],
            Some((range[0], range[1])),
            &mut buffer.index,
            self.options.limits.hdf5.frame_bytes,
        )?;
        numeric::read(
            &self.stored[self.intensity],
            Some((range[0], range[1])),
            &mut buffer.intensity,
            self.options.limits.hdf5.frame_bytes,
        )?;
        validate_indices(
            &buffer.index.pixels,
            self.metadata.shape[0] * self.metadata.shape[1],
            &mut Vec::new(),
        )?;
        self.check()?;
        buffer.shape = Some(self.metadata.shape);
        buffer.range = Some([range[0], range[1]]);
        Ok(())
    }
    /// Construct an explicitly selected deterministic derived image. This never
    /// represents the modeled background as measured native detector counts.
    /// # Errors
    /// Policy absent, invalid frame/components, source mutation or numeric overflow.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        if self.options.reconstruction != Reconstruction::DeterministicBackground {
            return Err(Error(
                "dense sparse-image output requires explicit DeterministicBackground policy".into(),
            ));
        }
        self.check()?;
        reconstruction::read(self, index, buffer)?;
        self.check()?;
        buffer.shape = Some(self.metadata.shape);
        Ok(())
    }
}
#[allow(clippy::too_many_arguments)]
fn discover(
    sources: &mut Sources,
    source: usize,
    path: &str,
    name: &str,
    stored: &mut Vec<Stored>,
    names: &mut BTreeMap<String, usize>,
    attributes: &mut Vec<Attributes>,
    limits: Limits,
    depth: usize,
) -> Result<(), Error> {
    if depth > limits.hdf5.link_traversals {
        return Err(Error("sparse group cycle/nesting limit".into()));
    }
    let (source, path) = sources.resolve(source, path)?;
    match sources.file(source).loc_type_by_name(&path)? {
        LocationType::Group => {
            let group = sources.file(source).group(&path)?;
            attributes.push(numeric::attributes(sources, source, &group, limits.hdf5)?);
            for child in sources.names(&group)? {
                let name = if name.is_empty() {
                    child.clone()
                } else {
                    format!("{name}/{child}")
                };
                discover(
                    sources,
                    source,
                    &format!("{}/{child}", path.trim_end_matches('/')),
                    &name,
                    stored,
                    names,
                    attributes,
                    limits,
                    depth + 1,
                )?;
            }
        }
        LocationType::Dataset => {
            if stored.len() >= limits.hdf5.datasets {
                return Err(Error("sparse dataset count exceeds limit".into()));
            }
            let dataset = sources.file(source).dataset(&path)?;
            let dtype = dataset.dtype()?;
            let shape = dataset.shape();
            if shape.len() > 32 {
                return Err(Error("sparse component rank exceeds limit".into()));
            }
            let descriptor = dtype.to_descriptor()?;
            let text = matches!(
                descriptor,
                TypeDescriptor::VarLenAscii
                    | TypeDescriptor::VarLenUnicode
                    | TypeDescriptor::FixedAscii(_)
                    | TypeDescriptor::FixedUnicode(_)
            );
            let (pixel_type, little_endian, storage, text_value) = if text {
                let filters = typed::admit_filters(&dataset, dtype.size(), limits.hdf5)?;
                let storage_allocated = typed::storage_allocated(&dataset)?;
                if !storage_allocated && dataset.size() != 0 {
                    return Err(Error(
                        "unallocated sparse text metadata is unsupported".into(),
                    ));
                }
                let storage = typed::Storage {
                    filters,
                    fill_value: None,
                    storage_allocated,
                    fill_time: dataset.dcpl()?.get_fill_time()?,
                };
                (
                    None,
                    None,
                    storage,
                    Some(numeric::texts(&dataset, limits.hdf5.text_bytes, false)?),
                )
            } else {
                let (ty, little) = typed::standard_type(&dtype)?;
                let storage = typed::admit_storage(&dataset, ty, limits.hdf5)?;
                shape
                    .iter()
                    .try_fold(ty.bytes(), |n, &d| n.checked_mul(d))
                    .ok_or_else(|| Error("sparse component byte-size overflow".into()))?;
                (Some(ty), Some(little), storage, None)
            };
            let typed::Storage {
                filters,
                fill_value,
                storage_allocated,
                fill_time,
            } = storage;
            sources.charge(
                name.len()
                    .saturating_add(path.len())
                    .saturating_mul(3)
                    .saturating_add(filters.len().saturating_mul(256))
                    .saturating_add(text_value.as_ref().map_or(0, |values| {
                        values
                            .iter()
                            .map(String::len)
                            .sum::<usize>()
                            .saturating_mul(2)
                            .saturating_add(
                                values
                                    .len()
                                    .saturating_mul(std::mem::size_of::<String>())
                                    .saturating_mul(2),
                            )
                    }))
                    .saturating_add(sources.path(source).as_os_str().len().saturating_mul(2))
                    .saturating_add(shape.len().saturating_mul(16))
                    .saturating_add(
                        filters
                            .iter()
                            .map(|f| f.parameters.len().saturating_mul(8))
                            .sum::<usize>(),
                    )
                    .saturating_add(768),
            )?;
            attributes.push(numeric::attributes(sources, source, &dataset, limits.hdf5)?);
            if names.insert(name.to_owned(), stored.len()).is_some() {
                return Err(Error("duplicate sparse logical component name".into()));
            }
            let layout = dataset.layout();
            stored.push(Stored {
                dataset,
                metadata: Component {
                    name: name.to_owned(),
                    source_file: sources.path(source).to_path_buf(),
                    dataset: path,
                    shape,
                    pixel_type,
                    little_endian,
                    type_descriptor: format!("{descriptor:?}"),
                    layout,
                    filters,
                    fill_value,
                    storage_allocated,
                    fill_time,
                    text: text_value,
                },
            });
        }
        _ => return Err(Error("unsupported sparse object kind".into())),
    }
    Ok(())
}
fn validate_auxiliary(
    stored: &[Stored],
    names: &BTreeMap<String, usize>,
    shape: [usize; 2],
    frames: usize,
    limits: Limits,
) -> Result<(), Error> {
    for name in [
        "normalization",
        "radius",
        "background_avg",
        "background_std",
    ] {
        if let Some(&index) = names.get(name) {
            let meta = &stored[index].metadata;
            if (name == "normalization" && meta.pixel_type.is_none())
                || (name != "normalization"
                    && !matches!(meta.pixel_type, Some(PixelType::F32 | PixelType::F64)))
            {
                return Err(Error(format!("sparse {name} requires IEEE float storage")));
            }
            match name {
                "normalization" if meta.shape != shape => {
                    return Err(Error("sparse normalization shape mismatch".into()));
                }
                "radius" if meta.shape.len() != 1 || meta.shape[0] < 2 => {
                    return Err(Error("sparse radius requires at least two values".into()));
                }
                "background_avg" | "background_std"
                    if meta.shape.len() != 2 || meta.shape[0] != frames =>
                {
                    return Err(Error("sparse background frame shape mismatch".into()));
                }
                _ => {}
            }
        }
    }
    if let Some(&radius) = names.get("radius") {
        let bins = stored[radius].metadata.shape[0];
        for name in ["background_avg", "background_std"] {
            if let Some(&index) = names.get(name)
                && stored[index].metadata.shape != [frames, bins]
            {
                return Err(Error("sparse background/radius bin count mismatch".into()));
            }
        }
        let mut buffer = ArrayBuffer::default();
        numeric::read(&stored[radius], None, &mut buffer, limits.component_bytes)?;
        let mut previous = f64::NEG_INFINITY;
        let mut bad = false;
        for i in 0..buffer.pixels.len() {
            let value = numeric::float(&buffer.pixels, i)?;
            if !value.is_finite() || value <= previous {
                bad = true;
            }
            previous = value;
        }
        if bad {
            return Err(Error(
                "sparse radius must be finite and strictly increasing".into(),
            ));
        }
    }
    if let Some(&dummy) = names.get("dummy")
        && (!matches!(stored[dummy].metadata.shape.as_slice(), [] | [1])
            || stored[dummy].metadata.pixel_type.is_none())
    {
        return Err(Error("sparse dummy must be scalar".into()));
    }
    Ok(())
}
fn validate_indices(
    pixels: &PixelBuffer,
    count: usize,
    seen: &mut Vec<usize>,
) -> Result<(), Error> {
    seen.clear();
    seen.try_reserve_exact(pixels.len())
        .map_err(|e| Error(e.to_string()))?;
    for i in 0..pixels.len() {
        let position = numeric::index(pixels, i)?;
        if position >= count {
            return Err(Error("sparse index outside detector shape".into()));
        }
        seen.push(position);
    }
    seen.sort_unstable();
    if seen.windows(2).any(|p| p[0] == p[1]) {
        return Err(Error("duplicate sparse index within frame".into()));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_validation_scratch_is_event_sized_and_keeps_source_order() {
        let source = PixelBuffer::U32(vec![9999, 10, 100]);
        let mut scratch = Vec::new();
        validate_indices(&source, 1_000_000, &mut scratch).unwrap();
        assert_eq!(scratch.len(), 3);
        assert!(scratch.capacity() <= 3);
        assert_eq!(source, PixelBuffer::U32(vec![9999, 10, 100]));
        validate_indices(&PixelBuffer::U32(vec![5]), 1_000_000, &mut scratch).unwrap();
        assert_eq!(scratch.len(), 1);
        assert!(scratch.capacity() <= 3);
    }
}
