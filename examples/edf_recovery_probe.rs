//! Scoped EDF complete-compatibility and exact available-sample proof.
use dioptrin_io::{
    PixelBuffer,
    edf::{self, recovery},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
fn signature(pixels: &PixelBuffer) -> Value {
    let mut hash = Sha256::new();
    macro_rules! digest {($($v:ident,$name:literal);*)=>{match pixels{$(PixelBuffer::$v(values)=>{for value in values{hash.update(value.to_le_bytes());}$name},)*_=>panic!("EDF requires scalar samples")}};}
    let dtype = digest!(Bool,"bool";F16,"float16";U8,"uint8";I8,"int8";U16,"uint16";I16,"int16";U32,"uint32";I32,"int32";U64,"uint64";I64,"int64";F32,"float32";F64,"float64");
    json!({"dtype":dtype,"shape":[pixels.len()],"sample_sha256":format!("{:x}",hash.finalize())})
}
fn window(m: &edf::Metadata) -> Value {
    json!({"declared_bytes":m.declared_stored_bytes,"effective_bytes":m.stored_bytes,"external_policy":m.external_window_policy.map(|p|format!("{p:?}")),"source":m.payload_source,"offset":m.payload_offset,"inline_layout_policy":m.inline_layout_policy.map(|p|format!("{p:?}")),"declared_header_bytes":m.declared_header_bytes,"parsed_header_bytes":m.parsed_header_bytes,"legacy_size_bytes":m.legacy_size_bytes,"declared_compression":m.declared_compression,"payload_interpretation":format!("{:?}",m.payload_interpretation)})
}
fn members(values: &[recovery::MemberReport]) -> Value {
    json!(values.iter().map(|m|json!({"encoded_range":[m.encoded_range.start,m.encoded_range.end],"decoded_range":[m.decoded_range.start,m.decoded_range.end],"integrity":format!("{:?}",m.integrity),"diagnostic":m.diagnostic})).collect::<Vec<_>>())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: edf_recovery_probe recovery|legacy-size|raw-ty1 FILE".into());
    }
    let path = &args[2];
    let mut options = edf::Options::default();
    match args[1].as_str() {
        "legacy-size" => options.inline_layout = edf::InlineLayout::LegacySizeAtParsedBoundary,
        "raw-ty1" => {
            options.payload_interpretation =
                edf::PayloadInterpretation::StoredRawForLabel("TY1".into())
        }
        "recovery" => {}
        _ => return Err("unknown EDF proof mode".into()),
    }
    if args[1] == "recovery" {
        let reader = recovery::Reader::open(
            path,
            edf::Limits::default(),
            recovery::Options {
                edf: options,
                ..Default::default()
            },
        )?;
        let mut buffer = recovery::SampleBuffer::default();
        let mut images = Vec::new();
        for index in 0..reader.image_records() {
            reader.read_samples_into(index, &mut buffer)?;
            let r = buffer.report().unwrap();
            let m = reader.metadata(index).unwrap();
            let mut image = signature(buffer.pixels().unwrap());
            image["frame"] = json!(index);
            image["frames"] = json!(reader.image_records());
            image["fields"] = json!(m.fields);
            image["payload_window"] = window(m);
            image["header_sha256"] = json!(format!("{:x}", Sha256::digest(&m.header)));
            image["availability"] = json!({"declared_shape":r.declared_shape,"declared_samples":r.declared_samples,"present_samples":[r.present_samples.start,r.present_samples.end],"missing_samples":r.missing_samples.as_ref().map(|r|[r.start,r.end]),"verified_samples":[r.verified_samples.start,r.verified_samples.end],"expected_payload_bytes":r.expected_payload_bytes,"available_payload_bytes":r.available_payload_bytes,"availability":format!("{:?}",r.availability),"outer_integrity":format!("{:?}",r.outer_integrity),"payload_integrity":format!("{:?}",r.payload_integrity),"payload_members":members(&r.payload_members),"diagnostic":r.diagnostic,"complete_verified_frame":r.complete_verified_frame,"partial_element_bytes":hex(buffer.partial_element_bytes().unwrap())});
            if r.declared_source_shape.len() != 2 {
                image["availability"]["declared_source_shape"] = json!(r.declared_source_shape);
                image["availability"]["record_index"] = json!(r.record_index);
                image["availability"]["complete_verified_record"] =
                    json!(r.complete_verified_record);
            }
            images.push(image);
        }
        let s = reader.source_report();
        let stop=reader.stop().map(|s|match s{recovery::Stop::TruncatedHeader{offset,bytes}=>json!({"kind":"TruncatedHeader","offset":offset,"bytes_hex":hex(bytes),"sha256":format!("{:x}",Sha256::digest(bytes))}),recovery::Stop::TruncatedPayload{image_record,offset,expected_bytes,available_bytes}=>json!({"kind":"TruncatedPayload","image_record":image_record,"offset":offset,"expected_bytes":expected_bytes,"available_bytes":available_bytes}),recovery::Stop::UnparsedRecord{offset,header,reason}=>json!({"kind":"UnparsedRecord","offset":offset,"header_hex":hex(header),"reason":reason}),recovery::Stop::NoImageHeader=>json!({"kind":"NoImageHeader"})});
        println!(
            "{}",
            json!({"source_recovery":{"format":"edf","images":images,"source_report":{"path":s.path,"encoded_bytes":s.encoded_bytes,"decoded_bytes":s.decoded_bytes,"compression":format!("{:?}",s.compression),"integrity":format!("{:?}",s.integrity),"verified_decoded_bytes":s.verified_decoded_bytes,"members":members(&s.members),"diagnostic":s.diagnostic},"stop":stop}})
        );
    } else {
        let reader = edf::Reader::open_with_options(path, edf::Limits::default(), options)?;
        let mut buffer = edf::FrameBuffer::default();
        let mut frames = Vec::new();
        for index in 0..reader.frames() {
            reader.read_frame_into(index, &mut buffer)?;
            let m = reader.metadata(index).unwrap();
            let mut frame = signature(buffer.pixels().unwrap());
            frame["frame"] = json!(index);
            frame["frames"] = json!(reader.frames());
            frame["shape"] = json!(buffer.shape());
            frame["fields"] = json!(m.fields);
            frame["payload_window"] = window(m);
            frame["header_sha256"] = json!(format!("{:x}", Sha256::digest(&m.header)));
            frames.push(frame);
        }
        println!("{}", json!({"frames":frames}));
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
