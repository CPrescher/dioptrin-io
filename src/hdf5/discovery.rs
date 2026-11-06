//! Detector convention evidence from bounded HDF5 metadata; no count decode.
use super::{
    Error,
    sources::Sources,
    typed::{Limits, Options, read_text},
};
use crate::discovery::Codec;
use hdf5_backend::{Group, LocationType};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Inspect producer/default/dataformat and detector conventions with the same
/// exact link resolver and text limits as the typed reader. Generic HDF5 remains
/// Hdf5 unless a conventional data-array layout supplies selection evidence.
/// Variable-length native string allocation has the typed reader's existing
/// post-read text bound; every successfully read string is charged cumulatively.
/// # Errors
/// Conflicting conventions, malformed/oversized metadata, unadmitted links,
/// traversal/member limits or changed source snapshots.
pub fn discover_snapshot(
    path: impl AsRef<Path>,
    options: Options,
    origin: Option<&Path>,
) -> Result<Codec, Error> {
    let limits = options.limits;
    if [
        limits.metadata_bytes,
        limits.text_bytes,
        limits.source_files,
        limits.group_members,
        limits.link_traversals,
        limits.datasets,
    ]
    .contains(&0)
        || [
            limits.metadata_bytes,
            limits.text_bytes,
            limits.source_files,
            limits.group_members,
            limits.link_traversals,
            limits.datasets,
        ]
        .contains(&usize::MAX)
    {
        return Err(Error(
            "HDF5 discovery limits must be positive and finite".into(),
        ));
    }
    let mut ctx = Context {
        sources: Sources::new_with_origin(path, limits, options.external_links, origin)?,
        limits,
        steps: 0,
        names: BTreeMap::new(),
    };
    let (_, _, root) = ctx.sources.group(0, "/")?;
    let creator = ctx.attribute(&root, c"creator")?.unwrap_or_default();
    let mut strong = BTreeSet::new();
    if creator.starts_with("pyFAI") {
        strong.insert(Codec::Sparse);
    }
    if creator.starts_with("LIMA") {
        let mut bragg = false;
        if let Some(entry) = ctx.attribute(&root, c"default")?
            && let Some((source, path, group)) = ctx.group(0, &entry)?
            && let Some(default) = ctx.attribute(&group, c"default")?
        {
            let target = if default.starts_with('/') {
                default
            } else {
                format!("{}/{default}", path.trim_end_matches('/'))
            };
            if let Some((_, _, group)) = ctx.group(source, &target)?
                && let Some(format) = ctx.attribute(&group, c"dataformat")?
            {
                bragg = format.contains("Bragg");
            }
        }
        strong.insert(if bragg { Codec::Sparse } else { Codec::Lima });
    }
    if let Some((source, path, entry)) = ctx.group(0, "/entry")? {
        if let Some(declared) = ctx.attribute(&entry, c"pyFAI_sparse_frames")? {
            if declared.is_empty() {
                return Err(Error("empty HDF5 sparse group declaration".into()));
            }
            // A sparse marker narrows the LIMA producer umbrella to its sparse variant.
            strong.remove(&Codec::Lima);
            strong.insert(Codec::Sparse);
        }
        if let Some((detector_source, detector_path, detector)) =
            ctx.group(source, &format!("{path}/instrument/detector"))?
            && ctx.has_name(detector_source, &detector_path, &detector, "description")?
            && ctx.has_name(detector_source, &detector_path, &detector, "data")?
        {
            let description_path = format!("{detector_path}/description");
            if let Some((text_source, text_path, LocationType::Dataset)) =
                ctx.object(detector_source, &description_path)?
            {
                let container = ctx.sources.file(text_source).dataset(&text_path)?;
                let text = read_text(&container, limits.text_bytes, false)?;
                ctx.sources.charge(text.len().saturating_add(64))?;
                if text == "Lambda"
                    && ctx.array(detector_source, &format!("{detector_path}/data"))?
                {
                    strong.insert(Codec::Lambda);
                }
            }
        }
    }
    let codec = if strong.len() > 1 {
        return Err(Error(format!(
            "conflicting HDF5 detector convention evidence: {strong:?}; select the layout explicitly"
        )));
    } else if let Some(codec) = strong.first() {
        *codec
    } else if ctx.eiger()? {
        Codec::Eiger
    } else {
        Codec::Hdf5
    };
    ctx.sources.check()?;
    Ok(codec)
}
struct Context {
    sources: Sources,
    limits: Limits,
    steps: usize,
    names: BTreeMap<(usize, String), Vec<String>>,
}
impl Context {
    fn attribute(
        &mut self,
        location: &hdf5_backend::Location,
        name: &std::ffi::CStr,
    ) -> Result<Option<String>, Error> {
        if !has_attr(location, name)? {
            return Ok(None);
        }
        let name = name
            .to_str()
            .map_err(|_| Error("invalid discovery attribute name".into()))?;
        let value = read_text(
            &*location.attr(name)?,
            self.limits.text_bytes.min(self.limits.metadata_bytes),
            true,
        )?;
        self.sources.charge(value.len().saturating_add(64))?;
        Ok(Some(value))
    }
    fn members(&mut self, source: usize, path: &str, group: &Group) -> Result<&[String], Error> {
        let key = (source, path.to_string());
        if !self.names.contains_key(&key) {
            self.sources.charge(path.len().saturating_add(96))?;
            self.names.insert(key.clone(), self.sources.names(group)?);
        }
        Ok(self.names.get(&key).unwrap())
    }
    fn has_name(
        &mut self,
        source: usize,
        path: &str,
        group: &Group,
        name: &str,
    ) -> Result<bool, Error> {
        Ok(self.members(source, path, group)?.iter().any(|v| v == name))
    }
    fn step(&mut self) -> Result<(), Error> {
        self.steps = self
            .steps
            .checked_add(1)
            .filter(|&n| n <= self.limits.link_traversals)
            .ok_or_else(|| Error("HDF5 discovery exceeds total traversal budget".into()))?;
        Ok(())
    }
    fn object(
        &mut self,
        mut source: usize,
        path: &str,
    ) -> Result<Option<(usize, String, LocationType)>, Error> {
        if path.is_empty()
            || path.len() > self.limits.text_bytes
            || path.as_bytes().contains(&0)
            || path.split('/').any(|p| p == "..")
        {
            return Err(Error(
                "invalid or oversized HDF5 discovery object path".into(),
            ));
        }
        let parts: Vec<_> = path
            .split('/')
            .filter(|p| !p.is_empty() && *p != ".")
            .take(self.limits.link_traversals.saturating_add(1))
            .collect();
        if parts.len() > self.limits.link_traversals {
            return Err(Error("HDF5 discovery path exceeds traversal budget".into()));
        }
        let mut resolved = String::from("/");
        let mut group = self.sources.file(source).group("/")?;
        for (index, part) in parts.iter().enumerate() {
            self.step()?;
            if !self.has_name(source, &resolved, &group, part)? {
                return Ok(None);
            }
            let requested = format!(
                "{}{part}",
                if resolved == "/" {
                    String::from("/")
                } else {
                    format!("{resolved}/")
                }
            );
            let (next_source, next_path) = self.sources.resolve(source, &requested)?;
            if self.sources.links.len() > self.limits.link_traversals {
                return Err(Error(
                    "HDF5 discovery exceeds total link traversal budget".into(),
                ));
            }
            source = next_source;
            resolved = next_path;
            let kind = self.sources.file(source).loc_type_by_name(&resolved)?;
            if index + 1 == parts.len() {
                return Ok(Some((source, resolved, kind)));
            }
            if kind != LocationType::Group {
                return Ok(None);
            }
            group = self.sources.file(source).group(&resolved)?;
        }
        Ok(Some((source, resolved, LocationType::Group)))
    }
    fn group(
        &mut self,
        source: usize,
        path: &str,
    ) -> Result<Option<(usize, String, Group)>, Error> {
        match self.object(source, path)? {
            Some((s, p, LocationType::Group)) => {
                let group = self.sources.file(s).group(&p)?;
                Ok(Some((s, p, group)))
            }
            _ => Ok(None),
        }
    }
    fn array(&mut self, source: usize, path: &str) -> Result<bool, Error> {
        match self.object(source, path)? {
            Some((s, p, LocationType::Dataset)) => {
                Ok(matches!(self.sources.file(s).dataset(&p)?.ndim(), 2 | 3))
            }
            _ => Ok(false),
        }
    }
    fn eiger(&mut self) -> Result<bool, Error> {
        let Some((source, path, entry)) = self.group(0, "/entry")? else {
            return Ok(false);
        };
        if self.has_name(source, &path, &entry, "data")? {
            match self.object(source, &format!("{path}/data"))? {
                Some((s, p, LocationType::Group)) => {
                    let group = self.sources.file(s).group(&p)?;
                    let names: Vec<_> = self
                        .members(s, &p, &group)?
                        .iter()
                        .filter(|n| data_name(n))
                        .cloned()
                        .collect();
                    if names.len() > self.limits.datasets {
                        return Err(Error("HDF5 discovery exceeds data-array limit".into()));
                    }
                    // Dataset link contents may need an explicit FollowSnapshot policy later;
                    // metadata convention evidence does not dereference every data leaf.
                    return Ok(!names.is_empty());
                }
                Some((s, p, LocationType::Dataset)) => {
                    return Ok(matches!(self.sources.file(s).dataset(&p)?.ndim(), 2 | 3));
                }
                _ => return Ok(false),
            }
        }
        let names: Vec<_> = self
            .members(source, &path, &entry)?
            .iter()
            .filter(|n| data_name(n))
            .cloned()
            .collect();
        if names.len() > self.limits.datasets {
            return Err(Error("HDF5 discovery exceeds data-array limit".into()));
        }
        Ok(!names.is_empty())
    }
}
fn data_name(name: &str) -> bool {
    name == "data"
        || name
            .strip_prefix("data_")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
}
fn has_attr(location: &hdf5_backend::Location, name: &std::ffi::CStr) -> Result<bool, Error> {
    hdf5_backend::sync::sync(|| {
        // SAFETY: live location, NUL-terminated name; native call holds HDF5 mutex.
        let exists = unsafe { hdf5_sys::h5a::H5Aexists(location.id(), name.as_ptr()) };
        if exists < 0 {
            Err(Error("cannot inspect HDF5 discovery attribute".into()))
        } else {
            Ok(exists > 0)
        }
    })
}
