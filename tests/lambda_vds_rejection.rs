#![cfg(feature = "hdf5")]
use dioptrin_io::hdf5::typed::{ExternalLinks, Options, Reader, Selection};
use std::path::Path;
#[test]
fn lambda_vds_requires_separate_source_mapping_policy_even_with_local_basename() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/lambda-vds-rejection");
    assert!(directory.join("module.h5").is_file());
    for name in ["missing-absolute.nxs", "present-relative.nxs"] {
        for external_links in [ExternalLinks::Reject, ExternalLinks::FollowSnapshot] {
            let error = Reader::open_snapshot(
                directory.join(name),
                Selection::Lambda,
                Options {
                    external_links,
                    ..Options::default()
                },
            )
            .err()
            .expect("VDS cannot inherit an external-link policy")
            .to_string();
            assert!(error.contains("virtual datasets"), "{name}: {error}");
        }
    }
}
