//! Test-only bridge to the pinned FabIO suite. No Python in the library/runtime.
use dioptrin_io::{PixelBuffer, PixelType, cbf, native, tiff, wrapped};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::error::Error;

fn report(
    index: usize,
    frames: usize,
    shape: [usize; 2],
    pixels: &PixelBuffer,
    fields: serde_json::Value,
) {
    let mut hash = Sha256::new();
    macro_rules! digest { ($($variant:ident,$name:literal);*) => {
        match pixels { $(PixelBuffer::$variant(values) => {
            for v in values { hash.update(v.to_le_bytes()); } $name
        }),* }
    }; }
    // Canonical little-endian scalar bytes, including floating bit patterns.
    let dtype = digest!(U8,"uint8";U16,"uint16";U32,"uint32";U64,"uint64";
        I8,"int8";I16,"int16";I32,"int32";I64,"int64";F32,"float32";F64,"float64");
    println!(
        "{}",
        json!({"frame":index,"frames":frames,"shape":shape,"dtype":dtype,
        "sample_sha256":format!("{:x}",hash.finalize()),"fields":fields})
    );
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
        ];
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
                "eigerimage":"hdf5","limaimage":"hdf5","lambdaimage":"hdf5"
            }})
        );
        return Ok(());
    }
    if args.len() < 3 {
        return Err("usage: parity_probe FORMAT PATH [WIDTH HEIGHT OFFSET DTYPE ENDIAN]".into());
    }
    let path = &args[2];
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
        let mut buffer = FrameBuffer::default();
        for i in 0..reader.frames() {
            reader.read_frame_into(i, &mut buffer)?;
            report(
                i,
                reader.frames(),
                buffer.shape().unwrap(),
                buffer.pixels().unwrap(),
                json!(reader.fields()),
            );
        }
        return Ok(());
    }
    let limits = native::Limits::default();
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
        );
        return Ok(());
    }
    let format = match args[1].as_str() {
        "edf" => wrapped::Format::Edf,
        "cbf" => wrapped::Format::Cbf(cbf::ByteOffsetMode::Legacy32),
        "cbf64" => wrapped::Format::Cbf(cbf::ByteOffsetMode::Extended64),
        "tiff" => wrapped::Format::Tiff,
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
            wrapped::Metadata::Native(m) => json!(m.fields),
            wrapped::Metadata::Edf(m) => json!(m.fields),
            wrapped::Metadata::Cbf(m) => json!(m.binary_fields),
            wrapped::Metadata::Tiff(_) => json!({}),
        };
        report(
            i,
            reader.frames(),
            buffer.shape().unwrap(),
            buffer.pixels().unwrap(),
            fields,
        );
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
