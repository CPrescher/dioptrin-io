//! Translate Python source choices and requirements; Rust owns all resolution.
use super::read_error;
use dioptrin_io::{
    discovery::Codec,
    hdf5::{
        contracts::{FileChoice, FileKind, Requirement},
        discovery, typed,
    },
    wrapped,
};
use pyo3::{exceptions::PyValueError, prelude::*, types::PyDict};
use std::path::PathBuf;

#[derive(FromPyObject)]
struct Choice {
    owner: PathBuf,
    stored_filename: String,
    actual_file: PathBuf,
}

#[derive(FromPyObject)]
struct Mapping {
    source_file: Option<PathBuf>,
    dataset: String,
    mapping_index: usize,
    stored_filename: String,
    declared_dataset: String,
    blocks: usize,
}

#[derive(FromPyObject)]
pub struct Sources {
    follow_external_links: bool,
    declared_fill: bool,
    file_choices: Vec<Choice>,
    printf_mappings: Vec<Mapping>,
}

fn requirements_error(requirements: &[Requirement]) -> PyResult<PyErr> {
    Python::attach(|py| {
        let mut items = Vec::with_capacity(requirements.len());
        for requirement in requirements {
            let item = PyDict::new(py);
            match requirement {
                Requirement::File {
                    kind,
                    owner,
                    stored_filename,
                    expected_file,
                } => {
                    item.set_item(
                        "kind",
                        match kind {
                            FileKind::Virtual => "virtual",
                            FileKind::ExternalRaw => "external_raw",
                        },
                    )?;
                    item.set_item("owner", owner)?;
                    item.set_item("stored_filename", stored_filename)?;
                    item.set_item("expected_file", expected_file)?;
                }
                Requirement::Printf {
                    owner,
                    dataset,
                    mapping_index,
                    stored_filename,
                    declared_dataset,
                } => {
                    item.set_item("kind", "printf")?;
                    item.set_item("owner", owner)?;
                    item.set_item("dataset", dataset)?;
                    item.set_item("mapping_index", mapping_index)?;
                    item.set_item("stored_filename", stored_filename)?;
                    item.set_item("declared_dataset", declared_dataset)?;
                }
            }
            items.push(item);
        }
        let exc = py
            .import("dioptrin_io._hdf5")?
            .getattr("Hdf5SourcesRequired")?;
        Ok(PyErr::from_value(exc.call1((items,))?))
    })
}

impl Sources {
    pub fn open(self, input: wrapped::Input, dataset: Option<&str>) -> PyResult<wrapped::Reader> {
        let options = typed::Options {
            external_links: if self.follow_external_links {
                typed::ExternalLinks::FollowSnapshot
            } else {
                typed::ExternalLinks::Reject
            },
            ..Default::default()
        };
        let selection = match dataset {
            Some(path) => typed::Selection::Dataset(path),
            None => {
                let path = input.source_path().map_err(read_error)?;
                let origin = input.provenance().map_err(read_error)?.origin_directory;
                match discovery::discover_snapshot(path, options, origin.as_deref())
                    .map_err(read_error)?
                {
                    Codec::Eiger => typed::Selection::Eiger,
                    Codec::Lima => typed::Selection::Lima,
                    Codec::Lambda => typed::Selection::Lambda,
                    _ => return Err(PyValueError::new_err("select an HDF5 image with dataset=")),
                }
            }
        };
        let choices: Vec<_> = self
            .file_choices
            .into_iter()
            .map(|c| FileChoice {
                owner: c.owner,
                stored_filename: c.stored_filename,
                actual_file: c.actual_file,
            })
            .collect();
        let plans: Vec<_> = self
            .printf_mappings
            .into_iter()
            .map(|p| typed::PrintfMapping {
                source_file: p.source_file,
                dataset: p.dataset,
                mapping_index: p.mapping_index,
                stored_filename: p.stored_filename,
                declared_dataset: p.declared_dataset,
                blocks: p.blocks,
            })
            .collect();
        let contracts = input
            .hdf5_source_contracts(selection, options, &choices, &plans)
            .map_err(read_error)?;
        if !contracts.requirements().is_empty() {
            return Err(requirements_error(contracts.requirements())?);
        }
        let fill = if self.declared_fill {
            typed::UnmappedRegions::DeclaredFill
        } else {
            typed::UnmappedRegions::Reject
        };
        input
            .read_hdf5_with_contracts(selection, options, &contracts, fill)
            .map_err(read_error)
    }
}
