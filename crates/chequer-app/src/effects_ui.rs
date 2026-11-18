
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use chequer_core::blend::BlendMode;
use chequer_core::effects::util::hash;
use chequer_core::effects::{Cancel, Category, Curve, EffectId, EffectParams, EffectSession, Palette, ParamKind, ParamSpec, ParamValue, all_effects, render};
use chequer_core::geom::IRect;
use chequer_core::raster::Raster;
use eframe::egui::{self, Align, Color32, Context, Layout, Ui, Vec2, vec2};

use crate::metrics::ITEM_SPACING;
use crate::color_popup::color_button;
use crate::app::{App, Command};
use crate::curve_editor::{Ghost, PlotStyle, curve_plot};
use crate::modal::ok_cancel;
use crate::i18n::{Text, TextScope, t, text_or_key};
use crate::icons::Icon;
use crate::skin::{self, Skin};
use crate::widgets::{Corner, DragSpec, PanelSpec, PanelState, Resize, SLIDER_ROW_HEIGHT, choice_combo, drag_value, floating_panel, icon_button_sized, plain_slider_stepped, text_button};

const WINDOW_WIDTH: f32 = 360.0;
const WINDOW_GAP: egui::Vec2 = vec2(240.0, 56.0);
const LABEL_WIDTH: f32 = 112.0;
const SLIDER_WIDTH: f32 = 104.0;
const NUMBER_WIDTH: f32 = 60.0;
const COMBO_WIDTH: f32 = 170.0;
const RESET_SIZE: f32 = 18.0;
const POINT_STEP: f64 = 0.01;
const POINT_LIMIT: f32 = 1.0;
const CURVE_SIZE: f32 = 192.0;
const CURVE_MAX_LABEL: &str = "255";
const CURVE_RED: Color32 = Color32::from_rgb(230, 90, 90);
const CURVE_GREEN: Color32 = Color32::from_rgb(90, 200, 100);
const CURVE_BLUE: Color32 = Color32::from_rgb(100, 140, 240);

pub fn effect_icon(id: EffectId) -> Option<Icon> {
    use chequer_core::effects::ids;
    let icons = [
        (ids::SEPIA, Icon::EffectSepia),
        (ids::POSTERIZE, Icon::EffectPosterize),
        (ids::HUE_SATURATION, Icon::EffectHueSaturation),
        (ids::BRIGHTNESS_CONTRAST, Icon::EffectBrightnessContrast),
        (ids::CURVES, Icon::EffectCurves),
        (ids::AUTO_LEVEL, Icon::RecolorWheel),
    ];
    icons.into_iter().find(|&(effect, _)| effect == id).map(|(_, icon)| icon)
}

pub fn effect_label(id: EffectId) -> &'static str {
    text_or_key(TextScope::Effect, id.as_str())
}

pub fn category_name(category: Category) -> &'static str {
    match category {
        Category::Adjust => t(Text::FxAdjustments),
        Category::Photo => t(Text::FxPhoto),
        Category::Distort => t(Text::FxDistort),
        Category::Object => t(Text::FxObject),
        Category::Blurs => t(Text::FxBlurs),
        Category::Stylize => t(Text::FxStylize),
        Category::Render => t(Text::FxRender),
        Category::Artistic => t(Text::FxArtistic),
        Category::Color => t(Text::FxColor),
        Category::Noise => t(Text::FxNoise),
        Category::Glitch => t(Text::FxGlitch),
    }
}

struct Preview {
    sender: Sender<(u64, Raster)>,
    receiver: Receiver<(u64, Raster)>,
    cancel: Cancel,
    generation: u64,
    shown: u64,
}

impl Preview {
    fn new() -> Self {
        let (sender, receiver) = channel();
        Self { sender, receiver, cancel: Cancel::default(), generation: 0, shown: 0 }
    }

    fn restart(&mut self, ctx: &Context, source: Arc<Raster>, area: IRect, params: EffectParams) {
        self.cancel.cancel();
        self.cancel = Cancel::default();
        self.generation += 1;
        let (generation, cancel, sender, ctx) = (self.generation, self.cancel.clone(), self.sender.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(result) = render(&source, area, &params, &cancel) {
                let _ = sender.send((generation, result));
                ctx.request_repaint();
            }
        });
    }

    fn latest(&mut self) -> Option<Raster> {
        let mut latest = None;
        while let Ok((generation, raster)) = self.receiver.try_recv() {
            if generation == self.generation {
                latest = Some(raster);
            }
        }
        latest
    }

    fn is_current_shown(&self) -> bool {
        self.shown == self.generation
    }
}

pub struct EffectWindow {
    tab: usize,
    params: EffectParams,
    session: EffectSession,
    panel: PanelState,
    preview: Preview,
}

impl EffectWindow {
    pub(crate) fn panel_rect(&self) -> egui::Rect {
        self.panel.hit_rect()
    }
}

#[derive(Default)]
struct Outcome {
    changed: bool,
    accept: bool,
    cancel: bool,
}

#[derive(PartialEq)]
enum Edit {
    None,
    Changed,
    Reset,
}

impl App {
    fn effect_palette(&self) -> Palette {
        Palette { primary: self.colors.primary, secondary: self.colors.secondary }
    }

    pub(crate) fn open_effect(&mut self, ctx: &Context, kind: EffectId) {
        if self.tabs.is_empty() {
            return;
        }
        self.flush_pending();
        let palette = self.effect_palette();
        let params = self.effect_memory.get(&kind).cloned().unwrap_or_else(|| EffectParams::defaults(kind, palette));
        let tab_index = self.active;
        let workspace = self.workspace;
        let Some(session) = self.tab_mut().doc.begin_effect() else {
            return;
        };
        let mut window = EffectWindow {
            tab: tab_index,
            params,
            panel: PanelState::at_corner(Corner::TOP_RIGHT, WINDOW_GAP, vec2(WINDOW_WIDTH, 0.0), workspace),
            session,
            preview: Preview::new(),
        };
        window.preview.restart(ctx, window.session.source().clone(), window.session.area(), window.params.clone());
        self.effect = Some(window);
        if kind.params().is_empty() {
            self.close_effect(true);
        }
    }

    pub(crate) fn repeat_effect(&mut self) {
        let Some(params) = self.last_effect.clone().filter(|_| !self.tabs.is_empty()) else {
            return;
        };
        self.flush_pending();
        let tab = self.tab_mut();
        let Some(mut session) = tab.doc.begin_effect() else {
            return;
        };
        let rendered = render(session.source(), session.area(), &params, &Cancel::default());
        let area = match rendered {
            Some(raster) => {
                tab.doc.show_effect(&mut session, &raster);
                tab.doc.finish_effect(session, params.id())
            }
            None => tab.doc.cancel_effect(session),
        };
        tab.mark_dirty(area);
    }

    pub(crate) fn close_effect(&mut self, accept: bool) {
        let Some(mut window) = self.effect.take() else {
            return;
        };
        window.preview.cancel.cancel();
        let Some(tab) = self.tabs.get_mut(window.tab) else {
            return;
        };
        let kind = window.params.id();
        let area = if accept {
            if !window.preview.is_current_shown() {
                if let Some(raster) = render(window.session.source(), window.session.area(), &window.params, &Cancel::default()) {
                    tab.doc.show_effect(&mut window.session, &raster);
                }
            }
            self.last_effect = Some(window.params.clone());
            self.effect_memory.insert(kind, window.params);
            tab.doc.finish_effect(window.session, kind)
        } else {
            self.effect_memory.insert(kind, window.params);
            tab.doc.cancel_effect(window.session)
        };
        tab.mark_dirty(area);
    }

    pub(crate) fn effect_allows(command: Command) -> bool {
        matches!(
            command,
            Command::ZoomIn | Command::ZoomOut | Command::ZoomWindow | Command::ActualSize | Command::ToggleGrid | Command::TogglePanel(_) | Command::ResetLayout | Command::Settings | Command::Welcome
        )
    }

    pub(crate) fn show_effect_window(&mut self, ctx: &Context) {
        let Some(mut window) = self.effect.take() else {
            return;
        };
        if window.tab != self.active || window.tab >= self.tabs.len() {
            self.effect = Some(window);
            self.close_effect(true);
            return;
        }
        if let Some(raster) = window.preview.latest() {
            let tab = &mut self.tabs[window.tab];
            let area = tab.doc.show_effect(&mut window.session, &raster);
            tab.mark_dirty(area);
            window.preview.shown = window.preview.generation;
        }
        let skin = self.skin.clone();
        let palette = self.effect_palette();
        let spec = PanelSpec {
            id: "effect",
            title: effect_label(window.params.id()),
            titled: true,
            resize: Resize::Fixed,
            default_size: None,
            min_size: vec2(WINDOW_WIDTH, 0.0),
            fit_height: true,
            clip_title: false,
            over_image: self.image_rect,
        };
        let mut state = window.panel;
        let mut outcome = Outcome::default();
        floating_panel(ctx, &skin, &spec, &mut state, self.workspace, &[], |ui| outcome = effect_content(ui, &skin, &mut window.params, palette));
        window.panel = state;
        if !ctx.egui_wants_keyboard_input() {
            outcome.accept |= ctx.input(|i| i.key_pressed(egui::Key::Enter));
            outcome.cancel |= ctx.input(|i| i.key_pressed(egui::Key::Escape));
        }
        outcome.cancel |= !state.open;
        if outcome.changed && !outcome.accept && !outcome.cancel {
            window.preview.restart(ctx, window.session.source().clone(), window.session.area(), window.params.clone());
        }
        self.effect = Some(window);
        if outcome.cancel {
            self.close_effect(false);
        } else if outcome.accept {
            self.close_effect(true);
        }
    }

    pub(crate) fn effects_menu_items(&mut self, ui: &mut Ui, ctx: &Context) {
        if let Some(last) = &self.last_effect {
            let text = format!("{}: {}", t(Text::FxRepeat), effect_label(last.id()));
            if ui.add(egui::Button::new(text).shortcut_text("Ctrl+F")).clicked() {
                self.run(ctx, Command::RepeatEffect);
                ui.close();
            }
            ui.separator();
        }
        let mut categories: Vec<Category> = Category::ALL.into_iter().filter(|&category| category != Category::Adjust).collect();
        categories.sort_by_key(|&category| category_name(category));
        for category in categories {
            ui.menu_button(category_name(category), |ui| {
                let mut kinds: Vec<EffectId> = all_effects().into_iter().filter(|id| id.category() == category).collect();
                kinds.sort_by_key(|&kind| effect_label(kind));
                for kind in kinds {
                    if ui.button(effect_label(kind)).clicked() {
                        self.run(ctx, Command::Effect(kind));
                        ui.close();
                    }
                }
            });
        }
    }
}

fn effect_content(ui: &mut Ui, skin: &Skin, params: &mut EffectParams, palette: Palette) -> Outcome {
    let mut outcome = Outcome::default();
    ui.spacing_mut().item_spacing = ITEM_SPACING;
    for (index, spec) in params.id().params().iter().enumerate() {
        if !params.is_shown(index) {
            continue;
        }
        let mut value = params.values()[index];
        let ghosts = if matches!(spec.kind, ParamKind::Curve) { curve_ghosts(params, index) } else { Vec::new() };
        match param_row(ui, skin, spec, &mut value, &ghosts) {
            Edit::None => {}
            Edit::Changed => {
                params.set(index, value);
                outcome.changed = true;
            }
            Edit::Reset => {
                params.reset(index, palette);
                outcome.changed = true;
            }
        }
    }
    let (accept, cancel) = ok_cancel(ui, skin, true);
    outcome.accept = accept;
    outcome.cancel = cancel;
    outcome
}

fn curve_tint(key: &str) -> Option<Color32> {
    match key {
        "curve_red" => Some(CURVE_RED),
        "curve_green" => Some(CURVE_GREEN),
        "curve_blue" => Some(CURVE_BLUE),
        _ => None,
    }
}

fn curve_ghosts(params: &EffectParams, shown: usize) -> Vec<Ghost> {
    let neutral = skin::theme().ink;
    params
        .id()
        .params()
        .iter()
        .zip(params.values())
        .enumerate()
        .filter(|&(index, _)| index != shown)
        .filter_map(|(_, (spec, value))| match value {
            ParamValue::Curve(curve) if *curve != Curve::identity() => Some(Ghost::of(curve, curve_tint(spec.key).unwrap_or(neutral))),
            _ => None,
        })
        .collect()
}

fn label_cell(ui: &mut Ui, text: &str) {
    ui.allocate_ui_with_layout(vec2(LABEL_WIDTH, SLIDER_ROW_HEIGHT), Layout::left_to_right(Align::Center), |ui| {
        ui.add(egui::Label::new(text).truncate());
    });
}

fn decimals_for(step: f32) -> usize {
    if step >= 1.0 { 0 } else if step >= 0.1 { 1 } else { 2 }
}

fn reset_button(ui: &mut Ui, skin: &Skin) -> bool {
    icon_button_sized(ui, skin, Icon::Refresh, false, true, vec2(RESET_SIZE, RESET_SIZE), t(Text::FxReset)).clicked()
}

fn number_row(ui: &mut Ui, skin: &Skin, name: &str, value: &mut f32, range: (f32, f32), step: f32, suffix: &'static str) -> Edit {
    let mut edit = Edit::None;
    label_cell(ui, name);
    let (min, max) = range;
    if let Some(unit) = plain_slider_stepped(ui, skin, ((*value - min) / (max - min)).clamp(0.0, 1.0), SLIDER_WIDTH, step / (max - min)) {
        *value = (min + unit * (max - min)) / step;
        *value = value.round() * step;
        edit = Edit::Changed;
    }
    let spec = DragSpec { speed: f64::from(step), suffix, max_decimals: Some(decimals_for(step)), wheel_step: f64::from(step) };
    if drag_value(ui, skin, value, min..=max, [NUMBER_WIDTH, SLIDER_ROW_HEIGHT], &spec).changed() {
        edit = Edit::Changed;
    }
    if reset_button(ui, skin) {
        edit = Edit::Reset;
    }
    edit
}

fn param_row(ui: &mut Ui, skin: &Skin, spec: &ParamSpec, value: &mut ParamValue, ghosts: &[Ghost]) -> Edit {
    let name = text_or_key(TextScope::Param, spec.key);
    ui.horizontal(|ui| match (spec.kind, &mut *value) {
        (ParamKind::Int { min, max, .. }, ParamValue::Int(current)) => {
            let mut shown = *current as f32;
            let edit = number_row(ui, skin, name, &mut shown, (min as f32, max as f32), 1.0, "");
            *current = shown.round() as i32;
            edit
        }
        (ParamKind::Float { min, max, step, .. }, ParamValue::Float(current)) => number_row(ui, skin, name, current, (min, max), step, ""),
        (ParamKind::Angle { min, max, .. }, ParamValue::Float(current)) => number_row(ui, skin, name, current, (min, max), 1.0, "°"),
        (ParamKind::Bool { .. }, ParamValue::Bool(current)) => {
            if ui.checkbox(current, name).changed() { Edit::Changed } else { Edit::None }
        }
        (ParamKind::Choice { options, .. }, ParamValue::Choice(current)) => {
            label_cell(ui, name);
            let items: Vec<(usize, &str)> = options.iter().enumerate().map(|(i, key)| (i, text_or_key(TextScope::Option, key))).collect();
            if choice_combo(ui, skin, &format!("effect-{}", spec.key), COMBO_WIDTH, current, &items) { Edit::Changed } else { Edit::None }
        }
        (ParamKind::Blend, ParamValue::Choice(current)) => {
            label_cell(ui, name);
            let labels = App::blend_labels();
            let items: Vec<(usize, &str)> = BlendMode::ALL.iter().enumerate().filter_map(|(i, mode)| labels.iter().find(|(m, _)| m == mode).map(|(_, label)| (i, *label))).collect();
            if choice_combo(ui, skin, &format!("effect-{}", spec.key), COMBO_WIDTH, current, &items) { Edit::Changed } else { Edit::None }
        }
        (ParamKind::Point, ParamValue::Point(x, y)) => {
            label_cell(ui, name);
            let field = DragSpec { speed: POINT_STEP, suffix: "", max_decimals: Some(2), wheel_step: POINT_STEP };
            let mut edit = Edit::None;
            for axis in [x, y] {
                if drag_value(ui, skin, axis, -POINT_LIMIT..=POINT_LIMIT, [NUMBER_WIDTH, SLIDER_ROW_HEIGHT], &field).changed() {
                    edit = Edit::Changed;
                }
            }
            if reset_button(ui, skin) { Edit::Reset } else { edit }
        }
        (ParamKind::Seed, ParamValue::Seed(seed)) => {
            label_cell(ui, name);
            if text_button(ui, skin, t(Text::FxRandomize)).clicked() {
                let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
                *seed = hash(*seed, nanos as i32, 1);
                Edit::Changed
            } else {
                Edit::None
            }
        }
        (ParamKind::Color { .. }, ParamValue::Color(color)) => {
            label_cell(ui, name);
            if color_button(ui, skin, color, true).changed() {
                Edit::Changed
            } else {
                Edit::None
            }
        }
        (ParamKind::Curve, ParamValue::Curve(curve)) => {
            let style = PlotStyle::new(Vec2::splat(CURVE_SIZE), CURVE_MAX_LABEL).tinted(curve_tint(spec.key)).with_ghosts(ghosts);
            let edit = if curve_plot(ui, curve, &style) { Edit::Changed } else { Edit::None };
            if reset_button(ui, skin) { Edit::Reset } else { edit }
        }
        (kind, other) => panic!("параметр «{}»: схема {kind:?} не подходит к значению {other:?}", spec.key),
    })
    .inner
}
