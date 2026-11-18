use chequer_core::brush::{
    BrushEngine, BrushSettings, BrushTextures, Curve, FLOW_RANGE, GRAIN_SCALE_RANGE, ROUNDNESS_RANGE, SCATTER_MAX, SIZE_FADE_RANGE, SPACING_RANGE,
};
use eframe::egui::{self, Sense, Ui};

use crate::curve_editor::{CURVE_EDITOR_SIZE, PlotStyle, curve_plot};
use crate::i18n::{Text, t};
use crate::skin::{self};
use crate::texture_editor::{TextureSlot, texture_rows};
use crate::widgets::{NumberFormat, slider_with_number};

const BRUSH_CURVE_MAX_LABEL: &str = "1";
const SECTION_MARGIN: i8 = 6;
pub(crate) const PERCENT: f32 = 100.0;
const FULL_TURN_DEGREES: f32 = 360.0;
pub(crate) const UNIT_RANGE: (f32, f32) = (0.0, 1.0);
const ANGLE_RANGE: (f32, f32) = (0.0, FULL_TURN_DEGREES);
const SCATTER_RANGE: (f32, f32) = (0.0, SCATTER_MAX);
const MORE_ID: &str = "brush-more";

pub(crate) fn framed_section(
    ui: &mut Ui,
    title: &str,
    default_open: bool,
    add_contents: impl FnOnce(&mut Ui),
) {
    egui::Frame::NONE
        .stroke(egui::Stroke::new(1.0, skin::theme().line_soft))
        .inner_margin(egui::Margin::same(SECTION_MARGIN))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::CollapsingHeader::new(title)
                .default_open(default_open)
                .show(ui, add_contents);
        });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BrushTab {
    Shape,
    Stroke,
    Dynamics,
    Random,
    Paint,
    Layers,
}

impl BrushTab {
    const ALL: [(BrushTab, Text); 6] = [
        (BrushTab::Shape, Text::BrushSectionShape),
        (BrushTab::Stroke, Text::BrushTabStroke),
        (BrushTab::Dynamics, Text::BrushTabDynamics),
        (BrushTab::Random, Text::BrushSectionRandom),
        (BrushTab::Paint, Text::BrushSectionPaint),
        (BrushTab::Layers, Text::BrushTabLayers),
    ];
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Basic,
    Extra,
}

#[derive(Clone, Copy)]
pub(crate) enum Unit {
    Percent,
    Degrees,
    Plain,
    Fine,
}

impl Unit {
    fn scale(self) -> f32 {
        match self {
            Self::Percent => PERCENT,
            Self::Degrees | Self::Plain | Self::Fine => 1.0,
        }
    }

    fn step(self) -> f64 {
        match self {
            Self::Percent | Self::Degrees => 1.0,
            Self::Plain => 0.1,
            Self::Fine => 0.01,
        }
    }

    fn format(self) -> NumberFormat {
        match self {
            Self::Percent => NumberFormat { decimals: 0, suffix: "%" },
            Self::Degrees => NumberFormat { decimals: 0, suffix: "°" },
            Self::Plain => NumberFormat { decimals: 1, suffix: "" },
            Self::Fine => NumberFormat { decimals: 3, suffix: "" },
        }
    }
}

type Field<T> = fn(&mut BrushSettings) -> &mut T;

#[derive(Clone, Copy)]
enum Control {
    Number { field: Field<f32>, range: (f32, f32), unit: Unit },
    Toggle(Field<bool>),
    Curve(Field<Curve>),
    Texture(TextureSlot),
}

struct Scope<'a> {
    settings: &'a BrushSettings,
    soft_edge: bool,
}

struct Row {
    tab: BrushTab,
    level: Level,
    label: Text,
    hint: Text,
    control: Control,
    shown: fn(&Scope) -> bool,
}

fn always(_: &Scope) -> bool {
    true
}

fn stamped(scope: &Scope) -> bool {
    scope.settings.engine == BrushEngine::Stamp
}

fn soft_edged(scope: &Scope) -> bool {
    scope.soft_edge
}

fn size_changes(scope: &Scope) -> bool {
    scope.settings.size_start != scope.settings.size_end
}

fn pressure_changes_size(scope: &Scope) -> bool {
    scope.settings.pressure_size > 0.0
}

fn pressure_changes_opacity(scope: &Scope) -> bool {
    scope.settings.pressure_opacity > 0.0
}

fn grained(scope: &Scope) -> bool {
    scope.settings.grain > 0.0
}

fn ribbon(scope: &Scope) -> bool {
    scope.settings.engine == BrushEngine::Ribbon
}

fn bristled(scope: &Scope) -> bool {
    ribbon(scope) && scope.settings.bristles > 0.0
}

macro_rules! number {
    ($field:ident, $range:expr, $unit:ident) => {
        Control::Number { field: |settings| &mut settings.$field, range: $range, unit: Unit::$unit }
    };
}

macro_rules! row {
    ($tab:ident, $level:ident, $label:ident, $hint:ident, $control:expr, $shown:ident) => {
        Row { tab: BrushTab::$tab, level: Level::$level, label: Text::$label, hint: Text::$hint, control: $control, shown: $shown }
    };
}

const ROWS: &[Row] = &[
    row!(Shape, Basic, BrushHardness, BrushHintHardness, number!(hardness, UNIT_RANGE, Percent), soft_edged),
    row!(Shape, Basic, BrushRoundness, BrushHintRoundness, number!(roundness, ROUNDNESS_RANGE, Percent), always),
    row!(Shape, Basic, BrushAngle, BrushHintAngle, number!(angle, ANGLE_RANGE, Degrees), always),
    row!(Shape, Basic, BrushFollowDirection, BrushHintFollowDirection, Control::Toggle(|settings| &mut settings.follow_direction), stamped),
    row!(Stroke, Basic, BrushCycleTips, BrushHintCycleTips, Control::Toggle(|settings| &mut settings.cycle_tips), stamped),
    row!(Stroke, Basic, BrushSpacing, BrushHintSpacing, number!(spacing, SPACING_RANGE, Percent), stamped),
    row!(Stroke, Basic, BrushScatter, BrushHintScatter, number!(scatter, SCATTER_RANGE, Percent), stamped),
    row!(Stroke, Basic, BrushSizeStart, BrushHintSizeStart, number!(size_start, UNIT_RANGE, Percent), always),
    row!(Stroke, Basic, BrushSizeEnd, BrushHintSizeEnd, number!(size_end, UNIT_RANGE, Percent), always),
    row!(Stroke, Extra, BrushSizeFade, BrushHintSizeFade, number!(size_fade, SIZE_FADE_RANGE, Plain), size_changes),
    row!(Dynamics, Basic, BrushPressureSize, BrushHintPressureSize, number!(pressure_size, UNIT_RANGE, Percent), always),
    row!(Dynamics, Basic, BrushPressureOpacity, BrushHintPressureOpacity, number!(pressure_opacity, UNIT_RANGE, Percent), always),
    row!(Dynamics, Basic, BrushSpeedSize, BrushHintSpeedSize, number!(velocity_size, UNIT_RANGE, Percent), always),
    row!(Dynamics, Basic, BrushSpeedOpacity, BrushHintSpeedOpacity, number!(velocity_opacity, UNIT_RANGE, Percent), always),
    row!(Dynamics, Extra, BrushPressureSizeCurve, BrushHintPressureSizeCurve, Control::Curve(|settings| &mut settings.pressure_size_curve), pressure_changes_size),
    row!(Dynamics, Extra, BrushPressureOpacityCurve, BrushHintPressureOpacityCurve, Control::Curve(|settings| &mut settings.pressure_opacity_curve), pressure_changes_opacity),
    row!(Dynamics, Extra, BrushPressureRoundness, BrushHintPressureRoundness, number!(pressure_roundness, UNIT_RANGE, Percent), always),
    row!(Dynamics, Extra, BrushPressureScatter, BrushHintPressureScatter, number!(pressure_scatter, UNIT_RANGE, Percent), stamped),
    row!(Dynamics, Extra, BrushPressureHardness, BrushHintPressureHardness, number!(pressure_hardness, UNIT_RANGE, Percent), soft_edged),
    row!(Dynamics, Extra, BrushPressureSpacing, BrushHintPressureSpacing, number!(pressure_spacing, UNIT_RANGE, Percent), stamped),
    row!(Dynamics, Extra, BrushTiltAngle, BrushHintTiltAngle, number!(tilt_angle, UNIT_RANGE, Percent), always),
    row!(Dynamics, Extra, BrushTiltRoundness, BrushHintTiltRoundness, number!(tilt_roundness, UNIT_RANGE, Percent), always),
    row!(Random, Basic, BrushSizeJitter, BrushHintSizeJitter, number!(size_jitter, UNIT_RANGE, Percent), stamped),
    row!(Random, Basic, BrushAngleJitter, BrushHintAngleJitter, number!(angle_jitter, UNIT_RANGE, Percent), stamped),
    row!(Random, Basic, BrushOpacityJitter, BrushHintOpacityJitter, number!(opacity_jitter, UNIT_RANGE, Percent), stamped),
    row!(Random, Extra, BrushHueJitter, BrushHintHueJitter, number!(hue_jitter, UNIT_RANGE, Percent), stamped),
    row!(Random, Extra, BrushSaturationJitter, BrushHintSaturationJitter, number!(saturation_jitter, UNIT_RANGE, Percent), stamped),
    row!(Random, Extra, BrushValueJitter, BrushHintValueJitter, number!(value_jitter, UNIT_RANGE, Percent), stamped),
    row!(Paint, Basic, BrushFlow, BrushHintFlow, number!(flow, FLOW_RANGE, Percent), always),
    row!(Paint, Basic, BrushAccumulate, BrushHintAccumulate, Control::Toggle(|settings| &mut settings.accumulate), stamped),
    row!(Paint, Basic, BrushGrain, BrushHintGrain, number!(grain, UNIT_RANGE, Percent), always),
    row!(Paint, Extra, BrushGrainScale, BrushHintGrainScale, number!(grain_scale, GRAIN_SCALE_RANGE, Plain), grained),
    row!(Paint, Extra, BrushTexture, BrushHintPaper, Control::Texture(TextureSlot::Paper), grained),
    row!(Paint, Basic, BrushDryness, BrushHintDryness, number!(dryness, UNIT_RANGE, Percent), always),
    row!(Paint, Extra, BrushDrynessSpeed, BrushHintDrynessSpeed, number!(velocity_dryness, UNIT_RANGE, Percent), always),
    row!(Paint, Extra, BrushDrynessPressure, BrushHintDrynessPressure, number!(pressure_dryness, UNIT_RANGE, Percent), always),
    row!(Paint, Extra, BrushDepletion, BrushHintDepletion, number!(depletion, UNIT_RANGE, Percent), always),
    row!(Paint, Basic, BrushBristles, BrushHintBristles, number!(bristles, UNIT_RANGE, Percent), ribbon),
    row!(Paint, Extra, BrushHairs, BrushHintHairs, Control::Texture(TextureSlot::Hairs), bristled),
];

pub(crate) struct BrushTargets<'a> {
    pub settings: &'a mut BrushSettings,
    pub textures: &'a mut BrushTextures,
    pub pick_texture: &'a mut Option<TextureSlot>,
}

fn visible_rows<'a>(tab: BrushTab, level: Level, scope: &'a Scope<'a>) -> impl Iterator<Item = &'static Row> + 'a {
    ROWS.iter().filter(move |row| row.tab == tab && row.level == level && (row.shown)(scope))
}

pub(crate) fn visible_tabs(settings: &BrushSettings, soft_edge: bool) -> Vec<(BrushTab, &'static str)> {
    let scope = Scope { settings, soft_edge };
    BrushTab::ALL
        .into_iter()
        .filter(|&(tab, _)| (tab == BrushTab::Layers && ribbon(&scope)) || [Level::Basic, Level::Extra].into_iter().any(|level| visible_rows(tab, level, &scope).next().is_some()))
        .map(|(tab, label)| (tab, t(label)))
        .collect()
}

pub(crate) fn tab_settings(ui: &mut Ui, skin: &skin::Skin, tab: BrushTab, targets: &mut BrushTargets, soft_edge: bool) -> bool {
    let scope = Scope { settings: targets.settings, soft_edge };
    let basic: Vec<&Row> = visible_rows(tab, Level::Basic, &scope).collect();
    let extra: Vec<&Row> = visible_rows(tab, Level::Extra, &scope).collect();
    let mut changed = false;
    for row in basic {
        changed |= show_row(ui, skin, row, targets);
    }
    if !extra.is_empty() {
        egui::CollapsingHeader::new(t(Text::BrushMore)).id_salt((MORE_ID, tab)).default_open(false).show(ui, |ui| {
            for row in extra {
                changed |= show_row(ui, skin, row, targets);
            }
        });
    }
    changed
}

fn show_row(ui: &mut Ui, skin: &skin::Skin, row: &Row, targets: &mut BrushTargets) -> bool {
    match row.control {
        Control::Number { field, range, unit } => number_row(ui, skin, row.label, row.hint, field(targets.settings), range, unit),
        Control::Toggle(field) => ui.checkbox(field(targets.settings), t(row.label)).on_hover_text(t(row.hint)).changed(),
        Control::Curve(field) => {
            hinted_label(ui, row.label, row.hint);
            curve_plot(ui, field(targets.settings), &PlotStyle::new(CURVE_EDITOR_SIZE, BRUSH_CURVE_MAX_LABEL))
        }
        Control::Texture(slot) => {
            let outcome = texture_rows(ui, skin, slot, row.label, row.hint, slot.of(targets.textures));
            if outcome.pick_file {
                *targets.pick_texture = Some(slot);
            }
            outcome.changed
        }
    }
}

pub(crate) fn hinted_label(ui: &mut Ui, label: Text, hint: Text) {
    ui.add(egui::Label::new(t(label)).sense(Sense::hover())).on_hover_text(t(hint));
}

pub(crate) fn number_row(ui: &mut Ui, skin: &skin::Skin, label: Text, hint: Text, value: &mut f32, range: (f32, f32), unit: Unit) -> bool {
    hinted_label(ui, label, hint);
    slider_row(ui, skin, value, range, unit)
}

pub(crate) fn slider_row(ui: &mut Ui, skin: &skin::Skin, value: &mut f32, range: (f32, f32), unit: Unit) -> bool {
    let scale = unit.scale();
    let mut shown = *value * scale;
    let changed = slider_with_number(ui, skin, &mut shown, range.0 * scale..=range.1 * scale, unit.step(), unit.format());
    if changed {
        *value = shown / scale;
    }
    changed
}
