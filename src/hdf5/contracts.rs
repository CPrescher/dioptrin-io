//! Metadata-derived source contracts for an explicitly selected HDF5 image.
//!
//! Callers opt into exact declared-path resolution. No directory scans, basename
//! substitution, native VDS lookup or sample reads occur here. Missing files and
//! unbounded printf declarations are requirements for the caller, never fill.
use super::{
    Error,
    sources::Sources,
    typed::{self, ExternalRawOptions, FilenameBinding, PrintfMapping, Selection, VdsOptions},
    vds::printf::contract_name,
};
use hdf5_backend::{Dataset, LocationToken, dataset::Layout, plist::DatasetCreate};
use hdf5_sys::h5p::{
    H5Pequal, H5Pget_external, H5Pget_external_count, H5Pget_virtual_count,
    H5Pget_virtual_dsetname, H5Pget_virtual_filename,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// An exact original filename owned by one physical HDF5 file and a user choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChoice {
    pub owner: PathBuf,
    pub stored_filename: String,
    pub actual_file: PathBuf,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Virtual,
    ExternalRaw,
}
/// Information still needed before a complete native source contract can open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Requirement {
    File {
        kind: FileKind,
        owner: PathBuf,
        stored_filename: String,
        expected_file: PathBuf,
    },
    /// The caller must select a finite block count; availability cannot define it.
    Printf {
        owner: PathBuf,
        dataset: String,
        mapping_index: usize,
        stored_filename: String,
        declared_dataset: String,
    },
}
struct Guard {
    source: usize,
    path: String,
    token: LocationToken,
    plist: DatasetCreate,
    shape: Option<Vec<usize>>,
}
/// An immutable metadata plan. It is not pixel/storage admission: pass the
/// resulting contracts to the typed/wrapped reader and retain its validation.
pub struct SourceContracts {
    sources: Sources,
    root: PathBuf,
    vds: Vec<FilenameBinding>,
    raw: Vec<FilenameBinding>,
    printf: Vec<PrintfMapping>,
    requirements: Vec<Requirement>,
    guards: Vec<Guard>,
}
impl SourceContracts {
    /// Resolve only the exact filenames declared by the selected source graph.
    /// Relative declarations use their owner's directory (explicit origin for
    /// a captured root); absolute declarations stay absolute. Choices override
    /// exact owner/name pairs. External-link policy is independent and unchanged.
    /// # Errors
    /// Invalid declarations/choices, cycles, mutation, missing datasets or bounds.
    pub fn resolve_snapshot(
        path: impl AsRef<Path>,
        selection: Selection<'_>,
        options: typed::Options,
        choices: &[FileChoice],
        printf: &[PrintfMapping],
        origin: Option<&Path>,
    ) -> Result<Self, Error> {
        typed::validate_limits(options.limits)?;
        let mut sources =
            Sources::new_with_origin(path, options.limits, options.external_links, origin)?;
        let root = sources.path(0).to_path_buf();
        validate_choices(choices, printf, options.limits)?;
        let choices: Vec<_> = choices
            .iter()
            .map(|c| FileChoice {
                owner: std::fs::canonicalize(&c.owner).unwrap_or_else(|_| c.owner.clone()),
                stored_filename: c.stored_filename.clone(),
                actual_file: c.actual_file.clone(),
            })
            .collect();
        validate_choices(&choices, printf, options.limits)?;
        let roots = typed::requests(
            &mut sources,
            selection,
            options.limits,
            &mut BTreeMap::new(),
        )?;
        if roots.is_empty() || roots.len() > options.limits.datasets {
            return Err(Error(
                "HDF5 source contract dataset limit/empty selection".into(),
            ));
        }
        let mut walker = Walker {
            result: Self {
                sources,
                root,
                vds: vec![],
                raw: vec![],
                printf: vec![],
                requirements: vec![],
                guards: vec![],
            },
            options,
            choices: &choices,
            plans: printf,
            origin,
            ancestors: vec![],
            visited: vec![],
            mappings: 0,
        };
        for (source, path) in roots {
            walker.visit(source, &path, 0)?;
        }
        walker.result.check_sources()?;
        Ok(walker.result)
    }
    pub fn requirements(&self) -> &[Requirement] {
        &self.requirements
    }
    pub fn vds_filenames(&self) -> &[FilenameBinding] {
        &self.vds
    }
    pub fn external_raw_filenames(&self) -> &[FilenameBinding] {
        &self.raw
    }
    pub fn printf_mappings(&self) -> &[PrintfMapping] {
        &self.printf
    }
    /// Only original defined, genuinely unmapped regions may use DeclaredFill.
    /// Missing files never reach storage admission under this policy.
    pub fn vds_options(&self, unmapped_regions: typed::UnmappedRegions) -> VdsOptions<'_> {
        VdsOptions {
            filenames: &self.vds,
            printf_mappings: &self.printf,
            unmapped_regions,
        }
    }
    pub fn external_raw_options(&self) -> ExternalRawOptions<'_> {
        ExternalRawOptions {
            filenames: &self.raw,
        }
    }
    pub fn source_bindings(&self) -> &[crate::SourceBinding] {
        &self.sources.bindings
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// # Errors
    /// Any indexed source, path alias, object, declaration or concrete extent changed.
    pub fn check_sources(&self) -> Result<(), Error> {
        self.sources.check()?;
        for guard in &self.guards {
            let dataset =
                super::sources::hard_dataset(self.sources.file(guard.source), &guard.path)?;
            let current_plist = dataset.dcpl()?;
            if dataset.loc_info()?.token != guard.token
                || guard.shape.as_ref().is_some_and(|s| *s != dataset.shape())
                || hdf5_backend::sync::sync(|| unsafe {
                    H5Pequal(guard.plist.id(), current_plist.id())
                }) <= 0
            {
                return Err(Error(
                    "HDF5 source contract metadata changed; reopen snapshot".into(),
                ));
            }
        }
        self.sources.check()
    }
    /// # Errors
    /// Caller decisions remain unresolved; lists all actual missing declarations.
    pub fn require_complete(&self) -> Result<(), Error> {
        self.check_sources()?;
        if self.requirements.is_empty() {
            Ok(())
        } else {
            Err(Error(format!(
                "HDF5 source contract needs caller choices: {:?}",
                self.requirements
            )))
        }
    }
}
fn validate_choices(
    choices: &[FileChoice],
    plans: &[PrintfMapping],
    limits: typed::Limits,
) -> Result<(), Error> {
    if choices.len() > limits.source_files || plans.len() > limits.datasets {
        return Err(Error("HDF5 source choice count limit".into()));
    }
    let mut bytes = 0usize;
    for (index, c) in choices.iter().enumerate() {
        if !c.owner.is_absolute()
            || !c.actual_file.is_absolute()
            || c.stored_filename.is_empty()
            || c.stored_filename.contains('\0')
            || c.stored_filename.len() > limits.text_bytes
            || choices[..index]
                .iter()
                .any(|p| p.owner == c.owner && p.stored_filename == c.stored_filename)
        {
            return Err(Error("invalid/duplicate HDF5 owner/filename choice".into()));
        }
        bytes = bytes
            .saturating_add(c.owner.as_os_str().len())
            .saturating_add(c.actual_file.as_os_str().len())
            .saturating_add(c.stored_filename.len())
            .saturating_add(128);
    }
    for p in plans {
        super::vds::printf::validate_plan(p, limits)?;
        bytes = bytes
            .saturating_add(p.stored_filename.len())
            .saturating_add(p.declared_dataset.len())
            .saturating_add(p.dataset.len())
            .saturating_add(256);
    }
    if bytes > limits.metadata_bytes {
        return Err(Error("HDF5 source choices exceed metadata bound".into()));
    }
    Ok(())
}
struct Walker<'a> {
    result: SourceContracts,
    options: typed::Options,
    choices: &'a [FileChoice],
    plans: &'a [PrintfMapping],
    origin: Option<&'a Path>,
    ancestors: Vec<(usize, LocationToken)>,
    visited: Vec<(usize, String)>,
    mappings: usize,
}
impl Walker<'_> {
    fn push_requirement(&mut self, r: Requirement) -> Result<(), Error> {
        if !self.result.requirements.contains(&r) {
            if self.result.requirements.len()
                >= self
                    .options
                    .limits
                    .source_files
                    .saturating_add(self.options.limits.datasets)
            {
                return Err(Error("HDF5 source requirement limit".into()));
            }
            self.result.requirements.push(r);
        }
        Ok(())
    }
    fn filename(
        &mut self,
        source: usize,
        stored: &str,
        kind: FileKind,
    ) -> Result<Option<usize>, Error> {
        if stored == "." && kind == FileKind::Virtual {
            return Ok(Some(source));
        }
        let owner = self.result.sources.path(source).to_path_buf();
        let base = if source == 0 {
            self.origin.unwrap_or_else(|| owner.parent().unwrap())
        } else {
            owner.parent().unwrap()
        };
        let actual = self
            .choices
            .iter()
            .find(|c| c.owner == owner && c.stored_filename == stored)
            .map(|c| c.actual_file.clone())
            .unwrap_or_else(|| base.join(stored));
        match std::fs::metadata(&actual) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.push_requirement(Requirement::File {
                    kind,
                    owner,
                    stored_filename: stored.into(),
                    expected_file: actual,
                })?;
                return Ok(None);
            }
            Err(e) => return Err(Error(format!("HDF5 source {}: {e}", actual.display()))),
            Ok(m) if !m.is_file() => {
                return Err(Error(format!(
                    "HDF5 source is not a regular file: {}",
                    actual.display()
                )));
            }
            _ => {}
        }
        let index = match kind {
            FileKind::Virtual => self.result.sources.add(&actual)?,
            FileKind::ExternalRaw => self.result.sources.add_raw(&actual)?,
        };
        let bindings = match kind {
            FileKind::Virtual => &mut self.result.vds,
            FileKind::ExternalRaw => &mut self.result.raw,
        };
        if let Some(existing) = bindings.iter().find(|b| b.stored_filename == stored) {
            if existing.actual_file != actual {
                return Err(Error(format!(
                    "ambiguous HDF5 filename binding across owners: {stored}"
                )));
            }
        } else {
            self.result.sources.charge(
                stored
                    .len()
                    .saturating_add(actual.as_os_str().len())
                    .saturating_add(128),
            )?;
            bindings.push(FilenameBinding {
                stored_filename: stored.into(),
                actual_file: actual,
            });
        }
        Ok(Some(index))
    }
    fn visit(&mut self, source: usize, path: &str, depth: usize) -> Result<(), Error> {
        if depth >= 32 || self.visited.len() >= self.options.limits.datasets {
            return Err(Error("HDF5 source contract traversal limit".into()));
        }
        let (source, path) = self.result.sources.resolve(source, path)?;
        let dataset = self.result.sources.file(source).dataset(&path)?;
        let token = dataset.loc_info()?.token;
        if self.ancestors.contains(&(source, token)) {
            return Err(Error("HDF5 VDS source cycle".into()));
        }
        if self.visited.contains(&(source, path.clone())) {
            return Ok(());
        }
        self.visited.push((source, path.clone()));
        self.ancestors.push((source, token));
        let virtual_dataset = dataset.layout() == Layout::Virtual;
        let plist = dataset.dcpl()?;
        self.result.sources.charge(path.len().saturating_add(256))?;
        self.result.guards.push(Guard {
            source,
            path: path.clone(),
            token,
            plist: plist.clone(),
            shape: if virtual_dataset {
                None
            } else {
                Some(dataset.shape())
            },
        });
        if virtual_dataset {
            self.virtual_dataset(source, &path, &dataset, &plist, depth)?;
        } else {
            self.raw_dataset(source, &plist)?;
        }
        self.ancestors.pop();
        Ok(())
    }
    fn virtual_dataset(
        &mut self,
        source: usize,
        path: &str,
        dataset: &Dataset,
        plist: &DatasetCreate,
        depth: usize,
    ) -> Result<(), Error> {
        let limits = self.options.limits;
        let mut count = 0;
        if hdf5_backend::sync::sync(|| unsafe { H5Pget_virtual_count(plist.id(), &mut count) }) < 0
            || count == 0
            || count > limits.datasets.saturating_sub(self.mappings)
        {
            return Err(Error("VDS mapping count limit/empty declaration".into()));
        }
        self.mappings += count;
        let owner = self.result.sources.path(source).to_path_buf();
        for index in 0..count {
            let filename = self.mapping_name(plist, index, true)?;
            let target = self.mapping_name(plist, index, false)?;
            let substitutions =
                contract_name(&filename, 0, limits)?.1 || contract_name(&target, 0, limits)?.1;
            let blocks = if substitutions {
                let mut plan = None;
                for p in self.plans {
                    let same_owner = match &p.source_file {
                        None => source == 0,
                        Some(f) => std::fs::canonicalize(f).is_ok_and(|p| p == owner),
                    };
                    if same_owner
                        && p.dataset == path
                        && p.mapping_index == index
                        && p.stored_filename == filename
                        && p.declared_dataset == target
                    {
                        if plan.is_some() {
                            return Err(Error("duplicate VDS printf plan".into()));
                        }
                        plan = Some(p.clone());
                    }
                }
                if let Some(p) = plan {
                    let blocks = p.blocks;
                    self.result.printf.push(p);
                    blocks
                } else {
                    self.push_requirement(Requirement::Printf {
                        owner: owner.clone(),
                        dataset: path.into(),
                        mapping_index: index,
                        stored_filename: filename,
                        declared_dataset: target,
                    })?;
                    continue;
                }
            } else {
                1
            };
            for block in 0..blocks {
                let name = contract_name(&filename, block, limits)?.0;
                let target = contract_name(&target, block, limits)?.0;
                if let Some(next) = self.filename(source, &name, FileKind::Virtual)? {
                    self.visit(next, &target, depth + 1)?;
                }
            }
        }
        // Never query a VDS dataspace: its conditional shape can resolve files
        // through HDF5's ambient search. Typed admission freezes the real domain.
        let _ = dataset;
        Ok(())
    }
    fn mapping_name(
        &mut self,
        plist: &DatasetCreate,
        index: usize,
        filename: bool,
    ) -> Result<String, Error> {
        hdf5_backend::sync::sync(|| unsafe {
            // SAFETY: live DCPL; measure first, then use bounded writable storage.
            let get = |p, n| {
                if filename {
                    H5Pget_virtual_filename(plist.id(), index, p, n)
                } else {
                    H5Pget_virtual_dsetname(plist.id(), index, p, n)
                }
            };
            let length = usize::try_from(get(std::ptr::null_mut(), 0))
                .map_err(|_| Error("cannot inspect VDS name".into()))?;
            if length == 0 || length > self.options.limits.text_bytes {
                return Err(Error("VDS filename/dataset text limit".into()));
            }
            self.result.sources.charge(length.saturating_add(1))?;
            let mut bytes = vec![0; length + 1];
            if get(bytes.as_mut_ptr().cast(), bytes.len()) != length as isize {
                return Err(Error("VDS name changed during inspection".into()));
            }
            bytes.truncate(length);
            String::from_utf8(bytes).map_err(|_| Error("non-UTF8 VDS filename/dataset".into()))
        })
    }
    fn raw_dataset(&mut self, source: usize, plist: &DatasetCreate) -> Result<(), Error> {
        let count = hdf5_backend::sync::sync(|| unsafe { H5Pget_external_count(plist.id()) });
        if count < 0 || count as usize > self.options.limits.group_members {
            return Err(Error("raw external window count limit".into()));
        }
        for index in 0..count as u32 {
            let bound = self
                .options
                .limits
                .text_bytes
                .min(self.options.limits.metadata_bytes);
            let mut bytes = vec![
                0xff;
                bound
                    .checked_add(2)
                    .ok_or_else(|| Error("raw filename bound overflow".into()))?
            ];
            let (mut offset, mut size) = (0, 0);
            if hdf5_backend::sync::sync(|| unsafe {
                H5Pget_external(
                    plist.id(),
                    index,
                    bytes.len(),
                    bytes.as_mut_ptr().cast(),
                    &mut offset,
                    &mut size,
                )
            }) < 0
            {
                return Err(Error("cannot inspect raw filename".into()));
            }
            let length = bytes
                .iter()
                .position(|b| *b == 0)
                .filter(|n| *n <= bound)
                .ok_or_else(|| Error("raw filename text limit".into()))?;
            bytes.truncate(length);
            self.result.sources.charge(length.saturating_add(128))?;
            let filename =
                String::from_utf8(bytes).map_err(|_| Error("non-UTF8 raw filename".into()))?;
            self.filename(source, &filename, FileKind::ExternalRaw)?;
        }
        Ok(())
    }
}
