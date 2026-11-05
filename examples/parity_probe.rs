//! Test-only bridge to the pinned FabIO suite. No Python in the library/runtime.
use dioptrin_io::{PixelBuffer, PixelType, cbf, native, tiff, wrapped};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::error::Error;

fn frame_record(
    index: usize,
    frames: usize,
    shape: impl Into<Vec<usize>>,
    pixels: &PixelBuffer,
    fields: serde_json::Value,
    sources: &[&std::path::Path],
) -> serde_json::Value {
    let shape = shape.into();
    let mut hash = Sha256::new();
    macro_rules! digest { ($($variant:ident,$name:literal);*) => {
        match pixels { $(PixelBuffer::$variant(values) => {
            for v in values { hash.update(v.to_le_bytes()); } $name
        }),*,_=>unreachable!() }
    }; }
    // Canonical little-endian scalar bytes, including floating bit patterns.
    let dtype = match pixels {
        PixelBuffer::C32(v) => {
            for pair in v {
                for x in pair {
                    hash.update(x.to_le_bytes());
                }
            }
            "complex64"
        }
        PixelBuffer::C64(v) => {
            for pair in v {
                for x in pair {
                    hash.update(x.to_le_bytes());
                }
            }
            "complex128"
        }
        PixelBuffer::CI16(v) => {
            for pair in v {
                for x in pair {
                    hash.update(x.to_le_bytes());
                }
            }
            "complex_int16"
        }
        _ => {
            digest!(U8,"uint8";U16,"uint16";U32,"uint32";U64,"uint64";I8,"int8";I16,"int16";I32,"int32";I64,"int64";F32,"float32";F64,"float64")
        }
    };
    json!({"frame":index,"frames":frames,"shape":shape,"dtype":dtype,
        "sample_sha256":format!("{:x}",hash.finalize()),"fields":fields,"sources":sources})
}
fn report(
    index: usize,
    frames: usize,
    shape: impl Into<Vec<usize>>,
    pixels: &PixelBuffer,
    fields: serde_json::Value,
    sources: &[&std::path::Path],
) {
    println!(
        "{}",
        frame_record(index, frames, shape, pixels, fields, sources)
    );
}

fn detector_fields(header: &dioptrin_io::detector_tiff::Header) -> serde_json::Value {
    use dioptrin_io::detector_tiff::{Header, Value};
    match header {
        Header::Pilatus(h) => json!(h.fields),
        Header::MarCcd(h) => serde_json::Value::Object(
            h.fields
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        match v {
                            Value::Unsigned(v) => json!(v),
                            Value::Signed(v) => json!(v),
                            Value::UnsignedArray(v) => json!(v),
                            Value::SignedArray(v) => json!(v),
                            Value::ShortArray(v) => json!(v),
                            Value::Bytes(v) => json!(v),
                        },
                    )
                })
                .collect(),
        ),
    }
}

fn tiff_view_record(
    index: usize,
    frames: usize,
    image: &tiff::view::Image,
    sources: &[&std::path::Path],
) -> serde_json::Value {
    let shape = image.provenance.shape.clone();
    match &image.pixels {
        tiff::view::Pixels::Scalar(pixels) => {
            frame_record(index, frames, shape, pixels, json!({}), sources)
        }
        tiff::view::Pixels::Bool(values) => {
            let mut hash = Sha256::new();
            for &value in values {
                hash.update([u8::from(value)]);
            }
            json!({"frame":index,"frames":frames,"shape":shape,"dtype":"bool","sample_sha256":format!("{:x}",hash.finalize()),"fields":{},"sources":sources})
        }
    }
}
fn tiff_view_policy(image: &tiff::view::Image) -> serde_json::Value {
    let p = &image.provenance;
    json!({"name":format!("{:?}",p.policy),"kind":"consumer_display_view",
        "operations":p.operations.iter().map(|operation|format!("{operation:?}")).collect::<Vec<_>>(),
        "source_shape":p.source_shape,"source_type":format!("{:?}",p.source_type),"source_bits_per_sample":p.source_bits_per_sample,
        "source_photometric":p.source_photometric,"source_planar_configuration":p.source_planar_configuration,
        "source_orientation":p.source_orientation,"source_compression":p.source_compression,"source_lossy":p.source_lossy,
        "output_type":format!("{:?}",p.output_type),"output_shape":p.shape})
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--capabilities") {
        let mut features = vec!["native", "edf", "cbf", "tiff", "wrappers"];
        let mut adapters = vec![
            "npy",
            "dtrek",
            "adsc",
            "mask",
            "hipic",
            "pnm",
            "spreadsheet",
            "raw",
            "edf",
            "cbf",
            "cbf64",
            "tiff",
            "tiff-pillow13",
            "tiff-tiffio",
            "marccd",
            "pilatus",
        ];
        for (enabled, feature, names) in [
            (cfg!(feature = "sparse"), "sparse", &["sparse"][..]),
            (cfg!(feature = "mar345"), "mar345", &["mar345"][..]),
            (cfg!(feature = "esperanto"), "esperanto", &["esperanto"][..]),
            (cfg!(feature = "fit2d"), "fit2d", &["fit2d"][..]),
            (cfg!(feature = "kcd"), "kcd", &["kcd"][..]),
            (cfg!(feature = "mpa"), "mpa", &["mpa"][..]),
            (cfg!(feature = "fixed"), "fixed", &["ge", "pixi"][..]),
            (cfg!(feature = "jpeg"), "jpeg", &["jpeg"][..]),
            (cfg!(feature = "jpeg2000"), "jpeg2000", &["jpeg2000"][..]),
            (
                cfg!(feature = "containers"),
                "containers",
                &["spe", "mrc", "dm3"][..],
            ),
            (
                cfg!(feature = "bruker"),
                "bruker",
                &["bruker86", "bruker86-view", "bruker100"][..],
            ),
            (cfg!(feature = "oxford"), "oxford", &["oxford"][..]),
            (cfg!(feature = "raxis"), "raxis", &["raxis"][..]),
            (cfg!(feature = "xsd"), "xsd", &["xsd"][..]),
        ] {
            if enabled {
                features.push(feature);
                adapters.extend(names);
            }
        }
        if cfg!(feature = "hdf5") {
            features.push("hdf5");
            adapters.extend(["hdf5", "eiger", "lima", "lambda"]);
        }
        println!(
            "{}",
            json!({"features":features,"adapters":adapters,"known_readers":{
                "numpyimage":"native","dtrekimage":"native","adscimage":"native","fit2dmaskimage":"native",
                "HiPiCimage":"native","pnmimage":"native","binaryimage":"native","fit2dspreadsheetimage":"native",
                "edfimage":"edf","cbfimage":"cbf","tifimage":"tiff","marccdimage":"tiff","pilatusimage":"tiff","hdf5image":"hdf5",
                "eigerimage":"hdf5","limaimage":"hdf5","lambdaimage":"hdf5",
                "speimage":"containers","mrcimage":"containers","dm3image":"containers",
                "brukerimage":"bruker","bruker100image":"bruker","OXDimage":"oxford","raxisimage":"raxis","xsdimage":"xsd",
                "GEimage":"fixed","pixiimage":"fixed","jpegimage":"jpeg","jpeg2kimage":"jpeg2000",
                "fit2dimage":"fit2d","kcdimage":"kcd","mpaimage":"mpa","esperantoimage":"esperanto","mar345image":"mar345","sparseimage":"sparse"
            }})
        );
        return Ok(());
    }
    if args.len() < 3 {
        return Err("usage: parity_probe FORMAT PATH [WIDTH HEIGHT OFFSET DTYPE ENDIAN]".into());
    }
    let path = &args[2];
    #[cfg(feature = "sparse")]
    if args[1] == "sparse" {
        use dioptrin_io::sparse::{
            ArrayBuffer, ExternalLinks, FrameBuffer, Options, Reader, Reconstruction,
            SparseFrameBuffer,
        };
        let reader = Reader::open(
            path,
            Options {
                external_links: ExternalLinks::FollowSnapshot,
                reconstruction: Reconstruction::DeterministicBackground,
                ..Default::default()
            },
        )?;
        let paths: Vec<_> = reader
            .sources()
            .iter()
            .map(|source| source.path.as_path())
            .collect();
        let mut components = Vec::new();
        let mut component = ArrayBuffer::default();
        for source in &reader.metadata().components {
            if source.pixel_type.is_some() {
                reader.read_component_into(&source.name, &mut component)?;
                let mut info = frame_record(
                    0,
                    1,
                    source.shape.clone(),
                    component.pixels().unwrap(),
                    json!({}),
                    &paths,
                );
                info["name"] = json!(source.name);
                info["dataset"] = json!(source.dataset);
                info["source_file"] = json!(source.source_file);
                info["source_little_endian"] = json!(source.little_endian);
                components.push(info);
            }
        }
        let mut events = SparseFrameBuffer::default();
        let mut dense = FrameBuffer::default();
        for index in 0..reader.frames() {
            reader.read_sparse_frame_into(index, &mut events)?;
            reader.read_frame_into(index, &mut dense)?;
            let count = events.index().unwrap().len();
            let mut row = frame_record(
                index,
                reader.frames(),
                reader.metadata().shape,
                dense.pixels().unwrap(),
                json!({}),
                &paths,
            );
            row["reconstruction_policy"] = json!({"name":"FabIO2026.6.NativeDense.NOISYFalse","kind":"deterministic_sparse_background","dense_is_derived":reader.metadata().dense_is_derived,"stochastic_noise":false});
            row["stored_sparse"] = json!({"events":count,"source_range":events.source_range(),
                "index":frame_record(index,reader.frames(),vec![count],events.index().unwrap(),json!({}),&paths),
                "intensity":frame_record(index,reader.frames(),vec![count],events.intensity().unwrap(),json!({}),&paths)});
            if index == 0 {
                row["source_components"] = json!(components);
            }
            println!("{row}");
        }
        return Ok(());
    }
    #[cfg(feature = "hdf5")]
    if matches!(args[1].as_str(), "hdf5" | "eiger" | "lima" | "lambda") {
        use dioptrin_io::hdf5::typed::{ExternalLinks, FrameBuffer, Options, Reader, Selection};
        let (file, selection) = match args[1].as_str() {
            "hdf5" => {
                let (f, d) = path
                    .split_once("::")
                    .ok_or("HDF5 input requires file::dataset")?;
                (f, Selection::Dataset(d))
            }
            "eiger" => (path.as_str(), Selection::Eiger),
            "lima" => (path.as_str(), Selection::Lima),
            _ => (path.as_str(), Selection::Lambda),
        };
        let reader = Reader::open_snapshot(
            file,
            selection,
            Options {
                external_links: ExternalLinks::FollowSnapshot,
                ..Options::default()
            },
        )?;
        let paths: Vec<_> = reader
            .sources()
            .iter()
            .map(|source| source.path.as_path())
            .collect();
        let mut buffer = FrameBuffer::default();
        for i in 0..reader.frames() {
            reader.read_frame_into(i, &mut buffer)?;
            report(
                i,
                reader.frames(),
                buffer.shape().unwrap(),
                buffer.pixels().unwrap(),
                json!(reader.fields()),
                &paths,
            );
        }
        return Ok(());
    }
    let limits = native::Limits::default();
    #[cfg(feature = "bruker")]
    if args[1] == "bruker86-view" {
        let mut reader = wrapped::Reader::open(
            path,
            wrapped::Format::Bruker,
            wrapped::Limits::default(),
            limits,
            tiff::Limits::default(),
        )?;
        let mut stored = wrapped::FrameBuffer::default();
        reader.read_frame_into(0, &mut stored)?;
        let wrapped::Metadata::Bruker(meta) = reader.metadata(0).unwrap() else {
            unreachable!()
        };
        let mut row = frame_record(
            0,
            1,
            stored.sample_shape().unwrap(),
            stored.pixels().unwrap(),
            json!(meta.fields),
            &reader.source_paths(),
        );
        if meta
            .linear
            .is_some_and(|[slope, offset]| slope != 1.0 || offset != 0.0)
        {
            let mut decoded_reader = wrapped::Reader::open(
                path,
                wrapped::Format::BrukerLinear,
                wrapped::Limits::default(),
                limits,
                tiff::Limits::default(),
            )?;
            let mut decoded = wrapped::FrameBuffer::default();
            decoded_reader.read_frame_into(0, &mut decoded)?;
            row["reference_view"] = frame_record(
                0,
                1,
                decoded.sample_shape().unwrap(),
                decoded.pixels().unwrap(),
                json!({}),
                &decoded_reader.source_paths(),
            );
            row["view_policy"] = json!({"name":"Bruker86.LINEAR.FabIO2026.6","kind":"encoded_float_view","coefficients":meta.linear,"stored_dtype":format!("{:?}",meta.pixel_type)});
        }
        println!("{row}");
        return Ok(());
    }
    if args[1] == "raw" {
        if args.len() != 8 {
            return Err("raw input needs WIDTH HEIGHT OFFSET DTYPE ENDIAN".into());
        }
        let ty = match args[6].as_str() {
            "uint8" => PixelType::U8,
            "uint16" => PixelType::U16,
            "uint32" => PixelType::U32,
            "uint64" => PixelType::U64,
            "int8" => PixelType::I8,
            "int16" => PixelType::I16,
            "int32" => PixelType::I32,
            "int64" => PixelType::I64,
            "float32" => PixelType::F32,
            "float64" => PixelType::F64,
            _ => return Err("unsupported raw dtype".into()),
        };
        let offset: i64 = args[5].parse()?;
        let offset = if offset >= 0 {
            native::Offset::Absolute(offset as u64)
        } else {
            native::Offset::FromEnd {
                trailing_bytes: offset.unsigned_abs() - 1,
            }
        };
        let little_endian = match args[7].as_str() {
            "<" => true,
            ">" => false,
            "=" => cfg!(target_endian = "little"),
            _ => return Err("invalid raw byte order".into()),
        };
        let reader = native::Reader::open_raw(
            path,
            native::RawLayout {
                shape: [args[4].parse()?, args[3].parse()?],
                pixel_type: ty,
                little_endian,
                offset,
            },
            limits,
        )?;
        let mut buffer = native::FrameBuffer::default();
        reader.read_frame_into(0, &mut buffer)?;
        report(
            0,
            1,
            buffer.shape().unwrap(),
            buffer.pixels().unwrap(),
            json!({}),
            &[],
        );
        return Ok(());
    }
    let format = match args[1].as_str() {
        #[cfg(feature = "mar345")]
        "mar345" => wrapped::Format::Mar345,
        #[cfg(feature = "esperanto")]
        "esperanto" => wrapped::Format::Esperanto,
        #[cfg(feature = "fit2d")]
        "fit2d" => wrapped::Format::Fit2d,
        #[cfg(feature = "kcd")]
        "kcd" => wrapped::Format::Kcd(dioptrin_io::kcd::Interpretation::SumReadouts),
        #[cfg(feature = "mpa")]
        "mpa" => wrapped::Format::Mpa,
        "marccd" => wrapped::Format::DetectorTiff(dioptrin_io::detector_tiff::Format::MarCcd),
        "pilatus" => wrapped::Format::DetectorTiff(dioptrin_io::detector_tiff::Format::Pilatus),
        #[cfg(feature = "fixed")]
        "ge" => wrapped::Format::Ge,
        #[cfg(feature = "fixed")]
        "pixi" => wrapped::Format::Pixi,
        #[cfg(feature = "jpeg")]
        "jpeg" => wrapped::Format::Jpeg(Default::default()),
        #[cfg(feature = "jpeg2000")]
        "jpeg2000" => wrapped::Format::Jpeg2000(Default::default()),
        #[cfg(feature = "containers")]
        "spe" => wrapped::Format::Spe,
        #[cfg(feature = "containers")]
        "mrc" => wrapped::Format::Mrc,
        #[cfg(feature = "containers")]
        "dm3" => wrapped::Format::Dm3,
        #[cfg(feature = "bruker")]
        "bruker86" | "bruker100" => wrapped::Format::Bruker,
        #[cfg(feature = "oxford")]
        "oxford" => wrapped::Format::Oxford,
        #[cfg(feature = "raxis")]
        "raxis" => {
            wrapped::Format::Raxis(dioptrin_io::raxis::Interpretation::PhotomultiplierExpanded)
        }
        #[cfg(feature = "xsd")]
        "xsd" => wrapped::Format::Xsd,
        "edf" => wrapped::Format::EdfExternal {
            policy: dioptrin_io::edf::ExternalPayloads::FollowCompressedSnapshot,
            basenames: true,
        },
        "cbf" => wrapped::Format::Cbf(cbf::ByteOffsetMode::Legacy32),
        "cbf64" => wrapped::Format::Cbf(cbf::ByteOffsetMode::Extended64),
        "tiff" | "tiff-pillow13" | "tiff-tiffio" => wrapped::Format::Tiff,
        name => wrapped::Format::Native(match name {
            "npy" => native::Format::Npy,
            "dtrek" | "adsc" => native::Format::Dtrek,
            "mask" => native::Format::Fit2dMask,
            "hipic" => native::Format::HiPic,
            "pnm" => native::Format::Pnm,
            "spreadsheet" => native::Format::Fit2dSpreadsheet,
            _ => return Err(format!("reader not implemented: {name}").into()),
        }),
    };
    let mut reader = wrapped::Reader::open(
        path,
        format,
        wrapped::Limits::default(),
        limits,
        tiff::Limits::default(),
    )?;
    let mut buffer = wrapped::FrameBuffer::default();
    for i in 0..reader.frames() {
        reader.read_frame_into(i, &mut buffer)?;
        let fields = match reader.metadata(i).unwrap() {
            #[cfg(feature = "mar345")]
            wrapped::Metadata::Mar345(m) => json!(m.fields),
            #[cfg(feature = "esperanto")]
            wrapped::Metadata::Esperanto(m) => json!(m.fields),
            #[cfg(feature = "fit2d")]
            wrapped::Metadata::Fit2d(_) => json!({}),
            #[cfg(feature = "kcd")]
            wrapped::Metadata::Kcd(_) => json!({}),
            #[cfg(feature = "mpa")]
            wrapped::Metadata::Mpa(_) => json!({}),
            wrapped::Metadata::DetectorTiff(m) => detector_fields(m.header),
            #[cfg(feature = "fixed")]
            wrapped::Metadata::Ge(_) | wrapped::Metadata::Pixi(_) => json!({}),
            #[cfg(feature = "jpeg")]
            wrapped::Metadata::Jpeg(_) => json!({}),
            #[cfg(feature = "jpeg2000")]
            wrapped::Metadata::Jpeg2000(_) => json!({}),
            #[cfg(feature = "containers")]
            wrapped::Metadata::Spe(m) => json!(m.fields),
            #[cfg(feature = "containers")]
            wrapped::Metadata::Mrc(_) | wrapped::Metadata::Dm3(_) => json!({}),
            #[cfg(feature = "bruker")]
            wrapped::Metadata::Bruker(m) => json!(m.fields),
            #[cfg(feature = "oxford")]
            wrapped::Metadata::Oxford(_) => json!({}),
            #[cfg(feature = "raxis")]
            wrapped::Metadata::Raxis(_) => json!({}),
            #[cfg(feature = "xsd")]
            wrapped::Metadata::Xsd(_) => json!({}),
            wrapped::Metadata::Native(m) => json!(m.fields),
            wrapped::Metadata::Edf(m) => json!(m.fields),
            wrapped::Metadata::Cbf(m) => json!(m.binary_fields),
            wrapped::Metadata::Tiff(_) => json!({}),
        };
        let mut row = frame_record(
            i,
            reader.frames(),
            buffer.sample_shape().unwrap(),
            buffer.pixels().unwrap(),
            fields,
            &reader.source_paths(),
        );
        if args[1] == "tiff-pillow13" || args[1] == "tiff-tiffio" {
            let policy = if args[1] == "tiff-pillow13" {
                tiff::view::Policy::Pillow13
            } else {
                tiff::view::Policy::FabioTiffIo
            };
            match reader.read_tiff_view(i, policy, limits.frame_bytes) {
                Ok(image) => {
                    row["reference_view"] =
                        tiff_view_record(i, reader.frames(), &image, &reader.source_paths());
                    row["view_policy"] = tiff_view_policy(&image);
                }
                Err(error) => row["view_error"] = json!(error.to_string()),
            }
            if let wrapped::Metadata::Tiff(metadata) = reader.metadata(i).unwrap() {
                row["opaque_metadata"]=json!(metadata.opaque_tags.iter().map(|(id,tag)|json!({"id":id,"field_type":tag.field_type,"count":tag.count,"ifd_entry":tag.ifd_entry,"value_bytes":tag.value_bytes,"reason":tag.reason})).collect::<Vec<_>>());
            }
        }
        println!("{row}");
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
