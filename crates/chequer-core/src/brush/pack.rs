use std::io::Cursor;

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, ImageFormat};
use serde::{Deserialize, Serialize};

use super::{Brush, BrushError, BrushMeta, BrushSettings, BrushTextures, BrushTip, LayerSpec, MAX_TRACK_LAYERS, MaskTip, PatternSpec, TIP_WEIGHT_RANGE, Texture, TipSpec, TipVariant, TrackLayer, build_tip};
use crate::container::{Chunk, Container, Tag};

pub const FILE_EXTENSION: &str = "chqbrush";

const MAGIC: Tag = *b"CHB\0";
const FORMAT_MAJOR: u16 = 1;
const FORMAT_MINOR: u16 = 3;
const META_TAG: Tag = *b"META";
const PRESET_TAG: Tag = *b"PRST";
const TIP_TAG: Tag = *b"TIPM";
const PAPER_TAG: Tag = *b"GRAN";
const HAIRS_TAG: Tag = *b"HAIR";
const LAYER_TAG: Tag = *b"LAYR";
const KNOWN_TAGS: [Tag; 4] = [META_TAG, PRESET_TAG, TIP_TAG, LAYER_TAG];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TipKind {
    Round,
    Mask,
    Set,
    Generated,
}

#[derive(Serialize, Deserialize)]
struct Preset {
    tip: TipKind,
    settings: BrushSettings,
    #[serde(default)]
    weights: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generated: Option<TipSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    paper: Option<PatternSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hairs: Option<PatternSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    layers: Vec<LayerSpec>,
}

pub fn encode_pack(brushes: &[Brush]) -> Result<Vec<u8>, BrushError> {
    let mut container = Container::new(FORMAT_MAJOR, FORMAT_MINOR);
    for brush in brushes {
        container.chunks.push(Chunk::critical(META_TAG, serde_json::to_vec(&brush.meta)?));
        let generated = match &brush.tip {
            BrushTip::Mask(mask) => mask.spec().cloned(),
            _ => None,
        };
        let (tip, weights) = match &brush.tip {
            BrushTip::Round => (TipKind::Round, Vec::new()),
            BrushTip::Mask(_) if generated.is_some() => (TipKind::Generated, Vec::new()),
            BrushTip::Mask(_) => (TipKind::Mask, Vec::new()),
            BrushTip::Set(variants) => (TipKind::Set, variants.iter().map(|variant| variant.weight).collect()),
        };
        let baked = generated.is_none();
        container.chunks.push(Chunk::critical(PRESET_TAG, serde_json::to_vec(&Preset { tip, settings: brush.settings.clone(), weights, generated, paper: spec_of(&brush.textures.paper), hairs: spec_of(&brush.textures.hairs), layers: brush.textures.layers.iter().map(|layer| layer.spec.clone()).collect() })?));
        if baked {
            for variant in brush.tip.variants() {
                container.chunks.push(Chunk::critical(TIP_TAG, encode_tip(&variant.mask)?));
            }
        }
        for (tag, texture) in [(PAPER_TAG, &brush.textures.paper), (HAIRS_TAG, &brush.textures.hairs)] {
            if let Some(texture) = texture {
                container.chunks.push(Chunk::new(tag, encode_gray(texture.width(), texture.height(), texture.pixels())?));
            }
        }
        for layer in &brush.textures.layers {
            container.chunks.push(Chunk::critical(LAYER_TAG, encode_gray(layer.texture.width(), layer.texture.height(), layer.texture.pixels())?));
        }
    }
    Ok(container.encode(MAGIC))
}

pub fn decode_pack(bytes: &[u8]) -> Result<Vec<Brush>, BrushError> {
    let container = Container::decode(bytes, MAGIC, FORMAT_MAJOR)?;
    container.reject_unknown_critical(&KNOWN_TAGS)?;
    let mut groups: Vec<Vec<&Chunk>> = Vec::new();
    for chunk in &container.chunks {
        match (chunk.tag == META_TAG, groups.last_mut()) {
            (false, Some(group)) => group.push(chunk),
            (true, _) => groups.push(vec![chunk]),
            (false, None) => {}
        }
    }
    if groups.is_empty() {
        return Err(BrushError::Empty);
    }
    groups.iter().map(|group| decode_brush(group)).collect()
}

fn decode_brush(group: &[&Chunk]) -> Result<Brush, BrushError> {
    let find = |tag: Tag, name: &'static str| group.iter().find(|chunk| chunk.tag == tag).ok_or(BrushError::MissingChunk(name));
    let meta: BrushMeta = serde_json::from_slice(&find(META_TAG, "META")?.payload)?;
    let preset: Preset = serde_json::from_slice(&find(PRESET_TAG, "PRST")?.payload)?;
    let masks = group.iter().filter(|chunk| chunk.tag == TIP_TAG).map(|chunk| decode_tip(&chunk.payload)).collect::<Result<Vec<_>, _>>()?;
    let tip = match preset.tip {
        TipKind::Round => BrushTip::Round,
        TipKind::Mask => BrushTip::Mask(masks.into_iter().next().ok_or(BrushError::MissingChunk("TIPM"))?),
        TipKind::Generated => BrushTip::Mask(build_tip(&preset.generated.ok_or(BrushError::Malformed("у сгенерированного наконечника нет описания"))?)?),
        TipKind::Set if masks.is_empty() => return Err(BrushError::MissingChunk("TIPM")),
        TipKind::Set => BrushTip::Set(
            masks
                .into_iter()
                .enumerate()
                .map(|(index, mask)| TipVariant { mask, weight: preset.weights.get(index).copied().unwrap_or(TIP_WEIGHT_RANGE.1).clamp(TIP_WEIGHT_RANGE.0, TIP_WEIGHT_RANGE.1) })
                .collect(),
        ),
    };
    let textures = BrushTextures {
        paper: decode_texture(group, PAPER_TAG, preset.paper)?,
        hairs: decode_texture(group, HAIRS_TAG, preset.hairs)?,
        layers: decode_layers(group, preset.layers)?,
    };
    Ok(Brush { meta, settings: preset.settings.clamped(), tip, textures })
}

fn decode_layers(group: &[&Chunk], specs: Vec<LayerSpec>) -> Result<Vec<TrackLayer>, BrushError> {
    let images: Vec<&Chunk> = group.iter().copied().filter(|chunk| chunk.tag == LAYER_TAG).collect();
    if images.len() != specs.len() || specs.len() > MAX_TRACK_LAYERS {
        return Err(BrushError::Malformed("число слоёв следа не совпадает с числом картинок или слишком велико"));
    }
    specs
        .into_iter()
        .zip(images)
        .map(|(spec, chunk)| {
            let spec = spec.clamped();
            if !spec.marks.is_empty() {
                return Ok(TrackLayer::marked(spec));
            }
            let gray = image::load_from_memory_with_format(&chunk.payload, ImageFormat::Png)?.to_luma8();
            Ok(TrackLayer { spec, texture: Texture::new(gray.width(), gray.height(), gray.into_raw())? })
        })
        .collect()
}

fn spec_of(texture: &Option<Texture>) -> Option<PatternSpec> {
    texture.as_ref().and_then(Texture::spec)
}

fn encode_tip(mask: &MaskTip) -> Result<Vec<u8>, BrushError> {
    encode_gray(mask.width(), mask.height(), mask.pixels())
}

fn encode_gray(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, BrushError> {
    let mut bytes = Vec::new();
    PngEncoder::new(Cursor::new(&mut bytes)).write_image(pixels, width, height, ExtendedColorType::L8)?;
    Ok(bytes)
}

fn decode_texture(group: &[&Chunk], tag: Tag, spec: Option<PatternSpec>) -> Result<Option<Texture>, BrushError> {
    let Some(chunk) = group.iter().find(|chunk| chunk.tag == tag) else {
        return Ok(None);
    };
    let gray = image::load_from_memory_with_format(&chunk.payload, ImageFormat::Png)?.to_luma8();
    Ok(Some(Texture::new(gray.width(), gray.height(), gray.into_raw())?.with_spec(spec)))
}

fn decode_tip(bytes: &[u8]) -> Result<MaskTip, BrushError> {
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Png)?;
    let gray = image.to_luma8();
    MaskTip::new(gray.width(), gray.height(), gray.into_raw())
}

#[cfg(test)]
mod tests {
    use super::super::{Mark, PatternKind, generate};
    use super::*;

    fn chalk() -> Brush {
        let tip = MaskTip::new(3, 2, vec![0, 128, 255, 255, 128, 0]).unwrap();
        let meta = BrushMeta { id: "x".into(), name: "Мел".into(), author: "я".into(), license: "CC0".into(), category: String::new(), names: Default::default(), tags: vec!["сухие".into()] };
        Brush { meta, settings: BrushSettings { spacing: 0.3, accumulate: true, ..BrushSettings::default() }, tip: BrushTip::Mask(tip), textures: BrushTextures::default() }
    }

    #[test]
    fn pack_of_two_brushes_round_trips() {
        let decoded = decode_pack(&encode_pack(&[chalk(), Brush::round()]).unwrap()).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].meta, chalk().meta);
        assert_eq!(decoded[0].settings, chalk().settings);
        let BrushTip::Mask(mask) = &decoded[0].tip else { panic!("tip lost") };
        assert_eq!(mask.pixels(), &[0, 128, 255, 255, 128, 0]);
        assert!(matches!(decoded[1].tip, BrushTip::Round));
    }

    #[test]
    fn tip_set_keeps_order_and_weights() {
        let dot = |value: u8| TipVariant { mask: MaskTip::new(1, 1, vec![value]).unwrap(), weight: f32::from(value) / 255.0 };
        let brush = Brush { tip: BrushTip::Set(vec![dot(255), dot(51)]), ..chalk() };
        let decoded = decode_pack(&encode_pack(&[brush]).unwrap()).unwrap();
        let variants = decoded[0].tip.variants();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[1].mask.pixels(), &[51]);
        assert!((variants[1].weight - 0.2).abs() < 1e-6);
    }

    #[test]
    fn generated_and_imported_textures_round_trip_with_their_specs() {
        let paper = generate(PatternSpec::preset(PatternKind::Fibers));
        let hairs = Texture::new(3, 1, vec![10, 200, 90]).unwrap();
        let brush = Brush { textures: BrushTextures { paper: Some(paper.clone()), hairs: Some(hairs.clone()), ..BrushTextures::default() }, ..chalk() };
        let decoded = decode_pack(&encode_pack(&[brush]).unwrap()).unwrap();
        assert_eq!(decoded[0].textures.paper, Some(paper));
        assert_eq!(decoded[0].textures.hairs, Some(hairs));
        assert_eq!(decode_pack(&encode_pack(&[chalk()]).unwrap()).unwrap()[0].textures, BrushTextures::default());
    }

    #[test]
    fn track_layers_round_trip_in_order_and_mismatched_count_is_refused() {
        let layer = |pitch: f32, value: u8| TrackLayer { spec: LayerSpec { pitch, pitches: vec![1.0, 1.5], amount: 0.5, ..LayerSpec::default() }, texture: Texture::new(2, 2, vec![value; 4]).unwrap() };
        let brush = Brush { textures: BrushTextures { layers: vec![layer(0.5, 10), layer(2.0, 200)], ..BrushTextures::default() }, ..chalk() };
        let bytes = encode_pack(&[brush.clone()]).unwrap();
        assert_eq!(decode_pack(&bytes).unwrap()[0].textures.layers, brush.textures.layers);
        let mut container = Container::decode(&bytes, MAGIC, FORMAT_MAJOR).unwrap();
        container.chunks.pop();
        assert!(matches!(decode_pack(&container.encode(MAGIC)), Err(BrushError::Malformed(_))));
    }

    #[test]
    fn marked_layer_keeps_its_marks_and_is_redrawn_from_them() {
        let marks = vec![Mark { start: 0.5, length: 2.0, thickness: 0.6, rounding: 1.0, ..Mark::default() }, Mark { start: 3.0, length: 1.0, sway: 0.2, ..Mark::default() }];
        let layer = TrackLayer::marked(LayerSpec { pitch: 4.0, marks, ..LayerSpec::default() });
        let brush = Brush { textures: BrushTextures { layers: vec![layer.clone()], ..BrushTextures::default() }, ..chalk() };
        let decoded = decode_pack(&encode_pack(&[brush]).unwrap()).unwrap();
        assert_eq!(decoded[0].textures.layers, vec![layer]);
    }

    #[test]
    fn generated_tip_is_stored_as_parameters_and_rebuilt_identically() {
        let spec = TipSpec { generator: "comb".into(), params: [("teeth".to_owned(), 5.0)].into(), seed: 9 };
        let tip = build_tip(&spec).unwrap();
        let bytes = encode_pack(&[Brush { tip: BrushTip::Mask(tip.clone()), ..chalk() }]).unwrap();
        assert!(bytes.windows(4).all(|window| window != TIP_TAG), "generated tip must not store pixels");
        let decoded = decode_pack(&bytes).unwrap();
        let BrushTip::Mask(rebuilt) = &decoded[0].tip else { panic!("tip lost") };
        assert_eq!(rebuilt.pixels(), tip.pixels());
        assert_eq!(rebuilt.spec(), Some(&spec));
    }

    #[test]
    fn hostile_settings_are_clamped_on_load() {
        let mut brush = Brush::round();
        brush.settings = BrushSettings { spacing: 0.0, roundness: 0.0, scatter: 1e9, ..BrushSettings::default() };
        let decoded = decode_pack(&encode_pack(&[brush]).unwrap()).unwrap();
        assert!(decoded[0].settings.spacing > 0.0 && decoded[0].settings.roundness > 0.0 && decoded[0].settings.scatter <= super::super::SCATTER_MAX);
    }

    #[test]
    fn file_without_brushes_is_an_error() {
        assert!(matches!(decode_pack(&Container::new(FORMAT_MAJOR, 0).encode(MAGIC)), Err(BrushError::Empty)));
    }

    #[test]
    fn default_pack_has_unique_named_brushes_with_ink() {
        let brushes = decode_pack(super::super::DEFAULT_PACK).unwrap();
        let mut ids: Vec<&str> = brushes.iter().map(|brush| brush.meta.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), brushes.len());
        for brush in &brushes {
            assert!(brush.meta.names.contains_key("ru") && brush.meta.names.contains_key("en"), "{} has no names", brush.meta.id);
            if let BrushTip::Mask(mask) = &brush.tip {
                assert!(mask.pixels().iter().any(|&texel| texel > 0), "{} is empty", brush.meta.id);
            }
        }
    }
}
