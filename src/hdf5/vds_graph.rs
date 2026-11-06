//! Source-bound VDS DAGs flattened into exact concrete/fill selections.
use super::*;
use hdf5_backend::{File, LocationToken};
use hdf5_sys::h5s::{
    H5S_seloper_t, H5Sget_select_type, H5Ssel_iter_close, H5Ssel_iter_create,
    H5Ssel_iter_get_seq_list, H5Sselect_hyperslab, H5Sselect_none, H5Sselect_project_intersection,
};
use std::sync::Arc;

const MAX_DEPTH: usize = 32;
const MAX_LEAVES: usize = 16_384;
type Key = (u64, LocationToken);
fn key(dataset: &Dataset) -> Result<Key, Error> {
    let i = dataset.loc_info()?;
    Ok((i.fileno, i.token))
}

struct Guard {
    file: File,
    path: String,
    dataset: Dataset,
    shape: Option<[usize; 3]>,
    plist: Option<hdf5_backend::plist::DatasetCreate>,
}
impl Guard {
    fn check(&self) -> Result<(), Error> {
        let current = crate::hdf5::sources::hard_dataset(&self.file, &self.path)?;
        if key(&current)? != key(&self.dataset)? {
            return Err(Error(
                "VDS graph dataset binding changed; reopen snapshot".into(),
            ));
        }
        if self.shape.is_some_and(|s| current.shape() != s) {
            return Err(Error(
                "VDS graph dataset extent changed; reopen snapshot".into(),
            ));
        }
        if let Some(plist) = &self.plist {
            let current = current.dcpl()?;
            if hdf5_backend::sync::sync(|| unsafe {
                hdf5_sys::h5p::H5Pequal(plist.id(), current.id())
            }) <= 0
            {
                return Err(Error(
                    "VDS graph mapping metadata changed; reopen snapshot".into(),
                ));
            }
        }
        Ok(())
    }
}
struct Concrete {
    dataset: Dataset,
    shape: [usize; 3],
    storage: Storage,
    little: bool,
}
#[derive(Clone)]
struct Origin {
    node: usize,
    file: PathBuf,
    dataset: String,
}
enum Target {
    Concrete(Arc<Concrete>),
    Fill {
        origin: Origin,
        value: Option<PixelBuffer>,
        word: Option<[u8; 16]>,
    },
}
struct Leaf {
    target: Arc<Target>,
    source: Dataspace,
    virtual_: Dataspace,
}
impl Clone for Leaf {
    fn clone(&self) -> Self {
        Self {
            target: self.target.clone(),
            source: self.source.clone(),
            virtual_: self.virtual_.clone(),
        }
    }
}
struct Node {
    identity: Key,
    fixed_extent: Option<[usize; 3]>,
    metadata: VdsNodeMetadata,
    leaves: Vec<Leaf>,
}
enum Input {
    Concrete(Arc<Concrete>),
    Nested(Arc<Node>),
}
impl Input {
    fn shape(&self) -> [usize; 3] {
        match self {
            Self::Concrete(c) => c.shape,
            Self::Nested(n) => n.metadata.frozen_shape,
        }
    }
    fn fill(&self) -> Option<PixelBuffer> {
        match self {
            Self::Concrete(c) => c.storage.fill_value.clone(),
            Self::Nested(n) => n.metadata.fill_value.clone(),
        }
    }
    fn fill_time(&self) -> FillTime {
        match self {
            Self::Concrete(c) => c.storage.fill_time,
            Self::Nested(n) => n.metadata.fill_time,
        }
    }
}
struct Prepared {
    filename: String,
    dataset: String,
    source_file: PathBuf,
    resolved: String,
    input: Input,
    original_source: SelectionRecord,
    original_virtual: SelectionRecord,
    source: Dataspace,
    virtual_record: SelectionRecord,
    virtual_space: Space,
    printf: Option<PrintfExpansion>,
}
pub(super) struct Graph {
    leaves: Vec<Leaf>,
    guards: Vec<Guard>,
}
impl Graph {
    pub fn check(&self) -> Result<(), Error> {
        for guard in &self.guards {
            guard.check()?;
        }
        Ok(())
    }
    pub fn read(
        &self,
        frame: usize,
        shape: [usize; 2],
        pixels: &mut PixelBuffer,
    ) -> Result<(), Error> {
        self.check()?;
        let memory = typed::memory_type(pixels.pixel_type())?;
        for leaf in &self.leaves {
            let selection = finite::Selection {
                source: leaf.source.clone(),
                virtual_: leaf.virtual_.clone(),
            };
            match leaf.target.as_ref() {
                Target::Concrete(c) => selection.read(&c.dataset, frame, shape, &memory, pixels)?,
                Target::Fill {
                    word: Some(word), ..
                } => fill_selection(&selection, frame, shape, pixels, word)?,
                Target::Fill { word: None, .. } => {
                    return Err(Error(
                        "undefined selected nested VDS fill cannot supply samples".into(),
                    ));
                }
            }
        }
        self.check()
    }
}
struct State<'s, 'b, 'o> {
    sources: &'s mut Sources,
    bindings: &'b mut Bindings<'o>,
    limits: Limits,
    indexed: &'s mut usize,
    pixel_type: PixelType,
    nodes: Vec<Arc<Node>>,
    active: Vec<Key>,
    guards: Vec<Guard>,
    concrete: Vec<Arc<Concrete>>,
    next_id: usize,
    planning_operations: usize,
}

fn selection_cost(s: &SelectionRecord) -> Result<usize, Error> {
    let bytes = match s {
        SelectionRecord::All {
            shape,
            maximum_shape,
        } => (shape.len() + maximum_shape.len()).checked_mul(8),
        SelectionRecord::Regular { .. } => Some(384),
        SelectionRecord::Irregular { blocks, .. } => blocks.len().checked_mul(96),
    };
    bytes
        .and_then(|n| n.checked_add(256))
        .ok_or_else(|| Error("VDS selection metadata overflow".into()))
}
fn mapping_cost(m: &Mapping) -> Result<usize, Error> {
    let mut n = 2048usize
        .checked_add(m.stored_filename.len())
        .and_then(|n| n.checked_add(m.declared_dataset.len()))
        .and_then(|n| n.checked_add(m.source_file.as_os_str().len()))
        .and_then(|n| n.checked_add(m.resolved_dataset.len()))
        .ok_or_else(|| Error("VDS mapping metadata overflow".into()))?;
    for s in [
        &m.source_selection,
        &m.virtual_selection,
        &m.frozen_source_selection,
        &m.frozen_virtual_selection,
    ] {
        n = n
            .checked_add(selection_cost(s)?)
            .ok_or_else(|| Error("VDS mapping metadata overflow".into()))?;
    }
    if let Some(p) = &m.printf_expansion {
        n = n
            .checked_add(selection_cost(&p.declaration.source_selection)?)
            .and_then(|n| n.checked_add(selection_cost(&p.declaration.virtual_selection).ok()?))
            .ok_or_else(|| Error("VDS printf metadata overflow".into()))?;
    }
    Ok(n)
}
fn node_cost(n: &VdsNodeMetadata) -> Result<usize, Error> {
    n.mappings.iter().try_fold(
        1024usize
            .saturating_add(n.source_file.as_os_str().len())
            .saturating_add(n.dataset.len())
            .saturating_add(n.unmapped_pixels_by_frame.len().saturating_mul(8)),
        |total, m| {
            total
                .checked_add(mapping_cost(m)?)
                .ok_or_else(|| Error("VDS node metadata overflow".into()))
        },
    )
}
fn validate_shape(shape: [usize; 3], ty: PixelType, limits: Limits) -> Result<(), Error> {
    let plane = shape[1]
        .checked_mul(shape[2])
        .filter(|&n| n > 0)
        .ok_or_else(|| Error("invalid VDS raster shape".into()))?;
    if shape[0] == 0
        || shape[0] > limits.frames
        || plane
            .checked_mul(ty.bytes())
            .is_none_or(|n| n > limits.frame_bytes)
    {
        return Err(Error("VDS frozen shape exceeds frame/count bounds".into()));
    }
    shape
        .iter()
        .try_fold(ty.bytes(), |n, &d| n.checked_mul(d))
        .ok_or_else(|| Error("VDS frozen byte count overflow".into()))?;
    Ok(())
}
fn all(shape: [usize; 3]) -> Result<Dataspace, Error> {
    let out = Dataspace::try_new(shape)?;
    let begin = [0; 3];
    let counts = [1; 3];
    let block = shape.map(|n| n as u64);
    if unsafe {
        H5Sselect_hyperslab(
            out.id(),
            H5S_seloper_t::H5S_SELECT_SET,
            begin.as_ptr(),
            std::ptr::null(),
            counts.as_ptr(),
            block.as_ptr(),
        )
    } < 0
    {
        return Err(Error("cannot initialize bounded VDS domain".into()));
    }
    Ok(out)
}
fn empty(shape: [usize; 3]) -> Result<Dataspace, Error> {
    let out = Dataspace::try_new(shape)?;
    if unsafe { H5Sselect_none(out.id()) } < 0 {
        return Err(Error("cannot initialize empty VDS selection".into()));
    }
    Ok(out)
}
fn record(
    space: &Dataspace,
    sources: &mut Sources,
    limits: Limits,
) -> Result<SelectionRecord, Error> {
    hdf5_backend::sync::sync(|| selection(space.id(), false, sources, limits))
}
fn word(value: &PixelBuffer) -> Result<[u8; 16], Error> {
    let mut copy = value.clone();
    let bytes = copy.bytes_mut();
    if value.len() != 1 || bytes.len() > 16 {
        return Err(Error("VDS fill must be one exact native scalar".into()));
    }
    let mut out = [0; 16];
    out[..bytes.len()].copy_from_slice(bytes);
    Ok(out)
}
fn add_leaf(
    leaves: &mut Vec<Leaf>,
    leaf: Leaf,
    sources: &mut Sources,
    limits: Limits,
) -> Result<(), Error> {
    if leaves.len() >= MAX_LEAVES || leaves.len() >= limits.group_members {
        return Err(Error("VDS expanded execution leaf limit exceeded".into()));
    }
    sources.charge(256)?;
    leaves.try_reserve(1).map_err(|e| Error(e.to_string()))?;
    leaves.push(leaf);
    Ok(())
}
impl State<'_, '_, '_> {
    fn operation(&mut self, amount: usize) -> Result<(), Error> {
        self.planning_operations = self
            .planning_operations
            .checked_add(amount)
            .filter(|&n| n <= finite::MAX_OPERATIONS)
            .ok_or_else(|| Error("VDS aggregate graph planning operation limit exceeded".into()))?;
        Ok(())
    }
    fn complexity(&self, space: &Dataspace) -> Result<usize, Error> {
        hdf5_backend::sync::sync(|| match unsafe { H5Sget_select_type(space.id()) } {
            H5S_sel_type::H5S_SEL_ALL | H5S_sel_type::H5S_SEL_NONE => Ok(1),
            H5S_sel_type::H5S_SEL_HYPERSLABS => {
                if unsafe { hdf5_sys::h5s::H5Sis_regular_hyperslab(space.id()) } > 0 {
                    let (mut start, mut stride, mut count, mut block) =
                        ([0; 3], [0; 3], [0; 3], [0; 3]);
                    if unsafe {
                        hdf5_sys::h5s::H5Sget_regular_hyperslab(
                            space.id(),
                            start.as_mut_ptr(),
                            stride.as_mut_ptr(),
                            count.as_mut_ptr(),
                            block.as_mut_ptr(),
                        )
                    } < 0
                    {
                        return Err(Error("cannot inspect frozen VDS complexity".into()));
                    }
                    finite::complexity(&SelectionRecord::Regular {
                        shape: [0; 3],
                        maximum_shape: [0; 3],
                        start,
                        stride,
                        count,
                        block,
                    })
                } else {
                    usize::try_from(unsafe {
                        hdf5_sys::h5s::H5Sget_select_hyper_nblocks(space.id())
                    })
                    .ok()
                    .filter(|&n| n > 0 && n <= finite::MAX_BLOCKS)
                    .ok_or_else(|| {
                        Error("VDS projected native block complexity exceeds limit".into())
                    })
                }
            }
            _ => Err(Error("unsupported frozen VDS selection class".into())),
        })
    }
    fn runs(&mut self, space: &Dataspace) -> Result<usize, Error> {
        hdf5_backend::sync::sync(|| {
            let mut left = finite::points(space)?;
            if left == 0 {
                return Ok(0);
            }
            let iter = Iter(unsafe { H5Ssel_iter_create(space.id(), 1, 0) });
            if iter.0 < 0 {
                return Err(Error("cannot inspect VDS selection run bound".into()));
            }
            let mut runs = 0usize;
            while left > 0 {
                let (mut offsets, mut lengths) = ([0u64; 64], [0usize; 64]);
                let (mut nseq, mut elements) = (0, 0);
                if unsafe {
                    H5Ssel_iter_get_seq_list(
                        iter.0,
                        64,
                        left,
                        &mut nseq,
                        &mut elements,
                        offsets.as_mut_ptr(),
                        lengths.as_mut_ptr(),
                    )
                } < 0
                    || nseq == 0
                    || nseq > 64
                    || elements == 0
                    || elements > left
                {
                    return Err(Error("invalid VDS plan run iterator".into()));
                }
                self.operation(nseq)?;
                runs = runs
                    .checked_add(nseq)
                    .ok_or_else(|| Error("VDS run count overflow".into()))?;
                left -= elements;
            }
            Ok(runs)
        })
    }
    fn preflight(
        &mut self,
        a: &Dataspace,
        b: &Dataspace,
        destination: Option<&Dataspace>,
    ) -> Result<usize, Error> {
        let product = self
            .complexity(a)?
            .checked_mul(self.complexity(b)?)
            .ok_or_else(|| Error("VDS selection complexity product overflow".into()))?;
        self.operation(
            product
                .checked_add(1)
                .ok_or_else(|| Error("VDS planning operation overflow".into()))?,
        )?;
        let mut runs = self
            .runs(a)?
            .checked_add(self.runs(b)?)
            .ok_or_else(|| Error("VDS selection run bound overflow".into()))?;
        if let Some(destination) = destination {
            self.complexity(destination)?;
            runs = runs
                .checked_add(self.runs(destination)?)
                .ok_or_else(|| Error("VDS projected run bound overflow".into()))?;
        }
        // Ordered interval intersection has at most the sum of input runs;
        // projection can split again only at destination run boundaries.
        self.operation(
            runs.checked_mul(2)
                .ok_or_else(|| Error("VDS native planning work overflow".into()))?,
        )?;
        Ok(runs.max(1))
    }
    fn combine(
        &mut self,
        a: &Dataspace,
        b: &Dataspace,
        op: H5S_seloper_t,
    ) -> Result<Dataspace, Error> {
        self.preflight(a, b, None)?;
        let out = hdf5_backend::sync::sync(|| finite::combination(a, b, op))?;
        self.complexity(&out)?;
        Ok(out)
    }
    fn project(
        &mut self,
        a: &Dataspace,
        destination: &Dataspace,
        intersection: &Dataspace,
    ) -> Result<Dataspace, Error> {
        let runs = self.preflight(a, intersection, Some(destination))?;
        self.sources.charge(
            runs.min(finite::MAX_BLOCKS)
                .checked_mul(96)
                .and_then(|n| n.checked_add(256))
                .ok_or_else(|| Error("VDS retained projection metadata overflow".into()))?,
        )?;
        let out = hdf5_backend::sync::sync(|| {
            let out = Space(unsafe {
                H5Sselect_project_intersection(a.id(), destination.id(), intersection.id())
            })
            .into_dataspace()?;
            if unsafe { hdf5_sys::h5s::H5Sselect_valid(out.id()) } <= 0 {
                return Err(Error(
                    "projected VDS selection exceeds source bounds".into(),
                ));
            }
            Ok(out)
        })?;
        self.complexity(&out)?;
        Ok(out)
    }
    fn difference(&mut self, a: &Dataspace, b: &Dataspace) -> Result<Dataspace, Error> {
        let kind = hdf5_backend::sync::sync(|| unsafe { H5Sget_select_type(b.id()) });
        if kind == H5S_sel_type::H5S_SEL_NONE {
            return Ok(a.clone());
        }
        if kind == H5S_sel_type::H5S_SEL_ALL {
            return hdf5_backend::sync::sync(|| {
                empty(
                    a.shape()
                        .try_into()
                        .map_err(|_| Error("VDS domain rank changed".into()))?,
                )
            });
        }
        self.combine(a, b, H5S_seloper_t::H5S_SELECT_NOTB)
    }
    fn guard(
        &mut self,
        dataset: &Dataset,
        owner: usize,
        path: &str,
        shape: Option<[usize; 3]>,
        master: bool,
    ) -> Result<(), Error> {
        if let Some(guard) = self
            .guards
            .iter_mut()
            .find(|g| g.file.id() == self.sources.file(owner).id() && g.path == path)
        {
            if key(&guard.dataset)? != key(dataset)?
                || guard
                    .shape
                    .zip(shape)
                    .is_some_and(|(before, after)| before != after)
            {
                return Err(Error("VDS source changed during graph indexing".into()));
            }
            if guard.shape.is_none() {
                guard.shape = shape;
            }
            return Ok(());
        }
        self.sources
            .charge(path.len().saturating_mul(2).saturating_add(512))?;
        self.guards.push(Guard {
            file: self.sources.file(owner).clone(),
            path: path.into(),
            dataset: dataset.clone(),
            shape,
            plist: if master { Some(dataset.dcpl()?) } else { None },
        });
        Ok(())
    }
    fn concrete(
        &mut self,
        dataset: &Dataset,
        owner: usize,
        path: &str,
    ) -> Result<Arc<Concrete>, Error> {
        let shape: [usize; 3] = dataset
            .shape()
            .try_into()
            .map_err(|_| Error("VDS concrete source requires rank3".into()))?;
        if shape.contains(&0)
            || shape
                .iter()
                .try_fold(self.pixel_type.bytes(), |n, &d| n.checked_mul(d))
                .is_none()
        {
            return Err(Error(
                "VDS concrete source is empty or overflows byte arithmetic".into(),
            ));
        }
        self.guard(dataset, owner, path, Some(shape), false)?;
        let id = key(dataset)?;
        for concrete in &self.concrete {
            if key(&concrete.dataset)? == id {
                return Ok(concrete.clone());
            }
        }
        let storage = typed::admit_storage(dataset, self.pixel_type, self.limits)?;
        if !storage.storage_allocated {
            return Err(Error("VDS mapped concrete source is not fully allocated; missing samples never become fill".into()));
        }
        let (_, little) = typed::standard_type(&dataset.dtype()?)?;
        self.sources.charge(512)?;
        let concrete = Arc::new(Concrete {
            dataset: dataset.clone(),
            shape,
            storage,
            little,
        });
        self.concrete.push(concrete.clone());
        Ok(concrete)
    }
    fn expand(
        &mut self,
        descriptor: Descriptor,
        owner: usize,
        path: &str,
        index: usize,
    ) -> Result<Vec<printf::Expanded>, Error> {
        let owner_file = self.sources.path(owner).to_path_buf();
        let context = printf::Owner {
            file: &owner_file,
            is_root: owner == 0,
            dataset: path,
            mapping_index: index,
        };
        let mut plan = None;
        for (i, p) in self.bindings.options.printf_mappings.iter().enumerate() {
            if p.dataset == path
                && p.mapping_index == index
                && p.stored_filename == descriptor.0
                && p.declared_dataset == descriptor.1
                && printf::matches(p, context, &descriptor.0, &descriptor.1)?
            {
                if plan.is_some() {
                    return Err(Error(
                        "conflicting duplicate active VDS printf plans".into(),
                    ));
                }
                plan = Some(i);
            }
        }
        if let Some(i) = plan {
            if let Some(alias) = &self.bindings.options.printf_mappings[i].source_file
                && self.sources.add(alias)? != owner
            {
                return Err(Error(
                    "printf owner path no longer resolves to the declared VDS source".into(),
                ));
            }
            self.bindings.printf_used.insert(i);
        }
        let expanded = printf::expand(
            descriptor,
            context,
            plan.map(|i| &self.bindings.options.printf_mappings[i]),
            self.sources,
            self.limits,
        )?;
        *self.indexed = self
            .indexed
            .checked_add(expanded.len().saturating_sub(1))
            .filter(|&n| n <= self.limits.datasets)
            .ok_or_else(|| Error("expanded VDS datasets exceed limit".into()))?;
        Ok(expanded)
    }
    fn node(
        &mut self,
        dataset: &Dataset,
        owner: usize,
        path: &str,
        depth: usize,
    ) -> Result<Arc<Node>, Error> {
        if depth >= MAX_DEPTH.min(self.limits.link_traversals) {
            return Err(Error("VDS recursive depth limit exceeded".into()));
        }
        let id = key(dataset)?;
        if self.active.contains(&id) {
            return Err(Error(
                "VDS mapping cycle detected through physical dataset identity".into(),
            ));
        }
        self.guard(dataset, owner, path, None, true)?;
        if let Some(node) = self.nodes.iter().find(|n| n.identity == id).cloned() {
            self.guard(dataset, owner, path, node.fixed_extent, true)?;
            return Ok(node);
        }
        self.active.push(id);
        let node_id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .filter(|&n| n <= self.limits.datasets)
            .ok_or_else(|| Error("VDS node count limit exceeded".into()))?;
        self.sources.charge(1024)?;
        let originals = descriptors(dataset, self.sources, self.limits, self.indexed)?;
        let mut declared = None;
        let mut maximum = None;
        let mut dynamic = [false; 3];
        let mut prepared = Vec::new();
        for (index, original) in originals.into_iter().enumerate() {
            let (shape, max) = selection_shape(&original.3)?;
            if declared.is_some_and(|s| s != shape) || maximum.is_some_and(|s| s != max) {
                return Err(Error("VDS original mapping extents disagree".into()));
            }
            declared = Some(shape);
            maximum = Some(max);
            if let SelectionRecord::Regular { count, block, .. } = &original.3 {
                for a in 0..3 {
                    dynamic[a] |= count[a] == u64::MAX || block[a] == u64::MAX;
                }
            }
            for expanded in self.expand(original, owner, path, index)? {
                let (
                    filename,
                    declared_dataset,
                    original_source,
                    virtual_record,
                    source_space,
                    virtual_space,
                ) = expanded.descriptor;
                let source_index = self.bindings.source(&filename, owner, self.sources)?;
                let (source_index, resolved) =
                    self.sources.resolve(source_index, &declared_dataset)?;
                let concrete =
                    crate::hdf5::sources::hard_dataset(self.sources.file(source_index), &resolved)?;
                let virtual_type = dataset.dtype()?;
                let source_type = concrete.dtype()?;
                if hdf5_backend::sync::sync(|| unsafe {
                    H5Tequal(virtual_type.id(), source_type.id())
                }) <= 0
                {
                    return Err(Error(
                        "VDS original source dtype must exactly match virtual dtype".into(),
                    ));
                }
                let input = if concrete.layout() == Layout::Virtual {
                    Input::Nested(self.node(&concrete, source_index, &resolved, depth + 1)?)
                } else {
                    Input::Concrete(self.concrete(&concrete, source_index, &resolved)?)
                };
                let source =
                    super::unlimited::freeze(&original_source, source_space, input.shape())?;
                let (original_source, original_virtual) =
                    if let Some(p) = &expanded.printf_expansion {
                        (
                            p.declaration.source_selection.clone(),
                            p.declaration.virtual_selection.clone(),
                        )
                    } else {
                        (original_source, virtual_record.clone())
                    };
                self.sources.charge(
                    selection_cost(&original_source)?
                        .checked_add(selection_cost(&original_virtual)?)
                        .and_then(|n| n.checked_add(1024))
                        .ok_or_else(|| Error("VDS original mapping metadata overflow".into()))?,
                )?;
                prepared.push(Prepared {
                    filename,
                    dataset: declared_dataset,
                    source_file: self.sources.path(source_index).to_path_buf(),
                    resolved,
                    input,
                    original_source,
                    original_virtual,
                    source,
                    virtual_record,
                    virtual_space,
                    printf: expanded.printf_expansion,
                });
            }
        }
        let declared = declared.ok_or_else(|| Error("empty VDS graph node".into()))?;
        let maximum = maximum.expect("nonempty checked");
        let mut shape = [0usize; 3];
        for a in 0..3 {
            if !dynamic[a] {
                shape[a] = usize::try_from(declared[a])
                    .map_err(|_| Error("VDS declared shape exceeds platform".into()))?;
            }
        }
        for p in &prepared {
            if let Some(end) = super::unlimited::virtual_end(
                &p.virtual_record,
                hdf5_backend::sync::sync(|| finite::points(&p.source))?,
            )? {
                shape[0] = shape[0].max(end);
            } else {
                let bounds = hdf5_backend::sync::sync(|| {
                    let v = Space(unsafe { hdf5_sys::h5s::H5Scopy(p.virtual_space.0) })
                        .into_dataspace()?;
                    finite::bounds(&v)
                })?;
                for a in 0..3 {
                    if dynamic[a] {
                        shape[a] =
                            shape[a].max(bounds.0[a].checked_add(bounds.1[a]).ok_or_else(
                                || Error("VDS frozen selected extent overflow".into()),
                            )?);
                    }
                }
            }
        }
        validate_shape(shape, self.pixel_type, self.limits)?;
        let mut printf_domains: std::collections::BTreeMap<usize, (Arc<PrintfDeclaration>, usize)> =
            std::collections::BTreeMap::new();
        for p in &prepared {
            if let Some(expansion) = &p.printf
                && expansion.declaration.plan.is_some()
            {
                let identity = Arc::as_ptr(&expansion.declaration) as usize;
                let points = hdf5_backend::sync::sync(|| {
                    usize::try_from(unsafe {
                        hdf5_sys::h5s::H5Sget_select_npoints(p.virtual_space.0)
                    })
                    .map_err(|_| Error("invalid expanded VDS block size".into()))
                })?;
                if let Some((_, total)) = printf_domains.get_mut(&identity) {
                    *total = total
                        .checked_add(points)
                        .ok_or_else(|| Error("VDS printf domain point overflow".into()))?;
                } else {
                    self.sources.charge(128)?;
                    printf_domains.insert(identity, (expansion.declaration.clone(), points));
                }
            }
        }
        for (declaration, points) in printf_domains.values() {
            printf::validate_domain(declaration, shape, *points)?;
        }

        let fixed_extent = if dynamic.iter().any(|&d| d) {
            None
        } else {
            Some(shape)
        };
        self.guard(dataset, owner, path, fixed_extent, true)?;
        self.sources.charge(
            shape[0]
                .checked_mul(8)
                .ok_or_else(|| Error("VDS coverage metadata overflow".into()))?,
        )?;
        let mut leaves = Vec::new();
        let mut mappings = Vec::new();
        let mut operations = 0usize;
        let mut coverage = hdf5_backend::sync::sync(|| empty(shape))?;
        for p in prepared {
            let virtual_ = super::unlimited::freeze(&p.virtual_record, p.virtual_space, shape)?;
            let source_points = hdf5_backend::sync::sync(|| finite::points(&p.source))?;
            let virtual_points = hdf5_backend::sync::sync(|| finite::points(&virtual_))?;
            if source_points == 0 || source_points != virtual_points {
                return Err(Error("incomplete VDS frozen mapping domain: exhausted source cannot become declared fill".into()));
            }
            let frozen_source = record(&p.source, self.sources, self.limits)?;
            let frozen_virtual = record(&virtual_, self.sources, self.limits)?;
            let weight = finite::complexity(&frozen_source)?
                .checked_add(finite::complexity(&frozen_virtual)?)
                .ok_or_else(|| Error("VDS selection complexity overflow".into()))?;
            operations = operations
                .checked_add(
                    shape[0]
                        .checked_mul(weight)
                        .ok_or_else(|| Error("VDS selected operation overflow".into()))?,
                )
                .filter(|&n| n <= finite::MAX_OPERATIONS)
                .ok_or_else(|| Error("VDS graph selection operation limit exceeded".into()))?;
            let overlap = self.combine(&coverage, &virtual_, H5S_seloper_t::H5S_SELECT_AND)?;
            if hdf5_backend::sync::sync(|| finite::points(&overlap))? > 0 {
                return Err(Error(
                    "overlapping VDS graph mappings are unsupported".into(),
                ));
            }
            coverage = self.combine(&coverage, &virtual_, H5S_seloper_t::H5S_SELECT_OR)?;
            let (source_origin, source_bounds) =
                hdf5_backend::sync::sync(|| finite::bounds(&p.source))?;
            let (virtual_origin, virtual_bounds) =
                hdf5_backend::sync::sync(|| finite::bounds(&virtual_))?;
            let (nested_node, filters, little, layout, allocated) = match &p.input {
                Input::Concrete(c) => (
                    None,
                    c.storage.filters.clone(),
                    c.little,
                    c.dataset.layout(),
                    true,
                ),
                Input::Nested(n) => (
                    Some(n.metadata.node_id),
                    Vec::new(),
                    typed::standard_type(&dataset.dtype()?)?.1,
                    Layout::Virtual,
                    false,
                ),
            };
            let rectangular = source_bounds == virtual_bounds
                && rectangle(&frozen_source, Some(p.input.shape())).is_ok()
                && rectangle(&frozen_virtual, None).is_ok();
            let m = Mapping {
                stored_filename: p.filename,
                declared_dataset: p.dataset,
                source_file: p.source_file,
                resolved_dataset: p.resolved,
                source_shape: p.input.shape(),
                source_selection: p.original_source,
                virtual_selection: p.original_virtual,
                frozen_source_selection: frozen_source,
                frozen_virtual_selection: frozen_virtual,
                nested_node,
                printf_expansion: p.printf,
                source_origin,
                virtual_origin,
                mapped_shape: virtual_bounds,
                selection_layout: if rectangular {
                    SelectionLayout::ContiguousRectangle
                } else {
                    SelectionLayout::FiniteSelection
                },
                source_bounding_shape: source_bounds,
                virtual_bounding_shape: virtual_bounds,
                selected_elements: source_points as u64,
                filters,
                source_pixel_type: self.pixel_type,
                source_little_endian: little,
                source_layout: layout,
                source_fill_value: p.input.fill(),
                source_fill_time: p.input.fill_time(),
                storage_allocated: allocated,
            };
            self.sources.charge(mapping_cost(&m)?)?;
            mappings.push(m);
            match p.input {
                Input::Concrete(c) => add_leaf(
                    &mut leaves,
                    Leaf {
                        target: Arc::new(Target::Concrete(c)),
                        source: p.source,
                        virtual_,
                    },
                    self.sources,
                    self.limits,
                )?,
                Input::Nested(n) => {
                    for leaf in &n.leaves {
                        if leaves.len() >= MAX_LEAVES.min(self.limits.group_members) {
                            return Err(Error(
                                "VDS expanded execution leaf limit exceeded before projection"
                                    .into(),
                            ));
                        }
                        let selected =
                            self.combine(&p.source, &leaf.virtual_, H5S_seloper_t::H5S_SELECT_AND)?;
                        if hdf5_backend::sync::sync(|| finite::points(&selected))? == 0 {
                            continue;
                        }
                        let source = self.project(&leaf.virtual_, &leaf.source, &p.source)?;
                        let output = self.project(&p.source, &virtual_, &leaf.virtual_)?;
                        if hdf5_backend::sync::sync(|| finite::points(&source))?
                            != hdf5_backend::sync::sync(|| finite::points(&output))?
                        {
                            return Err(Error(
                                "nested VDS projection changes selection element pairing".into(),
                            ));
                        }
                        add_leaf(
                            &mut leaves,
                            Leaf {
                                target: leaf.target.clone(),
                                source,
                                virtual_: output,
                            },
                            self.sources,
                            self.limits,
                        )?;
                    }
                }
            }
        }
        let domain = hdf5_backend::sync::sync(|| all(shape))?;
        let gap = self.difference(&domain, &coverage)?;
        let mut unmapped = Vec::new();
        unmapped
            .try_reserve_exact(shape[0])
            .map_err(|e| Error(e.to_string()))?;
        for frame in 0..shape[0] {
            let plane = Dataspace::try_new(shape)?.select((frame, .., ..))?;
            let selected = self.combine(&coverage, &plane, H5S_seloper_t::H5S_SELECT_AND)?;
            let covered = hdf5_backend::sync::sync(|| finite::points(&selected))?;
            unmapped.push(shape[1] * shape[2] - covered);
        }
        let fill = typed::fill(dataset, self.pixel_type)?;
        let fill_time = dataset.dcpl()?.get_fill_time()?;
        if hdf5_backend::sync::sync(|| finite::points(&gap))? > 0 {
            let valid = if fill_time == FillTime::Never {
                None
            } else {
                fill.clone()
            };
            let native = valid.as_ref().map(word).transpose()?;
            let target = Arc::new(Target::Fill {
                origin: Origin {
                    node: node_id,
                    file: self.sources.path(owner).to_path_buf(),
                    dataset: path.into(),
                },
                value: valid,
                word: native,
            });
            add_leaf(
                &mut leaves,
                Leaf {
                    target,
                    source: gap.clone(),
                    virtual_: gap,
                },
                self.sources,
                self.limits,
            )?;
        }
        shape[0]
            .checked_mul(leaves.len())
            .filter(|&n| n <= finite::MAX_OPERATIONS)
            .ok_or_else(|| Error("VDS flattened frame/leaf operation limit exceeded".into()))?;
        let constant = unmapped
            .first()
            .copied()
            .filter(|n| unmapped.iter().all(|v| v == n));
        let metadata = VdsNodeMetadata {
            node_id,
            source_file: self.sources.path(owner).to_path_buf(),
            dataset: path.into(),
            declared_shape: declared,
            frozen_shape: shape,
            master_max_shape: maximum,
            mappings,
            fill_value: fill,
            fill_time,
            unmapped_pixels_per_frame: constant,
            unmapped_pixels_by_frame: unmapped,
        };
        self.sources.charge(node_cost(&metadata)?)?;
        let node = Arc::new(Node {
            identity: id,
            fixed_extent,
            metadata,
            leaves,
        });
        self.active.pop();
        self.nodes.push(node.clone());
        Ok(node)
    }
}

pub(super) fn admit(
    dataset: &Dataset,
    owner: (usize, &str),
    sources: &mut Sources,
    bindings: &mut Bindings<'_>,
    limits: Limits,
    indexed: &mut usize,
    pixel_type: PixelType,
) -> Result<Admission, Error> {
    let mut state = State {
        sources,
        bindings,
        limits,
        indexed,
        pixel_type,
        nodes: Vec::new(),
        active: Vec::new(),
        guards: Vec::new(),
        concrete: Vec::new(),
        next_id: 0,
        planning_operations: 0,
    };
    let root = state.node(dataset, owner.0, owner.1, 0)?;
    let mut fills = Vec::new();
    let mut nodes = Vec::new();
    for n in &state.nodes {
        if n.metadata.node_id != root.metadata.node_id {
            state.sources.charge(node_cost(&n.metadata)?)?;
            nodes.push(n.metadata.clone());
        }
    }
    for leaf in &root.leaves {
        if let Target::Fill { origin, value, .. } = leaf.target.as_ref() {
            let value = value
                .as_ref()
                .ok_or_else(|| Error("selected nested VDS region has undefined fill".into()))?;
            if state.bindings.options.unmapped_regions != UnmappedRegions::DeclaredFill {
                return Err(Error(
                    "effective selected VDS fill requires explicit DeclaredFill policy".into(),
                ));
            }
            let origin_selection = record(&leaf.source, state.sources, limits)?;
            let virtual_selection = record(&leaf.virtual_, state.sources, limits)?;
            state.sources.charge(
                selection_cost(&origin_selection)?
                    .checked_add(selection_cost(&virtual_selection)?)
                    .and_then(|n| {
                        n.checked_add(512 + origin.file.as_os_str().len() + origin.dataset.len())
                    })
                    .ok_or_else(|| Error("VDS fill provenance metadata overflow".into()))?,
            )?;
            fills.push(FillRegion {
                origin_node: origin.node,
                source_file: origin.file.clone(),
                dataset: origin.dataset.clone(),
                origin_selection,
                virtual_selection,
                value: value.clone(),
            });
        }
    }
    state.sources.charge(
        node_cost(&root.metadata)?
            .checked_add(
                root.leaves
                    .len()
                    .checked_mul(256)
                    .ok_or_else(|| Error("VDS execution metadata overflow".into()))?,
            )
            .ok_or_else(|| Error("VDS aggregate metadata overflow".into()))?,
    )?;
    let r = &root.metadata;
    let metadata = VdsMetadata {
        declared_shape: r.declared_shape,
        frozen_shape: r.frozen_shape,
        master_max_shape: r.master_max_shape,
        mappings: r.mappings.clone(),
        unmapped_regions: state.bindings.options.unmapped_regions,
        unmapped_pixels_per_frame: r.unmapped_pixels_per_frame,
        unmapped_pixels_by_frame: r.unmapped_pixels_by_frame.clone(),
        node_id: r.node_id,
        source_file: r.source_file.clone(),
        dataset: r.dataset.clone(),
        fill_value: r.fill_value.clone(),
        nested_nodes: nodes,
        effective_fill_regions: fills,
    };
    let storage = Storage {
        filters: Vec::new(),
        fill_value: r.fill_value.clone(),
        fill_time: r.fill_time,
        storage_allocated: false,
    };
    let graph = Graph {
        leaves: root.leaves.clone(),
        guards: state.guards,
    };
    graph.check()?;
    Ok(Admission {
        mosaic: Mosaic { graph },
        metadata,
        storage,
    })
}

struct Iter(hid_t);
impl Drop for Iter {
    fn drop(&mut self) {
        hdf5_backend::sync::sync(|| {
            unsafe { H5Ssel_iter_close(self.0) };
        });
    }
}
fn fill_selection(
    selection: &finite::Selection,
    frame: usize,
    shape: [usize; 2],
    pixels: &mut PixelBuffer,
    word: &[u8; 16],
) -> Result<(), Error> {
    let width = pixels.pixel_type().bytes();
    hdf5_backend::sync::sync(|| {
        let memory = selection.intersection(frame)?;
        let count = finite::points(&memory)?;
        if count == 0 {
            return Ok(());
        }
        let offset = [frame as i64, 0, 0];
        if unsafe { hdf5_sys::h5s::H5Sselect_adjust(memory.id(), offset.as_ptr()) } < 0 {
            return Err(Error(
                "cannot position nested VDS fill in output frame".into(),
            ));
        }
        finite::set_extent(&memory, [1, shape[0], shape[1]])?;
        // Unit-size iteration avoids version/binding naming ambiguity:
        // offsets and lengths are sample indices here, then scaled explicitly.
        let iter = Iter(unsafe { H5Ssel_iter_create(memory.id(), 1, 0) });
        if iter.0 < 0 {
            return Err(Error("cannot create bounded VDS fill run iterator".into()));
        }
        let mut remaining = count;
        let output = pixels.bytes_mut();
        while remaining > 0 {
            let (mut offsets, mut lengths) = ([0u64; 64], [0usize; 64]);
            let (mut nseq, mut elements) = (0usize, 0usize);
            if unsafe {
                H5Ssel_iter_get_seq_list(
                    iter.0,
                    64,
                    remaining,
                    &mut nseq,
                    &mut elements,
                    offsets.as_mut_ptr(),
                    lengths.as_mut_ptr(),
                )
            } < 0
                || nseq == 0
                || nseq > 64
                || elements == 0
                || elements > remaining
            {
                return Err(Error("invalid bounded VDS fill run iterator result".into()));
            }
            let mut total = 0usize;
            for i in 0..nseq {
                let first = usize::try_from(offsets[i])
                    .map_err(|_| Error("VDS fill offset exceeds platform".into()))?;
                let start = first
                    .checked_mul(width)
                    .ok_or_else(|| Error("VDS fill byte offset overflow".into()))?;
                let end = first
                    .checked_add(lengths[i])
                    .and_then(|n| n.checked_mul(width))
                    .filter(|&n| n <= output.len())
                    .ok_or_else(|| Error("VDS fill run exceeds output frame".into()))?;
                for sample in output[start..end].chunks_exact_mut(width) {
                    sample.copy_from_slice(&word[..width]);
                }
                total = total
                    .checked_add(lengths[i])
                    .ok_or_else(|| Error("VDS fill run sample count overflow".into()))?;
            }
            if total != elements {
                return Err(Error("VDS fill iterator sample counts disagree".into()));
            }
            remaining -= elements;
        }
        Ok(())
    })
}
