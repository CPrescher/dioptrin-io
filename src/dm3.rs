//! Bounded DigitalMicrograph DM3 tag-directory indexing and native image arrays.
//! Structure words are big endian; values follow the original header byte order.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelBuffer, PixelType,
    binary::{copy_samples, resize_bytes},
    source::Snapshot,
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Clone, Debug)]
pub struct Tag {
    /// Hierarchical path. Unnamed entries use their zero-based source ordinal.
    pub path: Vec<String>,
    pub raw_name: Vec<u8>,
    pub descriptor: Vec<u32>,
    pub offset: u64,
    pub bytes: usize,
    /// Small values retained byte-for-byte. Large arrays remain source references.
    pub raw: Option<Vec<u8>>,
}
#[derive(Clone, Debug)]
pub struct Image {
    pub path: Vec<String>,
    pub tag_index: usize,
    pub source_dimensions: Vec<usize>,
    pub shape: [usize; 2],
    pub frames: usize,
    pub pixel_type: PixelType,
}
#[derive(Clone, Debug)]
pub struct Metadata {
    pub header: [u8; 12],
    pub declared_bytes: u32,
    pub little_endian: bool,
    pub tags: Vec<Tag>,
    pub images: Vec<Image>,
    pub selected_image: usize,
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
    limits: Limits,
}
#[derive(Clone, Debug)]
enum Type {
    Scalar(PixelType),
    String(usize),
    Struct(usize, Vec<(usize, Type)>),
    Array(Box<Type>, usize),
}
impl Type {
    fn bytes(&self) -> Option<usize> {
        match self {
            Self::Scalar(ty) => Some(ty.bytes()),
            Self::String(n) => n.checked_mul(2),
            Self::Array(t, n) => t.bytes()?.checked_mul(*n),
            Self::Struct(name, fields) => fields.iter().try_fold(*name, |sum, (name, ty)| {
                sum.checked_add(*name)?.checked_add(ty.bytes()?)
            }),
        }
    }
    fn scalar_array(&self) -> Option<(PixelType, usize)> {
        if let Self::Array(t, n) = self
            && let Self::Scalar(ty) = **t
        {
            Some((ty, *n))
        } else {
            None
        }
    }
}
struct Parser<'a> {
    file: &'a mut File,
    bytes: u64,
    limits: Limits,
    tags: Vec<Tag>,
    metadata: usize,
    entries: usize,
}
impl Parser<'_> {
    fn charge(&mut self, n: usize) -> Result<(), Error> {
        self.metadata = self
            .metadata
            .checked_add(n)
            .filter(|&n| n <= self.limits.metadata_bytes)
            .ok_or_else(|| Error("DM3 metadata exceeds budget".into()))?;
        Ok(())
    }
    fn take(&mut self, n: usize) -> Result<Vec<u8>, Error> {
        let end = self
            .file
            .stream_position()?
            .checked_add(n as u64)
            .filter(|&n| n <= self.bytes)
            .ok_or_else(|| Error("truncated DM3 tag/payload".into()))?;
        let mut bytes = Vec::new();
        resize_bytes(&mut bytes, n)?;
        self.file.read_exact(&mut bytes)?;
        debug_assert_eq!(self.file.stream_position()?, end);
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn word(&mut self) -> Result<u32, Error> {
        Ok(u32::from_be_bytes(self.take(4)?[..].try_into().unwrap()))
    }
    fn group(&mut self, path: &[String], depth: usize) -> Result<(), Error> {
        if depth > 64 {
            return Err(Error("DM3 tag group nesting exceeds limit".into()));
        }
        let sorted = self.u8()?;
        let open = self.u8()?;
        if sorted > 1 || open > 1 {
            return Err(Error("invalid DM3 group flags".into()));
        }
        let count = self.word()? as usize;
        self.entries = self
            .entries
            .checked_add(count)
            .filter(|&n| n <= self.limits.frames)
            .ok_or_else(|| Error("DM3 tag entry count exceeds limit".into()))?;
        for ordinal in 0..count {
            let kind = self.u8()?;
            let name_len = u16::from_be_bytes(self.take(2)?[..].try_into().unwrap()) as usize;
            self.charge(name_len.saturating_mul(3).saturating_add(128))?;
            let name = self.take(name_len)?;
            let text = if name.is_empty() {
                ordinal.to_string()
            } else {
                name.iter().map(|&b| char::from(b)).collect()
            };
            let mut full = path.to_vec();
            full.push(text);
            self.charge(full.iter().map(String::len).sum::<usize>())?;
            match kind {
                20 => self.group(&full, depth + 1)?,
                21 => {
                    if self.take(4)? != b"%%%%" {
                        return Err(Error("invalid DM3 tag separator".into()));
                    }
                    let words = self.word()? as usize;
                    if words == 0
                        || words > 65536
                        || words.saturating_mul(4) > self.limits.header_bytes
                    {
                        return Err(Error("DM3 type descriptor exceeds limit".into()));
                    }
                    self.charge(words.saturating_mul(8))?;
                    let mut descriptor = Vec::new();
                    descriptor
                        .try_reserve_exact(words)
                        .map_err(|e| Error(e.to_string()))?;
                    for _ in 0..words {
                        descriptor.push(self.word()?);
                    }
                    let ty = parse_type(&descriptor)?;
                    let bytes = ty
                        .bytes()
                        .ok_or_else(|| Error("DM3 value byte-size overflow".into()))?;
                    let offset = self.file.stream_position()?;
                    let end = offset
                        .checked_add(bytes as u64)
                        .filter(|&n| n <= self.bytes)
                        .ok_or_else(|| Error("DM3 array runs past end of file".into()))?;
                    let raw = if bytes <= 4096 {
                        self.charge(bytes)?;
                        Some(self.take(bytes)?)
                    } else {
                        self.file.seek(SeekFrom::Start(end))?;
                        None
                    };
                    self.tags.push(Tag {
                        path: full,
                        raw_name: name,
                        descriptor,
                        offset,
                        bytes,
                        raw,
                    });
                }
                _ => return Err(Error(format!("unsupported DM3 tag kind {kind}"))),
            }
        }
        Ok(())
    }
}
impl Reader {
    /// Index all tags/images; the last stored numeric ImageData/Data array is
    /// exposed by default, matching the pinned reference's last Data selection.
    /// # Errors
    /// Invalid headers/descriptors, nesting/resources, unsupported type or truncation.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut header = [0u8; 12];
        file.read_exact(&mut header)?;
        if u32::from_be_bytes(header[..4].try_into().unwrap()) != 3 {
            return Err(Error("only DigitalMicrograph DM3 is supported".into()));
        }
        let declared = u32::from_be_bytes(header[4..8].try_into().unwrap());
        if declared as u64 > snapshot.bytes()
            || ![0, 12, 16, 20].contains(&(snapshot.bytes() - u64::from(declared)))
        {
            return Err(Error("DM3 declared/source lengths disagree".into()));
        }
        let little = match u32::from_be_bytes(header[8..].try_into().unwrap()) {
            0 => false,
            1 => true,
            _ => return Err(Error("invalid DM3 byte order".into())),
        };
        let mut parser = Parser {
            file: &mut file,
            bytes: snapshot.bytes(),
            limits,
            tags: Vec::new(),
            metadata: 12,
            entries: 0,
        };
        parser.group(&[], 0)?;
        let position = parser.file.stream_position()?;
        let tail = usize::try_from(snapshot.bytes() - position)
            .map_err(|_| Error("DM3 trailing length overflow".into()))?;
        if tail > 8 || parser.take(tail)?.iter().any(|&b| b != 0) {
            return Err(Error("unexpected DM3 trailing data".into()));
        }
        let mut metadata_bytes = parser.metadata;
        let tags = parser.tags;
        // Index dimensions once; large tag directories must not cause a quadratic scan.
        let mut dimension_tags = std::collections::BTreeMap::<&[String], Vec<&Tag>>::new();
        for tag in &tags {
            if tag.path.len() >= 2 && tag.path[tag.path.len() - 2] == "Dimensions" {
                dimension_tags
                    .entry(&tag.path[..tag.path.len() - 2])
                    .or_default()
                    .push(tag);
            }
        }
        let mut images = Vec::new();
        for (index, tag) in tags
            .iter()
            .enumerate()
            .filter(|(_, t)| t.path.last().is_some_and(|p| p == "Data"))
        {
            let parent = &tag.path[..tag.path.len() - 1];
            let associated_dimensions = dimension_tags.get(parent);
            if parent.last().is_none_or(|name| name != "ImageData")
                && associated_dimensions.is_none()
            {
                continue;
            }
            let Some((pixel_type, count)) = parse_type(&tag.descriptor)?.scalar_array() else {
                return Err(Error(
                    "unsupported DM3 image-associated Data array layout; no thumbnail substitution"
                        .into(),
                ));
            };
            let associated_dimensions = associated_dimensions
                .ok_or_else(|| Error("DM3 ImageData/Data is missing Dimensions".into()))?;
            let mut dimensions: Vec<(usize, usize)> = Vec::new();
            for dimension in associated_dimensions {
                let ordinal = dimension
                    .path
                    .last()
                    .unwrap()
                    .parse::<usize>()
                    .map_err(|_| Error("DM3 dimension names must be numeric".into()))?;
                let value = integer_value(dimension, little)?
                    .ok_or_else(|| Error("DM3 dimension is not an integer scalar".into()))?;
                dimensions.push((ordinal, value));
            }
            dimensions.sort_by_key(|d| d.0);
            if dimensions.is_empty() {
                return Err(Error("DM3 image Data has no Dimensions".into()));
            }
            if dimensions.len() > 32
                || dimensions
                    .iter()
                    .enumerate()
                    .any(|(i, (n, d))| *n != i || *d == 0)
            {
                return Err(Error("invalid DM3 image dimension sequence".into()));
            }
            let dims: Vec<usize> = dimensions.into_iter().map(|(_, d)| d).collect();
            let total = dims
                .iter()
                .try_fold(1usize, |n, &d| n.checked_mul(d))
                .ok_or_else(|| Error("DM3 image shape overflow".into()))?;
            if total != count {
                return Err(Error("DM3 Data count does not match Dimensions".into()));
            }
            let shape = [dims.get(1).copied().unwrap_or(1), dims[0]];
            let pixels = limits.pixels(shape, pixel_type)?;
            let frames = total / pixels;
            if frames == 0
                || frames > limits.frames
                || pixels * pixel_type.bytes() > limits.encoded_bytes
            {
                return Err(Error("DM3 image exceeds frame/encoded limits".into()));
            }
            metadata_bytes = metadata_bytes
                .checked_add(
                    parent
                        .iter()
                        .map(|p| p.len().saturating_add(24))
                        .sum::<usize>()
                        .saturating_add(dims.len() * 8 + 128),
                )
                .filter(|&n| n <= limits.metadata_bytes)
                .ok_or_else(|| Error("DM3 image index exceeds metadata budget".into()))?;
            images.push(Image {
                path: parent.to_vec(),
                tag_index: index,
                source_dimensions: dims,
                shape,
                frames,
                pixel_type,
            });
        }
        if images.is_empty() {
            return Err(Error(
                "DM3 contains no supported numeric image Data/Dimensions".into(),
            ));
        }
        let selected_image = images.len() - 1;
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            limits,
            metadata: Metadata {
                header,
                declared_bytes: declared,
                little_endian: little,
                tags,
                images,
                selected_image,
            },
        })
    }
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.metadata.images[self.metadata.selected_image].frames
    }
    /// Select any stored image rather than substituting a thumbnail implicitly.
    /// # Errors
    /// Invalid image index or changed source.
    pub fn select_image(&mut self, index: usize) -> Result<(), Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        if index >= self.metadata.images.len() {
            return Err(Error("DM3 image outside source".into()));
        }
        self.metadata.selected_image = index;
        Ok(())
    }
    /// Read one native stored-order image section into reusable output storage.
    /// # Errors
    /// Invalid frame, changed source, truncation or allocation failure.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        let image = &self.metadata.images[self.metadata.selected_image];
        if index >= image.frames {
            return Err(Error("DM3 frame outside source".into()));
        }
        let tag = &self.metadata.tags[image.tag_index];
        let count = image.shape[0] * image.shape[1];
        let bytes = count * image.pixel_type.bytes();
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(tag.offset + index as u64 * bytes as u64))?;
        resize_bytes(&mut buffer.encoded, bytes)?;
        file.read_exact(&mut buffer.encoded)?;
        buffer
            .pixels
            .resize(image.pixel_type, count)
            .map_err(Error)?;
        copy_samples(
            &buffer.encoded,
            &mut buffer.pixels,
            self.metadata.little_endian,
        )?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        buffer.shape = Some(image.shape);
        Ok(())
    }
    /// Retrieve an explicitly chosen original tag payload under metadata bounds.
    /// Large image arrays should use the frame API instead.
    /// # Errors
    /// Invalid tag, oversized value, changed source or truncated read.
    pub fn read_tag_raw(&self, index: usize) -> Result<Vec<u8>, Error> {
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        let tag = self
            .metadata
            .tags
            .get(index)
            .ok_or_else(|| Error("DM3 tag outside source".into()))?;
        if tag.bytes > self.limits.metadata_bytes {
            return Err(Error(
                "DM3 requested tag exceeds metadata byte limit".into(),
            ));
        }
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(tag.offset))?;
        let mut bytes = Vec::new();
        resize_bytes(&mut bytes, tag.bytes)?;
        file.read_exact(&mut bytes)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        Ok(bytes)
    }
}
fn parse_type(info: &[u32]) -> Result<Type, Error> {
    fn parse(info: &[u32], position: &mut usize, depth: usize) -> Result<Type, Error> {
        if depth > 32 {
            return Err(Error("DM3 type nesting exceeds limit".into()));
        }
        let next = |position: &mut usize| -> Result<u32, Error> {
            let value = *info
                .get(*position)
                .ok_or_else(|| Error("truncated DM3 type descriptor".into()))?;
            *position += 1;
            Ok(value)
        };
        let code = next(position)?;
        let ty = match code {
            2 => PixelType::I16,
            3 => PixelType::I32,
            4 => PixelType::U16,
            5 => PixelType::U32,
            6 => PixelType::F32,
            7 => PixelType::F64,
            8 => PixelType::I8,
            9 | 10 => PixelType::U8,
            18 => return Ok(Type::String(next(position)? as usize)),
            20 => {
                let element = parse(info, position, depth + 1)?;
                return Ok(Type::Array(Box::new(element), next(position)? as usize));
            }
            15 => {
                let name_bytes = next(position)? as usize;
                let count = next(position)? as usize;
                if count > 1024 {
                    return Err(Error("DM3 struct exceeds field limit".into()));
                }
                let mut fields = Vec::new();
                for _ in 0..count {
                    let name = next(position)? as usize;
                    fields.push((name, parse(info, position, depth + 1)?));
                }
                return Ok(Type::Struct(name_bytes, fields));
            }
            _ => return Err(Error(format!("unsupported DM3 encoded type {code}"))),
        };
        Ok(Type::Scalar(ty))
    }
    let mut position = 0;
    let ty = parse(info, &mut position, 0)?;
    if position != info.len() {
        return Err(Error("extra DM3 descriptor words".into()));
    }
    Ok(ty)
}
fn integer_value(tag: &Tag, little: bool) -> Result<Option<usize>, Error> {
    let Some(raw) = &tag.raw else { return Ok(None) };
    let Type::Scalar(ty) = parse_type(&tag.descriptor)? else {
        return Ok(None);
    };
    let mut buffer = PixelBuffer::default();
    buffer.resize(ty, 1).map_err(Error)?;
    copy_samples(raw, &mut buffer, little)?;
    let value = match buffer {
        PixelBuffer::U8(v) => Some(v[0] as u64),
        PixelBuffer::U16(v) => Some(v[0] as u64),
        PixelBuffer::U32(v) => Some(v[0] as u64),
        PixelBuffer::U64(v) => Some(v[0]),
        PixelBuffer::I8(v) => u64::try_from(v[0]).ok(),
        PixelBuffer::I16(v) => u64::try_from(v[0]).ok(),
        PixelBuffer::I32(v) => u64::try_from(v[0]).ok(),
        PixelBuffer::I64(v) => u64::try_from(v[0]).ok(),
        _ => None,
    };
    value
        .map(|v| usize::try_from(v).map_err(|_| Error("DM3 dimension overflows usize".into())))
        .transpose()
}
