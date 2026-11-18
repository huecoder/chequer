use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::crystals;
use super::generators::BUILTIN;
use super::{BrushError, MAX_TIP_SIDE, MaskTip};
use crate::registry::{DuplicateId, Registered, Registry};

pub const GENERATED_TIP_SIDE: u32 = 256;
const EDGE_SOFTNESS_TEXELS: f32 = 1.5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TipSpec {
    pub generator: String,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
    #[serde(default)]
    pub seed: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct TipParam {
    pub key: &'static str,
    pub default: f32,
    pub range: (f32, f32),
}

pub struct TipValues<'a> {
    generator: &'a TipGenerator,
    given: &'a BTreeMap<String, f32>,
}

impl TipValues<'_> {
    pub fn get(&self, key: &str) -> f32 {
        let param = self.generator.params.iter().find(|param| param.key == key).unwrap_or_else(|| panic!("у генератора наконечника «{}» нет параметра «{key}»", self.generator.id));
        self.given.get(key).copied().unwrap_or(param.default).clamp(param.range.0, param.range.1)
    }
}

pub struct TipGenerator {
    pub id: &'static str,
    pub params: &'static [TipParam],
    pub render: fn(&TipValues, u32, u32) -> Result<MaskTip, BrushError>,
}

impl Registered for TipGenerator {
    type Id = &'static str;

    fn id(&self) -> &'static str {
        self.id
    }
}

fn catalog() -> &'static Registry<TipGenerator> {
    static CATALOG: OnceLock<Registry<TipGenerator>> = OnceLock::new();
    CATALOG.get_or_init(|| Registry::with_defs(BUILTIN.iter().chain(crystals::BUILTIN.iter()).copied()))
}

pub fn register_tip_generator(generator: &'static TipGenerator) -> Result<(), DuplicateId<&'static str>> {
    catalog().register(generator)
}

pub fn tip_generator(id: &str) -> Option<&'static TipGenerator> {
    catalog().ids().into_iter().find(|known| *known == id).and_then(|known| catalog().lookup(known))
}

pub fn tip_generator_ids() -> Vec<&'static str> {
    catalog().ids()
}

pub fn generate_tip(spec: &TipSpec, side: u32) -> Result<MaskTip, BrushError> {
    let generator = tip_generator(&spec.generator).ok_or_else(|| BrushError::Unsupported(format!("генератор наконечника «{}»", spec.generator)))?;
    (generator.render)(&TipValues { generator, given: &spec.params }, spec.seed, side)
}

pub fn build_tip(spec: &TipSpec) -> Result<MaskTip, BrushError> {
    Ok(generate_tip(spec, GENERATED_TIP_SIDE)?.with_spec(Some(spec.clone())))
}

pub fn rasterize(side: u32, coverage: impl Fn(f32, f32) -> f32) -> Result<MaskTip, BrushError> {
    if !(1..=MAX_TIP_SIDE).contains(&side) {
        return Err(BrushError::TipSize { width: side, height: side });
    }
    let center = side as f32 / 2.0;
    let pixels = (0..side * side)
        .map(|index| {
            let (x, y) = ((index % side) as f32 + 0.5 - center, (index / side) as f32 + 0.5 - center);
            (coverage(x / center, y / center).clamp(0.0, 1.0) * f32::from(u8::MAX)).round() as u8
        })
        .collect();
    MaskTip::new(side, side, pixels)
}

pub fn soft_edge(inside: f32, side: u32) -> f32 {
    (inside * side as f32 / (2.0 * EDGE_SOFTNESS_TEXELS)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(generator: &str) -> TipSpec {
        TipSpec { generator: generator.into(), params: BTreeMap::new(), seed: 3 }
    }

    #[test]
    fn every_builtin_generator_renders_ink_with_default_parameters() {
        for id in tip_generator_ids() {
            let tip = build_tip(&spec(id)).unwrap_or_else(|error| panic!("{id}: {error}"));
            assert_eq!((tip.width(), tip.height()), (GENERATED_TIP_SIDE, GENERATED_TIP_SIDE));
            assert!(tip.pixels().iter().any(|&texel| texel > 0), "{id} is empty");
            assert_eq!(tip.spec(), Some(&spec(id)));
        }
    }

    #[test]
    fn same_spec_gives_same_pixels_and_other_seed_gives_other_pixels() {
        let render = |seed: u32| build_tip(&TipSpec { seed, ..spec("noise") }).unwrap();
        assert_eq!(render(1).pixels(), render(1).pixels());
        assert_ne!(render(1).pixels(), render(2).pixels());
    }

    #[test]
    fn unknown_generator_is_unsupported_not_empty() {
        assert!(matches!(build_tip(&spec("nope")), Err(BrushError::Unsupported(_))));
    }

    #[test]
    fn hostile_parameters_are_clamped_to_the_declared_range() {
        let spec = TipSpec { params: [("count".to_owned(), 1e9)].into(), ..spec("bristles") };
        assert!(build_tip(&spec).is_ok());
    }

    #[test]
    fn side_outside_the_tip_limits_is_refused() {
        assert!(matches!(generate_tip(&spec("split"), 0), Err(BrushError::TipSize { .. })));
        assert!(matches!(generate_tip(&spec("split"), MAX_TIP_SIDE + 1), Err(BrushError::TipSize { .. })));
    }
}
