
mod adjust;
mod advanced_noise;
mod apply;
mod artistic;
mod blur;
mod color;
mod dither;
mod curve;
mod distort;
mod glitch;
pub mod ids;
mod noise;
mod object;
mod palette;
mod params;
mod photo;
mod registry;
mod render;
mod stylize;
pub mod util;

pub use apply::EffectSession;
pub use curve::Curve;
pub use registry::{EffectId, all_effects, register};
pub use params::{ColorSource, EffectParams, Palette, ParamKind, ParamSpec, ParamValue};
pub use util::Cancel;

use crate::geom::IRect;
use crate::raster::Raster;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Adjust,
    Photo,
    Distort,
    Object,
    Blurs,
    Stylize,
    Render,
    Artistic,
    Color,
    Noise,
    Glitch,
}

impl Category {
    pub const ALL: [Category; 11] = [
        Category::Adjust,
        Category::Photo,
        Category::Distort,
        Category::Object,
        Category::Blurs,
        Category::Stylize,
        Category::Render,
        Category::Artistic,
        Category::Color,
        Category::Noise,
        Category::Glitch,
    ];
}

pub struct Job<'a> {
    pub source: &'a Raster,
    pub area: IRect,
    pub params: &'a EffectParams,
    pub cancel: &'a Cancel,
}

pub struct EffectDef {
    pub id: EffectId,
    pub category: Category,
    pub params: &'static [ParamSpec],
    pub render: fn(&Job) -> Option<Raster>,
}

pub fn render(source: &Raster, area: IRect, params: &EffectParams, cancel: &Cancel) -> Option<Raster> {
    let area = area.intersect(source.bounds());
    if area.is_empty() {
        return None;
    }
    (params.id().def().render)(&Job { source, area, params, cancel })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SIDE: u32 = 48;

    fn gradient_sample() -> Raster {
        let mut raster = Raster::filled(SAMPLE_SIDE, SAMPLE_SIDE, [0, 0, 0, 255]);
        for y in 0..SAMPLE_SIDE {
            for x in 0..SAMPLE_SIDE {
                let alpha = if (12..36).contains(&x) && (12..36).contains(&y) { 255 } else { 0 };
                raster.set(x, y, [(x * 5) as u8, (y * 5) as u8, ((x + y) * 2) as u8, alpha]);
            }
        }
        raster
    }

    #[test]
    fn every_effect_renders_with_default_params() {
        let source = gradient_sample();
        let palette = Palette { primary: [10, 20, 30, 255], secondary: [200, 210, 220, 255] };
        let area = IRect::new(4, 6, 44, 40);
        assert!(registry::all_effects().len() >= 67, "в каталоге 40 эффектов Paint.NET, расширенный шум, 7 корректировок и 9 глитчей");
        for kind in registry::all_effects() {
            let params = EffectParams::defaults(kind, palette);
            let result = render(&source, area, &params, &Cancel::default()).unwrap_or_else(|| panic!("{kind:?}: нет результата"));
            assert_eq!((result.width() as i32, result.height() as i32), (area.width(), area.height()), "{kind:?}");
        }
    }

    #[test]
    fn shown_conditions_point_at_existing_choices() {
        for kind in registry::all_effects() {
            for spec in kind.params() {
                for (key, options) in spec.shown_when {
                    let Some(ParamKind::Choice { options: known, .. }) = kind.params().iter().find(|other| other.key == *key).map(|other| other.kind) else {
                        panic!("{kind:?}: условие параметра «{}» ссылается на «{key}», а это не список", spec.key);
                    };
                    assert!(options.iter().all(|option| known.contains(option)), "{kind:?}: в условии «{}» есть вариант, которого нет у «{key}»", spec.key);
                }
            }
        }
    }
}
