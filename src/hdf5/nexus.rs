//! Read-only NeXus navigation over an explicitly immutable, bounded HDF5 graph.
//! Returns owned source descriptors and typed attributes, not h5py identities.
use super::{
    Error,
    attributes::{self, AttributeValue, Attributes},
    sources::Sources,
    typed,
};
use crate::PixelBuffer;
use hdf5_backend::{LocationToken, LocationType};
use std::path::{Path, PathBuf};
pub use typed::{ExternalLinks, Link, Options, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Group,
    Dataset,
}
#[derive(Clone, Debug)]
pub struct Object {
    pub requested_path: String,
    pub source_file: PathBuf,
    pub path: String,
    pub kind: Kind,
    pub attributes: Attributes,
    pub shape: Option<Vec<usize>>,
    pub type_descriptor: Option<String>,
    pub layout: Option<String>,
    pub byte_order: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub object: Object,
    pub start_time: String,
    pub ordering_key: [u32; 6],
    pub ordering_qualification: Option<String>,
}
type IndexedEntry = (String, usize, String, [u32; 6]);
struct Node {
    source: usize,
    token: LocationToken,
    object: Object,
    members: Vec<(String, usize)>,
    start_time: Option<String>,
    start_time_issue: Option<String>,
}
/// A complete graph snapshot. Query methods re-index bounded metadata and compare
/// it before returning owned values; in-process unflushed shape/control changes
/// therefore invalidate navigation as well as filesystem/source replacement.
pub struct Reader {
    sources: Sources,
    nodes: Vec<Node>,
    roots: Vec<usize>,
    options: Options,
    root: PathBuf,
}
impl Reader {
    pub fn open_snapshot(path: impl AsRef<Path>, options: Options) -> Result<Self, Error> {
        validate(options)?;
        let sources = Sources::new_with_origin(path, options.limits, options.external_links, None)?;
        let root = sources.path(0).to_path_buf();
        let mut result = Self {
            sources,
            nodes: Vec::new(),
            roots: Vec::new(),
            options,
            root,
        };
        let mut index = 0;
        while index < result.sources.records.len() {
            let root = result.visit(index, "/")?;
            result.roots.push(root);
            index += 1;
        }
        result.cache_entry_times()?;
        result.sources.check()?;
        Ok(result)
    }
    fn visit(&mut self, source: usize, path: &str) -> Result<usize, Error> {
        self.visit_at(source, path, 0)
    }
    fn visit_at(&mut self, source: usize, path: &str, depth: usize) -> Result<usize, Error> {
        if depth >= 128 {
            return Err(Error(
                "NeXus object graph exceeds depth128; use a narrower metadata source".into(),
            ));
        }
        let (source, path) = self.sources.resolve(source, path)?;
        if self.sources.links.len() > self.options.limits.link_traversals {
            return Err(Error("NeXus link graph exceeds traversal limit".into()));
        }
        let info = self.sources.file(source).loc_info_by_name(&path)?;
        if let Some(index) = self
            .nodes
            .iter()
            .position(|n| n.source == source && n.token == info.token)
        {
            return Ok(index);
        }
        if self.nodes.len() >= self.options.limits.group_members {
            return Err(Error("NeXus total object limit".into()));
        }
        self.sources
            .charge(path.len().saturating_mul(3).saturating_add(512))?;
        let (kind, attrs, shape, descriptor, layout, names, start_time, byte_order) = match info
            .loc_type
        {
            LocationType::Group => {
                let group = self.sources.file(source).group(&path)?;
                let attrs =
                    attributes::snapshot(&mut self.sources, source, &group, self.options.limits)?;
                let names = self.sources.names(&group)?;
                let names = if group.create_plist()?.get_link_creation_order()?
                    != hdf5_backend::plist::group_create::LinkCreationOrder::Untracked
                {
                    self.sources.charge(
                        names
                            .len()
                            .checked_mul(128)
                            .ok_or_else(|| Error("NeXus creation-order scratch overflow".into()))?,
                    )?;
                    let mut order = Vec::new();
                    order
                        .try_reserve_exact(names.len())
                        .map_err(|e| Error(e.to_string()))?;
                    group.iter_visit(
                        hdf5_backend::IndexType::CreationOrder,
                        hdf5_backend::IterationOrder::Increasing,
                        |name, _| {
                            if order.len() >= names.len() {
                                return Err(hdf5_backend::Error::from("NeXus link count changed"));
                            }
                            let index = names
                                .binary_search_by(|value| value.as_str().cmp(name))
                                .map_err(|_| {
                                    hdf5_backend::Error::from("NeXus member name changed")
                                })?;
                            order.push(index);
                            Ok(())
                        },
                    )?;
                    if order.len() != names.len() {
                        return Err(Error("NeXus creation-order member count changed".into()));
                    }
                    let mut slots: Vec<_> = names.into_iter().map(Some).collect();
                    let mut ordered = Vec::new();
                    ordered
                        .try_reserve_exact(slots.len())
                        .map_err(|e| Error(e.to_string()))?;
                    for index in order {
                        ordered.push(slots[index].take().ok_or_else(|| {
                            Error("duplicate NeXus creation-order member".into())
                        })?);
                    }
                    ordered
                } else {
                    names
                };
                (Kind::Group, attrs, None, None, None, names, None, None)
            }
            LocationType::Dataset => {
                if self
                    .nodes
                    .iter()
                    .filter(|n| n.object.kind == Kind::Dataset)
                    .count()
                    >= self.options.limits.datasets
                {
                    return Err(Error("NeXus dataset limit".into()));
                }
                let dataset = self.sources.file(source).dataset(&path)?;
                let attrs =
                    attributes::snapshot(&mut self.sources, source, &dataset, self.options.limits)?;
                let shape = dataset.shape();
                if shape.len() > 32 {
                    return Err(Error("NeXus dataset rank limit".into()));
                }
                let descriptor = dataset.dtype()?.to_descriptor()?;
                let text: Option<String> = None;
                let descriptor = format!("{descriptor:?}");
                self.sources.charge(
                    descriptor
                        .len()
                        .saturating_add(shape.len().saturating_mul(8))
                        .saturating_add(text.as_ref().map_or(0, String::len)),
                )?;
                (
                    Kind::Dataset,
                    attrs,
                    Some(shape),
                    Some(descriptor),
                    Some(format!("{:?}", dataset.layout())),
                    Vec::new(),
                    text,
                    Some(format!("{:?}", dataset.dtype()?.byte_order())),
                )
            }
            _ => {
                return Err(Error(
                    "NeXus named datatypes/maps are unsupported metadata objects".into(),
                ));
            }
        };
        let id = self.nodes.len();
        self.nodes.push(Node {
            source,
            token: info.token,
            object: Object {
                requested_path: path.clone(),
                source_file: self.sources.path(source).to_path_buf(),
                path: path.clone(),
                kind,
                attributes: attrs,
                shape,
                type_descriptor: descriptor,
                layout,
                byte_order,
            },
            members: Vec::new(),
            start_time,
            start_time_issue: None,
        });
        let mut members = Vec::new();
        for name in names {
            let child = format!("{}/{}", path.trim_end_matches('/'), name);
            let next = self.visit_at(source, &child, depth + 1)?;
            members.push((name, next));
        }
        self.nodes[id].members = members;
        Ok(id)
    }
    fn cache_entry_times(&mut self) -> Result<(), Error> {
        let mut controls = Vec::new();
        for node in &self.nodes {
            if node.object.kind == Kind::Group
                && matches!(node.object.attributes.values.get("NX_class"),Some(AttributeValue::Text(values)) if values.len()==1&&values[0]=="NXentry")
                && let Some((_, id)) = node.members.iter().find(|(name, _)| name == "start_time")
                && !controls.contains(id)
            {
                controls.push(*id);
            }
        }
        for id in controls {
            let result = (|| -> Result<String, Error> {
                if self.nodes[id].object.kind != Kind::Dataset {
                    return Err(Error(
                        "NXentry start_time must be a scalar string dataset".into(),
                    ));
                }
                let dataset = self
                    .sources
                    .file(self.nodes[id].source)
                    .dataset(&self.nodes[id].object.path)?;
                if dataset.size() != 1 {
                    return Err(Error("NXentry start_time must contain one string".into()));
                }
                typed::admit_filters(&dataset, dataset.dtype()?.size(), self.options.limits)?;
                let values = attributes::texts(&dataset, self.options.limits.text_bytes, false)?;
                Ok(values[0].clone())
            })();
            match result {
                Ok(text) => {
                    self.sources
                        .charge(text.len().saturating_mul(2).saturating_add(64))?;
                    self.nodes[id].start_time = Some(text)
                }
                Err(error) => {
                    let issue: String = error.to_string().chars().take(512).collect();
                    self.sources
                        .charge(issue.len().saturating_mul(2).saturating_add(64))?;
                    self.nodes[id].start_time_issue = Some(issue);
                }
            }
        }
        Ok(())
    }
    pub fn check_sources(&self) -> Result<(), Error> {
        self.sources.check()?;
        let fresh = Self::open_snapshot(&self.root, self.options)?;
        if self.sources.records.len() != fresh.sources.records.len()
            || self
                .sources
                .records
                .iter()
                .zip(&fresh.sources.records)
                .any(|(a, b)| a.path != b.path || a.bytes != b.bytes)
            || self.sources.links.len() != fresh.sources.links.len()
            || self
                .sources
                .links
                .iter()
                .zip(&fresh.sources.links)
                .any(|(a, b)| {
                    a.source != b.source
                        || a.path != b.path
                        || a.external != b.external
                        || a.raw_value != b.raw_value
                })
            || self.nodes.len() != fresh.nodes.len()
            || self
                .nodes
                .iter()
                .zip(&fresh.nodes)
                .any(|(a, b)| a.source != b.source || a.token != b.token || !node_equal(a, b))
        {
            return Err(Error(
                "NeXus metadata/source graph changed; reopen snapshot".into(),
            ));
        }
        self.sources.check()
    }
    pub fn sources(&self) -> &[Source] {
        &self.sources.records
    }
    pub fn links(&self) -> &[Link] {
        &self.sources.links
    }
    pub fn source_bindings(&self) -> &[crate::SourceBinding] {
        &self.sources.bindings
    }
    fn lookup(&self, source: usize, path: &str) -> Result<Option<usize>, Error> {
        if path.len() > self.options.limits.text_bytes
            || path.as_bytes().contains(&0)
            || path.split('/').any(|v| v == "..")
        {
            return Err(Error("invalid NeXus query path".into()));
        }
        let mut id = *self
            .roots
            .get(source)
            .ok_or_else(|| Error("NeXus source root is absent".into()))?;
        let mut count = 0;
        for component in path.split('/').filter(|c| !c.is_empty() && *c != ".") {
            count += 1;
            if count > self.options.limits.link_traversals {
                return Err(Error("NeXus query traversal limit".into()));
            }
            let Some(next) = self.nodes[id]
                .members
                .iter()
                .find(|(name, _)| name == component)
                .map(|(_, id)| *id)
            else {
                return Ok(None);
            };
            id = next;
        }
        Ok(Some(id))
    }
    fn object(&self, id: usize, requested: String) -> Object {
        let mut object = self.nodes[id].object.clone();
        object.requested_path = requested;
        object
    }
    fn text(&self, id: usize, name: &str) -> Result<Option<&str>, Error> {
        match self.nodes[id].object.attributes.values.get(name) {
            None => Ok(None),
            Some(AttributeValue::Text(values)) if values.len() == 1 => Ok(Some(&values[0])),
            Some(AttributeValue::Numeric { .. }) if name == "NX_class" => Ok(None),
            _ => Err(Error(format!(
                "NeXus {name} control attribute must be one string"
            ))),
        }
    }
    fn classes(&self, id: usize, class: &str) -> Result<Vec<(String, usize)>, Error> {
        let mut result = Vec::new();
        for (name, child) in &self.nodes[id].members {
            if self.nodes[*child].object.kind == Kind::Group
                && self.text(*child, "NX_class")? == Some(class)
            {
                result.push((name.clone(), *child));
            }
        }
        Ok(result)
    }
    fn entries(&self) -> Result<Vec<IndexedEntry>, Error> {
        let mut result = Vec::new();
        let mut returned = 0usize;
        for (name, id) in &self.nodes[self.roots[0]].members {
            if self.nodes[*id].object.kind != Kind::Group
                || self.text(*id, "NX_class")? != Some("NXentry")
            {
                continue;
            }
            let Some((_, start)) = self.nodes[*id]
                .members
                .iter()
                .find(|(name, _)| name == "start_time")
            else {
                continue;
            };
            admit_output(
                &mut returned,
                object_cost(&self.nodes[*id].object)?.saturating_add(
                    self.nodes[*start]
                        .start_time
                        .as_ref()
                        .map_or(0, String::len),
                ),
                self.options.limits.metadata_bytes,
            )?;
            let raw = self.nodes[*start]
                .start_time
                .clone()
                .ok_or_else(|| Error("NXentry start_time is not a scalar text dataset".into()))?;
            let key = time_key(&raw)?;
            result.push((name.clone(), *id, raw, key));
        }
        result.sort_by_key(|entry| std::cmp::Reverse(entry.3));
        Ok(result)
    }
    pub fn get_entries(&self) -> Result<Vec<Entry>, Error> {
        self.check_sources()?;
        let values=self.entries()?.into_iter().map(|(name,id,start_time,ordering_key)|Entry{object:self.object(id,format!("/{name}")),ordering_qualification:(start_time.len()!=19).then(||"naive ISO first19 ordering; suffix/offset retained without UTC chronology claim".into()),start_time,ordering_key}).collect();
        self.sources.check()?;
        Ok(values)
    }
    pub fn get_entry(&self, name: &str) -> Result<Option<Object>, Error> {
        self.check_sources()?;
        let Some((_, id)) = self.nodes[self.roots[0]]
            .members
            .iter()
            .find(|(n, _)| n == name)
        else {
            return Ok(None);
        };
        if self.nodes[*id].object.kind != Kind::Group
            || self.text(*id, "NX_class")? != Some("NXentry")
            || !self.nodes[*id]
                .members
                .iter()
                .any(|(n, _)| n == "start_time")
        {
            return Ok(None);
        }
        let mut returned = 0;
        admit_output(
            &mut returned,
            object_cost(&self.nodes[*id].object)?,
            self.options.limits.metadata_bytes,
        )?;
        let object = self.object(*id, format!("/{name}"));
        self.sources.check()?;
        Ok(Some(object))
    }
    pub fn get_class(&self, group: &str, class: &str) -> Result<Vec<Object>, Error> {
        self.check_sources()?;
        let Some(id) = self.lookup(0, group)? else {
            return Ok(Vec::new());
        };
        let selected = self.classes(id, class)?;
        let mut returned = 0;
        for (_, id) in &selected {
            admit_output(
                &mut returned,
                object_cost(&self.nodes[*id].object)?,
                self.options.limits.metadata_bytes,
            )?;
        }
        let values = selected
            .into_iter()
            .map(|(name, id)| self.object(id, format!("{}/{name}", group.trim_end_matches('/'))))
            .collect();
        self.sources.check()?;
        Ok(values)
    }
    pub fn get_attr(&self, path: &str, name: &str) -> Result<Option<AttributeValue>, Error> {
        if name.is_empty() {
            return Err(Error(
                "NeXus attribute query requires a real nonempty name".into(),
            ));
        }
        self.check_sources()?;
        let value = self
            .lookup(0, path)?
            .and_then(|id| self.nodes[id].object.attributes.values.get(name).cloned());
        self.sources.check()?;
        Ok(value)
    }
    /// No filter means all immediate datasets. This intended operation is separate
    /// from pinned FabIO's broken default get_attr(None) call.
    pub fn get_data(
        &self,
        group: &str,
        filter: Option<(&str, &AttributeValue)>,
    ) -> Result<Vec<Object>, Error> {
        self.check_sources()?;
        let Some(id) = self.lookup(0, group)? else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        let mut returned = 0;
        for (name, child) in &self.nodes[id].members {
            if self.nodes[*child].object.kind != Kind::Dataset {
                continue;
            }
            if let Some((key, expected)) = filter {
                let Some(actual) = self.nodes[*child].object.attributes.values.get(key) else {
                    continue;
                };
                if !filter_equal(actual, expected)? {
                    continue;
                }
            }
            admit_output(
                &mut returned,
                object_cost(&self.nodes[*child].object)?,
                self.options.limits.metadata_bytes,
            )?;
            result.push(self.object(*child, format!("{}/{name}", group.trim_end_matches('/'))));
        }
        self.sources.check()?;
        Ok(result)
    }
    pub fn find_detector(&self, all: bool) -> Result<Vec<Object>, Error> {
        self.check_sources()?;
        let mut result = Vec::new();
        let mut returned = 0;
        for (name, entry, _, _) in self.entries()? {
            for (instrument_name, instrument) in self
                .classes(entry, "NXsubentry")?
                .into_iter()
                .chain(self.classes(entry, "NXinstrument")?)
            {
                for (detector_name, detector) in self.classes(instrument, "NXdetector")? {
                    admit_output(
                        &mut returned,
                        object_cost(&self.nodes[detector].object)?,
                        self.options.limits.metadata_bytes,
                    )?;
                    result.push(self.object(
                        detector,
                        format!("/{name}/{instrument_name}/{detector_name}"),
                    ));
                    if !all {
                        return Ok(result);
                    }
                }
            }
        }
        self.sources.check()?;
        Ok(result)
    }
    pub fn find_data(&self, all: bool) -> Result<Vec<Object>, Error> {
        self.check_sources()?;
        let mut result = Vec::new();
        let mut returned = 0;
        for (name, entry, _, _) in self.entries()? {
            let entry_path = format!("/{name}");
            for object in self.get_data(&entry_path, None)? {
                admit_output(
                    &mut returned,
                    object_cost(&object)?,
                    self.options.limits.metadata_bytes,
                )?;
                result.push(object);
                if !all {
                    return Ok(result);
                }
            }
            for class in ["NXinstrument", "NXsubentry"] {
                for (instrument_name, instrument) in self.classes(entry, class)? {
                    let instrument_path = format!("/{name}/{instrument_name}");
                    for object in self.get_data(&instrument_path, None)? {
                        admit_output(
                            &mut returned,
                            object_cost(&object)?,
                            self.options.limits.metadata_bytes,
                        )?;
                        result.push(object);
                        if !all {
                            return Ok(result);
                        }
                    }
                    for (detector_name, _) in self.classes(instrument, "NXdetector")? {
                        for object in
                            self.get_data(&format!("{instrument_path}/{detector_name}"), None)?
                        {
                            admit_output(
                                &mut returned,
                                object_cost(&object)?,
                                self.options.limits.metadata_bytes,
                            )?;
                            result.push(object);
                            if !all {
                                return Ok(result);
                            }
                        }
                    }
                }
            }
        }
        self.sources.check()?;
        Ok(result)
    }
    pub fn get_default_nxdata(&self) -> Result<Option<Object>, Error> {
        self.check_sources()?;
        let Some(entry_path) = self.text(self.roots[0], "default")? else {
            return Ok(None);
        };
        let Some(entry) = self.lookup(0, entry_path)? else {
            return Err(Error("NeXus default entry is missing".into()));
        };
        let Some(target) = self.text(entry, "default")? else {
            return Ok(None);
        };
        let id = if target.starts_with('/') {
            self.lookup(self.nodes[entry].source, target)?
        } else {
            let mut id = Some(entry);
            for part in target.split('/').filter(|c| !c.is_empty()) {
                id = id.and_then(|id| {
                    self.nodes[id]
                        .members
                        .iter()
                        .find(|(name, _)| name == part)
                        .map(|(_, id)| *id)
                });
            }
            id
        };
        let Some(id) = id else {
            return Err(Error("NeXus default data target is missing".into()));
        };
        let requested = if target.starts_with('/') {
            target.to_string()
        } else {
            format!("/{}/{target}", entry_path.trim_matches('/'))
        };
        let result = self.object(id, requested);
        self.sources.check()?;
        Ok(Some(result))
    }
    pub fn open_image<'a>(&'a self, object: &Object) -> Result<DatasetView<'a>, Error> {
        self.check_sources()?;
        let Some(id) = self
            .nodes
            .iter()
            .position(|node| object.kind == Kind::Dataset && object_equal(&node.object, object))
        else {
            return Err(Error(
                "NeXus dataset descriptor does not match this source snapshot".into(),
            ));
        };
        let logical = self.lookup(0, &object.requested_path)?;
        let physical = self.lookup(self.nodes[id].source, &object.requested_path)?;
        if logical != Some(id) && physical != Some(id) {
            return Err(Error(
                "NeXus requested path is not bound to this dataset descriptor".into(),
            ));
        }
        let reader = typed::Reader::open_snapshot(
            &object.source_file,
            typed::Selection::Dataset(&object.path),
            self.options,
        )?;
        self.sources.check()?;
        Ok(DatasetView {
            parent: self,
            reader,
        })
    }
}
pub struct DatasetView<'a> {
    parent: &'a Reader,
    reader: typed::Reader,
}
impl DatasetView<'_> {
    pub fn frames(&self) -> usize {
        self.reader.frames()
    }
    pub fn metadata(&self, index: usize) -> Result<Option<&typed::Metadata>, Error> {
        self.parent.check_sources()?;
        self.reader.check_sources()?;
        Ok(self.reader.metadata(index))
    }
    pub fn read_frame_into(
        &self,
        index: usize,
        buffer: &mut typed::FrameBuffer,
    ) -> Result<(), Error> {
        if let Err(error) = self.parent.check_sources() {
            *buffer = typed::FrameBuffer::default();
            return Err(error);
        }
        self.reader.read_frame_into(index, buffer)?;
        if let Err(error) = self.parent.check_sources() {
            *buffer = typed::FrameBuffer::default();
            return Err(error);
        }
        Ok(())
    }
}
fn validate(options: Options) -> Result<(), Error> {
    let l = options.limits;
    if [
        l.frames,
        l.frame_bytes,
        l.chunk_bytes,
        l.metadata_bytes,
        l.text_bytes,
        l.source_files,
        l.datasets,
        l.group_members,
        l.link_traversals,
    ]
    .iter()
    .any(|&v| v == 0 || v == usize::MAX)
    {
        return Err(Error("NeXus bounds must be positive and finite".into()));
    }
    Ok(())
}
fn time_key(text: &str) -> Result<[u32; 6], Error> {
    let b = text.as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return Err(Error(
            "unsupported NeXus start_time; strict naive ISO prefix required".into(),
        ));
    }
    let mut key = [0; 6];
    for (index, range) in [0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
        .into_iter()
        .enumerate()
    {
        let bytes = &b[range];
        if !bytes.iter().all(u8::is_ascii_digit) {
            return Err(Error("invalid NeXus start_time digits".into()));
        }
        key[index] = std::str::from_utf8(bytes)
            .unwrap()
            .parse()
            .map_err(|_| Error("invalid NeXus start_time".into()))?;
    }
    let [year, month, day, hour, minute, second] = key;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    if year == 0 || day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
        return Err(Error("out-of-range NeXus naive ISO timestamp".into()));
    }
    Ok(key)
}
fn pixels_equal(a: &PixelBuffer, b: &PixelBuffer) -> bool {
    match (a, b) {
        (PixelBuffer::F32(a), PixelBuffer::F32(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
        }
        (PixelBuffer::F64(a), PixelBuffer::F64(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
        }
        _ => a == b,
    }
}
fn value_equal(a: &AttributeValue, b: &AttributeValue) -> Result<bool, Error> {
    Ok(match (a, b) {
        (AttributeValue::Text(a), AttributeValue::Text(b)) => a == b,
        (
            AttributeValue::Numeric {
                shape: a,
                pixel_type: at,
                little_endian: al,
                values: av,
            },
            AttributeValue::Numeric {
                shape: b,
                pixel_type: bt,
                little_endian: bl,
                values: bv,
            },
        ) => a == b && at == bt && al == bl && pixels_equal(av, bv),
        _ => false,
    })
}
fn attrs_equal(a: &Attributes, b: &Attributes) -> bool {
    a.source_file == b.source_file
        && a.path == b.path
        && a.values.len() == b.values.len()
        && a.values.iter().all(|(key, value)| {
            b.values
                .get(key)
                .is_some_and(|other| value_equal(value, other).unwrap_or(false))
        })
        && a.declarations.len() == b.declarations.len()
        && a.declarations.iter().all(|(key, value)| {
            b.declarations.get(key).is_some_and(|other| {
                value.shape == other.shape && value.type_descriptor == other.type_descriptor
            })
        })
}
fn object_equal(a: &Object, b: &Object) -> bool {
    a.source_file == b.source_file
        && a.path == b.path
        && a.kind == b.kind
        && a.shape == b.shape
        && a.type_descriptor == b.type_descriptor
        && a.layout == b.layout
        && a.byte_order == b.byte_order
        && attrs_equal(&a.attributes, &b.attributes)
}
fn node_equal(a: &Node, b: &Node) -> bool {
    object_equal(&a.object, &b.object)
        && a.members == b.members
        && a.start_time == b.start_time
        && a.start_time_issue == b.start_time_issue
}

fn admit_output(used: &mut usize, add: usize, limit: usize) -> Result<(), Error> {
    *used = used
        .checked_add(add)
        .filter(|&n| n <= limit)
        .ok_or_else(|| Error("NeXus owned query output exceeds metadata budget".into()))?;
    Ok(())
}
fn object_cost(object: &Object) -> Result<usize, Error> {
    let mut used = 512usize
        .saturating_add(object.requested_path.len())
        .saturating_add(object.path.len())
        .saturating_add(object.source_file.as_os_str().len());
    for (key, value) in &object.attributes.values {
        let cost = match value {
            AttributeValue::Text(values) => values.iter().try_fold(64usize, |n, v| {
                n.checked_add(v.len().saturating_mul(2).saturating_add(48))
            }),
            AttributeValue::Numeric {
                shape,
                pixel_type,
                values,
                ..
            } => values
                .len()
                .checked_mul(pixel_type.bytes())
                .and_then(|n| n.checked_mul(2))
                .and_then(|n| n.checked_add(shape.len().saturating_mul(8) + 128)),
        }
        .ok_or_else(|| Error("NeXus attribute result size overflow".into()))?;
        used = used
            .checked_add(cost)
            .and_then(|n| n.checked_add(key.len().saturating_mul(2) + 128))
            .ok_or_else(|| Error("NeXus object result size overflow".into()))?;
    }
    for (key, value) in &object.attributes.declarations {
        used = used
            .checked_add(
                key.len().saturating_mul(2)
                    + value.type_descriptor.len()
                    + value.shape.len().saturating_mul(8)
                    + 128,
            )
            .ok_or_else(|| Error("NeXus declaration result size overflow".into()))?;
    }
    Ok(used)
}

#[derive(Clone, Copy)]
enum Scalar {
    Integer(i128),
    Float(f64),
}
fn scalar(values: &PixelBuffer) -> Result<Scalar, Error> {
    if values.len() != 1 {
        return Err(Error(
            "NeXus named numeric filters require one scalar; array truth is ambiguous".into(),
        ));
    }
    macro_rules! integer{($($variant:ident);*)=>{match values{$(PixelBuffer::$variant(v)=>Ok(Scalar::Integer(i128::from(v[0]))),)*PixelBuffer::F32(v)=>Ok(Scalar::Float(f64::from(v[0]))),PixelBuffer::F64(v)=>Ok(Scalar::Float(v[0])),_=>Err(Error("NeXus filter scalar type unsupported".into()))}};}
    integer!(U8;U16;U32;U64;I8;I16;I32;I64)
}
fn filter_equal(a: &AttributeValue, b: &AttributeValue) -> Result<bool, Error> {
    match (a, b) {
        (AttributeValue::Text(a), AttributeValue::Text(b)) => {
            if a.len() != 1 || b.len() != 1 {
                return Err(Error("NeXus named text filters require one string".into()));
            }
            Ok(a == b)
        }
        (AttributeValue::Numeric { values: a, .. }, AttributeValue::Numeric { values: b, .. }) => {
            Ok(match (scalar(a)?, scalar(b)?) {
                (Scalar::Integer(a), Scalar::Integer(b)) => a == b,
                (Scalar::Float(a), Scalar::Float(b)) => a == b,
                (Scalar::Integer(integer), Scalar::Float(float))
                | (Scalar::Float(float), Scalar::Integer(integer)) => {
                    float.is_finite() && float.fract() == 0.0 && float as i128 == integer
                }
            })
        }
        _ => Ok(false),
    }
}
