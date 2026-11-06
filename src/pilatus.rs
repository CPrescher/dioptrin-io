//! Shared bounded Pilatus acquisition metadata for TIFF and CBF.
//! Field schema follows MIT FabIO cbfimage.PilatusHeader, ESRF. Retained notice:
//! licenses/native-readers/FabIO-2026.6.0-copyright.txt. Original text is retained.
use std::collections::BTreeMap;
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Integer(i64),
    Float(f64),
    Sequence(Vec<Value>),
    Invalid(String),
}
#[derive(Debug, Clone)]
pub struct Header {
    /// Original ImageDescription text, including comments the field parser does
    /// not interpret. Original TIFF tag values are also retained by image metadata.
    pub description: String,
    /// Typed acquisition values, without applying corrections or changing units.
    pub typed_fields: BTreeMap<String, Value>,
    /// Last assignment to each key, as in the pinned reference. Units remain text.
    pub fields: BTreeMap<String, String>,
    /// Every interpreted assignment, retaining duplicate keys and source order.
    pub entries: Vec<(String, String)>,
    pub unparsed_lines: Vec<String>,
}
fn parse_text(description: &str) -> Header {
    let mut header = Header {
        description: description.into(),
        typed_fields: BTreeMap::new(),
        fields: BTreeMap::new(),
        entries: Vec::new(),
        unparsed_lines: Vec::new(),
    };
    for line in description.lines() {
        let original = line.trim_matches(|c: char| c.is_whitespace() || c == '\0');
        if original.is_empty() {
            continue;
        }
        let Some(text) = original.strip_prefix("# ") else {
            header.unparsed_lines.push(line.into());
            continue;
        };
        let text = text.trim();
        let Some(position) = text.find(|c: char| c.is_whitespace() || [',', ':', '='].contains(&c))
        else {
            header.unparsed_lines.push(line.into());
            continue;
        };
        let key = text[..position].trim();
        let value = text[position..]
            .trim_start_matches(|c: char| c.is_whitespace() || [',', ':', '='].contains(&c));
        if key.is_empty() {
            header.unparsed_lines.push(line.into());
            continue;
        }
        header.entries.push((key.into(), value.into()));
        header.fields.insert(key.into(), value.into());
    }
    header
}

impl Header {
    /// Parse acquisition text without performing any detector correction.
    /// # Errors
    /// The conservative retained-output estimate exceeds max_metadata_bytes.
    pub fn parse(description: &str, max_metadata_bytes: usize) -> Result<Self, String> {
        let lines = description.lines().count();
        let cost = description
            .len()
            .checked_mul(8)
            .and_then(|n| n.checked_add(lines.saturating_mul(512)))
            .filter(|&n| n <= max_metadata_bytes)
            .ok_or("Pilatus text exceeds metadata budget")?;
        let _ = cost;
        let mut result = parse_text(description);
        let cleaned: String = description
            .chars()
            .map(|c| if "()#:=,".contains(c) { ' ' } else { c })
            .collect();
        let joined = cleaned.replace("\\\r\n", "").replace("\\\n", "");
        for line in joined.lines() {
            let words: Vec<_> = line.split_whitespace().collect();
            let Some(&key) = words.first() else {
                continue;
            };
            let number = |index: usize| {
                words
                    .get(index)
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(Value::Float)
            };
            let text = |index: usize| words.get(index).map(|value| Value::Text((*value).into()));
            let value = if words.get(1) == Some(&"sensor") {
                match (text(0), number(3)) {
                    (Some(a), Some(b)) => Some(("sensor", Value::Sequence(vec![a, b]))),
                    _ => None,
                }
            } else if key == "Detector" {
                Some((key, Value::Text(words[1..].join(" "))))
            } else if ["Pixel_size", "Beam_xy", "Energy_range", "Gain_setting"].contains(&key) {
                let second = if key == "Pixel_size" { 4 } else { 2 };
                let values = if key == "Gain_setting" {
                    (text(1), text(second))
                } else {
                    (number(1), number(second))
                };
                match values {
                    (Some(a), Some(b)) => Some((key, Value::Sequence(vec![a, b]))),
                    _ => None,
                }
            } else if ["Count_cutoff", "N_excluded_pixels", "N_oscillations"].contains(&key) {
                words
                    .get(1)
                    .and_then(|value| value.parse().ok())
                    .map(|n| (key, Value::Integer(n)))
            } else if [
                "Excluded_pixels",
                "Flat_field",
                "Trim_file",
                "Image_path",
                "Oscillation_axis",
            ]
            .contains(&key)
            {
                text(1).map(|value| (key, value))
            } else if [
                "Exposure_time",
                "Exposure_period",
                "Tau",
                "Threshold_setting",
                "Wavelength",
                "Detector_distance",
                "Detector_Voffset",
                "Flux",
                "Filter_transmission",
                "Start_angle",
                "Angle_increment",
                "Detector_2theta",
                "Polarization",
                "Alpha",
                "Kappa",
                "Phi",
                "Phi_increment",
                "Chi",
                "Chi_increment",
                "Omega",
                "Omega_increment",
                "Start_position",
                "Position_increment",
                "Shutter_time",
            ]
            .contains(&key)
            {
                number(1).map(|value| (key, value))
            } else {
                continue;
            };
            if let Some((key, value)) = value {
                result.typed_fields.insert(key.into(), value);
            } else {
                result
                    .typed_fields
                    .insert(key.into(), Value::Invalid(line.into()));
            }
        }
        Ok(result)
    }
}
