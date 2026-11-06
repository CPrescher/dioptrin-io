//! Test-only bridge to the pinned FabIO suite. No Python in the library/runtime.
use dioptrin_io::{PixelBuffer, PixelType, cbf, native, tiff, wrapped};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::error::Error;

fn declared_dtype(ty: PixelType) -> &'static str {
    match ty {
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
    }
}
fn sample_dtype_and_hash(pixels: &PixelBuffer, hash: &mut Sha256) -> &'static str {
    macro_rules! digest { ($($variant:ident,$name:literal);*) => {
        match pixels { $(PixelBuffer::$variant(values) => {
            for v in values { hash.update(v.to_le_bytes()); } $name
        }),*,_=>unreachable!() }
    }; }
    // Canonical little-endian scalar bytes, including floating bit patterns.
    match pixels {
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
            digest!(Bool,"bool";F16,"float16";U8,"uint8";U16,"uint16";U32,"uint32";U64,"uint64";I8,"int8";I16,"int16";I32,"int32";I64,"int64";F32,"float32";F64,"float64")
        }
    }
}
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
    let dtype = sample_dtype_and_hash(pixels, &mut hash);
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

#[cfg(any(
    feature = "fixed",
    feature = "containers",
    feature = "oxford",
    feature = "raxis"
))]
fn header_bytes(bytes: &[u8]) -> serde_json::Value {
    let mut value =
        json!({"kind":"bytes","length":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes))});
    if bytes.len() <= 4096 {
        value["hex"] = json!(bytes_hex(bytes));
    }
    value
}

fn header_float(value: f64) -> serde_json::Value {
    if value.is_finite() {
        json!(value)
    } else {
        json!({"kind":"float","value":if value.is_nan(){"nan"}else if value>0.0{"inf"}else{"-inf"}})
    }
}

fn tiff_tag(value: &tiff::TagValue) -> serde_json::Value {
    use tiff::TagValue as V;
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

fn tiff_fields(metadata: &tiff::Metadata) -> serde_json::Value {
    json!({"tags":metadata.tags.iter().map(|(key,value)|(key.to_string(),tiff_tag(value))).collect::<serde_json::Map<_,_>>(),
        "shape":metadata.shape,"channels":metadata.channels,"bits_per_sample":metadata.bits_per_sample,
        "photometric":metadata.photometric_interpretation,"planar":metadata.planar_configuration,
        "compression":metadata.compression,"orientation":metadata.orientation,"little_endian":metadata.little_endian,
        "opaque_tags":metadata.opaque_tags.iter().map(|(k,v)|(k.to_string(),json!({"field_type":v.field_type,"count":v.count,"value_hex":bytes_hex(&v.value_bytes),"reason":v.reason}))).collect::<serde_json::Map<_,_>>()})
}

#[cfg(feature = "containers")]
fn dm3_value(value: &dioptrin_io::dm3::TagValue<'_>) -> serde_json::Value {
    use dioptrin_io::dm3::TagValue as V;
    match value {
        V::Numeric { values, scalar } => {
            let record = frame_record(0, 1, [values.len()], values, json!({}), &[]);
            let mut encoded = json!({"kind":"array","dtype":record["dtype"],"shape":[values.len()],"sha256":record["sample_sha256"]});
            if values.len() <= 256 || *scalar {
                macro_rules! items { ($($ty:ident),*) => {match values {$(PixelBuffer::$ty(v)=>json!(v),)* PixelBuffer::F32(v)=>json!(v.iter().map(|&n|header_float(f64::from(n))).collect::<Vec<_>>()),PixelBuffer::F64(v)=>json!(v.iter().map(|&n|header_float(n)).collect::<Vec<_>>()),_=>unreachable!()}}; }
                encoded["values"] = items!(U8, U16, U32, U64, I8, I16, I32, I64);
            }
            json!({"numeric":encoded,"scalar":scalar})
        }
        V::Utf16(units) => json!({"utf16":units}),
        V::Structure { name, fields } => {
            json!({"name_hex":bytes_hex(name),"fields":fields.iter().map(|(name,value)|json!({"name_hex":bytes_hex(name),"value":dm3_value(value)})).collect::<Vec<_>>()})
        }
        V::Opaque(raw) => json!({"opaque":header_bytes(raw)}),
    }
}

#[cfg(feature = "containers")]
fn mrc_fields(m: &dioptrin_io::mrc::Metadata) -> serde_json::Value {
    let p = &m.parsed;
    let mut fields = json!({"NX":p.nx,"NY":p.ny,"NZ":p.nz,"MODE":p.mode,"NXSTART":p.nxstart,"NYSTART":p.nystart,"NZSTART":p.nzstart,
        "MX":p.mx,"MY":p.my,"MZ":p.mz,"MAPC":p.mapc,"MAPR":p.mapr,"MAPS":p.maps,"ISPG":p.ispg,"NSYMBT":p.nsymbt,
        "EXTRA":header_bytes(&p.extra),"ORIGIN":p.origin.map(|n|header_float(f64::from(n))),"MAP":String::from_utf8_lossy(&p.map),
        "MACHST":header_bytes(&p.machst),"NLABL":p.nlabl,"extended":header_bytes(&m.extended_header)});
    for (key, n) in [
        ("CELL_A", p.xlen),
        ("CELL_B", p.ylen),
        ("CELL_C", p.zlen),
        ("CELL_ALPHA", p.alpha),
        ("CELL_BETA", p.beta),
        ("CELL_GAMMA", p.gamma),
        ("DMIN", p.dmin),
        ("DMAX", p.dmax),
        ("DMEAN", p.dmean),
        ("RMS", p.rms),
    ] {
        fields[key] = header_float(f64::from(n));
    }
    for (index, label) in p.label.as_chunks::<80>().0.iter().enumerate() {
        fields[format!("LABEL_{index:02}")] = json!(String::from_utf8_lossy(label).trim());
    }
    fields
}

fn pilatus_value(value: &dioptrin_io::pilatus::Value) -> serde_json::Value {
    use dioptrin_io::pilatus::Value as V;
    match value {
        V::Text(v) => json!(v),
        V::Integer(v) => json!(v),
        V::Float(v) => header_float(*v),
        V::Sequence(v) => json!(v.iter().map(pilatus_value).collect::<Vec<_>>()),
        V::Invalid(v) => json!({"invalid":v}),
    }
}
fn pilatus_fields(header: &dioptrin_io::pilatus::Header) -> serde_json::Value {
    json!(
        header
            .typed_fields
            .iter()
            .map(|(key, value)| (key.clone(), pilatus_value(value)))
            .collect::<serde_json::Map<_, _>>()
    )
}
fn metadata_details(metadata: wrapped::Metadata<'_>, _index: usize) -> serde_json::Value {
    match metadata {
        wrapped::Metadata::DetectorTiff(m) => json!({"image":tiff_fields(m.image)}),
        wrapped::Metadata::Native(m) => {
            json!({"shape":m.shape,"header_hex":bytes_hex(&m.header),"format":format!("{:?}",m.format),"source_shape":m.source_shape})
        }
        #[cfg(feature = "bruker")]
        wrapped::Metadata::Bruker(m) => {
            json!({"header_bytes":m.header.len(),"version":match m.version {dioptrin_io::bruker::Version::V86=>86,dioptrin_io::bruker::Version::V100=>100}})
        }
        #[cfg(feature = "fixed")]
        wrapped::Metadata::Ge(m) => {
            json!({"kind":format!("{:?}",m.kind),"shape":m.shape,"frames":m.frames,"header_hex":bytes_hex(&m.header),"edf_fields":m.edf_fields})
        }
        #[cfg(feature = "fixed")]
        wrapped::Metadata::Pixi(m) => {
            json!({"shape":m.shape,"header_hex":bytes_hex(&m.headers[_index])})
        }
        #[cfg(feature = "mar345")]
        wrapped::Metadata::Mar345(m) => {
            json!({"little_endian":m.little_endian,"pck_version":m.pck_version,"pck_v2_dialect":m.pck_v2_dialect.map(|v|format!("{v:?}")),"binary_scalars":m.binary_scalars().into_iter().map(|(k,v)| {use dioptrin_io::mar345::HeaderValue as V;
                (k, match v {V::Integer(n)=>json!(n),V::Float(n)=>header_float(n),V::Text(t)=>json!(t)})
            }).collect::<std::collections::BTreeMap<_,_>>(),"text_scalars":m.text_scalars().collect::<Vec<_>>()})
        }
        #[cfg(feature = "esperanto")]
        wrapped::Metadata::Esperanto(m) => {
            json!({"version":m.version,"header_lines":m.header_lines,"encoding":format!("{:?}",m.encoding),"shape":m.shape,"parsed_fields":m.parsed_fields.iter().map(|(k,v)| { use dioptrin_io::esperanto::HeaderValue as V; (k.clone(),match v {V::Integer(n)=>json!(n),V::Float(n)=>header_float(*n),V::Text(t)=>json!(t),V::Invalid(t)=>json!({"invalid":t})}) }).collect::<serde_json::Map<_,_>>()})
        }
        #[cfg(feature = "containers")]
        wrapped::Metadata::Spe(m) => {
            json!({"xml_offset":u64::from_le_bytes(m.header[678..686].try_into().unwrap()),"shape":m.shape,"version":m.version,"data_type":m.data_type,"frames":m.frames,"x_calibration":m.x_calibration,"regions":m.regions.iter().map(|r|json!({"roi":r.roi,"shape":r.shape})).collect::<Vec<_>>()})
        }
        #[cfg(feature = "containers")]
        wrapped::Metadata::Mrc(m) => {
            json!({"header_hex":bytes_hex(&m.header),"little_endian":m.little_endian,"extended":header_bytes(&m.extended_header)})
        }
        #[cfg(feature = "containers")]
        wrapped::Metadata::Dm3(m) => {
            json!({"declared_bytes":m.declared_bytes,"little_endian":m.little_endian,"tags":m.tags.iter().map(|tag|json!({"path":tag.path,"raw_name":bytes_hex(&tag.raw_name),"descriptor":tag.descriptor,"bytes":tag.bytes,
                "value": match tag.decode_small_value(m.little_endian, 1024*1024) {Ok(Some(value))=>dm3_value(&value),Ok(None)=>json!(null),Err(error)=>json!({"error":error.to_string()})}
            })).collect::<Vec<_>>()})
        }
        #[cfg(feature = "fit2d")]
        wrapped::Metadata::Fit2d(m) => {
            json!({"block_bytes":m.block_bytes,"header_hex":bytes_hex(&m.header),"arrays":m.arrays.iter().map(|array|json!({"name":array.name,"shape":array.shape,"pixel_type":format!("{:?}",array.pixel_type)})).collect::<Vec<_>>()})
        }
        #[cfg(feature = "mpa")]
        wrapped::Metadata::Mpa(m) => json!({"header_hex":bytes_hex(&m.header)}),
        #[cfg(feature = "oxford")]
        wrapped::Metadata::Oxford(m) => {
            json!({"shape":m.shape,"header_version":m.header_version,"compression":format!("{:?}",m.compression),"compression_label":header_bytes(&m.compression_label),"exception16":m.exception16,"exception32":m.exception32,"header_bytes":m.header.len(),"sections":m.sections.iter().map(|s|json!({"name":s.name,"offset":s.offset,"bytes":s.bytes})).collect::<Vec<_>>()})
        }
        #[cfg(feature = "jpeg")]
        wrapped::Metadata::Jpeg(m) => {
            json!({"jfif":m.jfif_headers().map(|j|json!({"version":j.version,"density_unit":j.density_unit,"density":j.density})).collect::<Vec<_>>()})
        }
        #[cfg(feature = "raxis")]
        wrapped::Metadata::Raxis(m) => json!({"little_endian":m.little_endian}),
        #[cfg(feature = "xsd")]
        wrapped::Metadata::Xsd(m) => {
            json!({"coding":format!("{:?}",m.coding).to_lowercase(),"md5":m.md5,"elements":m.shape[0]*m.shape[1]})
        }
        wrapped::Metadata::Cbf(m) => {
            json!({"header_hex":bytes_hex(&m.header),"cif_fields":m.cif.fields,"cif_diagnostics":m.cif.diagnostics,"cif_loops":m.cif.loops.iter().map(|group|json!({"columns":group.columns,"rows":group.rows})).collect::<Vec<_>>(),"pilatus":m.pilatus.as_ref().map(pilatus_fields)})
        }
        _ => json!({}),
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
        "source_shape":p.source_shape,"source_layout":format!("{:?}",p.source_layout),"source_components":p.source_components.iter().map(|source|json!({"origin":source.origin,"image_shape":source.image_shape,"shapes":source.shapes,"valid_shapes":source.valid_shapes,"sampling":source.sampling})).collect::<Vec<_>>(),"source_type":format!("{:?}",p.source_type),"source_bits_per_sample":p.source_bits_per_sample,
        "source_photometric":p.source_photometric,"source_planar_configuration":p.source_planar_configuration,
        "source_orientation":p.source_orientation,"source_compression":p.source_compression,"source_lossy":p.source_lossy,
        "output_type":format!("{:?}",p.output_type),"output_shape":p.shape})
}

fn tiff_components_record(buffer: &tiff::ycbcr::Buffer) -> serde_json::Value {
    fn signature(bytes: &[u8], shape: impl Into<Vec<usize>>) -> serde_json::Value {
        json!({"dtype":"uint8","shape":shape.into(),"sample_sha256":format!("{:x}",Sha256::digest(bytes))})
    }
    let [horizontal, vertical] = buffer.subsampling().unwrap();
    let chunks: Vec<_> = buffer.chunks().unwrap().iter().map(|chunk| {
        let planes: Vec<_> = chunk.planes.iter().map(|plane| {
            let mut info = signature(&plane.samples, plane.shape);
            info["component"] = json!(format!("{:?}", plane.component));
            info["sampling"] = json!(plane.sampling);
            info["valid_shape"] = json!(plane.valid_shape);
            info
        }).collect();
        let units = chunk.sampling_units.as_ref().map(|bytes| signature(bytes, [chunk.planes[1].shape[0],chunk.planes[1].shape[1],horizontal*vertical+2]));
        json!({"origin":chunk.origin,"image_shape":chunk.image_shape,"sampling_units":units,"planes":planes})
    }).collect();
    json!({"shape":buffer.shape(),"subsampling":buffer.subsampling(),"precision":buffer.precision(),"storage":format!("{:?}",buffer.storage().unwrap()),"chunks":chunks})
}

fn edf_window(metadata: &dioptrin_io::edf::Metadata) -> serde_json::Value {
    json!({"declared_bytes":metadata.declared_stored_bytes,"effective_bytes":metadata.stored_bytes,
        "external_policy":metadata.external_window_policy.map(|policy|format!("{policy:?}")),
        "source":metadata.payload_source,"offset":metadata.payload_offset,
        "inline_layout_policy":metadata.inline_layout_policy.map(|policy|format!("{policy:?}")),
        "declared_header_bytes":metadata.declared_header_bytes,"parsed_header_bytes":metadata.parsed_header_bytes,
        "legacy_size_bytes":metadata.legacy_size_bytes,"declared_compression":metadata.declared_compression,
        "payload_interpretation":format!("{:?}",metadata.payload_interpretation)})
}

fn bytes_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn edf_members(members: &[dioptrin_io::edf::recovery::MemberReport]) -> serde_json::Value {
    json!(
        members
            .iter()
            .map(|member| json!({
                "encoded_range":[member.encoded_range.start,member.encoded_range.end],
                "decoded_range":[member.decoded_range.start,member.decoded_range.end],
                "integrity":format!("{:?}",member.integrity),"diagnostic":member.diagnostic
            }))
            .collect::<Vec<_>>()
    )
}

#[cfg(feature = "hdf5")]
fn printf_mappings(
    config: &serde_json::Value,
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
fn vds_selection(selection: &dioptrin_io::hdf5::typed::VdsSelection) -> serde_json::Value {
    use dioptrin_io::hdf5::typed::VdsSelection;
    match selection {
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
        } => json!({
            "kind":"Regular","shape":shape,"maximum_shape":maximum_shape,
            "start":start,"stride":stride,"count":count,"block":block}),
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
fn vds_value(p: &PixelBuffer) -> serde_json::Value {
    let mut hash = Sha256::new();
    let dtype = sample_dtype_and_hash(p, &mut hash);
    json!({"dtype":dtype,"sample_sha256":format!("{:x}",hash.finalize())})
}
#[cfg(feature = "hdf5")]
fn vds_mapping(m: &dioptrin_io::hdf5::typed::VdsMapping) -> serde_json::Value {
    json!({"stored_filename":m.stored_filename,"declared_dataset":m.declared_dataset,"source_file":m.source_file,"resolved_dataset":m.resolved_dataset,"source_shape":m.source_shape,"selection_layout":format!("{:?}",m.selection_layout),"source_bounding_shape":m.source_bounding_shape,"virtual_bounding_shape":m.virtual_bounding_shape,"selected_elements":m.selected_elements,"source_selection":vds_selection(&m.source_selection),"virtual_selection":vds_selection(&m.virtual_selection),"frozen_source_selection":vds_selection(&m.frozen_source_selection),"frozen_virtual_selection":vds_selection(&m.frozen_virtual_selection),"nested_node":m.nested_node,"printf_expansion":m.printf_expansion.as_ref().map(|e| {let d=&e.declaration;json!({"block_index":e.block_index,"declaration":{"source_file":d.source_file,"dataset":d.dataset,"mapping_index":d.mapping_index,"stored_filename":d.stored_filename,"declared_dataset":d.declared_dataset,"source_selection":vds_selection(&d.source_selection),"virtual_selection":vds_selection(&d.virtual_selection),"blocks":d.blocks,"plan":d.plan.as_ref().map(|p|json!({"source_file":p.source_file,"dataset":p.dataset,"mapping_index":p.mapping_index,"stored_filename":p.stored_filename,"declared_dataset":p.declared_dataset,"blocks":p.blocks}))}})}),"source_origin":m.source_origin,"virtual_origin":m.virtual_origin,"mapped_shape":m.mapped_shape,"source_pixel_type":format!("{:?}",m.source_pixel_type),"source_little_endian":m.source_little_endian,"source_layout":format!("{:?}",m.source_layout),"source_fill_value":m.source_fill_value.as_ref().map(vds_value),"source_fill_time":format!("{:?}",m.source_fill_time),"storage_allocated":m.storage_allocated,"filters":m.filters.iter().map(|f|format!("{f:?}")).collect::<Vec<_>>()})
}
#[cfg(feature = "hdf5")]
fn vds_metadata(v: &dioptrin_io::hdf5::typed::VdsMetadata) -> serde_json::Value {
    json!({"node_id":v.node_id,"source_file":v.source_file,"dataset":v.dataset,"fill_value":v.fill_value.as_ref().map(vds_value),"declared_shape":v.declared_shape,"frozen_shape":v.frozen_shape,"master_max_shape":v.master_max_shape,"unmapped_regions":format!("{:?}",v.unmapped_regions),"unmapped_pixels_per_frame":v.unmapped_pixels_per_frame,"unmapped_pixels_by_frame":v.unmapped_pixels_by_frame,"mappings":v.mappings.iter().map(vds_mapping).collect::<Vec<_>>(),"nested_nodes":v.nested_nodes.iter().map(|n|json!({"node_id":n.node_id,"source_file":n.source_file,"dataset":n.dataset,"declared_shape":n.declared_shape,"frozen_shape":n.frozen_shape,"master_max_shape":n.master_max_shape,"mappings":n.mappings.iter().map(vds_mapping).collect::<Vec<_>>(),"fill_value":n.fill_value.as_ref().map(vds_value),"fill_time":format!("{:?}",n.fill_time),"unmapped_pixels_per_frame":n.unmapped_pixels_per_frame,"unmapped_pixels_by_frame":n.unmapped_pixels_by_frame})).collect::<Vec<_>>(),"effective_fill_regions":v.effective_fill_regions.iter().map(|r|json!({"origin_node":r.origin_node,"source_file":r.source_file,"dataset":r.dataset,"origin_selection":vds_selection(&r.origin_selection),"virtual_selection":vds_selection(&r.virtual_selection),"value":vds_value(&r.value)})).collect::<Vec<_>>()})
}

#[cfg(feature = "xcalibur")]
fn xcalibur_point(point: dioptrin_io::xcalibur::Point) -> serde_json::Value {
    json!({"ix":point.x,"iy":point.y})
}
#[cfg(feature = "xcalibur")]
fn xcalibur_polygons(values: &[dioptrin_io::xcalibur::Polygon]) -> serde_json::Value {
    json!(
        values
            .iter()
            .map(|p| json!({"itype":p.kind,"ipoints":p.points,"iax":p.x,"iay":p.y}))
            .collect::<Vec<_>>()
    )
}
#[cfg(feature = "xcalibur")]
fn xcalibur_points(values: &[dioptrin_io::xcalibur::BadPoint]) -> serde_json::Value {
    json!(values.iter().map(|p|json!({"spt":xcalibur_point(p.point),"sptreplace1":xcalibur_point(p.replacements[0]),"sptreplace2":xcalibur_point(p.replacements[1]),"itreatment":p.treatment})).collect::<Vec<_>>())
}
#[cfg(feature = "xcalibur")]
fn xcalibur_row(p: &dioptrin_io::xcalibur::BadRow) -> serde_json::Value {
    json!({"sptstart":xcalibur_point(p.start),"sptend":xcalibur_point(p.end),"sptstartreplace":xcalibur_point(p.replacements[0]),"sptendreplace":xcalibur_point(p.replacements[1]),"itreatment":p.treatment})
}
#[cfg(feature = "xcalibur")]
fn xcalibur_fields(m: &dioptrin_io::xcalibur::Characteristics) -> serde_json::Value {
    let mut fields = json!({"dwversion":m.version,"ddarkcurrentinADUpersec":header_float(m.dark_current),"dreadnoiseinADU":header_float(m.read_noise),
        "iisfip60origin":m.fip_origin[0],"ifip60xorigin":m.fip_origin[1],"ifip60yorigin":m.fip_origin[2],
        "inumofcornermasks":m.corner_count,"iacornermaskx":m.corner_x,"iacornermasky":m.corner_y,
        "inumofglowingcornermasks":m.glowing_count,"iaglowingcornermaskx":m.glowing_x,"iaglowingcornermasky":m.glowing_y});
    for (key, text) in [
        "ccharacteristicsfil",
        "cccdproducer",
        "cccdchiptype",
        "cccdchipserial",
        "ctaperproducer",
        "ctapertype",
        "ctaperserial",
    ]
    .iter()
    .zip(&m.text)
    {
        fields[*key] = match text.text() {
            Ok(text) => json!(text),
            Err(_) => json!({"source_bytes_hex":bytes_hex(&text.0)}),
        };
    }
    if let Some(values) = &m.polygons {
        fields["ibadpolygons"] = json!(values.len());
        fields["pschipbadpolygon"] = xcalibur_polygons(values);
    }
    if let Some(values) = &m.points {
        fields["ibadpoints"] = json!(values.len());
        fields["pschipbadpoint"] = xcalibur_points(values);
    }
    for (i, suffix) in ["", "1x1", "2x2", "4x4"].iter().enumerate() {
        if let Some(values) = &m.columns[i] {
            fields[format!("ibadcolumns{suffix}")] = json!(values.len());
            fields[format!("pschipbadcolumn{suffix}")] = json!(
                values
                    .iter()
                    .map(|p| {
                        let mut value = xcalibur_row(&p.row);
                        value["ilowlimit"] = json!(p.low_limit);
                        value["ihighlimit"] = json!(p.high_limit);
                        value
                    })
                    .collect::<Vec<_>>()
            );
        }
        if let Some(values) = &m.rows[i] {
            fields[format!("ibadrows{suffix}")] = json!(values.len());
            fields[format!("pschipbadrow{suffix}")] =
                json!(values.iter().map(xcalibur_row).collect::<Vec<_>>());
        }
    }
    if let Some(e) = &m.electronics {
        fields["iscintillatorid"] = json!(e.scintillator);
        fields["dgain_mo"] = header_float(e.gain_mo);
        fields["dgain_cu"] = header_float(e.gain_cu);
        if let Some(machine) = &e.machine {
            fields["chipmachinefunction"] = json!({"iismachinefunction":machine.enabled,"da_machinefct":header_float(machine.a),"db_machinefct":header_float(machine.b)});
        }
    }
    fields
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--capabilities") {
        let mut features = vec!["native", "edf", "cbf", "tiff", "wrappers"];
        let mut adapters = vec![
            "npy",
            "dtrek",
            "dtrek-header",
            "edf-header",
            "tiff-header",
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
            (
                cfg!(feature = "xcalibur"),
                "xcalibur",
                &["xcalibur-ccd", "xcalibur-bytes", "xcalibur-decompose"][..],
            ),
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
            json!({"features":features,"adapters":adapters,"input_apis":["content_discovery","nonseekable_stream","buffer_snapshot"],"known_readers":{
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
    let stream_origin = args
        .iter()
        .position(|s| s == "--stream-origin")
        .map(|i| {
            args.get(i + 1)
                .map(std::path::Path::new)
                .ok_or("stream origin requires a directory")
        })
        .transpose()?;
    let streaming = args.iter().any(|s| s == "--stream");
    let stream_input = if streaming && !args.iter().any(|s| s == "--edf-recovery") {
        Some(wrapped::Input::from_reader(
            std::io::stdin().lock(),
            stream_origin,
            wrapped::Limits::default(),
        )?)
    } else {
        None
    };
    if let Some(input) = &stream_input {
        let mut roots = input
            .transport_paths()?
            .iter()
            .map(|p| p.to_path_buf())
            .collect::<Vec<_>>();
        for root in roots.clone() {
            let canonical = std::fs::canonicalize(root)?;
            if !roots.contains(&canonical) {
                roots.push(canonical);
            }
        }
        eprintln!(
            "{}",
            json!({"input_transport":{"captured_root":input.source_path()?,"captured_roots":roots,"original_argument":path}})
        );
    }
    let open_image = |path: &str,
                      format: wrapped::Format,
                      transport: wrapped::Limits,
                      image: native::Limits,
                      tiff: tiff::Limits| {
        if let Some(input) = &stream_input {
            input.clone().read(format, image, tiff)
        } else {
            wrapped::Reader::open(path, format, transport, image, tiff)
        }
    };
    if args[1] == "discover" {
        let input = match &stream_input {
            Some(input) => input.clone(),
            None => wrapped::Input::open(path, wrapped::Limits::default())?,
        };
        let detection = input.discover()?;
        let resolution = detection.require_unique();
        let p = input.provenance()?;
        println!(
            "{}",
            json!({"candidates":detection.candidates.iter().map(|c|json!({"codec":format!("{:?}",c.codec),"evidence":format!("{:?}",c.evidence),"reason":c.reason,"feature":c.codec.feature(),"available":c.codec.available()})).collect::<Vec<_>>(),"selected":resolution.as_ref().ok().map(|c|format!("{c:?}")),"resolution_error":resolution.err().map(|e|e.to_string()),"provenance":{"stream":p.stream,"origin_directory":p.origin_directory,"encoded_bytes":p.encoded_bytes,"decoded_bytes":p.decoded_bytes,"compression":format!("{:?}",p.compression)}})
        );
        return Ok(());
    }
    let staged_path = stream_input
        .as_ref()
        .map(|input| input.source_path())
        .transpose()?;
    let source_path = staged_path.unwrap_or_else(|| std::path::Path::new(path));

    #[cfg(feature = "xcalibur")]
    if args[1] == "xcalibur-ccd" || args[1] == "xcalibur-bytes" {
        use dioptrin_io::xcalibur::{Projection, ReadPolicy, Reader};
        let policy = if args.iter().any(|s| s == "--retain-truncated-tail") {
            ReadPolicy::RetainTruncatedMachineFunction
        } else {
            ReadPolicy::Strict
        };
        let reader = Reader::open_with_policy(source_path, native::Limits::default(), policy)?;
        let parsed;
        let m = if args[1] == "xcalibur-bytes" {
            parsed = dioptrin_io::xcalibur::Characteristics::from_bytes_with_policy(
                &reader.metadata()?.original,
                native::Limits::default(),
                policy,
            )?;
            &parsed
        } else {
            reader.metadata()?
        };
        let mut record = json!({"kind":"xcalibur_characteristics","fields":xcalibur_fields(m),"source_sha256":format!("{:x}",Sha256::digest(&m.original)),"parsed_bytes":m.parsed_bytes,"source_bytes":m.original.len(),"trailing_sha256":format!("{:x}",Sha256::digest(&m.original[m.parsed_bytes..])),"missing_sections":m.columns.iter().filter(|v|v.is_none()).count()+m.rows.iter().filter(|v|v.is_none()).count()+usize::from(m.polygons.is_none())+usize::from(m.points.is_none())+usize::from(m.electronics.is_none())});
        record["incomplete_machine_function"] = match &m.incomplete_machine_function {
            Some(d) => {
                json!({"offset":d.offset,"expected_bytes":d.expected_bytes,"available_bytes":d.available_bytes})
            }
            None => json!(null),
        };
        if args.len() >= 5 && !args[3].starts_with("--") {
            let shape = [args[3].parse()?, args[4].parse()?];
            let projection = if args.get(5).is_some_and(|s| s == "fabio") {
                Projection::Fabio2026
            } else {
                Projection::Strict
            };
            let mask = reader.build_mask(shape, projection, native::Limits::default())?;
            record["mask"] = json!({"dtype":"uint8","shape":shape,"sha256":format!("{:x}",Sha256::digest(&mask.values)),"ignored_records":mask.ignored_records,"kind":"explicit_derived_mask"});
        }
        println!("{record}");
        return Ok(());
    }
    #[cfg(feature = "xcalibur")]
    if args[1] == "xcalibur-decompose" {
        let shape = [
            args.get(3).ok_or("height required")?.parse()?,
            args.get(4).ok_or("width required")?.parse()?,
        ];
        let full = args.get(5).is_some_and(|s| s == "full");
        let raw = native::Reader::open_raw(
            path,
            native::RawLayout {
                shape,
                pixel_type: PixelType::U8,
                little_endian: true,
                offset: native::Offset::Absolute(0),
            },
            native::Limits::default(),
        )?;
        let mut buffer = native::FrameBuffer::default();
        raw.read_frame_into(0, &mut buffer)?;
        let PixelBuffer::U8(values) = buffer.pixels().unwrap() else {
            unreachable!()
        };
        let regions =
            dioptrin_io::xcalibur::decompose(values, shape, full, native::Limits::default())?;
        let mask = regions.build_mask(native::Limits::default())?;
        println!(
            "{}",
            json!({"kind":"explicit_mask_decomposition","complete":regions.complete,"polygons":xcalibur_polygons(&regions.polygons),"points":xcalibur_points(&regions.points),"reconstructed":{"dtype":"uint8","shape":shape,"sha256":format!("{:x}",Sha256::digest(&mask.values))},"source_sha256":format!("{:x}",Sha256::digest(values))})
        );
        return Ok(());
    }

    if args[1] == "edf-header" {
        let reader = wrapped::HeaderReader::open(
            source_path,
            wrapped::HeaderFormat::Edf,
            wrapped::Limits::default(),
            native::Limits::default(),
            tiff::Limits::default(),
        )?;
        let wrapped::HeaderMetadata::Edf(headers) = reader.metadata()? else {
            unreachable!()
        };
        let general = headers
            .general_headers()
            .iter()
            .map(|m| json!(m.fields))
            .collect::<Vec<_>>();
        for (index, m) in headers.images().enumerate() {
            println!(
                "{}",
                json!({"header_only":true,"pixels_available":false,"frame":index,"fields":m.fields,"declared_shape":m.shape,"declared_dtype":declared_dtype(m.pixel_type),"general_headers":general,"source_prefix_hex":bytes_hex(&reader.source_prefix()?)})
            );
        }
        return Ok(());
    }
    if args[1] == "dtrek-header" {
        let reader = wrapped::HeaderReader::open(
            source_path,
            wrapped::HeaderFormat::Dtrek,
            wrapped::Limits::default(),
            native::Limits::default(),
            tiff::Limits::default(),
        )?;
        let wrapped::HeaderMetadata::Dtrek(m) = reader.metadata()? else {
            unreachable!()
        };
        println!(
            "{}",
            json!({"header_only":true,"fields":m.fields,"declared_shape":m.source_shape,"pixels_available":false})
        );
        return Ok(());
    }
    if args[1] == "tiff-header" {
        let reader = wrapped::HeaderReader::open(
            source_path,
            wrapped::HeaderFormat::Tiff,
            wrapped::Limits::default(),
            native::Limits::default(),
            tiff::Limits::default(),
        )?;
        let prefix = reader.source_prefix()?;
        let wrapped::HeaderMetadata::Tiff(pages) = reader.metadata()? else {
            unreachable!()
        };
        for (index, page) in pages.iter().enumerate() {
            println!(
                "{}",
                json!({"header_only":true,"frame":index,"fields":{"tags":page.tags.iter().map(|(k,v)|(k.to_string(),tiff_tag(v))).collect::<serde_json::Map<_,_>>()},"ifd_offset":page.ifd_offset,"source_prefix_hex":bytes_hex(&prefix),
                "opaque_tags":page.opaque_tags.iter().map(|(k,v)|(k.to_string(),json!({"field_type":v.field_type,"count":v.count,"value_hex":bytes_hex(&v.value_bytes)}))).collect::<serde_json::Map<_,_>>(),"pixels_available":false})
            );
        }
        return Ok(());
    }

    if args[1] == "edf" && args.iter().any(|value| value.starts_with("--edf-")) {
        use dioptrin_io::edf::{self, InlineLayout, PayloadInterpretation};
        let mut options = edf::Options {
            external_payloads: edf::ExternalPayloads::FollowCompressedSnapshot,
            external_basenames: true,
            external_window: edf::ExternalWindow::UpToDeclared,
            ..Default::default()
        };
        if stream_input.is_some() {
            options.source_directory = stream_origin.map(std::path::Path::to_path_buf);
            if stream_origin.is_none() {
                return Err("external EDF stream policy requires origin".into());
            }
        }
        if args.iter().any(|value| value == "--edf-inline-size") {
            options.inline_layout = InlineLayout::LegacySizeAtParsedBoundary;
        }
        if let Some(position) = args.iter().position(|value| value == "--edf-raw-label") {
            options.payload_interpretation = PayloadInterpretation::StoredRawForLabel(
                args.get(position + 1)
                    .ok_or("--edf-raw-label requires exact label")?
                    .clone(),
            );
        }
        if args.iter().any(|value| value == "--edf-recovery") {
            let recovery_options = edf::recovery::Options {
                edf: options,
                ..Default::default()
            };
            let reader = if streaming {
                edf::recovery::Reader::from_reader(
                    std::io::stdin().lock(),
                    stream_origin,
                    native::Limits::default(),
                    recovery_options,
                )?
            } else {
                edf::recovery::Reader::open(
                    source_path,
                    native::Limits::default(),
                    recovery_options,
                )?
            };
            if streaming {
                eprintln!(
                    "{}",
                    json!({"input_transport":{"captured_root":reader.source_report().path,"captured_roots":[reader.source_report().path],"original_argument":path}})
                );
            }
            let mut buffer = edf::recovery::SampleBuffer::default();
            let mut images = Vec::new();
            for i in 0..reader.image_records() {
                reader.read_samples_into(i, &mut buffer)?;
                let span = buffer.report().unwrap();
                let mut image = frame_record(
                    i,
                    reader.image_records(),
                    [span.present_samples.len()],
                    buffer.pixels().unwrap(),
                    json!(reader.metadata(i).unwrap().fields),
                    &[std::path::Path::new(path)],
                );
                image["availability"] = json!({"declared_shape":span.declared_shape,"declared_samples":span.declared_samples,
                    "present_samples":[span.present_samples.start,span.present_samples.end],
                    "missing_samples":span.missing_samples.as_ref().map(|range|[range.start,range.end]),
                    "verified_samples":[span.verified_samples.start,span.verified_samples.end],
                    "outer_integrity":format!("{:?}",span.outer_integrity),
                    "payload_integrity":format!("{:?}",span.payload_integrity),
                    "expected_payload_bytes":span.expected_payload_bytes,"available_payload_bytes":span.available_payload_bytes,
                    "availability":format!("{:?}",span.availability),"payload_members":edf_members(&span.payload_members),
                    "diagnostic":span.diagnostic,
                    "complete_verified_frame":span.complete_verified_frame,
                    "partial_element_bytes":bytes_hex(buffer.partial_element_bytes().unwrap())});
                image["payload_window"] = edf_window(reader.metadata(i).unwrap());
                images.push(image);
            }
            let source = reader.source_report();
            let stop = reader.stop().map(|stop|match stop {
                edf::recovery::Stop::TruncatedHeader{offset,bytes} => json!({"kind":"TruncatedHeader","offset":offset,"bytes_hex":bytes_hex(bytes),"sha256":format!("{:x}",Sha256::digest(bytes))}),
                edf::recovery::Stop::TruncatedPayload{image_record,offset,expected_bytes,available_bytes} => json!({"kind":"TruncatedPayload","image_record":image_record,"offset":offset,"expected_bytes":expected_bytes,"available_bytes":available_bytes}),
                edf::recovery::Stop::UnparsedRecord{offset,header,reason} => json!({"kind":"UnparsedRecord","offset":offset,"header_hex":bytes_hex(header),"reason":reason}),
                edf::recovery::Stop::NoImageHeader => json!({"kind":"NoImageHeader"}),
            });
            println!(
                "{}",
                json!({"sources":[path],"source_recovery":{"format":"edf","images":images,
                "source_report":{"path":source.path,"encoded_bytes":source.encoded_bytes,"decoded_bytes":source.decoded_bytes,
                    "compression":format!("{:?}",source.compression),"integrity":format!("{:?}",source.integrity),
                    "verified_decoded_bytes":source.verified_decoded_bytes,"members":edf_members(&source.members),"diagnostic":source.diagnostic},"stop":stop}})
            );
        } else {
            let reader =
                edf::Reader::open_with_options(source_path, native::Limits::default(), options)?;
            let paths: Vec<_> = reader
                .payload_sources()
                .iter()
                .map(|source| source.path.as_path())
                .collect();
            let mut buffer = edf::FrameBuffer::default();
            for i in 0..reader.frames() {
                reader.read_frame_into(i, &mut buffer)?;
                let mut row = frame_record(
                    i,
                    reader.frames(),
                    buffer.shape().unwrap(),
                    buffer.pixels().unwrap(),
                    json!(reader.metadata(i).unwrap().fields),
                    &paths,
                );
                row["metadata_details"] = json!({"general_headers":reader.general_headers().iter().map(|h|json!(h.fields)).collect::<Vec<_>>(),"incomplete_data":false});
                row["payload_window"] = edf_window(reader.metadata(i).unwrap());
                println!("{row}");
            }
        }
        return Ok(());
    }
    #[cfg(feature = "sparse")]
    if args[1] == "sparse" {
        use dioptrin_io::sparse::{
            ArrayBuffer, ExternalLinks, FrameBuffer, Options, Reconstruction, SparseFrameBuffer,
        };
        let input = stream_input.clone().map_or_else(
            || wrapped::Input::open(path, wrapped::Limits::default()),
            Ok,
        )?;
        let owner = input.read_sparse(
            Default::default(),
            Options {
                external_links: ExternalLinks::FollowSnapshot,
                reconstruction: Reconstruction::DeterministicBackground,
                ..Options::default()
            },
        )?;
        let reader = owner.sparse_reader().unwrap();
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
                if component.pixels().unwrap().len() == 1 {
                    let scalar = match component.pixels().unwrap() {
                        PixelBuffer::F32(v) => header_float(f64::from(v[0])),
                        PixelBuffer::F64(v) => header_float(v[0]),
                        PixelBuffer::U32(v) => json!(v[0]),
                        PixelBuffer::I32(v) => json!(v[0]),
                        PixelBuffer::U16(v) => json!(v[0]),
                        PixelBuffer::U8(v) => json!(v[0]),
                        _ => json!(null),
                    };
                    info["scalar_value"] = scalar;
                }
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
                row["metadata_details"] = json!({"components":components,"component_inventory":reader.metadata().components.iter().map(|c|json!({"name":c.name,"text":c.text,"dataset":c.dataset})).collect::<Vec<_>>(),"group_inventory":reader.metadata().attributes.iter().map(|a|json!(a.path)).collect::<Vec<_>>()});
            }
            println!("{row}");
        }
        return Ok(());
    }
    #[cfg(feature = "hdf5")]
    if matches!(args[1].as_str(), "hdf5" | "eiger" | "lima" | "lambda") {
        use dioptrin_io::hdf5::typed::{
            ExternalLinks, FilenameBinding, FrameBuffer, Options, Selection, UnmappedRegions,
            VdsOptions,
        };
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
        let options = Options {
            external_links: ExternalLinks::FollowSnapshot,
            ..Options::default()
        };
        let input = stream_input.clone().map_or_else(
            || wrapped::Input::open(file, wrapped::Limits::default()),
            Ok,
        )?;
        let owner = if let Some(position) = args.iter().position(|value| value == "--vds-map") {
            let config: serde_json::Value = serde_json::from_slice(&std::fs::read(
                args.get(position + 1)
                    .ok_or("--vds-map requires a JSON file")?,
            )?)?;
            let bindings: Vec<_> = config["filenames"]
                .as_array()
                .ok_or("VDS map requires filenames array")?
                .iter()
                .map(|binding| {
                    Ok(FilenameBinding {
                        stored_filename: binding["stored_filename"]
                            .as_str()
                            .ok_or("binding requires stored_filename")?
                            .into(),
                        actual_file: binding["actual_file"]
                            .as_str()
                            .ok_or("binding requires actual_file")?
                            .into(),
                    })
                })
                .collect::<Result<_, Box<dyn Error>>>()?;
            let unmapped_regions = match config["unmapped_regions"].as_str() {
                Some("Reject") => UnmappedRegions::Reject,
                Some("DeclaredFill") => UnmappedRegions::DeclaredFill,
                _ => return Err("VDS map requires explicit Reject or DeclaredFill policy".into()),
            };
            let printf = printf_mappings(&config)?;
            input.read_hdf5(
                selection,
                options,
                Some(VdsOptions {
                    filenames: &bindings,
                    unmapped_regions,
                    printf_mappings: &printf,
                }),
            )?
        } else {
            input.read_hdf5(selection, options, None)?
        };
        let reader = owner.hdf5_reader().unwrap();
        let paths: Vec<_> = reader
            .sources()
            .iter()
            .map(|source| source.path.as_path())
            .collect();
        let mut buffer = FrameBuffer::default();
        for i in 0..reader.frames() {
            reader.read_frame_into(i, &mut buffer)?;
            let mut row = frame_record(
                i,
                reader.frames(),
                buffer.shape().unwrap(),
                buffer.pixels().unwrap(),
                json!(reader.fields()),
                &paths,
            );
            let m = reader.metadata(i).unwrap();
            row["metadata_details"] = json!({"source_file":m.source_file,"dataset":m.dataset,"source_shape":m.virtual_dataset.as_ref().map_or(m.source_shape.clone(),|v|v.frozen_shape.to_vec())});
            if let Some(vds) = &reader.metadata(i).unwrap().virtual_dataset {
                row["virtual_dataset"] = json!({
                    "declared_shape":vds.declared_shape,"frozen_shape":vds.frozen_shape,
                    "master_max_shape":vds.master_max_shape,
                    "unmapped_regions":format!("{:?}",vds.unmapped_regions),
                    "unmapped_pixels_per_frame":vds.unmapped_pixels_per_frame,
                    "unmapped_pixels_by_frame":vds.unmapped_pixels_by_frame,
                    "fill_value":format!("{:?}",reader.metadata(i).unwrap().fill_value),
                    "mappings":vds.mappings.iter().map(|mapping|json!({
                        "stored_filename":mapping.stored_filename,"declared_dataset":mapping.declared_dataset,
                        "source_file":mapping.source_file,"resolved_dataset":mapping.resolved_dataset,
                        "source_shape":mapping.source_shape,"selection_layout":format!("{:?}",mapping.selection_layout),"source_bounding_shape":mapping.source_bounding_shape,"virtual_bounding_shape":mapping.virtual_bounding_shape,"selected_elements":mapping.selected_elements,"source_origin":mapping.source_origin,
                        "virtual_origin":mapping.virtual_origin,"mapped_shape":mapping.mapped_shape,
                        "source_selection":vds_selection(&mapping.source_selection),
                        "virtual_selection":vds_selection(&mapping.virtual_selection),
                        "source_pixel_type":format!("{:?}",mapping.source_pixel_type),
                        "source_little_endian":mapping.source_little_endian,"source_layout":format!("{:?}",mapping.source_layout),
                        "source_fill_value":format!("{:?}",mapping.source_fill_value),"source_fill_time":format!("{:?}",mapping.source_fill_time),
                        "filters":format!("{:?}",mapping.filters),"storage_allocated":mapping.storage_allocated
                    })).collect::<Vec<_>>()});
                // Keep the established comparison schema and expose the complete graph separately.
                row["vds_graph"] = vds_metadata(vds);
            }
            println!("{row}");
        }
        return Ok(());
    }
    let limits = native::Limits::default();
    #[cfg(feature = "bruker")]
    if args[1] == "bruker86-view" {
        let mut reader = open_image(
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
        row["metadata_details"] = metadata_details(reader.metadata(0).unwrap(), 0);
        if meta
            .linear
            .is_some_and(|[slope, offset]| slope != 1.0 || offset != 0.0)
        {
            let mut decoded_reader = open_image(
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
        if args.len() < 8 {
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
        let mut reader = open_image(
            path,
            wrapped::Format::Raw(native::RawLayout {
                shape: [args[4].parse()?, args[3].parse()?],
                pixel_type: ty,
                little_endian,
                offset,
            }),
            wrapped::Limits::default(),
            limits,
            tiff::Limits::default(),
        )?;
        let mut buffer = wrapped::FrameBuffer::default();
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
        "edf" => wrapped::Format::EdfExternalWindow {
            policy: dioptrin_io::edf::ExternalPayloads::FollowCompressedSnapshot,
            basenames: true,
            window: dioptrin_io::edf::ExternalWindow::UpToDeclared,
        },
        "cbf" => wrapped::Format::Cbf(cbf::ByteOffsetMode::Legacy32),
        "cbf64" => wrapped::Format::Cbf(cbf::ByteOffsetMode::Extended64),
        "tiff" | "tiff-pillow13" | "tiff-tiffio"
            if args.iter().any(|value| {
                value == "--tiff-compat" || value == "--ojpeg-independent-reset-tiles"
            }) =>
        {
            wrapped::Format::TiffWithOptions(tiff::Options {
                uncompressed_windows: if args.iter().any(|v| v == "--tiff-compat") {
                    tiff::UncompressedWindows::FromShape
                } else {
                    tiff::UncompressedWindows::Declared
                },
                tile_tables: if args.iter().any(|v| v == "--tiff-compat") {
                    tiff::TileTables::FromStripTags
                } else {
                    tiff::TileTables::Declared
                },
                ojpeg_tiles: if args.iter().any(|v| v == "--ojpeg-independent-reset-tiles") {
                    tiff::OjpegTiles::IndependentResetTiles
                } else {
                    tiff::OjpegTiles::Declared
                },
            })
        }
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
    let mut reader = open_image(
        path,
        format,
        wrapped::Limits::default(),
        limits,
        tiff::Limits::default(),
    )?;
    let mut buffer = wrapped::FrameBuffer::default();
    let mut dataset_hash = Sha256::new();
    for i in 0..reader.frames() {
        if args.iter().any(|value| value == "--tiff-available")
            && matches!(reader.metadata(i), Some(wrapped::Metadata::Tiff(metadata)) if metadata.compression == 32809)
        {
            let mut available = tiff::thunderscan::AvailableFrame::default();
            reader.read_tiff_thunderscan_available_into(i, &mut available)?;
            let pixels = available.pixels().unwrap();
            let mut row = frame_record(
                i,
                reader.frames(),
                [pixels.len()],
                pixels,
                json!({}),
                &reader.source_paths(),
            );
            let wrapped::Metadata::Tiff(metadata) = reader.metadata(i).unwrap() else {
                unreachable!()
            };
            row["fields"] = tiff_fields(metadata);
            row["source_recovery"] = json!({"format":"tiff-thunderscan","declared_shape":available.shape(),
                "source_bits":metadata.bits_per_sample,"source_pixel_type":format!("{:?}",metadata.pixel_type),
                "source_orientation":metadata.orientation,"source_little_endian":metadata.little_endian,"source_compression":metadata.compression,
                "available_rows":available.available_rows().unwrap().iter().map(|span|json!({"start_row":span.start_row,"rows":span.rows,"sample_offset":span.sample_offset})).collect::<Vec<_>>(),
                "missing_rows":available.missing_rows().unwrap().iter().map(|span|json!({"start_row":span.start_row,"rows":span.rows,"strip":span.strip,"reason":span.reason})).collect::<Vec<_>>(),
                "strips":available.strips().unwrap().iter().map(|strip|json!({"offset":strip.offset,"declared_bytes":strip.declared_bytes,"consumed_bytes":strip.consumed_bytes,"requested_rows":strip.requested_rows,"complete_rows":strip.complete_rows,"integrity":format!("{:?}",strip.integrity)})).collect::<Vec<_>>(),
                "complete":available.complete()});
            println!("{row}");
            continue;
        }
        if let wrapped::Metadata::Tiff(metadata) = reader.metadata(i).unwrap()
            && metadata.uniform_sample_shape().is_none()
        {
            let metadata_fields = tiff_fields(metadata);
            let mut components = tiff::ycbcr::Buffer::default();
            reader.read_tiff_ycbcr_into(i, &mut components)?;
            let mut row = json!({"frame":i,"frames":reader.frames(),"fields":metadata_fields,"sources":reader.source_paths(),"tiff_components":tiff_components_record(&components)});
            #[cfg(feature = "jpeg")]
            if let Some(original) = reader.tiff_ojpeg_metadata(i) {
                row["source_codec"] = json!({
                    "reconstruction":format!("{:?}",original.reconstruction),
                    "encoded_shape":original.encoded_shape,"precision":original.precision,"subsampling":original.subsampling,
                    "restart_interval":original.restart_interval,"restart_markers_inserted":original.restart_markers_inserted,
                    "tile_policy":format!("{:?}",original.tile_policy),"partition":format!("{:?}",original.partition),"scan_sources":original.scan_sources.iter().map(|scan|json!({"component":format!("{:?}",scan.component),"header_windows":scan.header_windows.iter().map(|w|json!({"offset":w.offset,"bytes":w.bytes,"role":w.role})).collect::<Vec<_>>(),"entropy_windows":scan.entropy_windows.iter().map(|w|json!({"offset":w.offset,"bytes":w.bytes,"role":w.role})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
                    "fragments":original.fragments.iter().map(|f|json!({"index":f.index,"source_offset":f.source_offset,"source_bytes":f.source_bytes,"header_bytes":f.header_bytes,"entropy_offset":f.entropy_offset,"entropy_bytes":f.entropy_bytes,"origin":f.origin,"stream_origin":f.stream_origin,"image_shape":f.image_shape,"encoded_shape":f.encoded_shape})).collect::<Vec<_>>(),
                    "source_windows":original.source_windows.iter().map(|window|json!({"offset":window.offset,"bytes":window.bytes,"role":window.role})).collect::<Vec<_>>(),
                    "baseline_sos_normalized":original.baseline_sos_normalized,
                    "original_jpeg_header_bytes":original.original_jpeg_header.len(),
                    "original_jpeg_header_sha256":format!("{:x}",Sha256::digest(&original.original_jpeg_header))});
            }
            let policy = match args[1].as_str() {
                "tiff-pillow13" => Some(tiff::view::Policy::Pillow13),
                "tiff-tiffio" => Some(tiff::view::Policy::FabioTiffIo),
                _ => None,
            };
            if let Some(policy) = policy {
                match reader.read_tiff_view(i, policy, limits.frame_bytes) {
                    Ok(image) => {
                        row["reference_view"] =
                            tiff_view_record(i, reader.frames(), &image, &reader.source_paths());
                        row["view_policy"] = tiff_view_policy(&image);
                    }
                    Err(error) => row["view_error"] = json!(error.to_string()),
                }
            }
            println!("{row}");
            continue;
        }
        reader.read_frame_into(i, &mut buffer)?;
        let fields = match reader.metadata(i).unwrap() {
            #[cfg(feature = "hdf5")]
            wrapped::Metadata::Hdf5(_) => json!(reader.hdf5_reader().unwrap().fields()),
            #[cfg(feature = "sparse")]
            wrapped::Metadata::Sparse(_) => json!({}),
            #[cfg(feature = "mar345")]
            wrapped::Metadata::Mar345(m) => json!(m.fields),
            #[cfg(feature = "esperanto")]
            wrapped::Metadata::Esperanto(m) => json!(m.fields),
            #[cfg(feature = "fit2d")]
            wrapped::Metadata::Fit2d(m) => json!(
                m.fields
                    .iter()
                    .map(|(k, v)| {
                        use dioptrin_io::fit2d::HeaderValue as V;
                        (
                            k.clone(),
                            match v {
                                V::Text(v) => json!(v),
                                V::Integer(v) => json!(v),
                                V::Float(v) => header_float(f64::from(*v)),
                            },
                        )
                    })
                    .collect::<serde_json::Map<_, _>>()
            ),
            #[cfg(feature = "kcd")]
            wrapped::Metadata::Kcd(m) => json!(m.fields),
            #[cfg(feature = "mpa")]
            wrapped::Metadata::Mpa(m) => json!(m.fields),
            wrapped::Metadata::DetectorTiff(m) => detector_fields(m.header),
            #[cfg(feature = "fixed")]
            wrapped::Metadata::Ge(m) => json!(
                m.fields
                    .iter()
                    .map(|(k, v)| {
                        use dioptrin_io::ge::HeaderValue as V;
                        (
                            k.to_string(),
                            match v {
                                V::Bytes(v) => header_bytes(v),
                                V::U16(v) => json!(v),
                                V::U32(v) => json!(v),
                                V::F32(v) => header_float(f64::from(*v)),
                                V::F64(v) => header_float(*v),
                            },
                        )
                    })
                    .collect::<serde_json::Map<_, _>>()
            ),
            #[cfg(feature = "fixed")]
            wrapped::Metadata::Pixi(_) => json!({}),
            #[cfg(feature = "jpeg")]
            wrapped::Metadata::Jpeg(_) => json!({}),
            #[cfg(feature = "jpeg2000")]
            wrapped::Metadata::Jpeg2000(_) => json!({}),
            #[cfg(feature = "containers")]
            wrapped::Metadata::Spe(m) => json!(m.fields),
            #[cfg(feature = "containers")]
            wrapped::Metadata::Mrc(m) => mrc_fields(m),
            #[cfg(feature = "containers")]
            wrapped::Metadata::Dm3(_) => json!({}),
            #[cfg(feature = "bruker")]
            wrapped::Metadata::Bruker(m) => json!(m.fields),
            #[cfg(feature = "oxford")]
            wrapped::Metadata::Oxford(m) => json!(
                m.fields
                    .iter()
                    .map(|(k, v)| {
                        use dioptrin_io::oxford::HeaderValue as V;
                        (
                            k.clone(),
                            match v {
                                V::Bytes(v) => header_bytes(v),
                                V::Unsigned(v) => json!(v),
                                V::Signed(v) => json!(v),
                                V::Float(v) => header_float(*v),
                            },
                        )
                    })
                    .collect::<serde_json::Map<_, _>>()
            ),
            #[cfg(feature = "raxis")]
            wrapped::Metadata::Raxis(m) => json!(
                m.fields
                    .iter()
                    .map(|(k, v)| {
                        use dioptrin_io::raxis::HeaderValue as V;
                        (
                            k.clone(),
                            match v {
                                V::Bytes(v) => header_bytes(v),
                                V::Integer(v) => json!(v),
                                V::Float(v) => header_float(f64::from(*v)),
                            },
                        )
                    })
                    .collect::<serde_json::Map<_, _>>()
            ),
            #[cfg(feature = "xsd")]
            wrapped::Metadata::Xsd(_) => json!({}),
            wrapped::Metadata::Native(m) => json!(m.fields),
            wrapped::Metadata::Edf(m) => json!(m.fields),
            wrapped::Metadata::Cbf(m) => json!(m.binary_fields),
            wrapped::Metadata::Tiff(m) => tiff_fields(m),
        };
        let mut row = frame_record(
            i,
            reader.frames(),
            buffer.sample_shape().unwrap(),
            buffer.pixels().unwrap(),
            fields,
            &reader.source_paths(),
        );
        row["metadata_details"] = metadata_details(reader.metadata(i).unwrap(), i);
        if args[1] == "npy" {
            sample_dtype_and_hash(buffer.pixels().unwrap(), &mut dataset_hash);
            if i + 1 == reader.frames() {
                row["metadata_details"]["dataset_sha256"] =
                    json!(format!("{:x}", dataset_hash.clone().finalize()));
            }
        }
        #[cfg(feature = "fit2d")]
        if let wrapped::Metadata::Fit2d(metadata) = reader.metadata(i).unwrap() {
            let mut arrays = serde_json::Map::new();
            let mut array_buffer = dioptrin_io::fit2d::FrameBuffer::default();
            for (index, array) in metadata.arrays.iter().enumerate() {
                if array.name == "data_array" {
                    continue;
                }
                reader.read_fit2d_array_into(index, &mut array_buffer)?;
                let record = frame_record(
                    0,
                    1,
                    array.shape,
                    array_buffer.pixels().unwrap(),
                    json!({}),
                    &reader.source_paths(),
                );
                arrays.insert(array.name.clone(), json!({"kind":"array","dtype":record["dtype"],"shape":array.shape,"sha256":record["sample_sha256"]}));
            }
            row["metadata_details"]["array_values"] = json!(arrays);
        }

        if let wrapped::Metadata::Tiff(metadata) = reader.metadata(i).unwrap()
            && let Some(compatibility) = &metadata.compatibility
        {
            row["tiff_compatibility"] = json!({
                "source_ifd_offset":compatibility.source_ifd_offset,
                "source_entries":compatibility.source_entries.iter().map(|entry|json!({
                    "tag":entry.tag,"field_type":entry.field_type,"count":entry.count,
                    "entry_offset":entry.entry_offset,"ifd_entry":entry.ifd_entry,"payload_offset":entry.payload_offset
                })).collect::<Vec<_>>(),
                "operations":compatibility.operations.iter().map(|operation|format!("{operation:?}")).collect::<Vec<_>>(),
                "kind":format!("{:?}",compatibility.kind),"rows_per_strip":compatibility.rows_per_strip,
                "windows":compatibility.windows.iter().map(|window|json!({"offset":window.offset,"bytes":window.bytes})).collect::<Vec<_>>()});
        }
        if let wrapped::Metadata::Edf(metadata) = reader.metadata(i).unwrap() {
            row["metadata_details"] = json!({"general_headers":reader.edf_general_headers()?.iter().map(|h|json!(h.fields)).collect::<Vec<_>>(),"incomplete_data":false});
            row["payload_window"] = edf_window(metadata);
        }
        #[cfg(feature = "oxford")]
        if let wrapped::Metadata::Oxford(metadata) = reader.metadata(i).unwrap()
            && metadata.compression == dioptrin_io::oxford::Compression::Ty5
        {
            let PixelBuffer::I32(values) = buffer.pixels().unwrap() else {
                return Err("Oxford TY5 native output must retain signed32 integers".into());
            };
            if values
                .len()
                .checked_mul(8)
                .is_none_or(|bytes| bytes > limits.frame_bytes)
            {
                row["view_error"] =
                    json!("Oxford reference float64 representation exceeds output limit");
            } else {
                // Exact representation conversion for this pinned Python API;
                // native production reads and the signature above stay I32.
                let pixels = PixelBuffer::F64(values.iter().copied().map(f64::from).collect());
                row["reference_view"] = frame_record(
                    i,
                    reader.frames(),
                    buffer.sample_shape().unwrap(),
                    &pixels,
                    json!({}),
                    &reader.source_paths(),
                );
                row["view_policy"] = json!({"name":"OxfordTY5.FabIO2026.6.Float64","kind":"reference_dtype_view", "source_type":"I32","output_type":"F64","exact_integer_conversion":true});
            }
        }
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
