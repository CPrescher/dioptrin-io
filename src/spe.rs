//! Native Princeton SPE 2/3 image input, with original header/footer metadata.
pub use crate::binary::{Error, FrameBuffer, Limits};
use crate::{
    PixelType,
    binary::{copy_samples, resize_bytes},
    source::Snapshot,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Clone)]
pub struct Region {
    pub shape: [usize; 2],
    pub offset: usize,
    pub bytes: usize,
    pub roi: [usize; 4],
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub version: u8,
    pub file_version: f32,
    pub shape: [usize; 2],
    pub frames: usize,
    pub pixel_type: PixelType,
    pub data_type: u16,
    pub frame_stride: usize,
    pub frame_metadata_bytes: usize,
    pub header: Vec<u8>,
    pub xml: Vec<u8>,
    pub regions: Vec<Region>,
    pub fields: BTreeMap<String, String>,
    /// Original v2 polynomial coefficients, when present. Counts are unaffected.
    pub calibration_coefficients: Option<[f64; 6]>,
    /// Metadata axis for the primary region, matching the reference interpretation.
    pub x_calibration: Vec<f64>,
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
}
impl Reader {
    /// Index a frozen SPE file. XML is parsed without DTD/external entities.
    /// Every declared frame/region must fit the source; no missing pixels are padded.
    /// # Errors
    /// Unsupported types/layouts, malformed binary/XML headers, bounds or truncation.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        if limits.header_bytes < 4100 || limits.metadata_bytes < 4100 {
            return Err(Error("SPE header exceeds limits".into()));
        }
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut header = vec![0u8; 4100];
        file.read_exact(&mut header)?;
        let u16_at = |p| u16::from_le_bytes(header[p..p + 2].try_into().unwrap());
        let f32_at = |p| f32::from_le_bytes(header[p..p + 4].try_into().unwrap());
        let data_type = u16_at(108);
        let pixel_type = match data_type {
            0 => PixelType::F32,
            1 => PixelType::I32,
            2 => PixelType::I16,
            3 => PixelType::U16,
            _ => return Err(Error(format!("unsupported SPE data type {data_type}"))),
        };
        let shape = [usize::from(u16_at(656)), usize::from(u16_at(42))];
        let count = limits.pixels(shape, pixel_type)?;
        let frame_bytes = count * pixel_type.bytes();
        if frame_bytes > limits.encoded_bytes {
            return Err(Error("SPE frame exceeds encoded byte limit".into()));
        }
        let frames = usize::try_from(i32::from_le_bytes(header[1446..1450].try_into().unwrap()))
            .map_err(|_| Error("negative SPE frame count".into()))?;
        if frames == 0 || frames > limits.frames {
            return Err(Error("SPE frame count exceeds limit".into()));
        }
        let xml_offset = u64::from_le_bytes(header[678..686].try_into().unwrap());
        let file_version = f32_at(1992);
        let version = if xml_offset == 0 { 2 } else { 3 };
        if !file_version.is_finite() || !(2.0..4.0).contains(&file_version) {
            return Err(Error("unsupported SPE file version".into()));
        }
        if (file_version >= 3.) != (version == 3) {
            return Err(Error("SPE version and XML footer location disagree".into()));
        }
        let mut fields = BTreeMap::new();
        fields.insert("version".into(), version.to_string());
        fields.insert("file_version".into(), file_version.to_string());
        fields.insert("data_type".into(), data_type.to_string());
        fields.insert("x_dim".into(), shape[1].to_string());
        fields.insert("y_dim".into(), shape[0].to_string());
        fields.insert("num_frames".into(), frames.to_string());
        fields.insert("detector".into(), "unspecified".into());
        fields.insert("grating".into(), float_text(f32_at(650) as f64));
        fields.insert("center_wavelength".into(), (f32_at(72) as f64).to_string());
        let mut regions = vec![Region {
            shape,
            offset: 0,
            bytes: frame_bytes,
            roi: [0, shape[1], 0, shape[0]],
        }];
        let mut frame_stride = frame_bytes;
        let mut frame_metadata_bytes = 0;
        let mut xml = Vec::new();
        let mut coefficients = None;
        let x_calibration;
        let payload_end;
        if version == 2 {
            payload_end = 4100u64
                .checked_add(
                    (frame_bytes as u64)
                        .checked_mul(frames as u64)
                        .ok_or_else(|| Error("SPE stack size overflow".into()))?,
                )
                .ok_or_else(|| Error("SPE size overflow".into()))?;
            if payload_end != snapshot.bytes() {
                return Err(Error(
                    "SPE v2 payload length does not match declared frames".into(),
                ));
            }
            fields.insert("exposure_time".into(), (f32_at(10) as f64).to_string());
            let date = &header[20..29];
            let time = &header[172..178];
            if let Some(text) = legacy_time(date, time) {
                fields.insert("time".into(), text);
            }
            if shape[1]
                .checked_mul(16)
                .is_none_or(|n| n > limits.metadata_bytes - 4100)
            {
                return Err(Error("SPE calibration exceeds metadata budget".into()));
            }
            let mut c = [0.; 6];
            for (i, value) in c.iter_mut().enumerate() {
                *value = f64::from_le_bytes(header[3263 + i * 8..3271 + i * 8].try_into().unwrap());
            }
            coefficients = Some(c);
            x_calibration = (1..=shape[1])
                .map(|x| {
                    c.iter()
                        .rev()
                        .fold(0., |value, &coefficient| value * x as f64 + coefficient)
                })
                .collect();
        } else {
            if xml_offset < 4100 || xml_offset > snapshot.bytes() {
                return Err(Error("invalid SPE XML offset".into()));
            }
            let xml_len = usize::try_from(snapshot.bytes() - xml_offset)
                .map_err(|_| Error("SPE XML length overflow".into()))?;
            if xml_len == 0
                || xml_len > limits.header_bytes
                || xml_len.saturating_mul(4).saturating_add(4100) > limits.metadata_bytes
            {
                return Err(Error("SPE XML exceeds metadata/header limit".into()));
            }
            resize_bytes(&mut xml, xml_len)?;
            file.seek(SeekFrom::Start(xml_offset))?;
            file.read_exact(&mut xml)?;
            let text =
                std::str::from_utf8(&xml).map_err(|_| Error("SPE XML must be UTF8".into()))?;
            let document = roxmltree::Document::parse_with_options(
                text,
                roxmltree::ParsingOptions {
                    allow_dtd: false,
                    nodes_limit: (limits.metadata_bytes / 32).min(100_000) as u32,
                    entity_resolver: None,
                },
            )
            .map_err(|e| Error(format!("invalid SPE XML: {e}")))?;
            let root = document.root_element();
            let namespace = root.tag_name().namespace();
            if root.tag_name().name() != "SpeFormat"
                || !matches!(
                    namespace,
                    None | Some("http://www.princetoninstruments.com/spe/2009")
                )
            {
                return Err(Error("unsupported SPE XML root/namespace".into()));
            }
            let structural = |node: roxmltree::Node<'_, '_>, name: &str| {
                node.is_element()
                    && node.tag_name().name() == name
                    && node.tag_name().namespace() == namespace
            };
            let mut formats = root.children().filter(|&n| structural(n, "DataFormat"));
            let format = formats
                .next()
                .ok_or_else(|| Error("missing SPE XML DataFormat".into()))?;
            if formats.next().is_some() {
                return Err(Error("multiple SPE XML DataFormat elements".into()));
            }
            let mut blocks = format.children().filter(|n| n.is_element());
            let frame = blocks
                .next()
                .filter(|&n| structural(n, "DataBlock") && n.attribute("type") == Some("Frame"))
                .ok_or_else(|| Error("SPE XML requires one Frame DataBlock".into()))?;
            if blocks.next().is_some() {
                return Err(Error(
                    "multiple/unsupported SPE XML DataFormat blocks".into(),
                ));
            }
            // DataBlock-like metadata cannot redirect the image byte layout.
            if document
                .descendants()
                .filter(|n| {
                    n.is_element()
                        && n.tag_name().name() == "DataBlock"
                        && n.attribute("type") == Some("Frame")
                })
                .any(|n| n != frame)
            {
                return Err(Error("ambiguous/misplaced SPE XML Frame DataBlock".into()));
            }
            let number = |node: roxmltree::Node<'_, '_>, key: &str| -> Result<usize, Error> {
                node.attribute(key)
                    .ok_or_else(|| Error(format!("missing SPE XML {key}")))?
                    .parse()
                    .map_err(|_| Error(format!("invalid SPE XML {key}")))
            };
            if number(frame, "count")? != frames {
                return Err(Error("SPE binary/XML frame counts disagree".into()));
            }
            let expected_format = match pixel_type {
                PixelType::F32 => "MonochromeFloating32",
                PixelType::I32 => "MonochromeSigned32",
                PixelType::I16 => "MonochromeSigned16",
                _ => "MonochromeUnsigned16",
            };
            if frame
                .attribute("pixelFormat")
                .is_some_and(|s| s != expected_format)
            {
                return Err(Error("SPE binary/XML pixel types disagree".into()));
            }
            let size = number(frame, "size")?;
            frame_stride = number(frame, "stride")?;
            if size == 0 || frame_stride < size || frame_stride > limits.encoded_bytes {
                return Err(Error("invalid/oversized SPE XML frame stride".into()));
            }
            regions.clear();
            let mut offset = 0usize;
            for region in frame.children().filter(|n| n.is_element()) {
                if !structural(region, "DataBlock")
                    || region.attribute("type") != Some("Region")
                    || region.children().any(|n| n.is_element())
                {
                    return Err(Error(
                        "unsupported SPE XML Frame/Region child layout".into(),
                    ));
                }
                if region
                    .attribute("pixelFormat")
                    .is_some_and(|s| s != expected_format)
                {
                    return Err(Error("SPE Region pixel type contradicts frame".into()));
                }
                if regions.len() >= limits.frames {
                    return Err(Error("SPE region count exceeds limit".into()));
                }
                if number(region, "count")? != 1 {
                    return Err(Error("SPE repeated Region blocks are unsupported".into()));
                }
                let region_shape = [number(region, "height")?, number(region, "width")?];
                let bytes = limits.pixels(region_shape, pixel_type)? * pixel_type.bytes();
                let stored = number(region, "stride")?;
                if number(region, "size")? != bytes || stored < bytes {
                    return Err(Error("SPE region size/shape mismatch".into()));
                }
                regions.push(Region {
                    shape: region_shape,
                    offset,
                    bytes,
                    roi: [0, region_shape[1], 0, region_shape[0]],
                });
                offset = offset
                    .checked_add(stored)
                    .filter(|&n| n <= size)
                    .ok_or_else(|| Error("SPE regions exceed frame DataBlock".into()))?;
            }
            if regions.is_empty() || regions[0].shape != shape {
                return Err(Error(
                    "SPE binary/XML primary region dimensions disagree".into(),
                ));
            }
            frame_metadata_bytes = frame_stride - offset;
            payload_end = 4100u64
                .checked_add(
                    (frame_stride as u64)
                        .checked_mul(frames as u64)
                        .ok_or_else(|| Error("SPE stack size overflow".into()))?,
                )
                .ok_or_else(|| Error("SPE size overflow".into()))?;
            if payload_end > xml_offset {
                return Err(Error("SPE frames overlap XML footer".into()));
            }
            let find = |name: &str| document.descendants().find(|n| n.has_tag_name(name));
            let node_text = |name: &str| find(name).and_then(|n| n.text()).map(str::trim);
            if let Some(origin) = find("Origin").and_then(|n| n.attribute("created")) {
                fields.insert("source_time".into(), origin.into());
                if let Some(time) = iso_time(origin) {
                    fields.insert("time".into(), time);
                }
            }
            if let Some(model) = find("Camera").and_then(|n| n.attribute("model")) {
                fields.insert("detector".into(), model.into());
            }
            let exposure = if let Some(legacy) = find("LegacyExperiment") {
                legacy
                    .descendants()
                    .find(|n| n.has_tag_name("Exposure"))
                    .and_then(|n| n.attribute("value"))
                    .and_then(|s| s.split_ascii_whitespace().next())
                    .and_then(|s| s.parse::<f64>().ok())
            } else if find("ShutterTiming").is_some() {
                node_text("ExposureTime")
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|v| v / 1000.)
            } else {
                let time = find("ReadoutControl")
                    .and_then(|n| n.descendants().find(|n| n.has_tag_name("Time")))
                    .and_then(|n| n.text())
                    .and_then(|s| s.trim().parse::<f64>().ok());
                let accumulations = node_text("Accumulations").and_then(|s| s.parse::<f64>().ok());
                if let Some(value) = accumulations {
                    fields.insert("accumulations".into(), value.to_string());
                }
                time.zip(accumulations).map(|(t, n)| t * n)
            };
            if let Some(value) = exposure {
                fields.insert("exposure_time".into(), value.to_string());
            }
            if let Some(grating) =
                find("Devices").and_then(|n| n.descendants().find(|n| n.has_tag_name("Grating")))
            {
                if let Some(value) = grating
                    .descendants()
                    .find(|n| n.has_tag_name("Selected"))
                    .and_then(|n| n.text())
                {
                    let value = value
                        .split_once('[')
                        .and_then(|(_, s)| s.split_once(']'))
                        .map(|(s, _)| s.replace(',', " "))
                        .unwrap_or_else(|| value.into());
                    fields.insert("grating".into(), value);
                }
                if let Some(value) = grating
                    .descendants()
                    .find(|n| n.has_tag_name("CenterWavelength"))
                    .and_then(|n| n.text())
                {
                    fields.insert("center_wavelength".into(), value.trim().into());
                }
            }
            let custom = node_text("Selection") == Some("CustomRegions");
            if custom
                && let Some(roi) = find("ReadoutControl")
                    .and_then(|n| n.descendants().find(|n| n.has_tag_name("CustomRegions")))
                    .and_then(|n| n.descendants().find(|n| n.has_tag_name("RegionOfInterest")))
            {
                let x = number(roi, "x")?;
                let y = number(roi, "y")?;
                let width = number(roi, "width")?;
                let height = number(roi, "height")?;
                regions[0].roi = [
                    x,
                    x.checked_add(width)
                        .ok_or_else(|| Error("SPE ROI overflow".into()))?,
                    y,
                    y.checked_add(height)
                        .ok_or_else(|| Error("SPE ROI overflow".into()))?,
                ];
            }
            x_calibration = if let Some(values) = find("WavelengthMapping")
                .and_then(|n| n.descendants().find(|n| n.has_tag_name("Wavelength")))
                .and_then(|n| n.text())
            {
                let mut all = Vec::new();
                for value in values.split(',') {
                    if all.len() >= limits.metadata_bytes / 16 {
                        return Err(Error("SPE wavelength array exceeds metadata budget".into()));
                    }
                    all.push(
                        value
                            .trim()
                            .parse::<f64>()
                            .map_err(|_| Error("invalid SPE wavelength calibration".into()))?,
                    );
                }
                let roi = regions[0].roi;
                all.get(roi[0]..roi[1])
                    .ok_or_else(|| Error("SPE wavelength calibration does not cover ROI".into()))?
                    .to_vec()
            } else {
                Vec::new()
            };
        }
        let budget = header
            .len()
            .saturating_add(xml.len().saturating_mul(3))
            .saturating_add(x_calibration.len().saturating_mul(8))
            .saturating_add(fields.values().map(String::len).sum::<usize>())
            .saturating_add(regions.len().saturating_mul(128));
        if budget > limits.metadata_bytes {
            return Err(Error("SPE retained metadata exceeds budget".into()));
        }
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            metadata: Metadata {
                version,
                file_version,
                shape,
                frames,
                pixel_type,
                data_type,
                frame_stride,
                frame_metadata_bytes,
                header,
                xml,
                regions,
                fields,
                calibration_coefficients: coefficients,
                x_calibration,
            },
        })
    }
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    #[must_use]
    pub fn frames(&self) -> usize {
        self.metadata.frames
    }
    /// Read the first stored ROI of a frame, matching FabIO's exposed image.
    /// # Errors
    /// Changed source, invalid frame/region, truncation or failed allocation.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        self.read_region_into(index, 0, buffer)
    }
    /// Read any explicitly indexed XML region without combining or reshaping ROIs.
    /// # Errors
    /// Same errors as `read_frame_into`.
    pub fn read_region_into(
        &self,
        index: usize,
        region: usize,
        buffer: &mut FrameBuffer,
    ) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        if index >= self.frames() {
            return Err(Error("SPE frame outside source".into()));
        }
        let region = self
            .metadata
            .regions
            .get(region)
            .ok_or_else(|| Error("SPE ROI outside source".into()))?;
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(
            4100 + index as u64 * self.metadata.frame_stride as u64 + region.offset as u64,
        ))?;
        resize_bytes(&mut buffer.encoded, region.bytes)?;
        file.read_exact(&mut buffer.encoded)?;
        buffer
            .pixels
            .resize(self.metadata.pixel_type, region.shape[0] * region.shape[1])
            .map_err(Error)?;
        copy_samples(&buffer.encoded, &mut buffer.pixels, true)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        buffer.shape = Some(region.shape);
        Ok(())
    }
}
fn float_text(value: f64) -> String {
    let text = value.to_string();
    if text.contains(['.', 'e', 'E']) {
        text
    } else {
        format!("{text}.0")
    }
}
fn legacy_time(date: &[u8], time: &[u8]) -> Option<String> {
    let date = std::str::from_utf8(date).ok()?;
    let time = std::str::from_utf8(time).ok()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|s| date.get(2..5) == Some(*s))?
        + 1;
    Some(format!(
        "{month:02}/{}/{} {}:{}:{}",
        date.get(..2)?,
        date.get(5..9)?,
        time.get(..2)?,
        time.get(2..4)?,
        time.get(4..6)?
    ))
}
fn iso_time(value: &str) -> Option<String> {
    let date = value.get(..10)?;
    let time = value.get(11..)?;
    let time = time.split(['+', '-', 'Z']).next()?;
    let time = if let Some((seconds, fraction)) = time.split_once('.') {
        format!("{seconds}.{:0<6}", &fraction[..fraction.len().min(6)])
    } else {
        time.into()
    };
    Some(format!(
        "{}/{}/{} {time}",
        date.get(5..7)?,
        date.get(8..10)?,
        date.get(..4)?
    ))
}
