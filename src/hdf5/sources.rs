//! Bounded explicit HDF5 link resolution and immutable source manifests.
use super::{
    Error,
    typed::{ExternalLinks, Limits},
};
use crate::source::Snapshot;
use hdf5_backend::{File, Group};
use hdf5_sys::{
    h5::{H5_index_t, H5_iter_order_t},
    h5l::{H5L_info2_t, H5L_type_t, H5Lget_info2, H5Lget_name_by_idx, H5Lget_val},
    h5p::H5P_DEFAULT,
};
use std::{
    ffi::CString,
    fs::File as OsFile,
    mem::MaybeUninit,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Source {
    pub path: PathBuf,
    pub bytes: u64,
}
#[derive(Debug, Clone)]
pub struct Link {
    pub source: PathBuf,
    pub path: String,
    pub external: bool,
    /// Native HDF5 link value, including flags and NUL-delimited strings.
    pub raw_value: Vec<u8>,
}
struct RawTracked {
    snapshot: Snapshot,
    handle: OsFile,
}
struct Tracked {
    snapshot: Snapshot,
    handle: OsFile,
    h5: File,
}
pub(super) struct Sources {
    tracked: Vec<Tracked>,
    raw: Vec<RawTracked>,
    pub records: Vec<Source>,
    pub links: Vec<Link>,
    limits: Limits,
    policy: ExternalLinks,
    metadata: usize,
    pub bindings: Vec<crate::SourceBinding>,
    root_origin: Option<PathBuf>,
}
impl Sources {
    pub fn new_with_origin(
        path: impl AsRef<Path>,
        limits: Limits,
        policy: ExternalLinks,
        origin: Option<&Path>,
    ) -> Result<Self, Error> {
        let origin = origin.map(std::fs::canonicalize).transpose()?;
        if origin.as_ref().is_some_and(|p| !p.is_dir()) {
            return Err(Error("HDF5 root origin must be a directory".into()));
        }
        let mut sources = Self {
            tracked: Vec::new(),
            raw: Vec::new(),
            records: Vec::new(),
            links: Vec::new(),
            limits,
            policy,
            metadata: 0,
            bindings: Vec::new(),
            root_origin: origin,
        };
        sources.add(path)?;
        Ok(sources)
    }
    pub fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.metadata = self
            .metadata
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.metadata_bytes)
            .ok_or_else(|| Error("HDF5 snapshot metadata exceeds budget".into()))?;
        Ok(())
    }
    pub(super) fn add(&mut self, path: impl AsRef<Path>) -> Result<usize, Error> {
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if !self.bindings.contains(&snapshot.binding) {
            if self.bindings.len()
                >= self
                    .limits
                    .source_files
                    .saturating_add(self.limits.link_traversals)
            {
                return Err(Error("HDF5 source-binding limit".into()));
            }
            self.charge(
                snapshot
                    .binding
                    .requested
                    .as_os_str()
                    .len()
                    .saturating_add(snapshot.path.as_os_str().len())
                    .saturating_add(128),
            )?;
            self.bindings.push(snapshot.binding.clone());
        }
        if !std::fs::metadata(&snapshot.path)?.is_file() {
            return Err(Error("HDF5 snapshots require regular files".into()));
        }
        if let Some(index) = self
            .tracked
            .iter()
            .position(|s| s.snapshot.path == snapshot.path)
        {
            self.tracked[index]
                .snapshot
                .check_handle(&self.tracked[index].handle)
                .map_err(Error)?;
            return Ok(index);
        }
        if self.tracked.len().saturating_add(self.raw.len()) >= self.limits.source_files {
            return Err(Error("HDF5 snapshot exceeds source-file limit".into()));
        }
        self.charge(
            snapshot
                .path
                .as_os_str()
                .len()
                .saturating_mul(2)
                .saturating_add(256),
        )?;
        let handle = OsFile::open(&snapshot.path)?;
        snapshot.check_handle(&handle).map_err(Error)?;
        let h5 = File::open(&snapshot.path)?;
        if std::fs::canonicalize(h5.filename())? != snapshot.path {
            return Err(Error("HDF5 opened an unexpected source file".into()));
        }
        snapshot.check_handle(&handle).map_err(Error)?;
        self.records.push(Source {
            path: snapshot.path.clone(),
            bytes: snapshot.bytes(),
        });
        self.tracked.push(Tracked {
            snapshot,
            handle,
            h5,
        });
        Ok(self.tracked.len() - 1)
    }
    /// Track an explicitly bound external raw file without opening it as HDF5.
    pub(super) fn add_raw(&mut self, path: &Path) -> Result<usize, Error> {
        let snapshot = Snapshot::open(path).map_err(Error)?;
        if !std::fs::metadata(&snapshot.path)?.is_file() {
            return Err(Error(
                "HDF5 raw external source requires a regular file".into(),
            ));
        }
        if !self.bindings.contains(&snapshot.binding) {
            if self.bindings.len()
                >= self
                    .limits
                    .source_files
                    .saturating_add(self.limits.link_traversals)
            {
                return Err(Error("HDF5 source-binding limit".into()));
            }
            self.charge(
                snapshot
                    .binding
                    .requested
                    .as_os_str()
                    .len()
                    .saturating_add(snapshot.path.as_os_str().len())
                    .saturating_add(128),
            )?;
            self.bindings.push(snapshot.binding.clone());
        }
        if let Some(index) = self
            .raw
            .iter()
            .position(|s| s.snapshot.path == snapshot.path)
        {
            self.raw[index]
                .snapshot
                .check_handle(&self.raw[index].handle)
                .map_err(Error)?;
            return Ok(index);
        }
        if self.tracked.len().saturating_add(self.raw.len()) >= self.limits.source_files {
            return Err(Error("HDF5 snapshot exceeds source-file limit".into()));
        }
        self.charge(
            snapshot
                .path
                .as_os_str()
                .len()
                .saturating_mul(2)
                .saturating_add(256),
        )?;
        let handle = OsFile::open(&snapshot.path)?;
        snapshot.check_handle(&handle).map_err(Error)?;
        self.records.push(Source {
            path: snapshot.path.clone(),
            bytes: snapshot.bytes(),
        });
        self.raw.push(RawTracked { snapshot, handle });
        Ok(self.raw.len() - 1)
    }
    pub(super) fn raw_file(&self, index: usize) -> &OsFile {
        &self.raw[index].handle
    }
    pub(super) fn raw_path(&self, index: usize) -> &Path {
        &self.raw[index].snapshot.path
    }
    pub(super) fn raw_bytes(&self, index: usize) -> u64 {
        self.raw[index].snapshot.bytes()
    }
    pub fn check(&self) -> Result<(), Error> {
        for binding in &self.bindings {
            binding.check().map_err(Error)?;
        }
        for source in &self.tracked {
            source
                .snapshot
                .check_handle(&source.handle)
                .map_err(Error)?;
        }
        for source in &self.raw {
            source
                .snapshot
                .check_handle(&source.handle)
                .map_err(Error)?;
        }
        // Link controls may change in an already-open native file before any
        // filesystem timestamp changes. Recheck bounded original bytes too.
        hdf5_backend::sync::sync(|| {
            for link in &self.links {
                let file = self
                    .tracked
                    .iter()
                    .find(|s| s.snapshot.path == link.source)
                    .ok_or_else(|| Error("HDF5 link source missing from snapshot".into()))?;
                let (parent, name) = hard_link_parent(&file.h5, &link.path)?;
                let path = CString::new(name)
                    .map_err(|_| Error("NUL in tracked HDF5 link path".into()))?;
                let mut info = MaybeUninit::<H5L_info2_t>::uninit();
                if unsafe {
                    H5Lget_info2(parent.id(), path.as_ptr(), info.as_mut_ptr(), H5P_DEFAULT)
                } < 0
                {
                    return Err(Error(
                        "HDF5 source link disappeared; reopen snapshot".into(),
                    ));
                }
                let kind = unsafe {
                    std::ptr::addr_of!((*info.as_ptr()).type_)
                        .cast::<std::ffi::c_int>()
                        .read()
                };
                let expected = if link.external {
                    H5L_type_t::H5L_TYPE_EXTERNAL as i32
                } else {
                    H5L_type_t::H5L_TYPE_SOFT as i32
                };
                if kind != expected {
                    return Err(Error(
                        "HDF5 source link kind changed; reopen snapshot".into(),
                    ));
                }
                let bytes =
                    unsafe { *(*std::ptr::addr_of_mut!((*info.as_mut_ptr()).u)).val_size() };
                if bytes != link.raw_value.len() {
                    return Err(Error(
                        "HDF5 source link value changed; reopen snapshot".into(),
                    ));
                }
                let mut raw = Vec::new();
                raw.try_reserve_exact(bytes)
                    .map_err(|e| Error(e.to_string()))?;
                raw.resize(bytes, 0);
                if unsafe {
                    H5Lget_val(
                        parent.id(),
                        path.as_ptr(),
                        raw.as_mut_ptr().cast(),
                        bytes,
                        H5P_DEFAULT,
                    )
                } < 0
                    || raw != link.raw_value
                {
                    return Err(Error(
                        "HDF5 source link value changed; reopen snapshot".into(),
                    ));
                }
            }
            Ok(())
        })?;
        Ok(())
    }
    pub fn file(&self, index: usize) -> &File {
        &self.tracked[index].h5
    }
    pub fn path(&self, index: usize) -> &Path {
        &self.tracked[index].snapshot.path
    }
    /// Follow every component ourselves so native external-file search paths
    /// cannot substitute sources and intermediate link files cannot go untracked.
    pub fn resolve(&mut self, mut source: usize, path: &str) -> Result<(usize, String), Error> {
        if path.len() > self.limits.metadata_bytes {
            return Err(Error("HDF5 link path exceeds metadata limit".into()));
        }
        let mut path = normalize(path)?;
        let mut traversals = 0usize;
        'restart: loop {
            if path.len() > self.limits.metadata_bytes {
                return Err(Error("HDF5 link path exceeds metadata limit".into()));
            }
            let components: Vec<_> = path
                .split('/')
                .filter(|s| !s.is_empty())
                .take(self.limits.link_traversals.saturating_add(1))
                .collect();
            if components.len() > self.limits.link_traversals {
                return Err(Error("HDF5 path exceeds traversal limit".into()));
            }
            let mut parent = String::from("/");
            for (position, name) in components.iter().enumerate() {
                traversals += 1;
                if traversals > self.limits.link_traversals {
                    return Err(Error("HDF5 link cycle or traversal limit exceeded".into()));
                }
                let group = self.file(source).group(&parent)?;
                let cname =
                    CString::new(*name).map_err(|_| Error("NUL in HDF5 link path".into()))?;
                let (kind, raw) = hdf5_backend::sync::sync(|| -> Result<_, Error> {
                    let mut info = MaybeUninit::<H5L_info2_t>::uninit();
                    // SAFETY: live group ID, NUL-terminated component, writable
                    // native info struct; all HDF5 calls hold the upstream mutex.
                    if unsafe {
                        H5Lget_info2(group.id(), cname.as_ptr(), info.as_mut_ptr(), H5P_DEFAULT)
                    } < 0
                    {
                        return Err(Error(format!(
                            "missing/unreadable HDF5 link {parent}{name}"
                        )));
                    }
                    // SAFETY: native success initialized this field. Read its C
                    // integer representation before constructing a Rust enum,
                    // because user-defined link IDs need not be enum variants.
                    let kind = unsafe {
                        std::ptr::addr_of!((*info.as_ptr()).type_)
                            .cast::<std::ffi::c_int>()
                            .read()
                    };
                    if kind == H5L_type_t::H5L_TYPE_HARD as i32 {
                        return Ok((kind, Vec::new()));
                    }
                    if kind != H5L_type_t::H5L_TYPE_SOFT as i32
                        && kind != H5L_type_t::H5L_TYPE_EXTERNAL as i32
                    {
                        return Err(Error("unsupported user-defined HDF5 link".into()));
                    }
                    // SAFETY: the link type selects val_size in the native union.
                    let bytes =
                        unsafe { *(*std::ptr::addr_of_mut!((*info.as_mut_ptr()).u)).val_size() };
                    if bytes == 0 || bytes > self.limits.metadata_bytes {
                        return Err(Error("HDF5 link value exceeds metadata limit".into()));
                    }
                    let mut raw = Vec::new();
                    raw.try_reserve_exact(bytes)
                        .map_err(|e| Error(e.to_string()))?;
                    raw.resize(bytes, 0);
                    // SAFETY: the native value length was checked and this buffer
                    // has exactly that many writable bytes.
                    if unsafe {
                        H5Lget_val(
                            group.id(),
                            cname.as_ptr(),
                            raw.as_mut_ptr().cast(),
                            bytes,
                            H5P_DEFAULT,
                        )
                    } < 0
                    {
                        return Err(Error("cannot read HDF5 link value".into()));
                    }
                    Ok((kind, raw))
                })?;
                if kind == H5L_type_t::H5L_TYPE_HARD as i32 {
                    parent.push_str(name);
                    parent.push('/');
                    continue;
                }
                let external = kind == H5L_type_t::H5L_TYPE_EXTERNAL as i32;
                if external && self.policy == ExternalLinks::Reject {
                    return Err(Error(
                        "external HDF5 link requires explicit FollowSnapshot policy".into(),
                    ));
                }
                self.charge(
                    raw.len()
                        .saturating_mul(2)
                        .saturating_add(parent.len())
                        .saturating_add(name.len())
                        .saturating_add(256),
                )?;
                self.links.push(Link {
                    source: self.path(source).to_path_buf(),
                    path: format!("{parent}{name}"),
                    external,
                    raw_value: raw.clone(),
                });
                let target = if external {
                    // HDF5's version-0 external-link value is one flags byte,
                    // a filename and an object path, each NUL terminated. Bound
                    // every scan in Rust; malformed strings never reach strlen.
                    if raw[0] != 0 {
                        return Err(Error(
                            "unsupported HDF5 external-link encoding version".into(),
                        ));
                    }
                    let mut strings = raw[1..].split(|&b| b == 0);
                    let filename = text(strings.next().unwrap_or_default())?;
                    let target = text(strings.next().unwrap_or_default())?.to_owned();
                    if filename.is_empty()
                        || target.is_empty()
                        || strings.next() != Some(&[][..])
                        || strings.next().is_some()
                    {
                        return Err(Error("malformed HDF5 external-link value".into()));
                    }
                    let file_path = Path::new(filename);
                    let file_path = if file_path.is_absolute() {
                        file_path.to_path_buf()
                    } else {
                        let base = if source == 0 {
                            self.root_origin
                                .as_deref()
                                .unwrap_or_else(|| self.path(source).parent().unwrap())
                        } else {
                            self.path(source).parent().unwrap()
                        };
                        base.join(file_path)
                    };
                    source = self.add(file_path)?;
                    target
                } else {
                    let value = raw
                        .strip_suffix(&[0])
                        .ok_or_else(|| Error("unterminated HDF5 soft link".into()))?;
                    let target = text(value)?;
                    if target.starts_with('/') {
                        target.into()
                    } else {
                        format!("{parent}{target}")
                    }
                };
                path = normalize(&format!(
                    "{target}/{}",
                    components[position + 1..].join("/")
                ))?;
                continue 'restart;
            }
            return Ok((source, path));
        }
    }
    pub fn group(&mut self, source: usize, path: &str) -> Result<(usize, String, Group), Error> {
        let (source, path) = self.resolve(source, path)?;
        let group = self.file(source).group(&path)?;
        Ok((source, path, group))
    }
    pub fn names(&mut self, group: &Group) -> Result<Vec<String>, Error> {
        let count = group.info()?.nlinks;
        if count > self.limits.group_members as u64 {
            return Err(Error("HDF5 group exceeds member limit".into()));
        }
        let mut names = Vec::new();
        for index in 0..count {
            let name = hdf5_backend::sync::sync(|| -> Result<String, Error> {
                // SAFETY: live group, current-group name, finite valid index.
                // The first call asks only for the required byte count.
                let size = unsafe {
                    H5Lget_name_by_idx(
                        group.id(),
                        c".".as_ptr(),
                        H5_index_t::H5_INDEX_NAME,
                        H5_iter_order_t::H5_ITER_INC,
                        index,
                        std::ptr::null_mut(),
                        0,
                        H5P_DEFAULT,
                    )
                };
                let size = usize::try_from(size)
                    .map_err(|_| Error("cannot read HDF5 member name length".into()))?;
                self.charge(size.saturating_mul(2).saturating_add(64))?;
                let mut bytes = vec![
                    0u8;
                    size.checked_add(1).ok_or_else(|| Error(
                        "HDF5 member name size overflow".into()
                    ))?
                ];
                // SAFETY: exact admitted output length plus the trailing NUL.
                let actual = unsafe {
                    H5Lget_name_by_idx(
                        group.id(),
                        c".".as_ptr(),
                        H5_index_t::H5_INDEX_NAME,
                        H5_iter_order_t::H5_ITER_INC,
                        index,
                        bytes.as_mut_ptr().cast(),
                        bytes.len(),
                        H5P_DEFAULT,
                    )
                };
                if actual < 0 || actual as usize != size {
                    return Err(Error("HDF5 member name changed during snapshot".into()));
                }
                bytes.truncate(size);
                String::from_utf8(bytes)
                    .map_err(|_| Error("non-UTF8 HDF5 member names are unsupported".into()))
            })?;
            names.push(name);
        }
        Ok(names)
    }
}
fn normalize(path: &str) -> Result<String, Error> {
    if path.is_empty() {
        return Err(Error("empty HDF5 object path".into()));
    }
    if path.as_bytes().contains(&0) || path.split('/').any(|p| p == "..") {
        return Err(Error(
            "unsupported NUL/parent component in HDF5 object path".into(),
        ));
    }
    let mut result = String::new();
    for component in path.split('/').filter(|p| !p.is_empty() && *p != ".") {
        result.push('/');
        result.push_str(component);
    }
    if result.is_empty() {
        result.push('/');
    }
    Ok(result)
}
fn text(bytes: &[u8]) -> Result<&str, Error> {
    if bytes.contains(&0) {
        return Err(Error("embedded NUL in HDF5 link string".into()));
    }
    std::str::from_utf8(bytes)
        .map_err(|_| Error("non-UTF8 HDF5 link strings are unsupported".into()))
}

/// Revalidate a previously resolved concrete path without ever following a
/// newly introduced soft/external component through native HDF5 lookup.
pub(super) fn hard_dataset(file: &File, path: &str) -> Result<hdf5_backend::Dataset, Error> {
    hdf5_backend::sync::sync(|| {
        if !path.starts_with('/') {
            return Err(Error("resolved HDF5 dataset path must be absolute".into()));
        }
        let mut components = path.split('/').filter(|part| !part.is_empty()).peekable();
        let mut group = file.group("/")?;
        while let Some(name) = components.next() {
            let cname =
                CString::new(name).map_err(|_| Error("NUL in resolved HDF5 component".into()))?;
            let mut info = MaybeUninit::<H5L_info2_t>::uninit();
            if unsafe { H5Lget_info2(group.id(), cname.as_ptr(), info.as_mut_ptr(), H5P_DEFAULT) }
                < 0
            {
                return Err(Error(
                    "resolved HDF5 path disappeared; reopen snapshot".into(),
                ));
            }
            let kind = unsafe {
                std::ptr::addr_of!((*info.as_ptr()).type_)
                    .cast::<std::ffi::c_int>()
                    .read()
            };
            if kind != H5L_type_t::H5L_TYPE_HARD as i32 {
                return Err(Error(
                    "resolved HDF5 path now contains a non-hard link; reopen snapshot".into(),
                ));
            }
            if components.peek().is_none() {
                return Ok(group.dataset(name)?);
            }
            group = group.group(name)?;
        }
        Err(Error("resolved HDF5 dataset path is empty".into()))
    })
}

// Every ancestor was hard at original resolution. Rechecking components before
// opening each next group prevents newly introduced native external traversal.
fn hard_link_parent<'a>(file: &File, path: &'a str) -> Result<(Group, &'a str), Error> {
    let (parent, name) = path
        .rsplit_once('/')
        .filter(|(_, name)| !name.is_empty())
        .ok_or_else(|| Error("invalid tracked HDF5 link path".into()))?;
    if !path.starts_with('/') {
        return Err(Error("tracked HDF5 link must be absolute".into()));
    }
    let mut group = file.group("/")?;
    for component in parent.split('/').filter(|p| !p.is_empty()) {
        let name =
            CString::new(component).map_err(|_| Error("NUL in tracked HDF5 parent".into()))?;
        let mut info = MaybeUninit::<H5L_info2_t>::uninit();
        if unsafe { H5Lget_info2(group.id(), name.as_ptr(), info.as_mut_ptr(), H5P_DEFAULT) } < 0 {
            return Err(Error("tracked HDF5 link parent disappeared".into()));
        }
        let kind = unsafe {
            std::ptr::addr_of!((*info.as_ptr()).type_)
                .cast::<std::ffi::c_int>()
                .read()
        };
        if kind != H5L_type_t::H5L_TYPE_HARD as i32 {
            return Err(Error(
                "tracked HDF5 link parent now contains non-hard traversal".into(),
            ));
        }
        group = group.group(component)?;
    }
    Ok((group, name))
}
