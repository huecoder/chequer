use chequer_core::brush::{TipSpec, TipVariant, build_tip, tip_generator};
use eframe::egui::Ui;

use crate::brush_settings::{Unit, slider_row};
use crate::i18n::{Text, TextScope, t, text_or_key};
use crate::skin;
use crate::widgets::text_button;

const FINE_RANGE_LIMIT: f32 = 1.0;

pub(crate) fn generator_label(id: &'static str) -> &'static str {
    text_or_key(TextScope::Option, id)
}

pub(crate) fn default_spec(generator: &str) -> TipSpec {
    TipSpec { generator: generator.into(), params: Default::default(), seed: 0 }
}

pub(crate) fn generator_rows(ui: &mut Ui, skin: &skin::Skin, tip: &mut TipVariant) -> bool {
    let Some(spec) = tip.mask.spec().cloned() else {
        return false;
    };
    let Some(generator) = tip_generator(&spec.generator) else {
        return false;
    };
    ui.label(generator_label(generator.id));
    let mut edited = spec.clone();
    for param in generator.params {
        let mut value = edited.params.get(param.key).copied().unwrap_or(param.default);
        ui.label(text_or_key(TextScope::Param, param.key));
        let unit = if param.range.1 <= FINE_RANGE_LIMIT { Unit::Fine } else { Unit::Plain };
        if slider_row(ui, skin, &mut value, param.range, unit) {
            edited.params.insert(param.key.to_owned(), value);
        }
    }
    if text_button(ui, skin, t(Text::TextureReseed)).clicked() {
        edited.seed = edited.seed.wrapping_add(1);
    }
    if edited == spec {
        return false;
    }
    match build_tip(&edited) {
        Ok(mask) => {
            tip.mask = mask;
            true
        }
        Err(_) => false,
    }
}
