//! Explicit finite expansion of HDF5 `%b` mappings; never source discovery.
use super::{Descriptor, Error, SelectionRecord, Sources, Space, typed::Limits};
use hdf5_sys::h5s::{
    H5S_seloper_t, H5Scopy, H5Screate_simple, H5Sget_select_npoints, H5Sselect_hyperslab,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Caller-approved finite scope of one original physical VDS declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintfMapping {
    /// None selects the root/input file, including owned anonymous captures.
    pub source_file: Option<PathBuf>,
    /// Resolved absolute dataset path of the VDS that owns this mapping.
    pub dataset: String,
    pub mapping_index: usize,
    pub stored_filename: String,
    pub declared_dataset: String,
    /// Exactly this contiguous prefix of zero-based unlimited-axis blocks.
    pub blocks: usize,
}
/// Original mapping and selected policy, shared by all expanded blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintfDeclaration {
    pub source_file: PathBuf,
    pub dataset: String,
    pub mapping_index: usize,
    pub stored_filename: String,
    pub declared_dataset: String,
    pub source_selection: SelectionRecord,
    pub virtual_selection: SelectionRecord,
    /// None for literal `%%` normalization without a block substitution.
    pub plan: Option<PrintfMapping>,
    pub blocks: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintfExpansion {
    pub declaration: Arc<PrintfDeclaration>,
    pub block_index: usize,
}
#[derive(Clone, Copy)]
pub(super) struct Owner<'a> {
    pub file: &'a Path,
    pub is_root: bool,
    pub dataset: &'a str,
    pub mapping_index: usize,
}
pub(super) struct Expanded {
    pub descriptor: Descriptor,
    pub printf_expansion: Option<PrintfExpansion>,
}
#[derive(Clone, Copy, Debug)]
struct Pattern {
    literal_bytes: usize,
    substitutions: usize,
    escaped: bool,
}
fn inspect(text: &str, limit: usize) -> Result<Pattern, Error> {
    if text.is_empty() || text.len() > limit || text.as_bytes().contains(&0) {
        return Err(Error(
            "VDS printf name is empty, contains NUL or exceeds text limit".into(),
        ));
    }
    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut literal_bytes = 0usize;
    let mut substitutions = 0usize;
    let mut escaped = false;
    while cursor < bytes.len() {
        if bytes[cursor] != b'%' {
            literal_bytes += 1;
            cursor += 1;
            continue;
        }
        match bytes.get(cursor + 1) {
            Some(b'%') => {
                literal_bytes += 1;
                escaped = true;
            }
            Some(b'b') => substitutions += 1,
            _ => {
                return Err(Error(
                    "unsupported HDF5 VDS printf specifier (only %b and %% are admitted)".into(),
                ));
            }
        }
        cursor += 2;
    }
    Ok(Pattern {
        literal_bytes,
        substitutions,
        escaped,
    })
}
fn expanded_len(pattern: Pattern, index: usize, limit: usize) -> Result<usize, Error> {
    let digits = if index == 0 {
        1
    } else {
        index.ilog10() as usize + 1
    };
    pattern
        .substitutions
        .checked_mul(digits)
        .and_then(|n| n.checked_add(pattern.literal_bytes))
        .filter(|&n| n > 0 && n <= limit)
        .ok_or_else(|| Error("expanded VDS printf name exceeds text bound".into()))
}
fn name(text: &str, pattern: Pattern, index: usize, limit: usize) -> Result<String, Error> {
    let length = expanded_len(pattern, index, limit)?;
    let mut result = String::new();
    result
        .try_reserve_exact(length)
        .map_err(|e| Error(e.to_string()))?;
    let number = index.to_string();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'%' {
            at += 1;
            continue;
        }
        result.push_str(&text[start..at]);
        match bytes[at + 1] {
            b'%' => result.push('%'),
            b'b' => result.push_str(&number),
            _ => unreachable!("inspected printf pattern"),
        }
        at += 2;
        start = at;
    }
    result.push_str(&text[start..]);
    Ok(result)
}
pub(super) fn has_substitutions(
    filename: &str,
    dataset: &str,
    limits: Limits,
) -> Result<bool, Error> {
    Ok(inspect(filename, limits.text_bytes)?.substitutions > 0
        || inspect(dataset, limits.text_bytes)?.substitutions > 0)
}
pub(in crate::hdf5) fn validate_plan(plan: &PrintfMapping, limits: Limits) -> Result<(), Error> {
    if plan
        .source_file
        .as_ref()
        .is_some_and(|p| p.as_os_str().is_empty())
        || !plan.dataset.starts_with('/')
        || plan.dataset.len() > limits.text_bytes
        || plan.dataset.as_bytes().contains(&0)
        || plan.mapping_index >= limits.datasets
        || plan.blocks == 0
        || plan.blocks > limits.datasets
        || !has_substitutions(&plan.stored_filename, &plan.declared_dataset, limits)?
    {
        return Err(Error("invalid/bounded VDS printf mapping plan".into()));
    }
    Ok(())
}
/// Identity matching only; the engine retains owner source/path-alias snapshots.
pub(super) fn matches(
    plan: &PrintfMapping,
    owner: Owner<'_>,
    filename: &str,
    source_dataset: &str,
) -> Result<bool, Error> {
    let owner_matches = match &plan.source_file {
        None => owner.is_root,
        Some(path) => std::fs::canonicalize(path)? == owner.file,
    };
    Ok(owner_matches
        && plan.dataset == owner.dataset
        && plan.mapping_index == owner.mapping_index
        && plan.stored_filename == filename
        && plan.declared_dataset == source_dataset)
}

fn selection_cost(selection: &SelectionRecord) -> Result<usize, Error> {
    let elements = match selection {
        SelectionRecord::All {
            shape,
            maximum_shape,
        } => shape
            .len()
            .checked_add(maximum_shape.len())
            .and_then(|n| n.checked_mul(8)),
        SelectionRecord::Regular { .. } => Some(384),
        SelectionRecord::Irregular { blocks, .. } => blocks.len().checked_mul(96),
    };
    elements
        .and_then(|n| n.checked_add(256))
        .ok_or_else(|| Error("VDS printf selection metadata overflow".into()))
}
fn copy(space: &Space) -> Result<Space, Error> {
    let result = unsafe { H5Scopy(space.0) };
    if result < 0 {
        return Err(Error("cannot copy VDS printf selection".into()));
    }
    Ok(Space(result))
}
fn finite_block(
    shape: [u64; 3],
    start: [u64; 3],
    stride: [u64; 3],
    count: [u64; 3],
    block: [u64; 3],
) -> Result<Space, Error> {
    let result = Space(unsafe { H5Screate_simple(3, shape.as_ptr(), shape.as_ptr()) });
    if result.0 < 0
        || unsafe {
            H5Sselect_hyperslab(
                result.0,
                H5S_seloper_t::H5S_SELECT_SET,
                start.as_ptr(),
                stride.as_ptr(),
                count.as_ptr(),
                block.as_ptr(),
            )
        } < 0
    {
        return Err(Error(
            "cannot construct bounded VDS printf block selection".into(),
        ));
    }
    Ok(result)
}
/// Expand before resolving any source. The engine owns global used-plan and
/// duplicate-plan checks, recursively resolves only returned literal names, and
/// reuses its native selection projection/codec/source checks for every block.
pub(super) fn expand(
    descriptor: Descriptor,
    owner: Owner<'_>,
    plan: Option<&PrintfMapping>,
    sources: &mut Sources,
    limits: Limits,
) -> Result<Vec<Expanded>, Error> {
    let Owner {
        file: owner_file,
        is_root: _,
        dataset: owner_dataset,
        mapping_index,
    } = owner;
    let (filename, dataset, source_selection, virtual_selection, source_space, virtual_space) =
        descriptor;
    let file_pattern = inspect(&filename, limits.text_bytes)?;
    let dataset_pattern = inspect(&dataset, limits.text_bytes)?;
    let dynamic = file_pattern.substitutions > 0 || dataset_pattern.substitutions > 0;
    if !dynamic && !file_pattern.escaped && !dataset_pattern.escaped {
        if plan.is_some() {
            return Err(Error(
                "printf plan supplied for a literal VDS mapping".into(),
            ));
        }
        return Ok(vec![Expanded {
            descriptor: (
                filename,
                dataset,
                source_selection,
                virtual_selection,
                source_space,
                virtual_space,
            ),
            printf_expansion: None,
        }]);
    }
    let blocks = if dynamic {
        let plan = plan.ok_or_else(|| {
            Error("VDS printf mapping requires explicit owner-qualified block plan".into())
        })?;
        validate_plan(plan, limits)?;
        if !matches(plan, owner, &filename, &dataset)? {
            return Err(Error(
                "VDS printf plan does not exactly match its source declaration".into(),
            ));
        }
        plan.blocks
    } else {
        if plan.is_some() {
            return Err(Error(
                "printf block plan supplied for escaped literal VDS names".into(),
            ));
        }
        1
    };
    let maximum_names = expanded_len(file_pattern, blocks - 1, limits.text_bytes)?
        .checked_add(expanded_len(
            dataset_pattern,
            blocks - 1,
            limits.text_bytes,
        )?)
        .ok_or_else(|| Error("VDS printf name size overflow".into()))?;
    let selection_bytes = selection_cost(&source_selection)?
        .checked_add(selection_cost(&virtual_selection)?)
        .ok_or_else(|| Error("VDS printf selection size overflow".into()))?;
    let plan_cost = plan.map_or(0, |p| {
        p.stored_filename
            .len()
            .saturating_add(p.declared_dataset.len())
            .saturating_add(p.dataset.len())
            .saturating_add(p.source_file.as_ref().map_or(0, |v| v.as_os_str().len()))
            .saturating_add(512)
    });
    let declaration_cost = selection_bytes
        .checked_add(plan_cost)
        .ok_or_else(|| Error("VDS printf plan metadata overflow".into()))?
        .checked_add(filename.len())
        .and_then(|n| n.checked_add(dataset.len()))
        .and_then(|n| n.checked_add(owner_dataset.len()))
        .and_then(|n| n.checked_add(owner_file.as_os_str().len()))
        .and_then(|n| n.checked_add(1024))
        .ok_or_else(|| Error("VDS printf declaration size overflow".into()))?;
    let expanded_cost = maximum_names
        .checked_mul(2)
        .and_then(|n| n.checked_add(selection_bytes))
        .and_then(|n| n.checked_add(1024))
        .and_then(|n| n.checked_mul(blocks))
        .ok_or_else(|| Error("VDS printf expansion metadata overflow".into()))?;
    sources.charge(
        declaration_cost
            .checked_add(expanded_cost)
            .ok_or_else(|| Error("VDS printf aggregate metadata overflow".into()))?,
    )?;
    let declaration = Arc::new(PrintfDeclaration {
        source_file: owner_file.to_path_buf(),
        dataset: owner_dataset.to_owned(),
        mapping_index,
        stored_filename: filename.clone(),
        declared_dataset: dataset.clone(),
        source_selection: source_selection.clone(),
        virtual_selection: virtual_selection.clone(),
        plan: plan.cloned(),
        blocks,
    });
    let mut out = Vec::new();
    out.try_reserve_exact(blocks)
        .map_err(|e| Error(e.to_string()))?;
    if !dynamic {
        out.push(Expanded {
            descriptor: (
                name(&filename, file_pattern, 0, limits.text_bytes)?,
                name(&dataset, dataset_pattern, 0, limits.text_bytes)?,
                source_selection,
                virtual_selection,
                source_space,
                virtual_space,
            ),
            printf_expansion: Some(PrintfExpansion {
                declaration,
                block_index: 0,
            }),
        });
        return Ok(out);
    }
    if !super::finite::record_is_finite(&source_selection) {
        return Err(Error("VDS printf sources require finite selections".into()));
    }
    let SelectionRecord::Regular {
        shape,
        maximum_shape: _,
        start,
        stride,
        count,
        block,
    } = virtual_selection
    else {
        return Err(Error(
            "VDS printf virtual mapping requires a regular unlimited hyperslab".into(),
        ));
    };
    let unlimited: Vec<_> = (0..3).filter(|&axis| count[axis] == u64::MAX).collect();
    if unlimited.len() != 1
        || stride.contains(&0)
        || count.contains(&0)
        || block.contains(&0)
        || block.contains(&u64::MAX)
    {
        return Err(Error(
            "VDS printf requires exactly one unlimited count and finite nonempty blocks".into(),
        ));
    }
    let axis = unlimited[0];
    let mut finite_count = count;
    finite_count[axis] = 1;
    if (0..3).any(|d| count[d] > 1 && stride[d] < block[d]) {
        return Err(Error(
            "overlapping VDS printf hyperslab blocks are unsupported".into(),
        ));
    }
    let mut frozen = shape;
    for dim in 0..3 {
        let repeats = if dim == axis {
            blocks as u64
        } else {
            count[dim]
        };
        let end = (repeats - 1)
            .checked_mul(stride[dim])
            .and_then(|n| n.checked_add(start[dim]))
            .and_then(|n| n.checked_add(block[dim]))
            .ok_or_else(|| Error("VDS printf block extent overflow".into()))?;
        if dim != axis && end > shape[dim] {
            return Err(Error(
                "VDS printf fixed-axis selection exceeds source declaration".into(),
            ));
        }
        frozen[dim] = if dim == axis { end } else { shape[dim] };
    }
    let frame_pixels = frozen[1]
        .checked_mul(frozen[2])
        .filter(|&n| n > 0 && n <= limits.frame_bytes as u64)
        .ok_or_else(|| {
            Error("VDS printf output shape exceeds minimum decoded-byte limit".into())
        })?;
    if frozen[0] > limits.frames as u64
        || frozen
            .iter()
            .try_fold(1u64, |n, &d| n.checked_mul(d))
            .is_none()
        || frame_pixels == 0
    {
        return Err(Error(
            "VDS printf frozen extent exceeds shape/frame bounds".into(),
        ));
    }
    hdf5_backend::sync::sync(|| {
        for index in 0..blocks {
            let mut block_start = start;
            block_start[axis] = (index as u64)
                .checked_mul(stride[axis])
                .and_then(|n| n.checked_add(start[axis]))
                .ok_or_else(|| Error("VDS printf block offset overflow".into()))?;
            let virtual_block = finite_block(frozen, block_start, stride, finite_count, block)?;
            if !matches!(&source_selection,SelectionRecord::All{shape,..} if shape.is_empty()) {
                let a = unsafe { H5Sget_select_npoints(source_space.0) };
                let b = unsafe { H5Sget_select_npoints(virtual_block.0) };
                if a <= 0 || a != b {
                    return Err(Error(
                        "VDS printf source and complete virtual block point counts differ".into(),
                    ));
                }
            }
            let record = SelectionRecord::Regular {
                shape: frozen,
                maximum_shape: frozen,
                start: block_start,
                stride,
                count: finite_count,
                block,
            };
            out.push(Expanded {
                descriptor: (
                    name(&filename, file_pattern, index, limits.text_bytes)?,
                    name(&dataset, dataset_pattern, index, limits.text_bytes)?,
                    source_selection.clone(),
                    record,
                    copy(&source_space)?,
                    virtual_block,
                ),
                printf_expansion: Some(PrintfExpansion {
                    declaration: Arc::clone(&declaration),
                    block_index: index,
                }),
            });
        }
        Ok(())
    })?;
    Ok(out)
}

/// An approved prefix must cover every point the original pattern owns inside
/// the final node domain. Another mapping may extend that domain, but must not
/// turn an unapproved block of this pattern into apparent declared fill.
pub(super) fn validate_domain(
    declaration: &PrintfDeclaration,
    frozen_shape: [usize; 3],
    planned_points: usize,
) -> Result<(), Error> {
    if declaration.plan.is_none() {
        return Ok(());
    }
    let SelectionRecord::Regular {
        start,
        stride,
        count,
        block,
        ..
    } = &declaration.virtual_selection
    else {
        return Err(Error(
            "printf domain requires original regular selection".into(),
        ));
    };
    let mut selected = 1usize;
    for axis in 0..3 {
        if stride[axis] == 0 || block[axis] == 0 || count[axis] == 0 {
            return Err(Error("invalid original printf selection".into()));
        }
        let available = (frozen_shape[axis] as u64).saturating_sub(start[axis]);
        let points = if available == 0 {
            0
        } else {
            let repetitions = (1 + (available - 1) / stride[axis]).min(count[axis]);
            let prefix = (repetitions - 1)
                .checked_mul(stride[axis])
                .ok_or_else(|| Error("printf domain extent overflow".into()))?;
            (repetitions - 1)
                .checked_mul(block[axis])
                .and_then(|v| v.checked_add(block[axis].min(available - prefix)))
                .ok_or_else(|| Error("printf selected domain overflow".into()))?
        };
        selected = usize::try_from(points)
            .ok()
            .and_then(|p| selected.checked_mul(p))
            .ok_or_else(|| Error("printf selected domain product overflow".into()))?;
    }
    if selected != planned_points {
        return Err(Error("approved printf prefix does not cover its original mapped samples within the final frozen domain".into()));
    }
    Ok(())
}

/// Parse the native pattern grammar for bounded caller source resolution.
pub(in crate::hdf5) fn contract_name(
    text: &str,
    index: usize,
    limits: Limits,
) -> Result<(String, bool), Error> {
    let pattern = inspect(text, limits.text_bytes)?;
    Ok((
        name(text, pattern, index, limits.text_bytes)?,
        pattern.substitutions > 0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_real_hdf5_tokens_expand_once_preserving_utf8_and_escaped_literals() {
        for (pattern, index, expected) in [
            ("part-%b-%b.h5", 10, "part-10-10.h5"),
            ("%%b/%b/%%%%", 2, "%b/2/%%"),
            ("élément%%-%b", 0, "élément%-0"),
            (
                "%b",
                usize::MAX,
                if usize::BITS == 64 {
                    "18446744073709551615"
                } else {
                    "4294967295"
                },
            ),
        ] {
            let parsed = inspect(pattern, 1024).unwrap();
            assert_eq!(name(pattern, parsed, index, 1024).unwrap(), expected);
        }
        for pattern in ["%", "%0b", "%03b", "%d", "%s", "%B", "a\0b", ""] {
            assert!(inspect(pattern, 1024).is_err(), "{pattern:?}");
        }
        assert_eq!(inspect("%%b", 1024).unwrap().substitutions, 0);
        assert!(expanded_len(inspect("%b%b", 1024).unwrap(), usize::MAX, 2).is_err());
    }
    #[test]
    fn explicit_nonempty_block_scope_and_owner_identity_are_mandatory() {
        let mut plan = PrintfMapping {
            source_file: None,
            dataset: "/data".into(),
            mapping_index: 0,
            stored_filename: "part-%b.h5".into(),
            declared_dataset: "/data".into(),
            blocks: 3,
        };
        let limits = Limits::default();
        validate_plan(&plan, limits).unwrap();
        let owner = Owner {
            file: Path::new("/root-capture"),
            is_root: true,
            dataset: "/data",
            mapping_index: 0,
        };
        assert!(matches(&plan, owner, "part-%b.h5", "/data").unwrap());
        assert!(
            !matches(
                &plan,
                Owner {
                    is_root: false,
                    ..owner
                },
                "part-%b.h5",
                "/data"
            )
            .unwrap()
        );
        assert!(
            !matches(
                &plan,
                Owner {
                    dataset: "/other",
                    ..owner
                },
                "part-%b.h5",
                "/data"
            )
            .unwrap()
        );
        assert!(
            !matches(
                &plan,
                Owner {
                    mapping_index: 1,
                    ..owner
                },
                "part-%b.h5",
                "/data"
            )
            .unwrap()
        );
        assert!(!matches(&plan, owner, "other-%b.h5", "/data").unwrap());
        plan.blocks = 0;
        assert!(validate_plan(&plan, limits).is_err());
        plan.blocks = limits.datasets + 1;
        assert!(validate_plan(&plan, limits).is_err());
        plan.blocks = 3;
        plan.stored_filename = "literal%%.h5".into();
        assert!(validate_plan(&plan, limits).is_err());
    }
}
