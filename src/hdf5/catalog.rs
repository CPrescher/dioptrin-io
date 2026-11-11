//! Bounded scalar image candidates, retaining each master-file dataset alias.
//! Admission and pixel reads remain with the typed reader. Virtual candidates
//! are listed without querying HDF5's conditional VDS extent or source search.
use super::{Error, sources::Sources, typed};
use hdf5_backend::{LocationToken, LocationType, dataset::Layout};

/// Enumerate 2D/3D ordinary datasets and virtual candidates through the exact
/// native link policy. Group cycles are skipped; dataset aliases are retained.
/// # Errors
/// Broken links, source mutation or metadata/traversal limits.
pub fn image_paths(
    path: impl AsRef<std::path::Path>,
    options: typed::Options,
    origin: Option<&std::path::Path>,
) -> Result<Vec<String>, Error> {
    typed::validate_limits(options.limits)?;
    let mut scan = Scan {
        sources: Sources::new_with_origin(path, options.limits, options.external_links, origin)?,
        limits: options.limits,
        ancestors: vec![],
        paths: vec![],
        visits: 0,
    };
    scan.visit(0, "/", "/", 0)?;
    scan.sources.check()?;
    scan.paths.sort();
    scan.paths.dedup();
    Ok(scan.paths)
}
struct Scan {
    sources: Sources,
    limits: typed::Limits,
    ancestors: Vec<(usize, LocationToken)>,
    paths: Vec<String>,
    visits: usize,
}
impl Scan {
    fn visit(&mut self, source: usize, path: &str, alias: &str, depth: usize) -> Result<(), Error> {
        self.visits += 1;
        if depth >= 128
            || self.visits > self.limits.group_members
            || alias.len() > self.limits.text_bytes
        {
            return Err(Error("HDF5 image catalog traversal limit".into()));
        }
        let (source, path) = self.sources.resolve(source, path)?;
        let file = self.sources.file(source);
        let info = file.loc_info_by_name(&path)?;
        match info.loc_type {
            LocationType::Dataset => {
                let dataset = file.dataset(&path)?;
                if dataset.layout() == Layout::Virtual || matches!(dataset.ndim(), 2 | 3) {
                    if self.paths.len() >= self.limits.datasets {
                        return Err(Error("HDF5 image catalog dataset limit".into()));
                    }
                    self.sources.charge(alias.len().saturating_add(64))?;
                    self.paths.push(alias.into());
                }
            }
            LocationType::Group => {
                if self.ancestors.contains(&(source, info.token)) {
                    return Ok(());
                }
                self.ancestors.push((source, info.token));
                let group = file.group(&path)?;
                for name in self.sources.names(&group)? {
                    self.visit(
                        source,
                        &format!("{}/{name}", path.trim_end_matches('/')),
                        &format!("{}/{name}", alias.trim_end_matches('/')),
                        depth + 1,
                    )?;
                }
                self.ancestors.pop();
            }
            _ => {}
        }
        Ok(())
    }
}
