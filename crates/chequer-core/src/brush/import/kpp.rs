use std::collections::HashMap;
use std::io::Read;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flate2::read::ZlibDecoder;
use roxmltree::{Document, Node};

use super::{Reader, decode_gbr};
use crate::brush::{Brush, BrushError, BrushMeta, BrushSettings, BrushTip, Curve, MaskTip, SCATTER_MAX};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const CHUNK_OVERHEAD: usize = 4;
const SUPPORTED_VERSIONS: [&str; 2] = ["2.2", "5.0"];
const PIXEL_ENGINE: &str = "paintbrush";
const SQUARE_TIP_SIDE: u32 = 16;
const FULL_MASK: u8 = u8::MAX;
const SENSORS_LIST: &str = "sensorslist";
const BROKEN_CURVE_OPEN: &str = "<curve><![CDATA[";
const BROKEN_CURVE_CLOSE: &str = "]]></curve>";
const SENSORS_DOCTYPE: &str = "<!DOCTYPE params>";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sensor {
    Pressure,
    Speed,
    Fuzzy,
    DrawingAngle,
    TiltDirection,
    TiltElevation,
}

impl Sensor {
    fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "pressure" => Self::Pressure,
            "speed" => Self::Speed,
            "fuzzy" | "fuzzystroke" => Self::Fuzzy,
            "drawingangle" => Self::DrawingAngle,
            "ascension" => Self::TiltDirection,
            "declination" => Self::TiltElevation,
            _ => return None,
        })
    }
}

struct Dynamic(Vec<(Sensor, Curve)>);

impl Dynamic {
    fn curve(&self, sensor: Sensor) -> Option<&Curve> {
        self.0.iter().find(|(kind, _)| *kind == sensor).map(|(_, curve)| curve)
    }

    fn has(&self, sensor: Sensor) -> bool {
        self.curve(sensor).is_some()
    }

    fn fall(&self, sensor: Sensor) -> f32 {
        self.curve(sensor).map_or(0.0, |curve| (curve.eval(0.0) - curve.eval(1.0)).max(0.0))
    }

    fn randomness(&self) -> f32 {
        self.curve(Sensor::Fuzzy).map_or(0.0, |curve| 1.0 - curve.eval(0.0).min(curve.eval(1.0)))
    }
}

struct Preset<'a> {
    params: HashMap<&'a str, &'a str>,
}

impl Preset<'_> {
    fn flag(&self, name: &str) -> bool {
        self.params.get(name).is_some_and(|value| value.trim() == "true")
    }

    fn number(&self, name: &str, default: f32) -> f32 {
        self.params.get(name).and_then(|value| value.trim().parse().ok()).unwrap_or(default)
    }

    fn dynamic(&self, name: &str, checkable: bool) -> Dynamic {
        let enabled = if checkable { self.flag(&format!("Pressure{name}")) } else { true };
        let uses_curve = self.params.get(format!("{name}UseCurve").as_str()).is_none_or(|value| value.trim() != "false");
        let sensors = self.params.get(format!("{name}Sensor").as_str()).filter(|_| enabled && uses_curve);
        Dynamic(sensors.map(|xml| parse_sensors(xml)).unwrap_or_default())
    }
}

pub fn decode_kpp(bytes: &[u8]) -> Result<Brush, BrushError> {
    let texts = png_texts(bytes)?;
    let version = texts.get("version").map(String::as_str).unwrap_or_default();
    if !SUPPORTED_VERSIONS.contains(&version) {
        return Err(BrushError::Unsupported(format!("версия пресета Krita «{version}»")));
    }
    let xml = texts.get("preset").ok_or(BrushError::MissingChunk("preset"))?.replace(BROKEN_CURVE_OPEN, "<curve>").replace(BROKEN_CURVE_CLOSE, "</curve>");
    let document = Document::parse(&xml)?;
    let root = document.root_element();
    let engine = root.attribute("paintopid").unwrap_or_default();
    if engine != PIXEL_ENGINE {
        return Err(BrushError::Unsupported(format!("движок Krita «{engine}», нужен «{PIXEL_ENGINE}»")));
    }
    let params = root.children().filter(|node| node.has_tag_name("param")).filter_map(|node| Some((node.attribute("name")?, node.text().unwrap_or_default()))).collect();
    let preset = Preset { params };
    let definition = preset.params.get("brush_definition").ok_or(BrushError::MissingChunk("brush_definition"))?;
    let definition = Document::parse(definition)?;
    let brush = definition.root_element();
    let mask = brush.children().find(|node| node.has_tag_name("MaskGenerator"));
    let tip = read_tip(brush, mask, &resources(root))?;
    let name = root.attribute("name").unwrap_or_default().to_owned();
    let settings = read_settings(&preset, brush, mask);
    Ok(Brush { meta: BrushMeta { name, ..BrushMeta::default() }, settings: settings.clamped(), tip, textures: Default::default() })
}

fn read_settings(preset: &Preset, brush: Node, mask: Option<Node>) -> BrushSettings {
    let mut settings = BrushSettings::default();
    let attribute = |name: &str, default: f32| brush.attribute(name).and_then(|value| value.parse().ok()).unwrap_or(default);
    settings.spacing = attribute("spacing", settings.spacing);
    settings.angle = attribute("angle", 0.0).to_degrees();
    if let Some(mask) = mask {
        let number = |name: &str, default: f32| mask.attribute(name).and_then(|value| value.parse().ok()).unwrap_or(default);
        settings.roundness = number("ratio", 1.0);
        settings.hardness = (number("hfade", 1.0) + number("vfade", 1.0)) / 2.0;
    }
    let size = preset.dynamic("Size", true);
    settings.pressure_size = f32::from(size.has(Sensor::Pressure));
    settings.pressure_size_curve = size.curve(Sensor::Pressure).cloned().unwrap_or_default();
    settings.size_jitter = size.randomness();
    settings.velocity_size = size.fall(Sensor::Speed);
    let (opacity, flow) = (preset.dynamic("Opacity", false), preset.dynamic("Flow", false));
    let pressure_source = if opacity.has(Sensor::Pressure) { &opacity } else { &flow };
    settings.pressure_opacity = f32::from(pressure_source.has(Sensor::Pressure));
    settings.pressure_opacity_curve = pressure_source.curve(Sensor::Pressure).cloned().unwrap_or_default();
    settings.opacity_jitter = opacity.randomness().max(flow.randomness());
    settings.velocity_opacity = opacity.fall(Sensor::Speed).max(flow.fall(Sensor::Speed));
    settings.flow = preset.number("OpacityValue", 1.0) * preset.number("FlowValue", 1.0);
    let rotation = preset.dynamic("Rotation", true);
    settings.follow_direction = rotation.has(Sensor::DrawingAngle);
    settings.angle_jitter = rotation.curve(Sensor::Fuzzy).map_or(0.0, |curve| (curve.eval(1.0) - curve.eval(0.0)).abs());
    settings.tilt_angle = f32::from(rotation.has(Sensor::TiltDirection));
    let ratio = preset.dynamic("Ratio", true);
    settings.pressure_roundness = ratio.curve(Sensor::Pressure).map_or(0.0, |curve| 1.0 - curve.eval(0.0));
    settings.tilt_roundness = f32::from(ratio.has(Sensor::TiltElevation));
    if preset.flag("PressureScatter") {
        settings.scatter = preset.number("Scattering/AxisX", 0.0).max(preset.number("Scattering/AxisY", 0.0)).clamp(0.0, SCATTER_MAX);
    }
    settings
}

fn read_tip(brush: Node, mask: Option<Node>, resources: &HashMap<String, Vec<u8>>) -> Result<BrushTip, BrushError> {
    let kind = brush.attribute("type").unwrap_or_default();
    if kind == "auto_brush" {
        let square = mask.and_then(|node| node.attribute("type")) == Some("rect");
        return Ok(if square { BrushTip::Mask(MaskTip::new(SQUARE_TIP_SIDE, SQUARE_TIP_SIDE, vec![FULL_MASK; (SQUARE_TIP_SIDE * SQUARE_TIP_SIDE) as usize])?) } else { BrushTip::Round });
    }
    let embedded = brush.attribute("filename").and_then(|file| resources.get(file));
    let Some(data) = embedded else {
        return Ok(BrushTip::Round);
    };
    match kind {
        "gbr_brush" => decode_gbr(data).map(|brush| brush.tip),
        "png_brush" => Ok(BrushTip::Mask(MaskTip::from_image(&image::load_from_memory(data)?)?)),
        other => Err(BrushError::Unsupported(format!("наконечник Krita «{other}»"))),
    }
}

fn resources(root: Node) -> HashMap<String, Vec<u8>> {
    let Some(list) = root.children().find(|node| node.has_tag_name("resources")) else {
        return HashMap::new();
    };
    list.children()
        .filter(|node| node.has_tag_name("resource"))
        .filter_map(|node| {
            let data = STANDARD.decode(node.text()?.split_whitespace().collect::<String>()).ok()?;
            Some((node.attribute("filename")?.to_owned(), data))
        })
        .collect()
}

fn parse_sensors(xml: &str) -> Vec<(Sensor, Curve)> {
    let xml = xml.replace(SENSORS_DOCTYPE, "");
    let Ok(document) = Document::parse(&xml) else {
        return Vec::new();
    };
    let root = document.root_element();
    let curve_of = |node: Node| node.children().find(|child| child.has_tag_name("curve")).and_then(|child| child.text()).map_or_else(Curve::linear, parse_curve);
    if root.attribute("id") == Some(SENSORS_LIST) {
        return root.children().filter(|node| node.has_tag_name("ChildSensor")).filter_map(|node| Some((Sensor::from_id(node.attribute("id")?)?, curve_of(node)))).collect();
    }
    root.attribute("id").and_then(Sensor::from_id).map(|sensor| vec![(sensor, curve_of(root))]).unwrap_or_default()
}

fn parse_curve(text: &str) -> Curve {
    let points = text
        .split(';')
        .filter_map(|pair| {
            let (x, y) = pair.split_once(',')?;
            Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
        })
        .collect();
    Curve::new(points)
}

fn png_texts(bytes: &[u8]) -> Result<HashMap<String, String>, BrushError> {
    let mut reader = Reader::new(bytes);
    if reader.take(PNG_SIGNATURE.len())? != PNG_SIGNATURE {
        return Err(BrushError::Malformed("это не PNG"));
    }
    let mut texts = HashMap::new();
    while reader.remaining() >= CHUNK_OVERHEAD * 2 {
        let length = reader.u32()? as usize;
        let kind = reader.take(CHUNK_OVERHEAD)?;
        let body = reader.take(length)?;
        reader.skip(CHUNK_OVERHEAD)?;
        match kind {
            b"tEXt" => {
                if let Some((keyword, text)) = split_keyword(body) {
                    texts.insert(keyword, String::from_utf8_lossy(text).into_owned());
                }
            }
            b"zTXt" => {
                if let Some((keyword, rest)) = split_keyword(body) {
                    let mut text = String::new();
                    ZlibDecoder::new(rest.get(1..).unwrap_or_default()).read_to_string(&mut text).map_err(|_| BrushError::Malformed("блок zTXt не распаковался"))?;
                    texts.insert(keyword, text);
                }
            }
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
    }
    Ok(texts)
}

fn split_keyword(body: &[u8]) -> Option<(String, &[u8])> {
    let end = body.iter().position(|&byte| byte == 0)?;
    Some((String::from_utf8_lossy(&body[..end]).into_owned(), &body[end + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_text_is_parsed_and_broken_text_falls_back_to_linear() {
        assert_eq!(parse_curve("0,0;0.5,0.2;1,1;").points(), &[(0.0, 0.0), (0.5, 0.2), (1.0, 1.0)]);
        assert!(parse_curve("мусор").is_linear());
    }

    #[test]
    fn sensor_list_keeps_known_sensors_with_their_curves() {
        let sensors = parse_sensors("<!DOCTYPE params> <params id=\"sensorslist\"> <ChildSensor id=\"fuzzy\"> <curve>0,0.5;1,1;</curve> </ChildSensor> <ChildSensor id=\"time\"/> <ChildSensor id=\"pressure\"> <curve>0,0;1,0.5;</curve> </ChildSensor> </params>");
        assert_eq!(sensors.len(), 2);
        assert!(Dynamic(sensors).randomness() > 0.4);
    }

    #[test]
    fn preset_in_png_text_chunk_yields_tip_and_pressure_curve() {
        let xml = concat!(
            "<Preset name=\"Тест\" paintopid=\"paintbrush\">",
            "<param name=\"brush_definition\" type=\"string\"><![CDATA[<Brush type=\"auto_brush\" spacing=\"0.2\" angle=\"0\"><MaskGenerator type=\"circle\" ratio=\"0.5\" hfade=\"0.8\" vfade=\"0.8\"/></Brush>]]></param>",
            "<param name=\"PressureSize\" type=\"string\"><![CDATA[true]]></param>",
            "<param name=\"SizeSensor\" type=\"string\"><![CDATA[<!DOCTYPE params><params id=\"pressure\"><curve>0,0;0.35,0.1;1,1;</curve></params>]]></param>",
            "</Preset>"
        );
        let mut png = PNG_SIGNATURE.to_vec();
        for (kind, body) in [(&b"tEXt"[..], b"version\x002.2".to_vec()), (&b"tEXt"[..], [b"preset\0".as_slice(), xml.as_bytes()].concat())] {
            png.extend((body.len() as u32).to_be_bytes());
            png.extend(kind);
            png.extend(&body);
            png.extend([0; CHUNK_OVERHEAD]);
        }
        png.extend(0u32.to_be_bytes());
        png.extend(b"IEND");
        png.extend([0; CHUNK_OVERHEAD]);
        let brush = decode_kpp(&png).unwrap();
        assert_eq!(brush.meta.name, "Тест");
        assert_eq!(brush.settings.spacing, 0.2);
        assert_eq!(brush.settings.roundness, 0.5);
        assert!((brush.settings.pressure_size_curve.eval(0.35) - 0.1).abs() < 1e-4);
        assert!(matches!(brush.tip, BrushTip::Round));
    }
}
