//! Test-only series/navigation/companion comparison entry points.
use dioptrin_io::{
    PixelBuffer, native,
    navigation::{Cursor, Position},
    series, wrapped,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, path::Path};
fn visit_le_bytes(p: &PixelBuffer, mut write: impl FnMut(&[u8])) {
    macro_rules! scalars {($($variant:ident);*)=>{match p {$(PixelBuffer::$variant(values)=>for value in values {write(&value.to_le_bytes());},)*_=>unreachable!()}};}
    match p {
        PixelBuffer::C32(values) => {
            for pair in values {
                for value in pair {
                    write(&value.to_le_bytes());
                }
            }
        }
        PixelBuffer::C64(values) => {
            for pair in values {
                for value in pair {
                    write(&value.to_le_bytes());
                }
            }
        }
        PixelBuffer::CI16(values) => {
            for pair in values {
                for value in pair {
                    write(&value.to_le_bytes());
                }
            }
        }
        _ => scalars!(Bool;F16;U8;U16;U32;U64;I8;I16;I32;I64;F32;F64),
    }
}
fn canonical_bytes(p: &PixelBuffer) -> Vec<u8> {
    let mut bytes = Vec::new();
    visit_le_bytes(p, |slice| bytes.extend_from_slice(slice));
    bytes
}
fn pixels(p: &PixelBuffer) -> Value {
    use dioptrin_io::PixelType;
    let dtype = match p.pixel_type() {
        PixelType::Bool => "bool",
        PixelType::F16 => "float16",
        PixelType::U8 => "uint8",
        PixelType::U16 => "uint16",
        PixelType::U32 => "uint32",
        PixelType::U64 => "uint64",
        PixelType::I8 => "int8",
        PixelType::I16 => "int16",
        PixelType::I32 => "int32",
        PixelType::I64 => "int64",
        PixelType::F32 => "float32",
        PixelType::F64 => "float64",
        PixelType::C32 => "complex64",
        PixelType::C64 => "complex128",
        PixelType::CI16 => "complex_int16",
    };
    let mut hash = Sha256::new();
    visit_le_bytes(p, |slice| hash.update(slice));
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
fn bytes_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}
fn header_float(v: f64) -> Value {
    if v.is_finite() {
        json!(v)
    } else {
        json!({"kind":"float","value":if v.is_nan(){"nan"}else if v>0.0{"inf"}else{"-inf"}})
    }
}
fn tiff_tag(value: &dioptrin_io::tiff::TagValue) -> Value {
    use dioptrin_io::tiff::TagValue as V;
    match value {
        V::Byte(v) => json!(v),
        V::Short(v) => json!(v),
        V::SignedByte(v) => json!(v),
        V::SignedShort(v) => json!(v),
        V::Signed(v) => json!(v),
        V::SignedBig(v) => json!(v),
        V::Unsigned(v) => json!(v),
        V::UnsignedBig(v) => json!(v),
        V::Float(v) => header_float(f64::from(*v)),
        V::Double(v) => header_float(*v),
        V::Ascii(v) => json!(v),
        V::List(values) => json!(values.iter().map(tiff_tag).collect::<Vec<_>>()),
        V::Rational(n, d) => json!({"kind":"rational","numerator":n,"denominator":d}),
        V::SRational(n, d) => json!({"kind":"rational","numerator":n,"denominator":d}),
        V::Ifd(v) => json!(v),
        V::IfdBig(v) => json!(v),
        _ => json!({"kind":"unsupported","native":format!("{value:?}")}),
    }
}
fn tiff_info(info: dioptrin_io::tiff::api::Info<'_>) -> Value {
    let m = info.source;
    json!({"index":info.index,"ifd_offset":info.ifd_offset,"images":info.images,
        "tags":m.tags.iter().map(|(key,value)|(key.to_string(),tiff_tag(value))).collect::<serde_json::Map<_,_>>(),
        "shape":m.shape,"source_pixel_type":format!("{:?}",m.pixel_type),"channels":m.channels,"bits_per_sample":m.bits_per_sample,
        "photometric":m.photometric_interpretation,"planar":m.planar_configuration,"compression":m.compression,
        "orientation":m.orientation,"little_endian":m.little_endian,"lossy":m.lossy,"fill_order":m.fill_order,
        "sample_layout":format!("{:?}",m.sample_layout),"compatibility":m.compatibility.as_ref().map(|c|json!({"source_ifd_offset":c.source_ifd_offset,"kind":format!("{:?}",c.kind),"rows_per_strip":c.rows_per_strip,"operations":c.operations.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"windows":c.windows.iter().map(|w|json!({"offset":w.offset,"bytes":w.bytes})).collect::<Vec<_>>(),"source_entries":c.source_entries.iter().map(|e|json!({"tag":e.tag,"field_type":e.field_type,"count":e.count,"entry_offset":e.entry_offset,"ifd_entry_hex":bytes_hex(&e.ifd_entry),"payload_offset":e.payload_offset})).collect::<Vec<_>>()})),
        "opaque_tags":m.opaque_tags.iter().map(|(k,v)|(k.to_string(),json!({"field_type":v.field_type,"count":v.count,"entry_hex":bytes_hex(&v.ifd_entry),"value_hex":bytes_hex(&v.value_bytes),"reason":v.reason}))).collect::<serde_json::Map<_,_>>()})
}
fn syntax_record(d: &dioptrin_io::cbf::syntax::Document) -> Value {
    use dioptrin_io::cbf::syntax::{Composite, Item, Token, Value as CValue, Version};
    fn token(d: &dioptrin_io::cbf::syntax::Document, t: Token) -> Value {
        json!({"span":[t.span.start,t.span.end],"content":[t.content.start,t.content.end],"style":format!("{:?}",t.style),"text":d.text(t),"raw_hex":d.raw(t).map(bytes_hex)})
    }
    fn value(d: &dioptrin_io::cbf::syntax::Document, v: CValue) -> Value {
        let mut result = json!({"token":token(d,v.token),"null":v.null.map(|v|format!("{v:?}"))});
        if d.version == Version::Cif20 {
            result["composite"] = match d.composite(v) {
                Some(Composite::List(values)) => {
                    json!({"kind":"List","values":values.iter().map(|v|value(d,*v)).collect::<Vec<_>>()})
                }
                Some(Composite::Table(entries)) => {
                    json!({"kind":"Table","entries":entries.iter().map(|entry|json!({"key":token(d,entry.key),"key_logical_text":logical(d,entry.key),"value":value(d,entry.value)})).collect::<Vec<_>>()})
                }
                None => Value::Null,
            };
            if v.composite.is_none() {
                result["logical_text"] = logical(d, v.token);
            }
        }
        result
    }
    fn logical(d: &dioptrin_io::cbf::syntax::Document, token: Token) -> Value {
        match d.logical_text(token) {
            Ok(v) => {
                json!({"text":v.text,"normalized_eol":v.normalized_eol,"prefix":v.prefix,"folded":v.folded})
            }
            Err(error) => json!({"error":error.to_string()}),
        }
    }
    fn items(d: &dioptrin_io::cbf::syntax::Document, values: &[Item]) -> Vec<Value> {
        values.iter().map(|item|match item {
        Item::Scalar(s)=>json!({"kind":"Scalar","name":token(d,s.name),"value":value(d,s.value),"span":[s.span.start,s.span.end]}),
        Item::Loop(l)=>json!({"kind":"Loop","columns":l.columns.iter().map(|t|token(d,*t)).collect::<Vec<_>>(),"values":l.values.iter().map(|v|value(d,*v)).collect::<Vec<_>>(),"rows":l.rows().count(),"span":[l.span.start,l.span.end]}),
        Item::SaveFrame(f)=>json!({"kind":"SaveFrame","name":token(d,f.name),"items":items(d,&f.items),"span":[f.span.start,f.span.end]}),
    }).collect()
    }
    json!({"kind":"cif_syntax","version":format!("{:?}",d.version),"provenance":format!("{:?}",d.provenance),"blocks":d.blocks.iter().map(|b|json!({"name":token(d,b.name),"items":items(d,&b.items),"span":[b.span.start,b.span.end]})).collect::<Vec<_>>(),"source_bytes":d.original.len(),"source_sha256":format!("{:x}",Sha256::digest(&d.original))})
}
#[cfg(feature = "columnfile")]
fn column_record(t: &dioptrin_io::columnfile::Table) -> Value {
    let mut digest = Sha256::new();
    for v in t.values() {
        digest.update(v.to_le_bytes());
    }
    json!({"kind":"columnfile","representation":"DecodedFloat64View","shape":t.shape(),"labels_hex":t.labels().map(bytes_hex).collect::<Vec<_>>(),"comments_hex":t.comments().map(bytes_hex).collect::<Vec<_>>(),"lexemes_hex":t.lexemes().map(bytes_hex).collect::<Vec<_>>(),"value_bits":t.values().iter().map(|v|format!("{:016x}",v.to_bits())).collect::<Vec<_>>(),"integer_exactness":t.integer_exactness().collect::<Vec<_>>(),"value_sha256":format!("{:x}",digest.finalize()),"source_sha256":format!("{:x}",Sha256::digest(t.original())),"source_bytes":t.original().len(),"provenance":format!("{:?}",t.provenance())})
}
#[cfg(feature = "chiplot")]
fn profile_record(p: &dioptrin_io::chiplot::Profile) -> Value {
    use dioptrin_io::chiplot::Number;
    let text = |range: &std::ops::Range<usize>| json!({"span":[range.start,range.end],"text":std::str::from_utf8(&p.original[range.clone()]).ok(),"hex":bytes_hex(&p.original[range.clone()])});
    json!({"kind":"chiplot","representation":"ExactSourceLexemes","headers":p.headers.iter().map(text).collect::<Vec<_>>(),"declaration":text(&p.declaration),"declared_points":p.declared_points,"curves":p.curves,"rows":p.rows.iter().map(|r|json!({"line":r.line,"samples":r.samples.iter().map(|s|json!({"offset":s.offset,"lexeme":s.lexeme,"number":match s.number {Number::I64(v)=>json!({"kind":"I64","value":v}),Number::U64(v)=>json!({"kind":"U64","value":v}),Number::IntegerText=>json!({"kind":"IntegerText"}),Number::Decimal=>json!({"kind":"Decimal"}),Number::NonFinite=>json!({"kind":"NonFinite"})}})).collect::<Vec<_>>()})).collect::<Vec<_>>(),"comments":p.comments.iter().map(|c|json!({"line":c.line,"offset":c.offset,"hex":bytes_hex(&c.text)})).collect::<Vec<_>>(),"diagnostics":p.diagnostics,"source_sha256":format!("{:x}",Sha256::digest(&p.original)),"source_bytes":p.original.len(),"provenance":format!("{:?}",p.provenance)})
}
fn pilatus_value(value: &dioptrin_io::pilatus::Value) -> Value {
    use dioptrin_io::pilatus::Value as P;
    match value {
        P::Text(v) => json!(v),
        P::Integer(v) => json!(v),
        P::Float(v) => header_float(*v),
        P::Sequence(values) => json!(values.iter().map(pilatus_value).collect::<Vec<_>>()),
        P::Invalid(v) => json!({"kind":"invalid","text":v}),
    }
}
fn pilatus_record(d: &dioptrin_io::pilatus::Document, queries: &[String]) -> Value {
    let h = &d.header;
    let cleaned = match d.cleaned_lines(dioptrin_io::pilatus::Limits::default().metadata_bytes) {
        Ok(v) => json!({"lines":v.lines,"diagnostics":v.diagnostics}),
        Err(error) => json!({"error":error.to_string()}),
    };
    json!({"kind":"pilatus_document","original_sha256":format!("{:x}",Sha256::digest(&d.original)),"source_bytes":d.original.len(),"provenance":format!("{:?}",d.provenance),"convention":d.convention,"description":h.description,"fields":h.fields,"entries":h.entries,"typed_fields":h.typed_fields.iter().map(|(k,v)|(k.clone(),pilatus_value(v))).collect::<serde_json::Map<_,_>>(),"unparsed_lines":h.unparsed_lines,"diagnostics":d.diagnostics,"cleaned_view":cleaned,"queries":queries.iter().map(|key|json!({"key":key,"contains":h.contains(key),"value":h.get(key).map(pilatus_value)})).collect::<Vec<_>>()})
}
#[cfg(feature = "hdf5")]
fn attribute_value(value: &dioptrin_io::hdf5::attributes::AttributeValue) -> Value {
    use dioptrin_io::hdf5::attributes::AttributeValue as A;
    match value {
        A::Text(values) => json!({"kind":"Text","values":values}),
        A::Numeric {
            shape,
            pixel_type,
            little_endian,
            values,
        } => {
            let data = canonical_bytes(values);
            json!({"kind":"Numeric","shape":shape,"pixel_type":format!("{pixel_type:?}"),"little_endian":little_endian,"pixels":pixels(values),"value_hex":bytes_hex(&data)})
        }
    }
}
#[cfg(feature = "hdf5")]
fn nexus_object(o: &dioptrin_io::hdf5::nexus::Object) -> Value {
    json!({"requested_path":o.requested_path,"source_file":o.source_file,"path":o.path,"kind":format!("{:?}",o.kind),"shape":o.shape,"type_descriptor":o.type_descriptor,"byte_order":o.byte_order,"layout":o.layout,"attributes":{"source_file":o.attributes.source_file,"path":o.attributes.path,"values":o.attributes.values.iter().map(|(k,v)|(k.clone(),attribute_value(v))).collect::<serde_json::Map<_,_>>(),"declarations":o.attributes.declarations.iter().map(|(k,v)|(k.clone(),json!({"shape":v.shape,"type_descriptor":v.type_descriptor}))).collect::<serde_json::Map<_,_>>()}})
}
#[cfg(feature = "hdf5")]
fn nexus_entry(e: &dioptrin_io::hdf5::nexus::Entry) -> Value {
    json!({"object":nexus_object(&e.object),"start_time":e.start_time,"ordering_key":e.ordering_key,"ordering_qualification":e.ordering_qualification})
}
#[cfg(feature = "hdf5")]
fn attribute_input(
    v: &Value,
) -> Result<dioptrin_io::hdf5::attributes::AttributeValue, Box<dyn Error>> {
    use dioptrin_io::{PixelType, hdf5::attributes::AttributeValue as A};
    let values = if let Some(a) = v["value"].as_array() {
        a.clone()
    } else {
        vec![v["value"].clone()]
    };
    let shape = if v["value"].is_array() {
        vec![values.len()]
    } else {
        vec![]
    };
    Ok(match v["dtype"].as_str().unwrap_or("text") {
        "text" => A::Text(
            values
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or("text filter string required")
                })
                .collect::<Result<_, _>>()?,
        ),
        "int64" => A::Numeric {
            shape,
            pixel_type: PixelType::I64,
            little_endian: cfg!(target_endian = "little"),
            values: PixelBuffer::I64(
                values
                    .iter()
                    .map(|v| v.as_i64().ok_or("i64 filter required"))
                    .collect::<Result<_, _>>()?,
            ),
        },
        "uint64" => A::Numeric {
            shape,
            pixel_type: PixelType::U64,
            little_endian: cfg!(target_endian = "little"),
            values: PixelBuffer::U64(
                values
                    .iter()
                    .map(|v| v.as_u64().ok_or("u64 filter required"))
                    .collect::<Result<_, _>>()?,
            ),
        },
        "float64" => A::Numeric {
            shape,
            pixel_type: PixelType::F64,
            little_endian: cfg!(target_endian = "little"),
            values: PixelBuffer::F64(
                values
                    .iter()
                    .map(|v| v.as_f64().ok_or("f64 filter required"))
                    .collect::<Result<_, _>>()?,
            ),
        },
        _ => return Err("unknown typed attribute filter".into()),
    })
}
#[cfg(feature = "hdf5")]
fn vds_selection(s: &dioptrin_io::hdf5::typed::VdsSelection) -> Value {
    use dioptrin_io::hdf5::typed::VdsSelection;
    match s {
        VdsSelection::All {
            shape,
            maximum_shape,
        } => json!({"kind":"All","shape":shape,"maximum_shape":maximum_shape}),
        VdsSelection::Regular {
            shape,
            maximum_shape,
            start,
            stride,
            count,
            block,
        } => {
            json!({"kind":"Regular","shape":shape,"maximum_shape":maximum_shape,"start":start,"stride":stride,"count":count,"block":block})
        }
        VdsSelection::Irregular {
            shape,
            maximum_shape,
            blocks,
        } => {
            json!({"kind":"Irregular","shape":shape,"maximum_shape":maximum_shape,"blocks":blocks})
        }
    }
}
#[cfg(feature = "hdf5")]
fn printf_mappings(
    config: &Value,
) -> Result<Vec<dioptrin_io::hdf5::typed::PrintfMapping>, Box<dyn Error>> {
    let Some(entries) = config.get("printf_mappings") else {
        return Ok(Vec::new());
    };
    entries
        .as_array()
        .ok_or("printf_mappings array required")?
        .iter()
        .map(|entry| {
            let owner = entry
                .get("source_file")
                .ok_or("printf owner source_file must be explicit (null for root input)")?;
            Ok(dioptrin_io::hdf5::typed::PrintfMapping {
                source_file: if owner.is_null() {
                    None
                } else {
                    Some(
                        owner
                            .as_str()
                            .ok_or("printf source_file must be path string or null")?
                            .into(),
                    )
                },
                dataset: entry["dataset"]
                    .as_str()
                    .ok_or("printf owner dataset required")?
                    .into(),
                mapping_index: usize::try_from(
                    entry["mapping_index"]
                        .as_u64()
                        .ok_or("printf mapping_index required")?,
                )?,
                stored_filename: entry["stored_filename"]
                    .as_str()
                    .ok_or("printf stored_filename required")?
                    .into(),
                declared_dataset: entry["declared_dataset"]
                    .as_str()
                    .ok_or("printf declared_dataset required")?
                    .into(),
                blocks: usize::try_from(entry["blocks"].as_u64().ok_or("printf blocks required")?)?,
            })
        })
        .collect()
}
#[cfg(feature = "hdf5")]
fn vds_value(p: &dioptrin_io::PixelBuffer) -> Value {
    json!({"pixels":pixels(p),"value_hex":bytes_hex(&canonical_bytes(p))})
}
#[cfg(feature = "hdf5")]
fn vds_mapping(m: &dioptrin_io::hdf5::typed::VdsMapping) -> Value {
    json!({"stored_filename":m.stored_filename,"declared_dataset":m.declared_dataset,"source_file":m.source_file,"resolved_dataset":m.resolved_dataset,"source_shape":m.source_shape,"selection_layout":format!("{:?}",m.selection_layout),"source_bounding_shape":m.source_bounding_shape,"virtual_bounding_shape":m.virtual_bounding_shape,"selected_elements":m.selected_elements,"source_selection":vds_selection(&m.source_selection),"virtual_selection":vds_selection(&m.virtual_selection),"frozen_source_selection":vds_selection(&m.frozen_source_selection),"frozen_virtual_selection":vds_selection(&m.frozen_virtual_selection),"nested_node":m.nested_node,"printf_expansion":m.printf_expansion.as_ref().map(|e| {let d=&e.declaration;json!({"block_index":e.block_index,"declaration":{"source_file":d.source_file,"dataset":d.dataset,"mapping_index":d.mapping_index,"stored_filename":d.stored_filename,"declared_dataset":d.declared_dataset,"source_selection":vds_selection(&d.source_selection),"virtual_selection":vds_selection(&d.virtual_selection),"blocks":d.blocks,"plan":d.plan.as_ref().map(|p|json!({"source_file":p.source_file,"dataset":p.dataset,"mapping_index":p.mapping_index,"stored_filename":p.stored_filename,"declared_dataset":p.declared_dataset,"blocks":p.blocks}))}})}),"source_origin":m.source_origin,"virtual_origin":m.virtual_origin,"mapped_shape":m.mapped_shape,"source_pixel_type":format!("{:?}",m.source_pixel_type),"source_little_endian":m.source_little_endian,"source_layout":format!("{:?}",m.source_layout),"source_fill_value":m.source_fill_value.as_ref().map(vds_value),"source_fill_time":format!("{:?}",m.source_fill_time),"storage_allocated":m.storage_allocated,"filters":m.filters.iter().map(|f|format!("{f:?}")).collect::<Vec<_>>()})
}
#[cfg(feature = "hdf5")]
fn vds_metadata(v: &dioptrin_io::hdf5::typed::VdsMetadata) -> Value {
    json!({"node_id":v.node_id,"source_file":v.source_file,"dataset":v.dataset,"fill_value":v.fill_value.as_ref().map(vds_value),"declared_shape":v.declared_shape,"frozen_shape":v.frozen_shape,"master_max_shape":v.master_max_shape,"unmapped_regions":format!("{:?}",v.unmapped_regions),"unmapped_pixels_per_frame":v.unmapped_pixels_per_frame,"unmapped_pixels_by_frame":v.unmapped_pixels_by_frame,"mappings":v.mappings.iter().map(vds_mapping).collect::<Vec<_>>(),"nested_nodes":v.nested_nodes.iter().map(|n|json!({"node_id":n.node_id,"source_file":n.source_file,"dataset":n.dataset,"declared_shape":n.declared_shape,"frozen_shape":n.frozen_shape,"master_max_shape":n.master_max_shape,"mappings":n.mappings.iter().map(vds_mapping).collect::<Vec<_>>(),"fill_value":n.fill_value.as_ref().map(vds_value),"fill_time":format!("{:?}",n.fill_time),"unmapped_pixels_per_frame":n.unmapped_pixels_per_frame,"unmapped_pixels_by_frame":n.unmapped_pixels_by_frame})).collect::<Vec<_>>(),"effective_fill_regions":v.effective_fill_regions.iter().map(|r|json!({"origin_node":r.origin_node,"source_file":r.source_file,"dataset":r.dataset,"origin_selection":vds_selection(&r.origin_selection),"virtual_selection":vds_selection(&r.virtual_selection),"value":vds_value(&r.value)})).collect::<Vec<_>>()})
}
fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let kind = args.get(1).ok_or("API command required")?;
    #[cfg(feature = "mar345")]
    if kind == "mar345-layout" {
        let dialect = match args.get(3).map(String::as_str) {
            None | Some("original") => dioptrin_io::mar345::PckV2Dialect::Original,
            Some("fabio8") => dioptrin_io::mar345::PckV2Dialect::FabioLegacyEightBit,
            _ => return Err("MAR345 dialect must be original or fabio8".into()),
        };
        let reader = dioptrin_io::mar345::Reader::open_with_options(
            args.get(2).ok_or("path required")?,
            Default::default(),
            dioptrin_io::mar345::Options {
                v2_dialect: dialect,
            },
        )?;
        let m = reader.metadata();
        let mut buffer = dioptrin_io::mar345::FrameBuffer::default();
        reader.read_frame_into(0, &mut buffer)?;
        let p = buffer.pixels().ok_or("pixels unavailable")?;
        println!(
            "{}",
            json!({"frames":reader.frames(),"shape":buffer.shape(),"pixel_type":format!("{:?}",p.pixel_type()),"pixels":pixels(p),"value_hex":bytes_hex(&canonical_bytes(p)),"metadata":{"shape":m.shape,"pixel_type":format!("{:?}",m.pixel_type),"base_pixel_type":format!("{:?}",m.base_pixel_type),"little_endian":m.little_endian,"pck_version":m.pck_version,"pck_v2_dialect":m.pck_v2_dialect.map(|v|format!("{v:?}")),"overflow_layout":format!("{:?}",m.overflow_layout),"header_hex":bytes_hex(&m.header),"binary_words":m.binary_words,"text_start":m.text_start,"fields":m.fields,"entries":m.entries,"overflow_records":m.overflow_records.iter().map(|r|json!({"one_based_index":r.one_based_index,"value":r.value})).collect::<Vec<_>>(),"overflow_bytes_hex":bytes_hex(&m.overflow_bytes),"marker_hex":bytes_hex(&m.marker),"encoded_bytes":m.encoded_bytes}})
        );
        return Ok(());
    }
    if kind == "npy-layout" {
        let reader = native::Reader::open(
            args.get(2).ok_or("path required")?,
            native::Format::Npy,
            Default::default(),
        )?;
        let m = reader.metadata();
        let mut images = Vec::new();
        let mut buffer = native::FrameBuffer::default();
        for frame in 0..reader.frames() {
            reader.read_frame_into(frame, &mut buffer)?;
            let p = buffer.pixels().ok_or("pixels absent")?;
            images.push(json!({"frame":frame,"shape":buffer.shape(),"pixel_type":format!("{:?}",p.pixel_type()),"pixels":pixels(p),"value_hex":bytes_hex(&canonical_bytes(p))}));
        }
        println!(
            "{}",
            json!({"frames":reader.frames(),"metadata":{"source_shape":m.source_shape,"shape":m.shape,"fortran_order":m.fortran_order,"pixel_type":format!("{:?}",m.pixel_type),"little_endian":m.little_endian,"fields":m.fields,"header_hex":bytes_hex(&m.header)},"images":images})
        );
        return Ok(());
    }
    if kind == "edf-array" {
        let c: Value =
            serde_json::from_slice(&std::fs::read(args.get(2).ok_or("request required")?)?)?;
        let reader = dioptrin_io::edf::Reader::open(
            c["path"].as_str().ok_or("path required")?,
            Default::default(),
        )?;
        let record = usize::try_from(c["record"].as_u64().unwrap_or(0))?;
        let m = reader.metadata(record).ok_or("record absent")?;
        let mut array = dioptrin_io::edf::ArrayBuffer::default();
        reader.read_array_into(record, &mut array)?;
        let p = array.pixels().ok_or("array absent")?;
        let mut output = json!({"records":reader.frames(),"metadata":{"source_shape":m.source_shape,"source_samples":m.source_samples,"plane_count":m.plane_count,"shape":m.shape,"frame_layout":format!("{:?}",m.frame_layout),"implicit_dimensions":m.implicit_dimensions,"record_index":m.record_index,"fields":m.fields,"header_hex":bytes_hex(&m.header),"pixel_type":format!("{:?}",m.pixel_type),"little_endian":m.little_endian},"array":{"shape":array.shape(),"pixel_type":format!("{:?}",p.pixel_type()),"pixels":pixels(p),"value_hex":bytes_hex(&canonical_bytes(p))}});
        if let Some(v) = c.get("leading") {
            let leading = v
                .as_array()
                .ok_or("leading array required")?
                .iter()
                .map(|v| usize::try_from(v.as_u64().ok_or("index required")?).map_err(Into::into))
                .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
            let mut plane = dioptrin_io::edf::FrameBuffer::default();
            reader.read_plane_into(record, &leading, &mut plane)?;
            let p = plane.pixels().ok_or("plane absent")?;
            output["plane"] = json!({"shape":plane.shape(),"pixel_type":format!("{:?}",p.pixel_type()),"pixels":pixels(p),"value_hex":bytes_hex(&canonical_bytes(p))});
        }
        println!("{output}");
        return Ok(());
    }
    if kind == "tiff-directories" {
        let catalog = dioptrin_io::tiff::directories::Catalog::open(
            args.get(2).ok_or("path required")?,
            Default::default(),
        )?;
        let dirs=catalog.directories()?.iter().map(|d|json!({"ifd_offset":d.ifd_offset,"next_ifd":d.next_ifd,"subifd_offsets":d.subifd_offsets,"primary_index":d.primary_index,"subifd_entry":d.subifd_entry.as_ref().map(|e|json!({"tag":e.tag,"field_type":e.field_type,"count":e.count,"entry_offset":e.entry_offset,"ifd_entry_hex":bytes_hex(&e.ifd_entry),"payload_offset":e.payload_offset})),"tags":d.tags.iter().map(|(k,v)|(k.to_string(),tiff_tag(v))).collect::<serde_json::Map<_,_>>(),"opaque_tags":d.opaque_tags.iter().map(|(k,v)|(k.to_string(),json!({"field_type":v.field_type,"count":v.count,"entry_hex":bytes_hex(&v.ifd_entry),"value_hex":bytes_hex(&v.value_bytes),"reason":v.reason}))).collect::<serde_json::Map<_,_>>()})).collect::<Vec<_>>();
        println!(
            "{}",
            json!({"primary_offsets":catalog.primary_offsets()?,"directories":dirs})
        );
        return Ok(());
    }
    if kind == "tiff-directory" {
        let offset = args.get(3).ok_or("offset required")?;
        let offset = if let Some(hex) = offset.strip_prefix("0x") {
            u64::from_str_radix(hex, 16)?
        } else {
            offset.parse()?
        };
        let mut reader = dioptrin_io::tiff::Reader::open_directory(
            args.get(2).ok_or("path required")?,
            offset,
            Default::default(),
            Default::default(),
        )?;
        let mut buffer = dioptrin_io::tiff::FrameBuffer::default();
        reader.read_frame_into(0, &mut buffer)?;
        let p = buffer.pixels().ok_or("pixels absent")?;
        let m = reader.metadata(0).ok_or("metadata absent")?;
        println!(
            "{}",
            json!({"frames":reader.frames(),"ifd_offset":offset,"shape":buffer.shape(),"pixel_type":format!("{:?}",p.pixel_type()),"pixels":pixels(p),"value_hex":bytes_hex(&canonical_bytes(p)),"metadata":{"little_endian":m.little_endian,"source_pixel_type":format!("{:?}",m.pixel_type),"tags":m.tags.iter().map(|(k,v)|(k.to_string(),tiff_tag(v))).collect::<serde_json::Map<_,_>>()}})
        );
        return Ok(());
    }
    #[cfg(feature = "hdf5")]
    if kind == "hdf5-layout" {
        use dioptrin_io::hdf5::typed::{
            ExternalLinks, ExternalRawOptions, FilenameBinding, Options, Reader, Selection,
            UnmappedRegions, VdsOptions,
        };
        let c: Value =
            serde_json::from_slice(&std::fs::read(args.get(2).ok_or("JSON request required")?)?)?;
        let read_bindings = |key: &str| -> Result<Option<Vec<FilenameBinding>>, Box<dyn Error>> {
            c.get(key)
                .map(|v| v.as_array().ok_or("filename bindings array required"))
                .transpose()?
                .map(|a| {
                    a.iter()
                        .map(|v| {
                            Ok::<_, Box<dyn Error>>(FilenameBinding {
                                stored_filename: v["stored_filename"]
                                    .as_str()
                                    .ok_or("stored_filename required")?
                                    .into(),
                                actual_file: v["actual_file"]
                                    .as_str()
                                    .ok_or("actual_file required")?
                                    .into(),
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()
        };
        let raw_bindings = read_bindings("raw_filenames")?;
        let vds_bindings = read_bindings("vds_filenames")?;
        let printf = printf_mappings(&c)?;
        let unmapped = match c["unmapped_regions"].as_str() {
            None | Some("reject") => UnmappedRegions::Reject,
            Some("declared_fill") => UnmappedRegions::DeclaredFill,
            _ => return Err("unknown unmapped region policy".into()),
        };
        let options = Options {
            external_links: if c["follow_external"].as_bool().unwrap_or(false) {
                ExternalLinks::FollowSnapshot
            } else {
                ExternalLinks::Reject
            },
            ..Default::default()
        };
        let reader = Reader::open_snapshot_with_storage(
            c["path"].as_str().ok_or("path required")?,
            Selection::Dataset(c["dataset"].as_str().ok_or("dataset required")?),
            options,
            vds_bindings.as_ref().map(|filenames| VdsOptions {
                filenames,
                unmapped_regions: unmapped,
                printf_mappings: &printf,
            }),
            raw_bindings
                .as_ref()
                .map(|v| ExternalRawOptions { filenames: v }),
            c["origin"].as_str().map(Path::new),
        )?;
        let mut metadata = Vec::new();
        let mut first_frames = std::collections::BTreeSet::new();
        for index in 0..reader.frames() {
            let m = reader.metadata(index).ok_or("metadata absent")?;
            if !first_frames.insert(m.first_frame) {
                continue;
            }
            metadata.push(json!({"source_file":m.source_file,"dataset":m.dataset,"source_shape":m.source_shape,"shape":m.shape,"frames":m.frames,"first_frame":m.first_frame,"pixel_type":format!("{:?}",m.pixel_type),"little_endian":m.little_endian,"type_descriptor":m.type_descriptor,"layout":format!("{:?}",m.layout),"filters":m.filters.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"fill_value":m.fill_value.as_ref().map(|v|json!({"pixels":pixels(v),"value_hex":bytes_hex(&canonical_bytes(v))})),"storage_allocated":m.storage_allocated,"fill_time":format!("{:?}",m.fill_time),"virtual_dataset":m.virtual_dataset.as_ref().map(vds_metadata),"external_raw":m.external_raw.as_ref().map(|r|json!({"frozen_bytes":r.frozen_bytes,"windows":r.windows.iter().map(|w|json!({"stored_filename":w.stored_filename,"source_file":w.source_file,"file_offset":w.file_offset,"declared_bytes":w.declared_bytes,"logical_offset":w.logical_offset,"effective_bytes":w.effective_bytes})).collect::<Vec<_>>()}))}));
        }
        let frames = if let Some(v) = c.get("frames") {
            v.as_array()
                .ok_or("frames array required")?
                .iter()
                .map(|v| {
                    usize::try_from(v.as_u64().ok_or("frame index required")?)
                        .map_err(|e| e.to_string().into())
                })
                .collect::<Result<Vec<_>, Box<dyn Error>>>()?
        } else {
            (0..reader.frames()).collect()
        };
        let mut images = Vec::new();
        let mut buffer = dioptrin_io::hdf5::typed::FrameBuffer::default();
        for frame in frames {
            match reader.read_frame_into(frame,&mut buffer) {
                Ok(())=>{let p=buffer.pixels().ok_or("pixels unavailable")?;images.push(json!({"frame":frame,"status":"ok","shape":buffer.shape(),"pixel_type":format!("{:?}",p.pixel_type()),"pixels":pixels(p),"value_hex":bytes_hex(&canonical_bytes(p))}));},
                Err(error)=>images.push(json!({"frame":frame,"status":"error","error":error.to_string(),"pixels_available":buffer.pixels().is_some()})),
            }
        }
        reader.check_sources()?;
        println!(
            "{}",
            json!({"kind":"hdf5_layout","native_hdf5_version":dioptrin_io::hdf5::library_version(),"frames":reader.frames(),"metadata":metadata,"images":images,"sources":reader.sources().iter().map(|s|json!({"path":s.path,"bytes":s.bytes})).collect::<Vec<_>>(),"source_bindings":reader.source_bindings().iter().map(|b|json!({"requested":b.requested,"resolved":b.resolved})).collect::<Vec<_>>(),"links":reader.links().iter().map(|l|json!({"source":l.source,"path":l.path,"external":l.external,"raw_hex":bytes_hex(&l.raw_value)})).collect::<Vec<_>>()})
        );
        return Ok(());
    }
    if kind.starts_with("pilatus") {
        use dioptrin_io::pilatus::{Document, Reader};
        let path = args.get(2).ok_or("source required")?;
        let record = match kind.as_str() {
            "pilatus" => pilatus_record(
                Reader::open(path, Default::default())?.metadata()?,
                &args[3..],
            ),
            "pilatus-bytes" => pilatus_record(
                &Document::from_bytes(&std::fs::read(path)?, Default::default())?,
                &args[3..],
            ),
            "pilatus-stream" => pilatus_record(
                &Document::from_reader(std::io::stdin().lock(), Default::default())?,
                &args[3..],
            ),
            _ => return Err("unknown Pilatus command".into()),
        };
        println!("{record}");
        return Ok(());
    }
    #[cfg(feature = "hdf5")]
    if kind == "nexus-api" {
        use dioptrin_io::hdf5::{
            nexus::Reader,
            typed::{ExternalLinks, Options},
        };
        let config: Value =
            serde_json::from_slice(&std::fs::read(args.get(2).ok_or("JSON request required")?)?)?;
        let options = Options {
            external_links: if config["follow_external"].as_bool().unwrap_or(false) {
                ExternalLinks::FollowSnapshot
            } else {
                ExternalLinks::Reject
            },
            ..Default::default()
        };
        let reader =
            Reader::open_snapshot(config["path"].as_str().ok_or("path required")?, options)?;
        let mut actions = Vec::new();
        for c in config["commands"].as_array().ok_or("commands required")? {
            let result = (|| -> Result<Value, Box<dyn Error>> {
                let path = || c["path"].as_str().ok_or("query path required");
                let all = c["all"].as_bool().unwrap_or(false);
                Ok(match c["op"].as_str().ok_or("op required")? {
                    "entries" => json!(
                        reader
                            .get_entries()?
                            .iter()
                            .map(nexus_entry)
                            .collect::<Vec<_>>()
                    ),
                    "entry" => reader
                        .get_entry(c["name"].as_str().ok_or("name required")?)?
                        .as_ref()
                        .map(nexus_object)
                        .unwrap_or(Value::Null),
                    "class" => json!(
                        reader
                            .get_class(path()?, c["class"].as_str().ok_or("class required")?)?
                            .iter()
                            .map(nexus_object)
                            .collect::<Vec<_>>()
                    ),
                    "attr" => reader
                        .get_attr(
                            path()?,
                            c["name"].as_str().ok_or("attribute name required")?,
                        )?
                        .as_ref()
                        .map(attribute_value)
                        .unwrap_or(Value::Null),
                    "data" => {
                        let filter = c.get("filter").map(attribute_input).transpose()?;
                        let filter = filter
                            .as_ref()
                            .map(|v| {
                                Ok::<_, Box<dyn Error>>((
                                    c["filter"]["name"].as_str().ok_or("filter name required")?,
                                    v,
                                ))
                            })
                            .transpose()?;
                        json!(
                            reader
                                .get_data(path()?, filter)?
                                .iter()
                                .map(nexus_object)
                                .collect::<Vec<_>>()
                        )
                    }
                    "detector" => json!(
                        reader
                            .find_detector(all)?
                            .iter()
                            .map(nexus_object)
                            .collect::<Vec<_>>()
                    ),
                    "find_data" => json!(
                        reader
                            .find_data(all)?
                            .iter()
                            .map(nexus_object)
                            .collect::<Vec<_>>()
                    ),
                    "default" => reader
                        .get_default_nxdata()?
                        .as_ref()
                        .map(nexus_object)
                        .unwrap_or(Value::Null),
                    "image" => {
                        let dataset = path()?;
                        let (parent, _) = dataset
                            .rsplit_once('/')
                            .ok_or("absolute dataset required")?;
                        let object = reader
                            .get_data(if parent.is_empty() { "/" } else { parent }, None)?
                            .into_iter()
                            .find(|o| o.requested_path == dataset)
                            .ok_or("dataset not indexed")?;
                        let view = reader.open_image(&object)?;
                        let index = usize::try_from(c["index"].as_u64().unwrap_or(0))?;
                        let mut buffer = dioptrin_io::hdf5::typed::FrameBuffer::default();
                        view.read_frame_into(index, &mut buffer)?;
                        json!({"object":nexus_object(&object),"frames":view.frames(),"index":index,"shape":buffer.shape(),"pixels":pixels(buffer.pixels().ok_or("HDF5 output unavailable")?)})
                    }
                    _ => return Err("unknown NeXus read API operation".into()),
                })
            })();
            actions.push(match result {
                Ok(v) => json!({"command":c,"status":"ok","result":v}),
                Err(error) => json!({"command":c,"status":"error","error":error.to_string()}),
            });
        }
        reader.check_sources()?;
        println!(
            "{}",
            json!({"kind":"nexus_api","actions":actions,"sources":reader.sources().iter().map(|s|json!({"path":s.path,"bytes":s.bytes})).collect::<Vec<_>>(),"links":reader.links().iter().map(|l|json!({"source":l.source,"path":l.path,"external":l.external,"raw_hex":bytes_hex(&l.raw_value)})).collect::<Vec<_>>()})
        );
        return Ok(());
    }
    if kind == "tiff-metadata-api" {
        use dioptrin_io::tiff::api::MetadataReader;
        let config: Value =
            serde_json::from_slice(&std::fs::read(args.get(2).ok_or("JSON request required")?)?)?;
        let reader = MetadataReader::open(
            config["path"].as_str().ok_or("path required")?,
            Default::default(),
        )?;
        let mut actions = Vec::new();
        for command in config["commands"].as_array().ok_or("commands required")? {
            let result = (|| -> Result<Value, Box<dyn Error>> {
                match command["op"].as_str().ok_or("op required")? {
                    "count" => Ok(json!({"images":reader.get_number_of_images()?})),
                    "directories" => Ok(
                        json!({"directories":reader.get_image_file_directories()?.collect::<Vec<_>>()}),
                    ),
                    "info" => {
                        let info = reader
                            .get_info(command["index"].as_i64().ok_or("signed index required")?)?;
                        Ok(
                            json!({"index":info.index,"images":info.images,"ifd_offset":info.source.ifd_offset,"tags":info.source.tags.iter().map(|(k,v)|(k.to_string(),tiff_tag(v))).collect::<serde_json::Map<_,_>>(),"opaque_tags":info.source.opaque_tags.iter().map(|(k,v)|(k.to_string(),json!({"field_type":v.field_type,"count":v.count,"entry_hex":bytes_hex(&v.ifd_entry),"value_hex":bytes_hex(&v.value_bytes),"reason":v.reason}))).collect::<serde_json::Map<_,_>>()}),
                        )
                    }
                    _ => Err("unknown TIFF metadata API operation".into()),
                }
            })();
            actions.push(match result {Ok(value)=>json!({"command":command,"status":"ok","result":value}),Err(error)=>json!({"command":command,"status":"error","error":error.to_string(),"pixels_available":false})});
        }
        println!("{}", json!({"kind":"tiff_metadata_api","actions":actions}));
        return Ok(());
    }
    if kind == "tiff-api" {
        use dioptrin_io::tiff::api::{Reader, Rows, ViewPolicy};
        let config: Value =
            serde_json::from_slice(&std::fs::read(args.get(2).ok_or("JSON request required")?)?)?;
        let options = dioptrin_io::tiff::Options {
            uncompressed_windows: match config["uncompressed_windows"].as_str() {
                Some("from_shape") => dioptrin_io::tiff::UncompressedWindows::FromShape,
                None | Some("declared") => dioptrin_io::tiff::UncompressedWindows::Declared,
                _ => return Err("unknown TIFF uncompressed-window policy".into()),
            },
            tile_tables: match config["tile_tables"].as_str() {
                Some("from_strip_tags") => dioptrin_io::tiff::TileTables::FromStripTags,
                None | Some("declared") => dioptrin_io::tiff::TileTables::Declared,
                _ => return Err("unknown TIFF tile-table policy".into()),
            },
            ojpeg_tiles: match config["ojpeg_tiles"].as_str() {
                None | Some("declared") => dioptrin_io::tiff::OjpegTiles::Declared,
                Some("independent_reset_tiles") => {
                    dioptrin_io::tiff::OjpegTiles::IndependentResetTiles
                }
                _ => return Err("unknown TIFF original-JPEG tile interpretation".into()),
            },
        };
        let mut reader = Reader::open_with_options(
            config["path"].as_str().ok_or("path required")?,
            Default::default(),
            options,
        )?;
        let mut buffer = dioptrin_io::tiff::FrameBuffer::default();
        let mut actions = Vec::new();
        for command in config["commands"].as_array().ok_or("commands required")? {
            let operation = command["op"].as_str().ok_or("op required")?;
            let result = (|| -> Result<Value, Box<dyn Error>> {
                if operation == "count" {
                    return Ok(json!({"images":reader.get_number_of_images()?}));
                }
                if operation == "directories" {
                    return Ok(json!({"directories":reader.get_image_file_directories()?}));
                }
                let index = command["index"].as_i64().ok_or("signed index required")?;
                if operation == "info" {
                    return Ok(tiff_info(reader.get_info(index)?));
                }
                let rows = if command.get("row_min").is_some() || command.get("row_max").is_some() {
                    Rows::Inclusive {
                        first: command["row_min"]
                            .as_i64()
                            .ok_or("signed row_min required")?,
                        last: command["row_max"]
                            .as_i64()
                            .ok_or("signed row_max required")?,
                    }
                } else {
                    Rows::All
                };
                if operation == "view" {
                    let policy = if command["mono"].as_bool().unwrap_or(false) {
                        ViewPolicy::FabioTiffIoMono
                    } else {
                        ViewPolicy::FabioTiffIo
                    };
                    let view = reader.read_view(index, rows, policy)?;
                    let data = match &view.image.pixels {
                        dioptrin_io::tiff::view::Pixels::Scalar(p) => pixels(p),
                        _ => return Err("TIFF API scalar view required".into()),
                    };
                    return Ok(
                        json!({"pixels":data,"shape":view.image.provenance.shape,"rows":[view.rows.start,view.rows.end],"ifd_offset":view.ifd_offset,"policy":format!("{:?}",view.policy),"operations":view.operations.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"view_operations":view.image.provenance.operations.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"source_orientation":view.image.provenance.source_orientation,"source_type":format!("{:?}",view.image.provenance.source_type)}),
                    );
                }
                match operation {
                    "data" => reader.get_data_into(index, rows, &mut buffer)?,
                    "image" => reader.get_image_into(index, rows, &mut buffer)?,
                    _ => return Err("unknown TIFF API operation".into()),
                }
                Ok(
                    json!({"pixels":pixels(buffer.pixels().ok_or("TIFF output unavailable")?),"shape":buffer.sample_shape(),"spatial_shape":buffer.shape()}),
                )
            })();
            actions.push(match result {
                Ok(value)=>json!({"command":command,"status":"ok","result":value}),
                Err(error)=>json!({"command":command,"status":"error","error":error.to_string(),"pixels_available":(["data","image"].contains(&operation)&&buffer.pixels().is_some())}),
            });
        }
        println!(
            "{}",
            json!({"kind":"tiff_api","interpretation_policy":format!("{options:?}"),"actions":actions})
        );
        return Ok(());
    }
    if kind == "cbf-encoded" {
        use dioptrin_io::cbf::{ByteOffsetMode, EncodedBuffer, Reader};
        let path = args.get(2).ok_or("source required")?;
        let mode = if args.iter().any(|v| v == "--legacy32") {
            ByteOffsetMode::Legacy32
        } else {
            ByteOffsetMode::Extended64
        };
        let reader = if args.iter().any(|v| v == "--decoded-admission") {
            Reader::open_with_mode(path, Default::default(), mode)?
        } else {
            Reader::open_encoded(path, Default::default())?
        };
        let mut raw = EncodedBuffer::default();
        reader.read_encoded_into(&mut raw)?;
        let data = raw.bytes().ok_or("encoded payload unavailable")?;
        let integrity = raw.integrity().ok_or("integrity unavailable")?;
        println!(
            "{}",
            json!({"kind":"cbf_encoded","representation":"OriginalEncodedPayload","bytes":data.len(),"sha256":format!("{:x}",Sha256::digest(data)),"prefix_hex":bytes_hex(&data[..data.len().min(64)]),"declared_md5":integrity.declared_md5.map(|v|bytes_hex(&v)),"computed_md5":bytes_hex(&integrity.computed_md5),"checksum_matches":integrity.matches(),"shape":reader.metadata().shape,"source_pixel_type":format!("{:?}",reader.metadata().pixel_type),"binary_fields":reader.metadata().binary_fields,"header_sha256":format!("{:x}",Sha256::digest(&reader.metadata().header))})
        );
        return Ok(());
    }
    #[cfg(feature = "columnfile")]
    if kind.starts_with("columnfile") {
        use dioptrin_io::columnfile::{Format, Reader, Table};
        let path = args.get(2).ok_or("source required")?;
        let format = if args.iter().any(|s| s == "--binary") {
            Format::Binary
        } else {
            Format::Ascii
        };
        let result = match kind.as_str() {
            "columnfile" => column_record(Reader::open(path, format, Default::default())?.table()?),
            "columnfile-bytes" => column_record(&Table::from_bytes(
                &std::fs::read(path)?,
                format,
                Default::default(),
            )?),
            "columnfile-stream" => column_record(&Table::from_reader(
                std::io::stdin().lock(),
                format,
                Default::default(),
            )?),
            _ => return Err("unknown columnfile command".into()),
        };
        println!("{result}");
        return Ok(());
    }
    #[cfg(feature = "chiplot")]
    if kind.starts_with("chiplot") {
        use dioptrin_io::chiplot::{Profile, Reader};
        let path = args.get(2).ok_or("source required")?;
        let record = match kind.as_str() {
            "chiplot" => profile_record(Reader::open(path, Default::default())?.metadata()?),
            "chiplot-bytes" => profile_record(&Profile::from_bytes(
                &std::fs::read(path)?,
                Default::default(),
            )?),
            "chiplot-stream" => profile_record(&Profile::from_reader(
                std::io::stdin().lock(),
                Default::default(),
            )?),
            _ => return Err("unknown CHIPLOT command".into()),
        };
        println!("{record}");
        return Ok(());
    }
    if kind.starts_with("cif-syntax") {
        use dioptrin_io::cbf::syntax::{Document, Reader};
        let path = args.get(2).ok_or("source required")?;
        let record = match kind.as_str() {
            "cif-syntax" => syntax_record(Reader::open(path, Default::default())?.metadata()?),
            "cif-syntax-bytes" => syntax_record(&Document::from_bytes(
                &std::fs::read(path)?,
                Default::default(),
            )?),
            "cif-syntax-stream" => syntax_record(&Document::from_reader(
                std::io::stdin().lock(),
                Default::default(),
            )?),
            _ => return Err("unknown CIF syntax command".into()),
        };
        println!("{record}");
        return Ok(());
    }
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
