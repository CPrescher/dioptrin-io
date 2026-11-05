use super::*;
pub(super) fn read(reader: &Reader, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
    let mut sparse = SparseFrameBuffer::default();
    reader.read_sparse_frame_into(index, &mut sparse)?;
    let limit = reader.options.limits.component_bytes;
    let ty = reader.metadata.intensity_type;
    let shape = reader.metadata.shape;
    let count = shape[0] * shape[1];
    let mut mask = ArrayBuffer::default();
    numeric::read(&reader.stored[reader.mask], None, &mut mask, limit)?;
    let mut dummy = PixelBuffer::default();
    dummy.resize(ty, 1).map_err(Error)?;
    if let Some(&component) = reader.names.get("dummy") {
        let mut raw = ArrayBuffer::default();
        numeric::read(&reader.stored[component], None, &mut raw, limit)?;
        if raw.pixels.pixel_type() == ty {
            dummy = raw.pixels;
        } else {
            set(&mut dummy, 0, scalar_exact(&raw.pixels)?, false)?;
        }
    } else {
        match &mut dummy {
            PixelBuffer::F32(v) => v[0] = f32::NAN,
            PixelBuffer::F64(v) => v[0] = f64::NAN,
            _ => {}
        }
    }
    let names = ["radius", "background_avg", "background_std"];
    let components: Vec<_> = names
        .iter()
        .map(|name| reader.names.get(*name).copied())
        .collect();
    let present = components.iter().filter(|c| c.is_some()).count();
    if present != 0 && present != 3 {
        return Err(Error(
            "partial sparse background metadata cannot define the selected reconstruction".into(),
        ));
    }
    let mut normalization = ArrayBuffer::default();
    if let Some(&component) = reader.names.get("normalization") {
        numeric::read(&reader.stored[component], None, &mut normalization, limit)?;
    }
    buffer.pixels.resize(ty, count).map_err(Error)?;
    buffer.pixels.bytes_mut().fill(0);
    if present == 3 {
        let radius_component = components[0].unwrap();
        let background_component = components[1].unwrap();
        if reader.stored[background_component].metadata.pixel_type != Some(PixelType::F32)
            || reader.stored[radius_component].metadata.pixel_type
                != reader.stored[reader.mask].metadata.pixel_type
        {
            return Err(Error("reference deterministic reconstruction requires f32 background and matching radius/mask float widths".into()));
        }
        let mut radius = ArrayBuffer::default();
        numeric::read(&reader.stored[radius_component], None, &mut radius, limit)?;
        let mut background = ArrayBuffer::default();
        numeric::read(
            &reader.stored[background_component],
            Some((index, index + 1)),
            &mut background,
            limit,
        )?;
        let bins = radius.pixels.len();
        let start = numeric::float(&radius.pixels, 0)?;
        let end = numeric::float(&radius.pixels, bins - 1)?;
        let delta = (end - start) / (bins - 1) as f64;
        let inverse = (bins - 1) as f64 / (end - start);
        if !inverse.is_finite() || delta <= 0.0 || !delta.is_finite() {
            return Err(Error(
                "sparse radius grid cannot define finite reconstruction spacing".into(),
            ));
        }
        let epsilon = if radius.pixels.pixel_type() == PixelType::F32 {
            f64::from(f32::EPSILON)
        } else {
            f64::EPSILON
        };
        let tolerance = 16.0 * epsilon * start.abs().max(end.abs()).max(delta.abs());
        for i in 0..bins {
            if (numeric::float(&radius.pixels, i)? - (start + i as f64 * delta)).abs() > tolerance {
                return Err(Error(
                    "deterministic sparse reconstruction requires a uniform radial grid".into(),
                ));
            }
        }
        for i in 0..count {
            let position = (numeric::float(&mask.pixels, i)? - start) * inverse;
            if !position.is_finite() || position < 0.0 || position >= bins as f64 {
                copy_sample(&dummy, 0, &mut buffer.pixels, i);
                continue;
            }
            let lower = position as usize;
            let mut value = if lower + 1 == bins {
                numeric::float(&background.pixels, lower)?
            } else {
                let fraction = position - lower as f64;
                (1.0 - fraction) * numeric::float(&background.pixels, lower)?
                    + fraction * numeric::float(&background.pixels, lower + 1)?
            };
            if !normalization.pixels.is_empty() {
                value *= f64::from(normalization32(&normalization.pixels, i)?);
            }
            set(&mut buffer.pixels, i, value, true)?;
        }
    }
    // Reference-native semantics assign original outliers last, including events
    // at masked locations. No normalization/rounding touches those source words.
    for i in 0..sparse.index.pixels.len() {
        let position = numeric::index(&sparse.index.pixels, i)?;
        copy_sample(&sparse.intensity.pixels, i, &mut buffer.pixels, position);
    }
    Ok(())
}
fn copy_sample(input: &PixelBuffer, source: usize, output: &mut PixelBuffer, target: usize) {
    macro_rules! copy {($($variant:ident);*)=>{match (input,output){$((PixelBuffer::$variant(source_values),PixelBuffer::$variant(target_values))=>target_values[target]=source_values[source],)*_=>unreachable!("admitted sparse event/dummy dtype")}};}
    copy!(U8;U16;U32;U64;I8;I16;I32;I64;F32;F64);
}
fn scalar_exact(pixels: &PixelBuffer) -> Result<f64, Error> {
    macro_rules! small {($($variant:ident);*)=>{match pixels {$(PixelBuffer::$variant(v)=>Ok(f64::from(v[0])),)*PixelBuffer::F64(v)=>Ok(v[0]),PixelBuffer::U64(v)=>{let f=v[0]as f64;if f>=18446744073709551616.0||f as u64!=v[0]{Err(Error("sparse dummy integer cannot be converted exactly".into()))}else{Ok(f)}},PixelBuffer::I64(v)=>{let f=v[0]as f64;if f>=9223372036854775808.0||f as i64!=v[0]{Err(Error("sparse dummy integer cannot be converted exactly".into()))}else{Ok(f)}},_=>Err(Error("unsupported sparse dummy source type".into()))}};}
    small!(U8;U16;U32;I8;I16;I32;F32)
}
fn normalization32(pixels: &PixelBuffer, index: usize) -> Result<f32, Error> {
    macro_rules! numbers {($($variant:ident);*)=>{match pixels {$(PixelBuffer::$variant(v)=>{let out=v[index]as f32;if out.is_infinite()&&(v[index]as f64).is_finite(){return Err(Error("sparse normalization exceeds explicit f32 model range".into()));}Ok(out)},)*_=>Err(Error("unsupported sparse normalization type".into()))}};}
    numbers!(U8;U16;U32;U64;I8;I16;I32;I64;F32;F64)
}
fn set(pixels: &mut PixelBuffer, index: usize, value: f64, round: bool) -> Result<(), Error> {
    let value = if round && numeric::is_integer(pixels.pixel_type()) {
        value.round()
    } else {
        value
    };
    macro_rules! integer {($($variant:ident,$ty:ty);*)=>{match pixels {$(PixelBuffer::$variant(v)=>{
        if !value.is_finite()||value.fract()!=0.0||value<(<$ty>::MIN as f64)||value>=((<$ty>::MAX as f64)+1.0)||(round&&value>=9223372036854775808.0){return Err(Error("sparse reconstructed integer overflows declared dtype/reference rounding range".into()));}
        v[index]=value as $ty;Ok(())
    },)*PixelBuffer::F32(v)=>{let cast=value as f32;if value.is_finite()&&(!cast.is_finite()||(!round&&f64::from(cast)!=value)){return Err(Error("sparse derived f32 value overflows or dummy loses precision".into()));}v[index]=cast;Ok(())},PixelBuffer::F64(v)=>{v[index]=value;Ok(())},_=>Err(Error("unsupported sparse reconstruction type".into()))}};}
    integer!(U8,u8;U16,u16;U32,u32;U64,u64;I8,i8;I16,i16;I32,i32;I64,i64)
}
