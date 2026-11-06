//! Test-only series/navigation/companion comparison entry points.
use dioptrin_io::{
    PixelBuffer, native,
    navigation::{Cursor, Position},
    series, wrapped,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, path::Path};
fn pixels(p: &PixelBuffer) -> Value {
    let mut hash = Sha256::new();
    macro_rules! digest {($($variant:ident,$name:literal);*)=>{match p {$(PixelBuffer::$variant(values)=>{for v in values {hash.update(v.to_le_bytes());}$name}),*,_=>unreachable!()}};}
    let dtype = match p {
        PixelBuffer::C32(v) => {
            for p in v {
                for v in p {
                    hash.update(v.to_le_bytes());
                }
            }
            "complex64"
        }
        PixelBuffer::C64(v) => {
            for p in v {
                for v in p {
                    hash.update(v.to_le_bytes());
                }
            }
            "complex128"
        }
        PixelBuffer::CI16(v) => {
            for p in v {
                for v in p {
                    hash.update(v.to_le_bytes());
                }
            }
            "complex_int16"
        }
        _ => {
            digest!(U8,"uint8";U16,"uint16";U32,"uint32";U64,"uint64";I8,"int8";I16,"int16";I32,"int32";I64,"int64";F32,"float32";F64,"float64")
        }
    };
    json!({"dtype":dtype,"sample_sha256":format!("{:x}",hash.finalize())})
}
fn fields(m: wrapped::Metadata<'_>) -> Value {
    match m {
        wrapped::Metadata::Edf(m) => json!(m.fields),
        wrapped::Metadata::Native(m) => json!(m.fields),
        wrapped::Metadata::Cbf(m) => json!(m.binary_fields),
        _ => json!(null),
    }
}
fn open(path: &Path, config: &Value) -> Result<wrapped::Reader, wrapped::Error> {
    let input = wrapped::Input::open(path, Default::default())?;
    let format = match config["format"].as_str().unwrap_or("auto") {
        "edf" => wrapped::Format::Edf,
        "tiff" => wrapped::Format::Tiff,
        "npy" => wrapped::Format::Native(native::Format::Npy),
        "cbf" => wrapped::Format::Cbf(dioptrin_io::cbf::ByteOffsetMode::Extended64),
        "auto" => return input.read_discovered(Default::default(), Default::default()),
        _ => {
            return Err(
                std::io::Error::other("unsupported test-only series opener configuration").into(),
            );
        }
    };
    input.read(format, Default::default(), Default::default())
}
#[cfg(feature = "xcalibur")]
fn run_record(m: &dioptrin_io::xcalibur::run::RunDescription) -> Value {
    fn float(v: f64) -> Value {
        if v.is_finite() {
            json!(v)
        } else {
            json!({"kind":"float","value":if v.is_nan(){"nan"}else if v>0.0{"inf"}else{"-inf"}})
        }
    }
    let mut sweeps = Vec::new();
    let mut bits = Vec::new();
    for s in &m.sweeps {
        let mut fields = json!({"iscanindex":s.scan_index,"iscantype":s.scan_type,"dwunknown1":s.unknown1,"iunknown3":s.unknown[0],"iunknown4":s.unknown[1],"iunknown5":s.unknown[2],"iunknown6":s.unknown[3]});
        let mut b = json!({});
        for (name, v) in [
            ("domega", s.omega),
            ("dtheta", s.theta),
            ("dkappa", s.kappa),
            ("dphi", s.phi),
            ("dstart", s.start),
            ("dend", s.end),
            ("dwidth", s.width),
            ("dunknown2", s.unknown2),
            ("dexposure", s.exposure),
        ] {
            fields[name] = float(v);
            b[name] = json!(format!("{:016x}", v.to_bits()));
        }
        sweeps.push(fields);
        bits.push(b);
    }
    let hex = |b: &[u8]| b.iter().map(|v| format!("{v:02x}")).collect::<String>();
    json!({"kind":"xcalibur_run","fields":{"cprefix":m.prefix.text().ok(),"cfolder":m.folder.text().ok(),"inumofsweeps":m.declared_sweeps,"dunknown":float(m.unknown),"pssweep":sweeps},"float_bits":{"dunknown":format!("{:016x}",m.unknown.to_bits()),"pssweep":bits},"raw_text_hex":{"cprefix":hex(&m.prefix.0),"cfolder":hex(&m.folder.0)},"source_sha256":format!("{:x}",Sha256::digest(&m.original)),"source_bytes":m.original.len(),"parsed_bytes":m.parsed_bytes,"trailer_sha256":format!("{:x}",Sha256::digest(m.trailing_bytes())),"trailer_hex":hex(m.trailing_bytes()),"sweep_sha256":m.sweeps.iter().map(|s|format!("{:x}",Sha256::digest(s.original))).collect::<Vec<_>>(),"provenance":format!("{:?}",m.provenance)})
}
fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let kind = args.get(1).ok_or("API command required")?;
    #[cfg(feature = "xcalibur")]
    if kind.starts_with("xcalibur-run") {
        use dioptrin_io::xcalibur::run::{Reader, RunDescription};
        let path = args.get(2).ok_or("source required")?;
        let record = match kind.as_str() {
            "xcalibur-run" => run_record(Reader::open(path, Default::default())?.metadata()?),
            "xcalibur-run-bytes" => run_record(&RunDescription::from_bytes(
                &std::fs::read(path)?,
                Default::default(),
            )?),
            "xcalibur-run-stream" => run_record(&RunDescription::from_reader(
                std::io::stdin().lock(),
                Default::default(),
            )?),
            _ => return Err("unknown companion command".into()),
        };
        println!("{record}");
        return Ok(());
    }
    if kind.starts_with("cif-document") {
        use dioptrin_io::cbf::document::{Document, Reader};
        let path = args.get(2).ok_or("source required")?;
        let owned;
        let reader;
        let document = match kind.as_str() {
            "cif-document" => {
                reader = Reader::open(path, Default::default())?;
                reader.metadata()?
            }
            "cif-document-bytes" => {
                owned = Document::from_bytes(&std::fs::read(path)?, Default::default())?;
                &owned
            }
            "cif-document-stream" => {
                owned = Document::from_reader(std::io::stdin().lock(), Default::default())?;
                &owned
            }
            _ => return Err("unknown CIF document command".into()),
        };
        println!(
            "{}",
            json!({"kind":"cif_document","data_blocks":document.cif.data_blocks,"fields":document.cif.fields,"entries":document.cif.entries,"loops":document.cif.loops.iter().map(|g|json!({"columns":g.columns,"rows":g.rows})).collect::<Vec<_>>(),"diagnostics":document.cif.diagnostics,"coverage":"HeaderProjection","coverage_warning":document.coverage_warning,"provenance":format!("{:?}",document.provenance),"source_sha256":format!("{:x}",Sha256::digest(&document.original)),"source_bytes":document.original.len()})
        );
        return Ok(());
    }
    let config: Value =
        serde_json::from_slice(&std::fs::read(args.get(2).ok_or("JSON request required")?)?)?;
    if kind == "filename" {
        let numbered = series::Numbered::from_seed(
            config["seed"].as_str().ok_or("seed required")?,
            config["step"].as_u64().unwrap_or(1),
        )?;
        let paths = config["numbers"]
            .as_array()
            .ok_or("numbers required")?
            .iter()
            .map(|n| {
                numbered
                    .path(n.as_u64().ok_or("nonnegative number required")?)
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        println!(
            "{}",
            json!({"first":numbered.first,"digits":numbered.digits,"paths":paths})
        );
        return Ok(());
    }
    if kind != "series" {
        return Err("unknown API command".into());
    }
    let paths = config["paths"]
        .as_array()
        .ok_or("paths required")?
        .iter()
        .map(|p| {
            p.as_str()
                .ok_or("path string required")
                .map(std::path::PathBuf::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let cfg = config.clone();
    let opener = move |p: &Path| open(p, &cfg);
    let source = series::Series::open(paths, opener, series::Limits::default())?;
    let files = source
        .files()
        .iter()
        .map(|f| json!({"path":f.path,"first_frame":f.first_frame,"frames":f.frames}))
        .collect::<Vec<_>>();
    let mut cursor = Cursor::new(source);
    let mut actions = Vec::new();
    for command in config["commands"].as_array().ok_or("commands required")? {
        let result = match command["op"].as_str().ok_or("operation required")? {
            "read" => cursor
                .read_at_signed(
                    command["index"]
                        .as_i64()
                        .ok_or("signed frame index required")?,
                )
                .map(|_| ()),
            "next" => cursor.next().map(|_| ()),
            "previous" => cursor.previous().map(|_| ()),
            "first" => cursor.first().map(|_| ()),
            "last" => cursor.last().map(|_| ()),
            "relative" => cursor
                .seek_relative(command["offset"].as_i64().ok_or("offset required")?)
                .map(|_| ()),
            _ => return Err("unknown navigation operation".into()),
        };
        let mut value = json!({"command":command,"status":if result.is_ok(){"ok"}else{"error"},"error":result.err().map(|e|e.to_string()),"position":match cursor.position(){Position::BeforeFirst=>json!({"kind":"BeforeFirst"}),Position::AfterLast=>json!({"kind":"AfterLast"}),Position::Frame(i)=>json!({"kind":"Frame","index":i})},"current":cursor.current_index(),"pixels_available":cursor.buffer().is_some()});
        if let Some(buffer) = cursor.buffer() {
            let index = cursor.current_index().unwrap();
            let location = cursor.source().locate(index).unwrap();
            let mut frame = pixels(buffer.pixels().unwrap());
            frame["shape"] = json!(buffer.sample_shape().unwrap());
            frame["index"] = json!(index);
            frame["file_index"] = json!(location.file_index);
            frame["local_frame"] = json!(location.local_frame);
            frame["path"] = json!(location.path);
            if let Some(metadata) = cursor.source().current_metadata_ref()? {
                frame["fields"] = fields(metadata);
            }
            value["frame"] = frame;
        }
        actions.push(value);
    }
    println!(
        "{}",
        json!({"frames":cursor.frames(),"files":files,"actions":actions})
    );
    Ok(())
}
