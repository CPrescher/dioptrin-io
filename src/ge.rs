//! GE a-Si Angio stored-order frames, including APS blank/EDF header conventions.
//! The source orientation flags and every original header byte remain metadata.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderKind {
    Standard8001,
    ApsBlank8192,
    ApsEdf8192,
}
#[derive(Clone, Debug)]
pub enum HeaderValue {
    Bytes(Vec<u8>),
    U16(u16),
    U32(u32),
    F32(f32),
    F64(f64),
}
#[derive(Clone, Debug)]
pub struct Metadata {
    pub kind: HeaderKind,
    pub shape: [usize; 2],
    pub frames: usize,
    pub pixel_type: PixelType,
    pub standard_header_bytes: usize,
    pub user_header_bytes: usize,
    /// Complete original standard/user header, including unparsed bytes.
    pub header: Vec<u8>,
    /// Named binary fields for the standard 8001 header; byte strings stay exact.
    pub fields: BTreeMap<&'static str, HeaderValue>,
    /// Original Latin-1 EDF assignments for the APS EDF-in-GE convention.
    pub edf_fields: BTreeMap<String, String>,
}
pub struct Reader {
    snapshot: Snapshot,
    file: File,
    metadata: Metadata,
    frame_bytes: usize,
    offset: u64,
}
impl Reader {
    /// Open frozen GE frames. APS blank/EDF layouts use their exact 8192-byte,
    /// 2048x2048-u16 storage convention and retain its provenance in `kind`.
    /// # Errors
    /// Unknown headers/layouts, invalid dimensions, source lengths or resource limits.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        let snapshot = Snapshot::open(path).map_err(Error)?;
        let mut file = File::open(&snapshot.path)?;
        snapshot.check_handle(&file).map_err(Error)?;
        let mut prefix = [0u8; 30];
        file.read_exact(&mut prefix)?;
        let u16_at = |p| usize::from(u16::from_le_bytes(prefix[p..p + 2].try_into().unwrap()));
        let u32_at = |p| u32::from_le_bytes(prefix[p..p + 4].try_into().unwrap()) as usize;
        let (kind, standard, user, shape, depth, declared_frames) =
            if &prefix[..10] == b"ADEPT\0\0\0\0\0" {
                if u16_at(10) != 8001 {
                    return Err(Error("unsupported GE standard header version".into()));
                }
                let standard = u32_at(12);
                let user = u32_at(18);
                if standard < 3332 {
                    return Err(Error(
                        "GE standard header shorter than version 8001 fields".into(),
                    ));
                }
                (
                    HeaderKind::Standard8001,
                    standard,
                    user,
                    [u16_at(24), u16_at(26)],
                    u16_at(28),
                    Some(u16_at(22)),
                )
            } else if prefix[..10].iter().all(|&b| b == 0) {
                (HeaderKind::ApsBlank8192, 8192, 0, [2048, 2048], 16, None)
            } else if prefix.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'{') {
                (HeaderKind::ApsEdf8192, 8192, 0, [2048, 2048], 16, None)
            } else {
                return Err(Error("unknown GE image header".into()));
            };
        let pixel_type = match depth {
            8 => PixelType::U8,
            16 => PixelType::U16,
            32 => PixelType::U32,
            _ => return Err(Error(format!("unsupported GE depth {depth}"))),
        };
        let count = limits.pixels(shape, pixel_type)?;
        let frame_bytes = count * pixel_type.bytes();
        if frame_bytes > limits.encoded_bytes {
            return Err(Error("GE frame exceeds encoded-byte limit".into()));
        }
        let offset = standard
            .checked_add(user)
            .filter(|&n| n <= limits.header_bytes && n as u64 <= snapshot.bytes())
            .ok_or_else(|| Error("GE header lengths exceed source/header limits".into()))?;
        offset
            .checked_mul(3)
            .and_then(|n| n.checked_add(HEADER_FIELDS.len() * 128))
            .filter(|&n| n <= limits.metadata_bytes)
            .ok_or_else(|| Error("GE header exceeds metadata-byte limit".into()))?;
        let mut header = Vec::new();
        resize_bytes(&mut header, offset)?;
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut header)?;
        let mut fields = BTreeMap::new();
        let mut edf_fields = BTreeMap::new();
        let declared_frames = match kind {
            HeaderKind::Standard8001 => {
                let mut position = 0;
                for &(name, bytes, ty) in HEADER_FIELDS {
                    let raw = &header[position..position + bytes];
                    position += bytes;
                    let value = match ty {
                        b'b' => HeaderValue::Bytes(raw.to_vec()),
                        b'h' => HeaderValue::U16(u16::from_le_bytes(raw.try_into().unwrap())),
                        b'l' => HeaderValue::U32(u32::from_le_bytes(raw.try_into().unwrap())),
                        b'f' => HeaderValue::F32(f32::from_le_bytes(raw.try_into().unwrap())),
                        b'd' => HeaderValue::F64(f64::from_le_bytes(raw.try_into().unwrap())),
                        _ => unreachable!(),
                    };
                    fields.insert(name, value);
                }
                declared_frames
            }
            HeaderKind::ApsBlank8192 => {
                if header.iter().any(|&b| b != 0) {
                    return Err(Error("APS blank GE header contains nonzero bytes".into()));
                }
                None
            }
            HeaderKind::ApsEdf8192 => {
                edf_fields =
                    parse_edf(&header, limits.metadata_bytes.saturating_sub(header.len()))?;
                let required = |key: &str| -> Result<&str, Error> {
                    edf_fields
                        .get(key)
                        .map(String::as_str)
                        .ok_or_else(|| Error(format!("missing GE EDF {key}")))
                };
                let number = |key: &str| -> Result<usize, Error> {
                    required(key)?
                        .parse()
                        .map_err(|_| Error(format!("invalid GE EDF {key}")))
                };
                if number("EDF_HeaderSize")? != 8192
                    || number("Dim_1")? != 2048
                    || number("Dim_2")? != 2048
                    || number("Size")? != frame_bytes
                    || required("DataType")? != "UnsignedShort"
                    || required("ByteOrder")? != "LowByteFirst"
                {
                    return Err(Error(
                        "GE EDF metadata contradicts APS storage convention".into(),
                    ));
                }
                Some(number("Num_Images")?)
            }
        };
        let payload = snapshot.bytes() - offset as u64;
        if payload == 0 || !payload.is_multiple_of(frame_bytes as u64) {
            return Err(Error("GE source has truncated/trailing frame bytes".into()));
        }
        let frames = usize::try_from(payload / frame_bytes as u64)
            .map_err(|_| Error("GE frame count overflows usize".into()))?;
        if frames > limits.frames || declared_frames.is_some_and(|n| n == 0 || n != frames) {
            return Err(Error(
                "GE source/header frame counts disagree or exceed limit".into(),
            ));
        }
        snapshot.check_handle(&file).map_err(Error)?;
        Ok(Self {
            snapshot,
            file,
            frame_bytes,
            offset: offset as u64,
            metadata: Metadata {
                kind,
                shape,
                frames,
                pixel_type,
                standard_header_bytes: standard,
                user_header_bytes: user,
                header,
                fields,
                edf_fields,
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
    /// Read one stored frame without applying source flip/reorder flags.
    /// # Errors
    /// Invalid index, changed source, truncation or allocation failure.
    pub fn read_frame_into(&self, index: usize, buffer: &mut FrameBuffer) -> Result<(), Error> {
        buffer.shape = None;
        self.snapshot.check_handle(&self.file).map_err(Error)?;
        if index >= self.frames() {
            return Err(Error("GE frame outside source".into()));
        }
        let mut file = File::open(&self.snapshot.path)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        file.seek(SeekFrom::Start(
            self.offset + index as u64 * self.frame_bytes as u64,
        ))?;
        resize_bytes(&mut buffer.encoded, self.frame_bytes)?;
        file.read_exact(&mut buffer.encoded)?;
        let shape = self.metadata.shape;
        buffer
            .pixels
            .resize(self.metadata.pixel_type, shape[0] * shape[1])
            .map_err(Error)?;
        copy_samples(&buffer.encoded, &mut buffer.pixels, true)?;
        self.snapshot.check_handle(&file).map_err(Error)?;
        buffer.shape = Some(shape);
        Ok(())
    }
}
fn parse_edf(header: &[u8], budget: usize) -> Result<BTreeMap<String, String>, Error> {
    let text: String = header.iter().map(|&b| char::from(b)).collect();
    let text = text.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0');
    let body = text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or_else(|| Error("invalid GE EDF header braces".into()))?;
    let mut fields = BTreeMap::new();
    let mut retained = 0usize;
    for assignment in body.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (key, value) = assignment
            .split_once('=')
            .ok_or_else(|| Error("invalid GE EDF assignment".into()))?;
        retained = retained
            .checked_add(key.len().saturating_add(value.len()).saturating_add(128))
            .filter(|&n| n <= budget)
            .ok_or_else(|| Error("GE EDF assignments exceed metadata budget".into()))?;
        if fields
            .insert(key.trim().to_owned(), value.trim().to_owned())
            .is_some()
        {
            return Err(Error("duplicate GE EDF assignment".into()));
        }
    }
    Ok(fields)
}

// Descriptors adapted from the MIT FabIO GE_HEADER_INFO table, pinned 2026.6.0.
// Copyright 2007-2020 APS; 2010-2020 ESRF. Authors: Antonino Miceli, Jon Wright,
// Jérôme Kieffer, Joel Bernier. Retained notice: licenses/native-readers/FabIO-GE-Pixi-MIT.txt.
const HEADER_FIELDS: &[(&str, usize, u8)] = &[
    ("ImageFormat", 10, b'b'),
    ("VersionOfStandardHeader", 2, b'h'),
    ("StandardHeaderSizeInBytes", 4, b'l'),
    ("VersionOfUserHeader", 2, b'h'),
    ("UserHeaderSizeInBytes", 4, b'l'),
    ("NumberOfFrames", 2, b'h'),
    ("NumberOfRowsInFrame", 2, b'h'),
    ("NumberOfColsInFrame", 2, b'h'),
    ("ImageDepthInBits", 2, b'h'),
    ("AcquisitionDate", 20, b'b'),
    ("AcquisitionTime", 20, b'b'),
    ("DUTID", 20, b'b'),
    ("Operator", 50, b'b'),
    ("DetectorSignature", 20, b'b'),
    ("TestSystemName", 20, b'b'),
    ("TestStationRevision", 20, b'b'),
    ("CoreBundleRevision", 20, b'b'),
    ("AcquisitionName", 40, b'b'),
    ("AcquisitionParameterRevision", 20, b'b'),
    ("OriginalNumberOfRows", 2, b'h'),
    ("OriginalNumberOfColumns", 2, b'h'),
    ("RowNumberUpperLeftPointArchiveROI", 2, b'h'),
    ("ColNumberUpperLeftPointArchiveROI", 2, b'h'),
    ("Swapped", 2, b'h'),
    ("Reordered", 2, b'h'),
    ("HorizontalFlipped", 2, b'h'),
    ("VerticalFlipped", 2, b'h'),
    ("WindowValueDesired", 2, b'h'),
    ("LevelValueDesired", 2, b'h'),
    ("AcquisitionMode", 2, b'h'),
    ("AcquisitionType", 2, b'h'),
    ("UserAcquisitionCoffFileName1", 100, b'b'),
    ("UserAcquisitionCoffFileName2", 100, b'b'),
    ("FramesBeforeExpose", 2, b'h'),
    ("FramesDuringExpose", 2, b'h'),
    ("FramesAfterExpose", 2, b'h'),
    ("IntervalBetweenFrames", 2, b'h'),
    ("ExposeTimeDelayInMicrosecs", 8, b'd'),
    ("TimeBetweenFramesInMicrosecs", 8, b'd'),
    ("FramesToSkipExpose", 2, b'h'),
    ("ExposureMode", 2, b'h'),
    ("PrepPresetTimeInMicrosecs", 8, b'd'),
    ("ExposePresetTimeInMicrosecs", 8, b'd'),
    ("AcquisitionFrameRateInFps", 4, b'f'),
    ("FOVSelect", 2, b'h'),
    ("ExpertMode", 2, b'h'),
    ("SetVCommon1", 8, b'd'),
    ("SetVCommon2", 8, b'd'),
    ("SetAREF", 8, b'd'),
    ("SetAREFTrim", 4, b'l'),
    ("SetSpareVoltageSource", 8, b'd'),
    ("SetCompensationVoltageSource", 8, b'd'),
    ("SetRowOffVoltage", 8, b'd'),
    ("SetRowOnVoltage", 8, b'd'),
    ("StoreCompensationVoltage", 4, b'l'),
    ("RampSelection", 2, b'h'),
    ("TimingMode", 2, b'h'),
    ("Bandwidth", 2, b'h'),
    ("ARCIntegrator", 2, b'h'),
    ("ARCPostIntegrator", 2, b'h'),
    ("NumberOfRows", 4, b'l'),
    ("RowEnable", 2, b'h'),
    ("EnableStretch", 2, b'h'),
    ("CompEnable", 2, b'h'),
    ("CompStretch", 2, b'h'),
    ("LeftEvenTristate", 2, b'h'),
    ("RightOddTristate", 2, b'h'),
    ("TestModeSelect", 4, b'l'),
    ("AnalogTestSource", 4, b'l'),
    ("VCommonSelect", 4, b'l'),
    ("DRCColumnSum", 4, b'l'),
    ("TestPatternFrameDelta", 4, b'l'),
    ("TestPatternRowDelta", 4, b'l'),
    ("TestPatternColumnDelta", 4, b'l'),
    ("DetectorHorizontalFlip", 2, b'h'),
    ("DetectorVerticalFlip", 2, b'h'),
    ("DFNAutoScrubOnOff", 2, b'h'),
    ("FiberChannelTimeOutInMicrosecs", 4, b'l'),
    ("DFNAutoScrubDelayInMicrosecs", 4, b'l'),
    ("StoreAECROI", 2, b'h'),
    ("TestPatternSaturationValue", 2, b'h'),
    ("TestPatternSeed", 4, b'l'),
    ("ExposureTimeInMillisecs", 4, b'f'),
    ("FrameRateInFps", 4, b'f'),
    ("kVp", 4, b'f'),
    ("mA", 4, b'f'),
    ("mAs", 4, b'f'),
    ("FocalSpotInMM", 4, b'f'),
    ("GeneratorType", 20, b'b'),
    ("StrobeIntensityInFtL", 4, b'f'),
    ("NDFilterSelection", 2, b'h'),
    ("RefRegTemp1", 8, b'd'),
    ("RefRegTemp2", 8, b'd'),
    ("RefRegTemp3", 8, b'd'),
    ("Humidity1", 4, b'f'),
    ("Humidity2", 4, b'f'),
    ("DetectorControlTemp", 8, b'd'),
    ("DoseValueInmR", 8, b'd'),
    ("TargetLevelROIRow0", 2, b'h'),
    ("TargetLevelROICol0", 2, b'h'),
    ("TargetLevelROIRow1", 2, b'h'),
    ("TargetLevelROICol1", 2, b'h'),
    ("FrameNumberForTargetLevelROI", 2, b'h'),
    ("PercentRangeForTargetLevel", 2, b'h'),
    ("TargetValue", 2, b'h'),
    ("ComputedMedianValue", 2, b'h'),
    ("LoadZero", 2, b'h'),
    ("MaxLUTOut", 2, b'h'),
    ("MinLUTOut", 2, b'h'),
    ("MaxLinear", 2, b'h'),
    ("Reserved", 2, b'h'),
    ("ElectronsPerCount", 2, b'h'),
    ("ModeGain", 2, b'h'),
    ("TemperatureInDegC", 8, b'd'),
    ("LineRepaired", 2, b'h'),
    ("LineRepairFileName", 100, b'b'),
    ("CurrentLongitudinalInMM", 4, b'f'),
    ("CurrentTransverseInMM", 4, b'f'),
    ("CurrentCircularInMM", 4, b'f'),
    ("CurrentFilterSelection", 4, b'l'),
    ("DisableScrubAck", 2, b'h'),
    ("ScanModeSelect", 2, b'h'),
    ("DetectorAppSwVersion", 20, b'b'),
    ("DetectorNIOSVersion", 20, b'b'),
    ("DetectorPeripheralSetVersion", 20, b'b'),
    ("DetectorPhysicalAddress", 20, b'b'),
    ("PowerDown", 2, b'h'),
    ("InitialVoltageLevel_VCOMMON", 8, b'd'),
    ("FinalVoltageLevel_VCOMMON", 8, b'd'),
    ("DmrCollimatorSpotSize", 10, b'b'),
    ("DmrTrack", 5, b'b'),
    ("DmrFilter", 5, b'b'),
    ("FilterCarousel", 2, b'h'),
    ("Phantom", 20, b'b'),
    ("SetEnableHighTime", 2, b'h'),
    ("SetEnableLowTime", 2, b'h'),
    ("SetCompHighTime", 2, b'h'),
    ("SetCompLowTime", 2, b'h'),
    ("SetSyncLowTime", 2, b'h'),
    ("SetConvertLowTime", 2, b'h'),
    ("SetSyncHighTime", 2, b'h'),
    ("SetEOLTime", 2, b'h'),
    ("SetRampOffsetTime", 2, b'h'),
    ("FOVStartingValue", 2, b'h'),
    ("ColumnBinning", 2, b'h'),
    ("RowBinning", 2, b'h'),
    ("BorderColumns64", 2, b'h'),
    ("BorderRows64", 2, b'h'),
    ("FETOffRows64", 2, b'h'),
    ("FOVStartColumn128", 2, b'h'),
    ("FOVStartRow128", 2, b'h'),
    ("NumberOfColumns128", 2, b'h'),
    ("NumberOfRows128", 2, b'h'),
    ("VFPAquisition", 2000, b'b'),
    ("Comment", 200, b'b'),
];
