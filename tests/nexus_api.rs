#![cfg(feature = "hdf5")]
mod common;
use dioptrin_io::{
    PixelBuffer,
    hdf5::{
        attributes::AttributeValue,
        nexus::Reader,
        typed::{self, ExternalLinks, Options},
    },
};
use hdf5_backend::{File, types::VarLenUnicode};
use std::str::FromStr;
fn text(group: &hdf5_backend::Location, key: &str, value: &str) {
    group
        .new_attr::<VarLenUnicode>()
        .shape(())
        .create(key)
        .unwrap()
        .write_scalar(&VarLenUnicode::from_str(value).unwrap())
        .unwrap();
}
fn fixture(path: &std::path::Path) {
    let file = File::create(path).unwrap();
    text(&file, "default", "latest");
    for (name, time) in [
        ("old", "2020-01-01T00:00:00Z"),
        ("latest", "2026-01-01T00:00:00+02:00"),
    ] {
        let entry = file.create_group(name).unwrap();
        text(&entry, "NX_class", "NXentry");
        entry
            .new_dataset::<VarLenUnicode>()
            .shape(())
            .create("start_time")
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str(time).unwrap())
            .unwrap();
        let instrument = entry.create_group("instrument").unwrap();
        text(&instrument, "NX_class", "NXinstrument");
        let detector = instrument.create_group("detector").unwrap();
        text(&detector, "NX_class", "NXdetector");
        let data = detector
            .new_dataset::<u64>()
            .shape([1, 3])
            .create("counts")
            .unwrap();
        data.write_raw(&[u64::MAX, 1u64 << 63, (1u64 << 53) + 1])
            .unwrap();
        data.new_attr::<u64>()
            .shape([2])
            .create("reserved")
            .unwrap()
            .write_raw(&[u64::MAX, (1u64 << 53) + 1])
            .unwrap();
        text(&data, "role", "signal");
        data.new_attr::<u8>()
            .shape(())
            .create("signal")
            .unwrap()
            .write_scalar(&1)
            .unwrap();
        let nxdata = entry.create_group("plot").unwrap();
        text(&nxdata, "NX_class", "NXdata");
        text(&entry, "default", "plot");
    }
    file.flush().unwrap();
}
#[test]
fn owned_entries_detectors_defaults_and_explicit_dataset_selection_preserve_u64() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nexus.h5");
    fixture(&path);
    let reader = Reader::open_snapshot(&path, Options::default()).unwrap();
    let entries = reader.get_entries().unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|e| e.object.requested_path.as_str())
            .collect::<Vec<_>>(),
        ["/latest", "/old"]
    );
    assert!(entries[0].ordering_qualification.is_some());
    assert!(reader.get_entry("latest").unwrap().is_some());
    assert!(reader.get_entry("missing").unwrap().is_none());
    assert_eq!(reader.find_detector(true).unwrap().len(), 2);
    assert_eq!(
        reader.get_default_nxdata().unwrap().unwrap().requested_path,
        "/latest/plot"
    );
    let data = reader
        .get_data("/latest/instrument/detector", None)
        .unwrap();
    assert_eq!(data.len(), 1);
    let filter = AttributeValue::Numeric {
        shape: vec![],
        pixel_type: dioptrin_io::PixelType::I64,
        little_endian: true,
        values: PixelBuffer::I64(vec![1]),
    };
    assert_eq!(
        reader
            .get_data("/latest/instrument/detector", Some(("signal", &filter)))
            .unwrap()
            .len(),
        1
    );
    let attr = reader
        .get_attr("/latest/instrument/detector/counts", "reserved")
        .unwrap()
        .unwrap();
    let AttributeValue::Numeric {
        values: PixelBuffer::U64(values),
        ..
    } = attr
    else {
        panic!()
    };
    assert_eq!(values, [u64::MAX, (1u64 << 53) + 1]);
    let image = reader.open_image(&data[0]).unwrap();
    let mut buffer = typed::FrameBuffer::default();
    image.read_frame_into(0, &mut buffer).unwrap();
    assert_eq!(
        buffer.pixels(),
        Some(&PixelBuffer::U64(vec![
            u64::MAX,
            1u64 << 63,
            (1u64 << 53) + 1
        ]))
    );
    let mut forged = data[0].clone();
    forged.source_file = dir.path().join("other.h5");
    assert!(reader.open_image(&forged).is_err());
}
#[test]
fn mutable_metadata_invalidates_all_queries_and_active_image_views() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nexus.h5");
    fixture(&path);
    let writer = File::open_rw(&path).unwrap();
    let reader = Reader::open_snapshot(&path, Options::default()).unwrap();
    let data = reader
        .get_data("/latest/instrument/detector", None)
        .unwrap();
    let view = reader.open_image(&data[0]).unwrap();
    let mut buffer = typed::FrameBuffer::default();
    view.read_frame_into(0, &mut buffer).unwrap();
    writer
        .group("old")
        .unwrap()
        .attr("NX_class")
        .unwrap()
        .write_scalar(&VarLenUnicode::from_str("changed").unwrap())
        .unwrap();
    assert!(reader.get_entries().is_err());
    assert!(view.read_frame_into(0, &mut buffer).is_err());
    assert!(buffer.pixels().is_none());
}
#[test]
fn traversal_depth_and_resource_limits_are_admitted_before_recursion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deep.h5");
    {
        let file = File::create(&path).unwrap();
        let mut group = file.group("/").unwrap();
        for _ in 0..140 {
            group = group.create_group("a").unwrap();
        }
        file.flush().unwrap();
    }
    let options = Options {
        limits: typed::Limits {
            link_traversals: 512,
            ..typed::Limits::default()
        },
        ..Options::default()
    };
    assert!(Reader::open_snapshot(&path, options).is_err());
    let path = dir.path().join("small.h5");
    fixture(&path);
    for limits in [
        typed::Limits {
            metadata_bytes: 256,
            ..typed::Limits::default()
        },
        typed::Limits {
            datasets: 1,
            ..typed::Limits::default()
        },
        typed::Limits {
            group_members: 1,
            ..typed::Limits::default()
        },
    ] {
        assert!(
            Reader::open_snapshot(
                &path,
                Options {
                    limits,
                    ..Options::default()
                }
            )
            .is_err()
        );
    }
}
#[test]
fn external_graph_requires_policy_and_tracks_every_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.h5");
    fixture(&target);
    let master = dir.path().join("master.h5");
    {
        let file = File::create(&master).unwrap();
        file.link_external("target.h5", "/latest", "entry").unwrap();
        file.flush().unwrap();
    }
    assert!(Reader::open_snapshot(&master, Options::default()).is_err());
    let reader = Reader::open_snapshot(
        &master,
        Options {
            external_links: ExternalLinks::FollowSnapshot,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(reader.sources().len(), 2);
    assert!(reader.get_entry("entry").unwrap().is_some());
    let changed = common::mutate_source(&target, || std::fs::write(&target, b"changed"));
    assert_eq!(reader.get_entries().is_err(), changed);
}

#[test]
fn growing_dataset_and_malformed_time_have_explicit_scopes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("growing.h5");
    fixture(&path);
    let writer = File::open_rw(&path).unwrap();
    let growing = writer
        .new_dataset::<u64>()
        .shape((1..,))
        .chunk((1,))
        .create("growing")
        .unwrap();
    writer.flush().unwrap();
    let reader = Reader::open_snapshot(&path, Options::default()).unwrap();
    growing.resize((2,)).unwrap();
    assert!(reader.get_entries().is_err());
    drop(reader);
    drop(growing);
    drop(writer);
    let path = dir.path().join("badtime.h5");
    fixture(&path);
    {
        let f = File::open_rw(&path).unwrap();
        f.dataset("old/start_time")
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str("not-a-time").unwrap())
            .unwrap();
        f.flush().unwrap();
    }
    let reader = Reader::open_snapshot(&path, Options::default()).unwrap();
    assert!(reader.get_entry("latest").unwrap().is_some());
    assert!(reader.get_entry("old").unwrap().is_some());
    assert!(reader.get_entries().is_err());
}
#[test]
fn repeated_alias_results_are_bounded_before_attribute_cloning() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aliases.h5");
    {
        let file = File::create(&path).unwrap();
        let entry = file.create_group("entry").unwrap();
        text(&entry, "NX_class", "NXentry");
        text(&entry, "large", &"x".repeat(32000));
        entry
            .new_dataset::<VarLenUnicode>()
            .shape(())
            .create("start_time")
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str("2026-01-01T00:00:00").unwrap())
            .unwrap();
        for index in 0..8 {
            file.link_hard("entry", &format!("alias{index}")).unwrap();
        }
        file.flush().unwrap();
    }
    let options = Options {
        limits: typed::Limits {
            metadata_bytes: 160000,
            ..typed::Limits::default()
        },
        ..Options::default()
    };
    let reader = Reader::open_snapshot(&path, options).unwrap();
    assert!(reader.get_entry("entry").unwrap().is_some());
    assert!(reader.get_entries().is_err());
}

#[test]
fn tracked_creation_order_is_preserved_for_equal_entry_times() {
    use hdf5_backend::plist::group_create::LinkCreationOrder;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("order.h5");
    {
        let file = File::with_options()
            .with_fcpl(|p| p.link_creation_order(LinkCreationOrder::Tracked))
            .create(&path)
            .unwrap();
        for name in ["z_first", "a_second"] {
            let entry = file.create_group(name).unwrap();
            text(&entry, "NX_class", "NXentry");
            entry
                .new_dataset::<VarLenUnicode>()
                .shape(())
                .create("start_time")
                .unwrap()
                .write_scalar(&VarLenUnicode::from_str("2026-01-01T00:00:00").unwrap())
                .unwrap();
        }
        file.flush().unwrap();
    }
    let reader = Reader::open_snapshot(&path, Options::default()).unwrap();
    assert_eq!(
        reader
            .get_entries()
            .unwrap()
            .iter()
            .map(|e| e.object.requested_path.as_str())
            .collect::<Vec<_>>(),
        ["/z_first", "/a_second"]
    );
}

#[test]
fn unrelated_start_time_arrays_are_not_timestamp_controls() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("generic.h5");
    fixture(&path);
    {
        let file = File::open_rw(&path).unwrap();
        let unrelated = file.create_group("unrelated").unwrap();
        unrelated
            .new_dataset::<u64>()
            .shape([2])
            .create("start_time")
            .unwrap()
            .write_raw(&[1, 2])
            .unwrap();
        let text_group = file.create_group("other").unwrap();
        text_group
            .new_dataset::<VarLenUnicode>()
            .shape([2])
            .create("start_time")
            .unwrap()
            .write_raw(&[
                VarLenUnicode::from_str("not").unwrap(),
                VarLenUnicode::from_str("a clock").unwrap(),
            ])
            .unwrap();
        file.flush().unwrap();
    }
    let reader = Reader::open_snapshot(&path, Options::default()).unwrap();
    assert!(reader.get_entry("latest").unwrap().is_some());
    assert_eq!(reader.get_entries().unwrap().len(), 2);
    assert_eq!(
        reader.get_data("/unrelated", None).unwrap()[0].shape,
        Some(vec![2])
    );
}
