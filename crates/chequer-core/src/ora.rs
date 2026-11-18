use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::blend::BlendMode;
use crate::document::{Document, Layer, MAX_SIDE};
use crate::io::{IoError, SaveOptions, decode, encode};
use crate::raster::Raster;

pub const FILE_EXTENSION: &str = "ora";

const MIME_ENTRY: &str = "mimetype";
const MIME_TYPE: &[u8] = b"image/openraster";
const STACK_ENTRY: &str = "stack.xml";
const MERGED_ENTRY: &str = "mergedimage.png";
const LAYER_DIR: &str = "data";
const FORMAT_VERSION: &str = "0.0.3";
const OPACITY_MAX: f32 = 255.0;
const VISIBLE: &str = "visible";
const CHANNELS: usize = 4;

const BLEND_OPS: [(BlendMode, &str); 17] = [
    (BlendMode::Normal, "svg:src-over"),
    (BlendMode::Multiply, "svg:multiply"),
    (BlendMode::Additive, "svg:plus"),
    (BlendMode::ColorBurn, "svg:color-burn"),
    (BlendMode::ColorDodge, "svg:color-dodge"),
    (BlendMode::Overlay, "svg:overlay"),
    (BlendMode::Difference, "svg:difference"),
    (BlendMode::Lighten, "svg:lighten"),
    (BlendMode::Darken, "svg:darken"),
    (BlendMode::Screen, "svg:screen"),
    (BlendMode::Xor, "svg:xor"),
    (BlendMode::HardLight, "svg:hard-light"),
    (BlendMode::SoftLight, "svg:soft-light"),
    (BlendMode::Color, "svg:color"),
    (BlendMode::Luminosity, "svg:luminosity"),
    (BlendMode::Hue, "svg:hue"),
    (BlendMode::Saturation, "svg:saturation"),
];

pub(crate) fn invalid(message: impl ToString) -> IoError {
    IoError::Io(std::io::Error::other(message.to_string()))
}

fn blend_op(blend: BlendMode) -> &'static str {
    BLEND_OPS.iter().find(|(mode, _)| *mode == blend).map_or(BLEND_OPS[0].1, |(_, op)| op)
}

fn blend_from_op(op: &str) -> BlendMode {
    BLEND_OPS.iter().find(|(_, name)| *name == op).map_or(BlendMode::Normal, |(mode, _)| *mode)
}

fn escape_attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn stack_xml(document: &Document) -> String {
    let layers: String = document
        .layers()
        .iter()
        .enumerate()
        .rev()
        .map(|(index, layer)| {
            format!(
                "<layer name=\"{}\" src=\"{LAYER_DIR}/layer{index}.png\" x=\"0\" y=\"0\" opacity=\"{:.4}\" visibility=\"{}\" composite-op=\"{}\"/>",
                escape_attribute(&layer.name),
                f32::from(layer.opacity) / OPACITY_MAX,
                if layer.visible { VISIBLE } else { "hidden" },
                blend_op(layer.blend),
            )
        })
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><image version=\"{FORMAT_VERSION}\" w=\"{}\" h=\"{}\"><stack>{layers}</stack></image>",
        document.width(),
        document.height(),
    )
}

pub fn encode_document(document: &Document) -> Result<Vec<u8>, IoError> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let png = SaveOptions::for_extension("png").ok_or_else(|| invalid("png"))?;
    writer.start_file(MIME_ENTRY, stored).map_err(invalid)?;
    writer.write_all(MIME_TYPE)?;
    writer.start_file(STACK_ENTRY, deflated).map_err(invalid)?;
    writer.write_all(stack_xml(document).as_bytes())?;
    for (index, layer) in document.layers().iter().enumerate() {
        writer.start_file(format!("{LAYER_DIR}/layer{index}.png"), stored).map_err(invalid)?;
        writer.write_all(&encode(layer.raster(), &png)?)?;
    }
    writer.start_file(MERGED_ENTRY, stored).map_err(invalid)?;
    writer.write_all(&encode(&document.flattened(document.bounds()), &png)?)?;
    Ok(writer.finish().map_err(invalid)?.into_inner())
}

fn read_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, IoError> {
    let mut bytes = Vec::new();
    archive.by_name(name).map_err(invalid)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn place_on_canvas(layer: &Raster, x: i32, y: i32, width: u32, height: u32) -> Result<Raster, IoError> {
    let mut canvas = vec![0u8; width as usize * height as usize * CHANNELS];
    let source = layer.as_bytes();
    for row in 0..layer.height() as i64 {
        let target_y = i64::from(y) + row;
        if target_y < 0 || target_y >= i64::from(height) {
            continue;
        }
        for column in 0..layer.width() as i64 {
            let target_x = i64::from(x) + column;
            if target_x < 0 || target_x >= i64::from(width) {
                continue;
            }
            let from = (row as usize * layer.width() as usize + column as usize) * CHANNELS;
            let to = (target_y as usize * width as usize + target_x as usize) * CHANNELS;
            canvas[to..to + CHANNELS].copy_from_slice(&source[from..from + CHANNELS]);
        }
    }
    Raster::from_rgba(width, height, canvas).map_err(IoError::Raster)
}

fn dimension(node: roxmltree::Node, name: &str) -> Result<u32, IoError> {
    node.attribute(name).and_then(|value| value.parse().ok()).filter(|side| (1..=MAX_SIDE).contains(side)).ok_or_else(|| invalid(format!("stack.xml: {name}")))
}

fn offset(node: roxmltree::Node, name: &str) -> i32 {
    node.attribute(name).and_then(|value| value.parse().ok()).unwrap_or(0)
}

pub fn decode_document(bytes: &[u8]) -> Result<Document, IoError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(invalid)?;
    let stack = String::from_utf8(read_entry(&mut archive, STACK_ENTRY)?).map_err(invalid)?;
    let xml = roxmltree::Document::parse(&stack).map_err(invalid)?;
    let image = xml.root_element();
    let (width, height) = (dimension(image, "w")?, dimension(image, "h")?);
    let mut layers = Vec::new();
    for node in image.descendants().filter(|node| node.has_tag_name("layer")).collect::<Vec<_>>().into_iter().rev() {
        let source = node.attribute("src").ok_or_else(|| invalid("stack.xml: src"))?;
        let raster = place_on_canvas(&decode(&read_entry(&mut archive, source)?)?, offset(node, "x"), offset(node, "y"), width, height)?;
        let mut layer = Layer::new(node.attribute("name").unwrap_or_default().to_owned(), raster);
        layer.visible = node.attribute("visibility").is_none_or(|value| value == VISIBLE);
        layer.opacity = node.attribute("opacity").and_then(|value| value.parse::<f32>().ok()).map_or(u8::MAX, |opacity| (opacity.clamp(0.0, 1.0) * OPACITY_MAX).round() as u8);
        layer.blend = blend_from_op(node.attribute("composite-op").unwrap_or_default());
        layers.push(layer);
    }
    if layers.is_empty() {
        return Err(invalid("stack.xml: no layers"));
    }
    let top = layers.len() - 1;
    Document::from_layers(layers, top).map_err(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_survive_round_trip() {
        let mut bottom = Layer::new("a & b".to_owned(), Raster::from_rgba(2, 1, vec![1, 2, 3, 255, 4, 5, 6, 255]).unwrap());
        bottom.opacity = 128;
        let mut top = Layer::new("top".to_owned(), Raster::from_rgba(2, 1, vec![9, 9, 9, 0, 7, 8, 9, 255]).unwrap());
        top.blend = BlendMode::Multiply;
        top.visible = false;
        let document = Document::from_layers(vec![bottom, top], 1).unwrap();
        let restored = decode_document(&encode_document(&document).unwrap()).unwrap();
        assert_eq!(restored.layers().len(), 2);
        assert_eq!(restored.layers()[0].name, "a & b");
        assert_eq!(restored.layers()[0].opacity, 128);
        assert_eq!(restored.layers()[0].raster().as_bytes(), document.layers()[0].raster().as_bytes());
        assert_eq!(restored.layers()[1].blend, BlendMode::Multiply);
        assert!(!restored.layers()[1].visible);
    }
}
