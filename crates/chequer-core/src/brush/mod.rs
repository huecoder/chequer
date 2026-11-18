mod crystals;
mod curve;
mod dab;
mod dryness;
mod generator;
mod generators;
mod grain;
mod import;
mod marks;
mod mask;
mod pack;
mod pattern;
mod preview;
mod texture;
pub mod tint;
mod track;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

pub use curve::{Curve, MAX_CURVE_POINTS};
pub use grain::{GRAIN_SCALE_RANGE, grained, grained_deposit, hash_unit, value_noise};
pub use dab::{Dab, DabSampler, Jitter};
pub use dryness::dry_gate;
pub use generator::{GENERATED_TIP_SIDE, TipGenerator, TipParam, TipSpec, TipValues, build_tip, generate_tip, rasterize, register_tip_generator, soft_edge, tip_generator, tip_generator_ids};
pub use import::{IMPORT_EXTENSIONS, decode_foreign};
pub use marks::{MARK_LENGTH_RANGE, MARK_OFFSET_RANGE, MARK_START_RANGE, MARK_SWAY_CYCLES_RANGE, MARK_SWAY_RANGE, MARK_THICKNESS_RANGE, MARK_UNIT_RANGE, MAX_MARKS, Mark, render_marks};
pub use mask::{MAX_TIP_SIDE, MaskTip};
pub use pack::{FILE_EXTENSION, decode_pack, encode_pack};
pub use pattern::{HAIR_KINDS, HAIR_SLOTS, PAPER_KINDS, PatternKind, PatternSpec, TEXTURE_SIDE, generate};
pub use preview::{PreviewStyle, stroke_preview};
pub use texture::{MAX_TEXTURE_SIDE, Texture};
pub use track::{LayerSpec, MAX_PITCH_STEPS, MAX_TRACK_LAYERS, PITCH_RANGE, PITCH_STEP_RANGE, TrackLayer, TrackSampler};

use crate::container::ContainerError;

const FALLBACK_ID: &str = "builtin.round";
const FALLBACK_CATEGORY: &str = "basic";
pub const DEFAULT_PACK: &[u8] = include_bytes!("../../assets/default_brushes.chqbrush");

pub const SPACING_RANGE: (f32, f32) = (0.01, 4.0);
pub const ROUNDNESS_RANGE: (f32, f32) = (0.05, 1.0);
pub const FLOW_RANGE: (f32, f32) = (0.01, 1.0);
pub const SCATTER_MAX: f32 = 5.0;
pub const SIZE_FADE_RANGE: (f32, f32) = (0.1, 100.0);
pub const TIP_WEIGHT_RANGE: (f32, f32) = (0.01, 1.0);
pub const MAX_TIP_VARIANTS: usize = 16;
const FULL_TURN_DEGREES: f32 = 360.0;

#[derive(Debug)]
pub enum BrushError {
    Container(ContainerError),
    Image(image::ImageError),
    Json(serde_json::Error),
    TipSize { width: u32, height: u32 },
    MissingChunk(&'static str),
    Malformed(&'static str),
    Unsupported(String),
    Xml(roxmltree::Error),
    Empty,
}

impl fmt::Display for BrushError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(e) => write!(f, "{e}"),
            Self::Image(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "{e}"),
            Self::TipSize { width, height } => write!(f, "наконечник {width}×{height}: допустимо от 1 до {MAX_TIP_SIDE} пикселей по стороне"),
            Self::MissingChunk(name) => write!(f, "в кисти нет блока «{name}»"),
            Self::Malformed(what) => write!(f, "файл повреждён: {what}"),
            Self::Unsupported(what) => write!(f, "не поддерживается: {what}"),
            Self::Xml(e) => write!(f, "{e}"),
            Self::Empty => write!(f, "в файле нет ни одной кисти"),
        }
    }
}

impl std::error::Error for BrushError {}

impl From<ContainerError> for BrushError {
    fn from(error: ContainerError) -> Self {
        Self::Container(error)
    }
}

impl From<image::ImageError> for BrushError {
    fn from(error: image::ImageError) -> Self {
        Self::Image(error)
    }
}

impl From<roxmltree::Error> for BrushError {
    fn from(error: roxmltree::Error) -> Self {
        Self::Xml(error)
    }
}

impl From<serde_json::Error> for BrushError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushEngine {
    #[default]
    Stamp,
    Ribbon,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushMeta {
    pub id: String,
    pub name: String,
    pub names: BTreeMap<String, String>,
    pub author: String,
    pub license: String,
    pub category: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushSettings {
    pub engine: BrushEngine,
    pub spacing: f32,
    pub angle: f32,
    pub follow_direction: bool,
    pub angle_jitter: f32,
    pub size_jitter: f32,
    pub opacity_jitter: f32,
    pub scatter: f32,
    pub roundness: f32,
    pub flow: f32,
    pub accumulate: bool,
    pub pressure_size: f32,
    pub pressure_size_curve: Curve,
    pub pressure_opacity: f32,
    pub pressure_opacity_curve: Curve,
    pub pressure_roundness: f32,
    pub pressure_scatter: f32,
    pub pressure_hardness: f32,
    pub pressure_spacing: f32,
    pub tilt_angle: f32,
    pub tilt_roundness: f32,
    pub velocity_size: f32,
    pub velocity_opacity: f32,
    pub hue_jitter: f32,
    pub saturation_jitter: f32,
    pub value_jitter: f32,
    pub hardness: f32,
    pub size_start: f32,
    pub size_end: f32,
    pub size_fade: f32,
    pub grain: f32,
    pub grain_scale: f32,
    pub dryness: f32,
    pub velocity_dryness: f32,
    pub pressure_dryness: f32,
    pub depletion: f32,
    pub bristles: f32,
    pub cycle_tips: bool,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            engine: BrushEngine::Stamp,
            spacing: 0.1,
            angle: 0.0,
            follow_direction: false,
            angle_jitter: 0.0,
            size_jitter: 0.0,
            opacity_jitter: 0.0,
            scatter: 0.0,
            roundness: 1.0,
            flow: 1.0,
            accumulate: false,
            pressure_size: 0.5,
            pressure_size_curve: Curve::linear(),
            pressure_opacity: 0.0,
            pressure_opacity_curve: Curve::linear(),
            pressure_roundness: 0.0,
            pressure_scatter: 0.0,
            pressure_hardness: 0.0,
            pressure_spacing: 0.0,
            tilt_angle: 0.0,
            tilt_roundness: 0.0,
            velocity_size: 0.0,
            velocity_opacity: 0.0,
            hue_jitter: 0.0,
            saturation_jitter: 0.0,
            value_jitter: 0.0,
            hardness: 0.75,
            size_start: 1.0,
            size_end: 1.0,
            size_fade: 10.0,
            grain: 0.0,
            grain_scale: 3.0,
            dryness: 0.0,
            velocity_dryness: 0.0,
            pressure_dryness: 0.0,
            depletion: 0.0,
            bristles: 0.0,
            cycle_tips: false,
        }
    }
}

impl BrushSettings {
    pub fn pressure_size_scale(&self, pressure: f32) -> f32 {
        1.0 + (self.pressure_size_curve.eval(pressure) - 1.0) * self.pressure_size
    }

    pub fn clamped(self) -> Self {
        let unit = |value: f32| value.clamp(0.0, 1.0);
        Self {
            spacing: self.spacing.clamp(SPACING_RANGE.0, SPACING_RANGE.1),
            angle: self.angle.rem_euclid(FULL_TURN_DEGREES),
            angle_jitter: unit(self.angle_jitter),
            size_jitter: unit(self.size_jitter),
            opacity_jitter: unit(self.opacity_jitter),
            scatter: self.scatter.clamp(0.0, SCATTER_MAX),
            roundness: self.roundness.clamp(ROUNDNESS_RANGE.0, ROUNDNESS_RANGE.1),
            flow: self.flow.clamp(FLOW_RANGE.0, FLOW_RANGE.1),
            accumulate: self.accumulate && self.engine == BrushEngine::Stamp,
            pressure_size: unit(self.pressure_size),
            pressure_opacity: unit(self.pressure_opacity),
            pressure_roundness: unit(self.pressure_roundness),
            pressure_scatter: unit(self.pressure_scatter),
            pressure_hardness: unit(self.pressure_hardness),
            pressure_spacing: unit(self.pressure_spacing),
            tilt_angle: unit(self.tilt_angle),
            tilt_roundness: unit(self.tilt_roundness),
            velocity_size: unit(self.velocity_size),
            velocity_opacity: unit(self.velocity_opacity),
            hue_jitter: unit(self.hue_jitter),
            saturation_jitter: unit(self.saturation_jitter),
            value_jitter: unit(self.value_jitter),
            hardness: unit(self.hardness),
            size_start: unit(self.size_start),
            size_end: unit(self.size_end),
            size_fade: self.size_fade.clamp(SIZE_FADE_RANGE.0, SIZE_FADE_RANGE.1),
            grain: unit(self.grain),
            grain_scale: self.grain_scale.clamp(GRAIN_SCALE_RANGE.0, GRAIN_SCALE_RANGE.1),
            dryness: unit(self.dryness),
            velocity_dryness: unit(self.velocity_dryness),
            pressure_dryness: unit(self.pressure_dryness),
            depletion: unit(self.depletion),
            bristles: unit(self.bristles),
            ..self
        }
    }
}

#[derive(Clone, Debug)]
pub struct TipVariant {
    pub mask: MaskTip,
    pub weight: f32,
}

impl TipVariant {
    pub fn new(mask: MaskTip) -> Self {
        Self { mask, weight: TIP_WEIGHT_RANGE.1 }
    }
}

#[derive(Clone, Debug)]
pub enum BrushTip {
    Round,
    Mask(MaskTip),
    Set(Vec<TipVariant>),
}

impl BrushTip {
    pub fn from_variants(mut variants: Vec<TipVariant>) -> Self {
        match variants.len() {
            0 => Self::Round,
            1 => Self::Mask(variants.remove(0).mask),
            _ => Self::Set(variants),
        }
    }

    pub fn variants(&self) -> Vec<TipVariant> {
        match self {
            Self::Round => Vec::new(),
            Self::Mask(mask) => vec![TipVariant::new(mask.clone())],
            Self::Set(variants) => variants.clone(),
        }
    }

    pub fn mask_for(&self, variant: usize) -> Option<&MaskTip> {
        match self {
            Self::Round => None,
            Self::Mask(mask) => Some(mask),
            Self::Set(variants) => variants.get(variant).or(variants.first()).map(|picked| &picked.mask),
        }
    }

    pub fn variant_count(&self) -> usize {
        match self {
            Self::Round => 0,
            Self::Mask(_) => 1,
            Self::Set(variants) => variants.len(),
        }
    }

    pub fn pick_variant(&self, unit: f32) -> usize {
        let Self::Set(variants) = self else {
            return 0;
        };
        let mut remaining = unit * variants.iter().map(|variant| variant.weight).sum::<f32>();
        for (index, variant) in variants.iter().enumerate() {
            if remaining < variant.weight {
                return index;
            }
            remaining -= variant.weight;
        }
        variants.len().saturating_sub(1)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BrushTextures {
    pub paper: Option<Texture>,
    pub hairs: Option<Texture>,
    pub layers: Vec<TrackLayer>,
}

#[derive(Clone, Debug)]
pub struct Brush {
    pub meta: BrushMeta,
    pub settings: BrushSettings,
    pub tip: BrushTip,
    pub textures: BrushTextures,
}

impl Brush {
    pub fn round() -> Self {
        Self { meta: BrushMeta { id: FALLBACK_ID.into(), category: FALLBACK_CATEGORY.into(), ..BrushMeta::default() }, settings: BrushSettings::default(), tip: BrushTip::Round, textures: BrushTextures::default() }
    }

    pub fn paper(&self) -> &Texture {
        self.textures.paper.as_ref().unwrap_or_else(|| Texture::builtin_paper())
    }

    pub fn hairs(&self) -> &Texture {
        self.textures.hairs.as_ref().unwrap_or_else(|| Texture::builtin_hairs())
    }
}

impl Brush {
    pub fn from_tip_image(name: &str, image: &image::DynamicImage) -> Result<Self, BrushError> {
        let tip = BrushTip::Mask(MaskTip::from_image(image)?);
        Ok(Self { meta: BrushMeta { name: name.into(), ..BrushMeta::default() }, settings: BrushSettings::default(), tip, textures: BrushTextures::default() })
    }
}

impl Default for Brush {
    fn default() -> Self {
        Self::round()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavier_variant_is_picked_over_a_wider_share_of_the_unit_interval() {
        let dot = |weight: f32| TipVariant { mask: MaskTip::new(1, 1, vec![255]).unwrap(), weight };
        let tip = BrushTip::Set(vec![dot(0.25), dot(0.75)]);
        assert_eq!(tip.pick_variant(0.2), 0);
        assert_eq!(tip.pick_variant(0.3), 1);
        assert_eq!(tip.pick_variant(0.999), 1);
    }
}
