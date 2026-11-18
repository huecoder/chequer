use chequer_core::brush::{LayerSpec, MARK_LENGTH_RANGE, MARK_OFFSET_RANGE, MARK_START_RANGE, MARK_SWAY_CYCLES_RANGE, MARK_SWAY_RANGE, MARK_THICKNESS_RANGE, MARK_UNIT_RANGE, MAX_MARKS, MAX_TRACK_LAYERS, Mark, PAPER_KINDS, PITCH_RANGE, PatternSpec, TrackLayer, generate};
use eframe::egui::{self, Id, Margin, Sense, TextureOptions, Ui, Vec2, vec2};

use crate::brush_preview::paint_texture;
use crate::brush_settings::{UNIT_RANGE, Unit, hinted_label, number_row};
use crate::icons::Icon;
use crate::i18n::{Text, t};
use crate::skin;
use crate::texture_editor::{kind_text, texture_handle};
use crate::widgets::{bevel_over, combo_box, icon_button, text_button};

const LAYER_THUMB: Vec2 = vec2(56.0, 56.0);
const CARD_MARGIN: i8 = 6;
const CARD_STROKE: f32 = 1.0;
const ADD_COMBO_WIDTH: f32 = 180.0;
const THUMB_CACHE: &str = "layer-thumb";
const PITCHES_CACHE: &str = "layer-pitches";
const PITCHES_EXAMPLE: &str = "1 1.2 0.8";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayerPick {
    New,
    Replace(usize),
}

#[derive(Default)]
pub(crate) struct LayersOutcome {
    pub changed: bool,
    pub pick: Option<LayerPick>,
}

fn format_pitches(pitches: &[f32]) -> String {
    pitches.iter().map(f32::to_string).collect::<Vec<_>>().join(" ")
}

fn parse_pitches(text: &str) -> Option<Vec<f32>> {
    text.split(|symbol: char| symbol.is_whitespace() || symbol == ';').filter(|token| !token.is_empty()).map(|token| token.replace(',', ".").parse().ok()).collect()
}

fn pitches_row(ui: &mut Ui, skin: &skin::Skin, spec: &mut LayerSpec) -> bool {
    hinted_label(ui, Text::BrushLayerPitches, Text::BrushHintLayerPitches);
    let id = ui.id().with(PITCHES_CACHE);
    let canonical = format_pitches(&spec.pitches);
    let (mut shown, known) = ui.data(|data| data.get_temp::<(String, String)>(id)).unwrap_or_else(|| (canonical.clone(), canonical.clone()));
    if known != canonical {
        shown = canonical.clone();
    }
    let field = ui.add(egui::TextEdit::singleline(&mut shown).hint_text(PITCHES_EXAMPLE).desired_width(f32::INFINITY));
    bevel_over(ui, skin, field.rect, skin::Bevel::Sunken);
    let mut known = canonical;
    let mut changed = false;
    if field.changed()
        && let Some(pitches) = parse_pitches(&shown)
    {
        *spec = LayerSpec { pitches, ..spec.clone() }.clamped();
        known = format_pitches(&spec.pitches);
        changed = true;
    }
    ui.data_mut(|data| data.insert_temp(id, (shown, known)));
    changed
}

fn mark_card(ui: &mut Ui, skin: &skin::Skin, index: usize, mark: &mut Mark, removed: &mut Option<usize>) -> bool {
    let mut changed = false;
    egui::Frame::NONE.stroke(egui::Stroke::new(CARD_STROKE, skin::theme().line_soft)).inner_margin(Margin::same(CARD_MARGIN)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(format!("{} {}", t(Text::BrushMark), index + 1));
            if icon_button(ui, skin, Icon::Close, false, t(Text::BrushMarkRemove)).clicked() {
                *removed = Some(index);
            }
        });
        changed |= number_row(ui, skin, Text::BrushMarkStart, Text::BrushHintMarkStart, &mut mark.start, MARK_START_RANGE, Unit::Fine);
        changed |= number_row(ui, skin, Text::BrushMarkLength, Text::BrushHintMarkLength, &mut mark.length, MARK_LENGTH_RANGE, Unit::Fine);
        changed |= number_row(ui, skin, Text::BrushMarkOffset, Text::BrushHintMarkOffset, &mut mark.offset, MARK_OFFSET_RANGE, Unit::Fine);
        changed |= number_row(ui, skin, Text::BrushMarkThickness, Text::BrushHintMarkThickness, &mut mark.thickness, MARK_THICKNESS_RANGE, Unit::Fine);
        changed |= number_row(ui, skin, Text::BrushMarkRounding, Text::BrushHintMarkRounding, &mut mark.rounding, MARK_UNIT_RANGE, Unit::Percent);
        changed |= number_row(ui, skin, Text::BrushMarkSway, Text::BrushHintMarkSway, &mut mark.sway, MARK_SWAY_RANGE, Unit::Fine);
        changed |= number_row(ui, skin, Text::BrushMarkSwayCycles, Text::BrushHintMarkSwayCycles, &mut mark.sway_cycles, MARK_SWAY_CYCLES_RANGE, Unit::Plain);
    });
    changed
}

fn marks_editor(ui: &mut Ui, skin: &skin::Skin, marks: &mut Vec<Mark>) -> bool {
    let mut changed = false;
    let mut removed = None;
    for (index, mark) in marks.iter_mut().enumerate() {
        ui.push_id(("mark", index), |ui| changed |= mark_card(ui, skin, index, mark, &mut removed));
    }
    if let Some(index) = removed {
        marks.remove(index);
        changed = true;
    }
    ui.add_enabled_ui(marks.len() < MAX_MARKS, |ui| {
        if text_button(ui, skin, t(Text::BrushMarkAdd)).clicked() {
            marks.push(Mark::default());
            changed = true;
        }
    });
    changed
}

fn layer_card(ui: &mut Ui, skin: &skin::Skin, index: usize, layer: &mut TrackLayer, outcome: &mut LayersOutcome, removed: &mut Option<usize>) {
    egui::Frame::NONE.stroke(egui::Stroke::new(CARD_STROKE, skin::theme().line_soft)).inner_margin(Margin::same(CARD_MARGIN)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let handle = texture_handle(ui.ctx(), Id::new((THUMB_CACHE, index)), &layer.texture, TextureOptions::LINEAR);
            let (rect, _) = ui.allocate_exact_size(LAYER_THUMB, Sense::hover());
            paint_texture(ui, &handle, rect);
            bevel_over(ui, skin, rect, skin::Bevel::Sunken);
            if text_button(ui, skin, t(Text::BrushLayerReplace)).clicked() {
                outcome.pick = Some(LayerPick::Replace(index));
            }
            if icon_button(ui, skin, Icon::Close, false, t(Text::BrushLayerRemove)).clicked() {
                *removed = Some(index);
            }
        });
        let mut shape_changed = number_row(ui, skin, Text::BrushLayerPitch, Text::BrushHintLayerPitch, &mut layer.spec.pitch, PITCH_RANGE, Unit::Fine);
        outcome.changed |= number_row(ui, skin, Text::BrushLayerAmount, Text::BrushHintLayerAmount, &mut layer.spec.amount, UNIT_RANGE, Unit::Percent);
        outcome.changed |= pitches_row(ui, skin, &mut layer.spec);
        if layer.is_marked() {
            shape_changed |= marks_editor(ui, skin, &mut layer.spec.marks);
        }
        if shape_changed {
            layer.redraw_marks();
            outcome.changed = true;
        }
    });
}

pub(crate) fn layers_section(ui: &mut Ui, skin: &skin::Skin, layers: &mut Vec<TrackLayer>) -> LayersOutcome {
    let mut outcome = LayersOutcome::default();
    let mut removed = None;
    for (index, layer) in layers.iter_mut().enumerate() {
        ui.push_id(index, |ui| layer_card(ui, skin, index, layer, &mut outcome, &mut removed));
    }
    if let Some(index) = removed {
        layers.remove(index);
        outcome.changed = true;
    }
    ui.add_enabled_ui(layers.len() < MAX_TRACK_LAYERS, |ui| {
        combo_box(ui, skin, "layer-add", ADD_COMBO_WIDTH, t(Text::BrushLayerAdd), |ui| {
            if ui.selectable_label(false, t(Text::BrushLayerFromImage)).clicked() {
                outcome.pick = Some(LayerPick::New);
            }
            if ui.selectable_label(false, t(Text::BrushLayerFromMarks)).clicked() {
                layers.push(TrackLayer::marked(LayerSpec { marks: vec![Mark::default()], ..LayerSpec::default() }));
                outcome.changed = true;
            }
            for kind in PAPER_KINDS {
                if ui.selectable_label(false, t(kind_text(kind))).clicked() {
                    layers.push(TrackLayer { spec: LayerSpec::default(), texture: generate(PatternSpec::preset(kind)).with_spec(None) });
                    outcome.changed = true;
                }
            }
        });
    });
    outcome
}
