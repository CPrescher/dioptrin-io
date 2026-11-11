#![cfg(feature = "hdf5")]
use dioptrin_io::{
    PixelBuffer,
    hdf5::{
        contracts::{FileChoice, FileKind, Requirement, SourceContracts},
        typed::{self, Options, Selection, UnmappedRegions},
    },
};
use std::path::{Path, PathBuf};
fn fixture(group: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(group)
        .join(name)
}
fn read(path: &Path, dataset: &str, contracts: &SourceContracts) -> typed::Reader {
    contracts.require_complete().unwrap();
    typed::Reader::open_snapshot_with_storage(
        path,
        Selection::Dataset(dataset),
        Options::default(),
        Some(contracts.vds_options(UnmappedRegions::DeclaredFill)),
        Some(contracts.external_raw_options()),
        None,
    )
    .unwrap()
}
fn hex(pixels: &PixelBuffer) -> String {
    let bytes: Vec<u8> = match pixels {
        PixelBuffer::U64(v) => v.iter().flat_map(|v| v.to_le_bytes()).collect(),
        PixelBuffer::F16(v) => v.iter().flat_map(|v| v.to_le_bytes()).collect(),
        PixelBuffer::Bool(v) => v.clone(),
        PixelBuffer::C32(v) => v
            .iter()
            .flatten()
            .flat_map(|v| v.to_bits().to_le_bytes())
            .collect(),
        PixelBuffer::C64(v) => v
            .iter()
            .flatten()
            .flat_map(|v| v.to_bits().to_le_bytes())
            .collect(),
        other => panic!("unqualified test type: {other:?}"),
    };
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
#[test]
fn exact_missing_absolute_names_need_choices_and_never_search_basenames() {
    let path = fixture("vds", "finite.h5");
    let missing = SourceContracts::resolve_snapshot(
        &path,
        Selection::Dataset("/data"),
        Options::default(),
        &[],
        &[],
        None,
    )
    .unwrap();
    assert_eq!(missing.requirements().len(), 2);
    assert!(missing.require_complete().is_err());
    assert!(missing.requirements().iter().all(|r|matches!(r, Requirement::File { kind:FileKind::Virtual, expected_file, .. } if expected_file.starts_with("/missing/acquisition"))));
    let choices: Vec<_> = ["module-a.h5", "module-b.h5"]
        .into_iter()
        .map(|n| FileChoice {
            owner: path.clone(),
            stored_filename: format!("/missing/acquisition/{n}"),
            actual_file: fixture("vds", n),
        })
        .collect();
    let resolved = SourceContracts::resolve_snapshot(
        &path,
        Selection::Dataset("/data"),
        Options::default(),
        &choices,
        &[],
        None,
    )
    .unwrap();
    assert!(resolved.requirements().is_empty());
    let reader = read(&path, "/data", &resolved);
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("vds", "provenance.json")).unwrap()).unwrap();
    assert_eq!(
        reader.frames(),
        expected["expected_i16"].as_array().unwrap().len()
    );
    for (index, frame) in expected["expected_i16"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let expected: Vec<i16> = frame
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r.as_array().unwrap())
            .map(|v| v.as_i64().unwrap() as i16)
            .collect();
        let mut b = typed::FrameBuffer::default();
        reader.read_frame_into(index, &mut b).unwrap();
        assert!(matches!(b.pixels(),Some(PixelBuffer::I16(v)) if *v == expected));
    }
    let duplicated = vec![choices[0].clone(), choices[0].clone()];
    assert!(
        SourceContracts::resolve_snapshot(
            &path,
            Selection::Dataset("/data"),
            Options::default(),
            &duplicated,
            &[],
            None
        )
        .is_err()
    );
}
#[test]
fn nested_contracts_match_independent_original_words_and_detect_cycles() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("vds-nested", "provenance.json")).unwrap())
            .unwrap();
    let mut frames = 0;
    for (name, case) in manifest["cases"].as_object().unwrap() {
        let path = fixture("vds-nested", case["master"].as_str().unwrap());
        let dataset = case["dataset"].as_str().unwrap();
        let contracts = SourceContracts::resolve_snapshot(
            &path,
            Selection::Dataset(dataset),
            Options::default(),
            &[],
            &[],
            None,
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let reader = read(&path, dataset, &contracts);
        for (index, expected) in case["frame_hex"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .rev()
        {
            let mut b = typed::FrameBuffer::default();
            reader.read_frame_into(index, &mut b).unwrap();
            assert_eq!(
                hex(b.pixels().unwrap()),
                expected.as_str().unwrap(),
                "{name}:{index}"
            );
            frames += 1;
        }
    }
    assert!(frames >= 20);
    let cycle = SourceContracts::resolve_snapshot(
        fixture("vds-nested", "cycle.h5"),
        Selection::Dataset("/data"),
        Options::default(),
        &[],
        &[],
        None,
    );
    assert!(cycle.err().unwrap().to_string().contains("cycle"));
}
#[test]
fn printf_requires_explicit_finite_scope_and_preserves_every_declared_block() {
    let path = fixture("vds-printf", "files-u64.h5");
    let selection = Selection::Dataset("/data");
    let pending =
        SourceContracts::resolve_snapshot(&path, selection, Options::default(), &[], &[], None)
            .unwrap();
    assert!(matches!(
        pending.requirements(),
        [Requirement::Printf { .. }]
    ));
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("vds-printf", "provenance.json")).unwrap())
            .unwrap();
    let case = &manifest["cases"]["files-u64"];
    let plans: Vec<_> = case["printf_mappings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| typed::PrintfMapping {
            source_file: None,
            dataset: p["dataset"].as_str().unwrap().into(),
            mapping_index: p["mapping_index"].as_u64().unwrap() as usize,
            stored_filename: p["stored_filename"].as_str().unwrap().into(),
            declared_dataset: p["declared_dataset"].as_str().unwrap().into(),
            blocks: p["blocks"].as_u64().unwrap() as usize,
        })
        .collect();
    let contracts =
        SourceContracts::resolve_snapshot(&path, selection, Options::default(), &[], &plans, None)
            .unwrap();
    let reader = read(&path, "/data", &contracts);
    for (index, expected) in case["frame_hex"].as_array().unwrap().iter().enumerate() {
        let mut b = typed::FrameBuffer::default();
        reader.read_frame_into(index, &mut b).unwrap();
        assert_eq!(hex(b.pixels().unwrap()), expected.as_str().unwrap());
    }
}
#[test]
fn raw_windows_and_source_mutation_are_retained_by_wrapped_reader() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["external.h5", "external-a.bin", "external-b.bin"] {
        std::fs::copy(fixture("hdf5-layouts", name), temp.path().join(name)).unwrap();
    }
    let path = temp.path().join("external.h5");
    let contracts = SourceContracts::resolve_snapshot(
        &path,
        Selection::Dataset("/data"),
        Options::default(),
        &[],
        &[],
        None,
    )
    .unwrap();
    assert_eq!(contracts.external_raw_filenames().len(), 2);
    contracts.require_complete().unwrap();
    let reader = read(&path, "/data", &contracts);
    let mut b = typed::FrameBuffer::default();
    reader.read_frame_into(0, &mut b).unwrap();
    assert!(matches!(b.pixels(), Some(PixelBuffer::C32(_))));
    #[cfg(feature = "wrappers")]
    {
        let input = dioptrin_io::wrapped::Input::open(&path, Default::default()).unwrap();
        let mut wrapped = input
            .read_hdf5_with_contracts(
                Selection::Dataset("/data"),
                Options::default(),
                &contracts,
                UnmappedRegions::Reject,
            )
            .unwrap();
        let mut w = dioptrin_io::wrapped::FrameBuffer::default();
        wrapped.read_frame_into(0, &mut w).unwrap();
        assert_eq!(hex(w.pixels().unwrap()), hex(b.pixels().unwrap()));
    }
    std::fs::remove_file(temp.path().join("external-b.bin")).unwrap();
    assert!(contracts.check_sources().is_err());
    assert!(reader.read_frame_into(0, &mut b).is_err());
}
#[test]
fn catalog_retains_exact_aliases_and_does_not_resolve_vds_source_names() {
    let path = fixture("vds", "finite.h5");
    let paths = dioptrin_io::hdf5::catalog::image_paths(path, Options::default(), None).unwrap();
    assert_eq!(paths, vec!["/data", "/entry/instrument/detector/data"]);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("master.h5");
    let f = hdf5_backend::File::create(&path).unwrap();
    f.new_dataset::<u16>().shape((2, 3)).create("data").unwrap();
    f.link_soft("/data", "alias").unwrap();
    f.link_hard("/", "cycle").unwrap();
    f.flush().unwrap();
    assert_eq!(
        dioptrin_io::hdf5::catalog::image_paths(path, Options::default(), None).unwrap(),
        vec!["/alias", "/data"]
    );
}

#[cfg(feature = "wrappers")]
#[test]
fn captured_inputs_use_only_the_explicit_origin_and_guard_contract_ownership() {
    use dioptrin_io::wrapped::{FrameBuffer, Input};
    let path = fixture("vds-nested", "grand.h5");
    let bytes = std::fs::read(&path).unwrap();
    let selection = Selection::Dataset("/f16");
    let input = Input::from_bytes(&bytes, None, Default::default()).unwrap();
    assert!(
        input
            .hdf5_source_contracts(selection, Options::default(), &[], &[])
            .err()
            .unwrap()
            .to_string()
            .contains("origin directory")
    );
    let input = Input::from_bytes(&bytes, path.parent(), Default::default()).unwrap();
    let contracts = input
        .hdf5_source_contracts(selection, Options::default(), &[], &[])
        .unwrap();
    contracts.require_complete().unwrap();
    let wrong = Input::open(&path, Default::default()).unwrap();
    assert!(
        wrong
            .read_hdf5_with_contracts(
                selection,
                Options::default(),
                &contracts,
                UnmappedRegions::DeclaredFill
            )
            .err()
            .unwrap()
            .to_string()
            .contains("different input")
    );
    let mut reader = input
        .read_hdf5_with_contracts(
            selection,
            Options::default(),
            &contracts,
            UnmappedRegions::DeclaredFill,
        )
        .unwrap();
    let mut b = FrameBuffer::default();
    reader.read_frame_into(0, &mut b).unwrap();
    assert!(matches!(b.pixels(), Some(PixelBuffer::F16(_))));
}
