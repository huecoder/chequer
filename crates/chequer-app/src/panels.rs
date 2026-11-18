
use chequer_core::history::HistoryKind;
use eframe::egui::{self, Align, Align2, Color32, ColorImage, Context, PointerButton, Pos2, Rect, Response, ScrollArea, Sense, Stroke, TextureOptions, Ui, Vec2, vec2};

use std::path::Path;

use crate::metrics::{INLINE_SPACING_TIGHT, ITEM_SPACING};
use crate::anim::{Glide, LayerMove, Slide, Slot, apply_layer_move, snap_to_pixels};
use crate::app::{App, COLORS_PANEL_SIZE, ColorSlot, Command, FileEvent, HISTORY_PANEL_SIZE, IMAGE_EXTENSIONS, LAYERS_PANEL_SIZE, PanelId, Panels, SWATCHES_PANEL_SIZE, TIMELINE_PANEL_SIZE, TOOLS_PANEL_SIZE};
use crate::color::{Hsv, hsv_to_rgb, parse_hex, rgb_to_hsv, to_hex};
use crate::i18n::{Text, t};
use crate::icons::Icon;
use crate::palette::{self, MAX_COLORS, Palette, Rgb, colors_of_raster};
use crate::skin::{self, Bevel, Font, Plate, Skin};
use crate::tools::Tool;
use crate::view::color32;
use crate::widgets::{DragSpec, ICON_BUTTON_SIZE, PanelSpec, Resize, SLIDER_ROW_HEIGHT, Track, bevel_over, choice_combo, drag_value, floating_panel, list_frame, gradient_slider, hover_text_with_hotkey, icon_button, icon_button_sized, number_field, plain_slider_stepped, select_all_on_focus};
use crate::scroll_fx::ScrollFx;

const ROW_HEIGHT: f32 = 20.0;
const LAYER_ROW_HEIGHT: f32 = 38.0;
const LAYER_NEST_INDENT: f32 = 30.0;
const LAYER_NEST_DRAG_DISTANCE: f32 = 16.0;
const LAYER_DROP_MARKER_WIDTH: f32 = 2.0;
const LAYER_DROP_ARROW: f32 = 5.0;
const LAYER_THUMB_HALF: f32 = 16.0;
const LAYER_THUMB_CENTER_X: f32 = 22.0;
const LAYER_MASK_ICON_OFFSET: f32 = 34.0;
const WHEEL_SIZE: usize = 120;
const LIST_TO_BAR_GAP: f32 = 4.0;
const LAYER_BLEND_COMBO_HEIGHT: f32 = 24.0;
const LAYER_PROPS_BAR_HEIGHT: f32 = LAYER_BLEND_COMBO_HEIGHT + SLIDER_ROW_HEIGHT + 4.0 * LIST_TO_BAR_GAP;
const LAYER_OPACITY_FIELD_WIDTH: f32 = 52.0;
const LAYER_MASK_BADGE_SIZE: f32 = 12.0;
const LAYER_NAME_LEFT: f32 = 46.0;
const LAYER_RENAME_HEIGHT: f32 = 20.0;
const PLUS_CELL_ALPHA: u8 = 90;
const PLUS_CROSS_GRAY: u8 = 128;
const PALETTE_UNDO_LIMIT: usize = 64;
const PLUS_CROSS_HALF: f32 = 3.0;
const PERCENT_MAX: f32 = 100.0;
const OPACITY_WHEEL_STEP: f32 = 0.01;
const HELD_SHADOW_PIXELS: f32 = 2.0;
const HELD_SHADOW_ALPHA: u8 = 70;
const BOTTOM_BAR_HEIGHT: f32 = crate::widgets::ICON_BUTTON_SIZE + 2.0 * LIST_TO_BAR_GAP;
const PALETTE_FRAME_MARGIN: f32 = 4.0;
const PICKER_MENU_WIDTH: f32 = 150.0;
const COMPACT_SWATCHES_SIZE: Vec2 = vec2(41.0, 39.0);
const COMPACT_SWATCH_SIDE: f32 = 26.0;
const COMPACT_SWATCH_SHIFT: Vec2 = vec2(15.0, 13.0);
const COMPACT_SWATCHES_GAP: f32 = 6.0;
const COMPACT_POPUP_WIDTH: f32 = 236.0;
const PICKER_COLUMN_GAP: f32 = 6.0;
const SLIDER_HEIGHT: f32 = 18.0;
const CHANNEL_LABEL_WIDTH: f32 = 12.0;
const NUMBER_WIDTH: f32 = 40.0;
const VALUE_SLIDER_WIDTH: f32 = 22.0;
const BESIDE_FIELDS_WIDTH: f32 = 170.0;
const COLORS_PALETTE_HEIGHT: f32 = 170.0;
const INLINE_LAYOUT_MENU_KEY: &str = "picker-layout-inline";
const HUE_STOPS: usize = 6;
const DEGREES_PER_HUE_STOP: f32 = 60.0;
const MAX_CHANNEL: f32 = 255.0;
const TOOL_BUTTON_GAP: f32 = 1.0;
pub const TOOL_BUTTON_SIZE: f32 = 24.0;
pub const TOOL_COLUMNS: usize = 2;
const MIN_VISIBLE_VALUE: f32 = 0.05;
const MIN_VISIBLE_SATURATION: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSelector {
    Wheel,
    Square,
    None,
}

impl ColorSelector {
    pub fn to_setting(self) -> &'static str {
        match self {
            Self::Wheel => "wheel",
            Self::Square => "square",
            Self::None => "none",
        }
    }

    pub fn from_setting(value: &str) -> Self {
        match value {
            "square" => Self::Square,
            "none" => Self::None,
            _ => Self::Wheel,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwatchSize {
    Small,
    Medium,
    Large,
}

impl SwatchSize {
    pub fn pixels(self) -> f32 {
        match self {
            Self::Small => 10.0,
            Self::Medium => 14.0,
            Self::Large => 20.0,
        }
    }

    pub fn to_setting(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    pub fn from_setting(value: &str) -> Self {
        match value {
            "small" => Self::Small,
            "large" => Self::Large,
            _ => Self::Medium,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayerDoubleClick {
    #[default]
    Properties,
    Rename,
}

impl LayerDoubleClick {
    pub fn to_setting(self) -> &'static str {
        match self {
            LayerDoubleClick::Properties => "properties",
            LayerDoubleClick::Rename => "rename",
        }
    }

    pub fn from_setting(value: &str) -> Self {
        if value == "rename" { LayerDoubleClick::Rename } else { LayerDoubleClick::Properties }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickerLayout {
    pub selector: ColorSelector,
    pub rgb: bool,
    pub hsv: bool,
    pub alpha: bool,
    pub hex: bool,
    pub beside: bool,
    pub swatch: SwatchSize,
}

impl PickerLayout {
    pub const COMPACT: Self = Self { selector: ColorSelector::Wheel, rgb: false, hsv: false, alpha: false, hex: false, beside: false, swatch: SwatchSize::Medium };
    pub const FULL: Self = Self { selector: ColorSelector::Wheel, rgb: true, hsv: true, alpha: true, hex: true, beside: false, swatch: SwatchSize::Medium };
}

impl Default for PickerLayout {
    fn default() -> Self {
        Self { rgb: true, alpha: true, hex: true, ..Self::COMPACT }
    }
}

#[derive(Clone, Copy)]
enum LayerMenuItem {
    Run(Command),
    AddMaskedBelow,
    Release,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutMenu {
    None,
    Popup,
    Inline,
}

fn picker_width(layout: PickerLayout) -> f32 {
    let selector = WHEEL_SIZE as f32 + PICKER_COLUMN_GAP + VALUE_SLIDER_WIDTH;
    if layout.beside && layout.selector != ColorSelector::None {
        selector + PICKER_COLUMN_GAP + BESIDE_FIELDS_WIDTH
    } else {
        COMPACT_POPUP_WIDTH
    }
}

fn inline_layout_menu_id() -> egui::Id {
    egui::Id::NULL.with(INLINE_LAYOUT_MENU_KEY)
}

fn layout_menu_content(ui: &mut Ui, layout: &mut PickerLayout) -> bool {
    ui.horizontal(|ui| {
        for (preset, label) in [(PickerLayout::COMPACT, Text::PresetCompact), (PickerLayout::FULL, Text::PresetFull)] {
            if ui.button(t(label)).clicked() {
                *layout = PickerLayout { swatch: layout.swatch, ..preset };
            }
        }
    });
    ui.separator();
    picker_selector_list(ui, layout);
    ui.separator();
    picker_channel_controls(ui, layout);
    ui.separator();
    ui.button(t(Text::PickerMore)).clicked()
}

pub(crate) const SELECTOR_LABELS: [(ColorSelector, Text); 3] =
    [(ColorSelector::Wheel, Text::SelectorWheel), (ColorSelector::Square, Text::SelectorSquare), (ColorSelector::None, Text::SelectorNone)];

fn picker_selector_list(ui: &mut Ui, layout: &mut PickerLayout) {
    for (selector, label) in SELECTOR_LABELS {
        ui.selectable_value(&mut layout.selector, selector, t(label));
    }
}

pub(crate) fn picker_channel_controls(ui: &mut Ui, layout: &mut PickerLayout) {
    egui::Grid::new("picker-channels").num_columns(2).spacing(vec2(PICKER_COLUMN_GAP * 2.0, 2.0)).show(ui, |ui| {
        ui.checkbox(&mut layout.rgb, t(Text::ShowRgb));
        ui.checkbox(&mut layout.hsv, t(Text::ShowHsv));
        ui.end_row();
        ui.checkbox(&mut layout.alpha, t(Text::ShowAlpha));
        ui.checkbox(&mut layout.hex, t(Text::ShowHex));
        ui.end_row();
    });
    ui.checkbox(&mut layout.beside, t(Text::SlidersBeside));
}

impl App {
    pub(crate) fn show_panels(&mut self, ctx: &Context, workspace: Rect) {
        self.colors.palette_hovered = false;
        let skin = self.skin.clone();
        let skin = skin.as_ref();
        let over_image = self.image_rect;
        let spec = |id, title, titled, resize, default_size, min_size, fit_height, clip_title| PanelSpec {
            id,
            title,
            titled,
            resize,
            default_size: Some(default_size),
            min_size,
            fit_height,
            clip_title,
            over_image,
        };
        let neighbors = |panels: &Panels, except: PanelId| -> Vec<Rect> {
            PanelId::ALL
                .into_iter()
                .filter(|&id| id != except)
                .filter_map(|id| panels.get(id).filter(|state| state.open).map(|state| state.rect()))
                .collect()
        };

        let others = neighbors(&self.panels, PanelId::Tools);
        let mut state = self.panels.tools;
        floating_panel(ctx, skin, &spec("tools", t(Text::Tools), false, Resize::Fixed, TOOLS_PANEL_SIZE, vec2(0.0, 0.0), true, true), &mut state, workspace, &others, |ui| {
            self.tools_content(ui);
        });
        self.panels.tools = state;

        let others = neighbors(&self.panels, PanelId::History);
        let mut state = self.panels.history;
        floating_panel(ctx, skin, &spec("history", t(Text::History), true, Resize::Both, HISTORY_PANEL_SIZE, vec2(160.0, 110.0), false, false), &mut state, workspace, &others, |ui| {
            self.history_content(ui);
        });
        self.panels.history = state;

        let others = neighbors(&self.panels, PanelId::Layers);
        let mut state = self.panels.layers;
        floating_panel(ctx, skin, &spec("layers", t(Text::Layers), true, Resize::Both, LAYERS_PANEL_SIZE, vec2(200.0, 130.0), false, false), &mut state, workspace, &others, |ui| {
            self.layers_content(ui, ctx);
        });
        self.panels.layers = state;

        let others = neighbors(&self.panels, PanelId::Timeline);
        let mut state = self.panels.timeline;
        floating_panel(ctx, skin, &spec("timeline", t(Text::Timeline), true, Resize::Both, TIMELINE_PANEL_SIZE, vec2(320.0, 150.0), false, false), &mut state, workspace, &others, |ui| {
            self.timeline_content(ui, ctx);
        });
        self.panels.timeline = state;

        if !self.settings.compact_colors {
            let others = neighbors(&self.panels, PanelId::Colors);
            let mut state = self.panels.colors;
            floating_panel(ctx, skin, &spec("colors", t(Text::Colors), true, Resize::Width, COLORS_PANEL_SIZE, vec2(250.0, 0.0), true, false), &mut state, workspace, &others, |ui| {
                self.colors_content(ui, ctx);
            });
            self.panels.colors = state;
        }

        if !self.settings.palette_below {
            let others = neighbors(&self.panels, PanelId::Swatches);
            let mut state = self.panels.swatches;
            floating_panel(ctx, skin, &spec("swatches", t(Text::Swatches), true, Resize::Both, SWATCHES_PANEL_SIZE, vec2(110.0, 90.0), false, false), &mut state, workspace, &others, |ui| {
                self.swatches_content(ui, ctx);
            });
            self.panels.swatches = state;
        }
    }

    fn tools_content(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing = vec2(TOOL_BUTTON_GAP, TOOL_BUTTON_GAP);
        let current = self.tool;
        let mut chosen = None;
        for row in Tool::all().chunks(TOOL_COLUMNS) {
            let width = (TOOL_BUTTON_SIZE + TOOL_BUTTON_GAP) * (TOOL_COLUMNS / row.len()) as f32 - TOOL_BUTTON_GAP;
            ui.horizontal(|ui| {
                for &tool in row {
                    let tip = if tool.is_available() { tool.name().to_owned() } else { format!("{} - {}", tool.name(), t(Text::SoonHint)) };
                    let size = vec2(width, TOOL_BUTTON_SIZE);
                    let button = icon_button_sized(ui, &self.skin, tool.icon(), tool == current, tool.is_available(), size, "");
                    if hover_text_with_hotkey(button, &tip, tool.hotkey()).clicked() {
                        chosen = Some(tool);
                    }
                }
            });
        }
        if let Some(tool) = chosen {
            self.set_tool(tool);
        }
        if self.settings.compact_colors {
            self.compact_colors(ui);
        }
    }

    fn compact_colors(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.add_space(COMPACT_SWATCHES_GAP);
        let swatches = self.swatch_pair(ui, COMPACT_SWATCHES_SIZE, COMPACT_SWATCH_SIDE, COMPACT_SWATCH_SHIFT);
        ui.horizontal(|ui| {
            let size = vec2(TOOL_BUTTON_SIZE, TOOL_BUTTON_SIZE);
            if icon_button_sized(ui, &self.skin, Icon::Swap, false, true, size, t(Text::SwapColors)).clicked() {
                self.colors.swap();
            }
            if icon_button_sized(ui, &self.skin, Icon::Refresh, false, true, size, t(Text::ResetColors)).clicked() {
                self.colors.reset_to_defaults();
            }
        });
        for (_, swatch) in swatches {
            egui::Popup::from_toggle_button_response(&swatch).align(egui::RectAlign::RIGHT_START).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                skin::menu_style(ui);
                ui.set_width(picker_width(self.settings.picker));
                self.colors_body(ui, &ctx, LayoutMenu::Inline);
            });
        }
    }

    fn history_content(&mut self, ui: &mut Ui) {
        let (kinds, cursor): (Vec<HistoryKind>, usize) = {
            let history = self.tab().doc.history();
            (history.kinds().collect(), history.cursor())
        };
        let list_height = (ui.available_height() - BOTTOM_BAR_HEIGHT).max(ROW_HEIGHT);
        let mut jump = None;
        let skin = self.skin.clone();
        list_frame(ui, &skin, |ui| {
            ScrollArea::vertical().max_height(list_height).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let mut row = |ui: &mut Ui, index: usize, icon: Icon, text: &str| {
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
                    let current = index == cursor;
                    if current {
                        ui.painter().rect_filled(rect, 0.0, skin::theme().highlight);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 0.0, skin::theme().list_hover);
                    }
                    let undone = index > cursor;
                    let tint = if undone { Color32::from_white_alpha(90) } else { Color32::WHITE };
                    let icon_rect = Rect::from_center_size(rect.left_center() + vec2(12.0, 0.0), vec2(crate::icons::icon_side(), crate::icons::icon_side()));
                    skin.icons.image(icon).tint(tint).paint_at(ui, icon_rect);
                    let color = if current { skin::theme().highlight_text } else if undone { skin::theme().ink_dim } else { skin::theme().ink };
                    skin::text_at(ui.painter(), rect.left_center() + vec2(26.0, 0.0), Align2::LEFT_CENTER, text, Font::Body, color);
                    if response.clicked() {
                        jump = Some(index);
                    }
                };
                row(ui, 0, Icon::Open, t(self.base_history_text()));
                for (i, kind) in kinds.iter().enumerate() {
                    row(ui, i + 1, history_icon(*kind), history_label(*kind));
                }
            }).with_fx(ui, skin::theme().field);
        });
        ui.add_space(LIST_TO_BAR_GAP);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = INLINE_SPACING_TIGHT;
            let can_undo = self.tab().can_undo();
            let can_redo = cursor < kinds.len();
            let ctx = ui.ctx().clone();
            if ui.add_enabled_ui(can_undo, |ui| icon_button(ui, &self.skin, Icon::Undo, false, t(Text::Undo))).inner.clicked() {
                self.run(&ctx, Command::Undo);
            }
            if ui.add_enabled_ui(can_redo, |ui| icon_button(ui, &self.skin, Icon::Redo, false, t(Text::Redo))).inner.clicked() {
                self.run(&ctx, Command::Redo);
            }
        });
        if let Some(index) = jump {
            self.flush_pending();
            let tab = self.tab_mut();
            let area = tab.doc.jump_to(index);
            tab.mark_dirty(area);
            tab.mark_all_dirty();
        }
    }

    fn base_history_text(&self) -> Text {
        if self.tab().path.is_some() { Text::HistoryOpened } else { Text::HNew }
    }

    fn layer_glides(&mut self, ui: &Ui, glides_id: egui::Id, count: usize) -> Vec<Glide> {
        let stored = ui.data(|data| data.get_temp::<(usize, Vec<Glide>)>(glides_id));
        let mut glides = stored.filter(|(tab, glides)| *tab == self.active && glides.len() == count).map_or_else(|| vec![Glide::REST; count], |(_, glides)| glides);
        if let Some(layer_move) = self.layer_move.take() {
            apply_layer_move(&mut glides, layer_move, LAYER_ROW_HEIGHT);
        }
        glides
    }

    fn layer_row_motion(&self, ui: &mut Ui, ctx: &Context, rows: &[(String, bool)], active: usize) -> Option<(Slot, f32)> {
        let watch_id = ui.scope_id().with("layer-watch");
        let slot_id = ui.scope_id().with("layer-slot");
        let ghost_id = ui.scope_id().with("layer-ghost");
        let count = rows.len();
        let names: Vec<String> = rows.iter().map(|(name, _)| name.clone()).collect();
        let seen = ui.data(|data| data.get_temp::<LayerWatch>(watch_id)).unwrap_or_else(|| LayerWatch { tab: self.active, active, names: names.clone() });
        ui.data_mut(|data| data.insert_temp(watch_id, LayerWatch { tab: self.active, active, names }));
        if seen.tab == self.active && count == seen.names.len() + 1 {
            ui.data_mut(|data| data.insert_temp(slot_id, Slot::start(Slide::In, active)));
        } else if seen.tab == self.active && count + 1 == seen.names.len() {
            let name = seen.names.get(seen.active).cloned().unwrap_or_default();
            ui.data_mut(|data| {
                data.insert_temp(slot_id, Slot::start(Slide::Out, seen.active));
                data.insert_temp(ghost_id, name);
            });
        }
        let slot = ui.data(|data| data.get_temp::<Slot>(slot_id))?;
        let motion = slot.extent(ctx, LAYER_ROW_HEIGHT).map(|extent| (slot, extent));
        if motion.is_none() {
            ui.data_mut(|data| data.remove::<Slot>(slot_id));
        }
        motion
    }

    fn layers_content(&mut self, ui: &mut Ui, ctx: &Context) {
        let rows: Vec<(String, bool)> = self.tab().doc.layers().iter().map(|l| (l.name.clone(), l.visible)).collect();
        let active = self.tab().doc.active_layer();
        let masked: Vec<bool> = self.tab().doc.layers().iter().map(|l| l.masked).collect();
        let is_mask: Vec<bool> = (0..masked.len()).map(|index| self.tab().doc.is_mask(index)).collect();
        let collapsed_flags: Vec<bool> = self.tab().doc.layers().iter().map(|l| l.collapsed).collect();
        let hidden: Vec<bool> = (0..masked.len()).map(|index| self.tab().doc.mask_parent(index).is_some_and(|parent| collapsed_flags[parent])).collect();
        let child_counts: Vec<usize> = (0..masked.len()).map(|parent| if is_mask[parent] { (0..parent).rev().take_while(|&child| masked[child]).count() } else { 0 }).collect();
        let thumbs: Vec<egui::TextureId> = self.tab_mut().layer_thumbnails(ctx).iter().map(|t| t.id()).collect();
        let quick_settings = self.settings.layer_quick_settings;
        let props_bar_height = if quick_settings { LAYER_PROPS_BAR_HEIGHT } else { 0.0 };
        let list_height = (ui.available_height() - BOTTOM_BAR_HEIGHT - props_bar_height).max(LAYER_ROW_HEIGHT);
        let double_click = self.settings.layer_double_click;
        let mut open_properties = false;
        let mut select = None;
        let mut toggle = None;
        let mut fold = None;
        let rename_id = ui.scope_id().with("layer-rename");
        let mut renaming: Option<(usize, String)> = ui.data(|data| data.get_temp(rename_id));
        let mut rename_commit: Option<(usize, String)> = None;
        let mut reorder = None;
        let mut nest = None;
        let mut unnest = None;
        let mut menu_pick: Option<LayerMenuItem> = None;
        let skin = self.skin.clone();
        let drag_id = ui.scope_id().with("layer-drag");
        let glides_id = ui.scope_id().with("layer-glides");
        let mut held: Option<HeldLayer> = ui.data(|data| data.get_temp(drag_id));
        let mut glides = self.layer_glides(ui, glides_id, rows.len());
        let motion = self.layer_row_motion(ui, ctx, &rows, active);
        let ghost_name: Option<String> = ui.data(|data| data.get_temp(ui.scope_id().with("layer-ghost")));
        list_frame(ui, &skin, |ui| {
            ScrollArea::vertical().max_height(list_height).auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let mut slots = Vec::with_capacity(rows.len());
                for index in (0..rows.len()).rev() {
                    if hidden[index] {
                        continue;
                    }
                    paint_closing_layer(ui, &skin, motion, ghost_name.as_deref(), Some(index));
                    let row_height = motion.filter(|(slot, _)| slot.slide == Slide::In && slot.index == index).map_or(LAYER_ROW_HEIGHT, |(_, extent)| extent);
                    let (slot, response) = ui.allocate_exact_size(vec2(ui.available_width(), row_height.clamp(0.0, LAYER_ROW_HEIGHT)), Sense::click_and_drag());
                    slots.push(RowSlot { index, rect: slot, hovered: response.hovered() });
                    if response.drag_started() {
                        let grab = response.interact_pointer_pos().map_or(0.0, |pos| pos.y - slot.top());
                        let grab_x = response.interact_pointer_pos().map_or(0.0, |pos| pos.x);
                        held = Some(HeldLayer { index, grab, grab_x });
                    }
                    let eye_response = ui.interact(layer_eye_rect(full_row(slot)), ui.scope_id().with(("eye", index)), Sense::click());
                    let fold_clicked = is_mask[index] && ui.interact(layer_fold_rect(full_row(slot)), ui.scope_id().with(("fold", index)), Sense::click()).clicked();
                    if eye_response.clicked() {
                        toggle = Some(index);
                    } else if fold_clicked {
                        fold = Some(index);
                    } else if response.clicked() {
                        select = Some(index);
                    }
                    if response.double_clicked() {
                        match double_click {
                            LayerDoubleClick::Properties => open_properties = true,
                            LayerDoubleClick::Rename => {
                                renaming = Some((index, rows[index].0.clone()));
                                ui.memory_mut(|memory| memory.request_focus(rename_id.with(index)));
                            }
                        }
                    }
                    if response.secondary_clicked() {
                        select = Some(index);
                    }
                    response.context_menu(|ui| {
                        skin::menu_style(ui);
                        let count = rows.len();
                        let entries = [
                            (Text::AddLayer, LayerMenuItem::Run(Command::AddLayer), true),
                            (Text::LayerMenuAddMasked, LayerMenuItem::AddMaskedBelow, true),
                            (Text::DuplicateLayer, LayerMenuItem::Run(Command::DuplicateLayer), true),
                            (Text::DeleteLayer, LayerMenuItem::Run(Command::DeleteLayer), count > 1),
                            (Text::MergeLayerDown, LayerMenuItem::Run(Command::MergeDown), index > 0),
                            (Text::MoveLayerUp, LayerMenuItem::Run(Command::LayerUp), index + 1 < count),
                            (Text::MoveLayerDown, LayerMenuItem::Run(Command::LayerDown), index > 0),
                            (Text::LayerMenuRelease, LayerMenuItem::Release, masked[index]),
                            (Text::LayerProperties, LayerMenuItem::Run(Command::LayerProperties), true),
                        ];
                        for (label, item, enabled) in entries {
                            if ui.add_enabled(enabled, egui::Button::new(t(label))).clicked() {
                                select = Some(index);
                                menu_pick = Some(item);
                                ui.close();
                            }
                        }
                    });
                }
                paint_closing_layer(ui, &skin, motion, ghost_name.as_deref(), None);
                let pointer = held.and_then(|_| ui.ctx().pointer_interact_pos());
                let pointer_y = pointer.map(|pointer| pointer.y);
                let target = pointer_y.map(|y| nearest_row(&slots, y));
                let intent = held.zip(pointer).zip(target).map(|((layer, pointer), target)| drop_intent(layer.index, target, pointer.x - layer.grab_x, &masked));
                let slide_to = intent.map(|intent| match intent {
                    DropIntent::Reorder(row) => row,
                    DropIntent::Nest(_) | DropIntent::Unnest => held.map_or(0, |layer| layer.index),
                });
                let held_index = held.map(|layer| layer.index);
                let list_clip = ui.clip_rect();
                for slot in &slots {
                    if held_index == Some(slot.index) {
                        continue;
                    }
                    glides[slot.index].retarget(slide_target(held_index, slide_to, slot.index));
                    let shown = full_row(slot.rect).translate(vec2(0.0, glides[slot.index].value(ctx)));
                    if motion.is_some_and(|(growing, _)| growing.slide == Slide::In && growing.index == slot.index) {
                        ui.set_clip_rect(list_clip.intersect(slot.rect));
                    }
                    let renamed = renaming.as_ref().is_some_and(|(at, _)| *at == slot.index);
                    let view = LayerRowView { name: if renamed { "" } else { rows[slot.index].0.as_str() }, visible: rows[slot.index].1, thumb: thumbs.get(slot.index).copied(), active: slot.index == active, hovered: slot.hovered, masked: masked[slot.index], is_mask: is_mask[slot.index], collapsed: collapsed_flags[slot.index], children: child_counts[slot.index] };
                    paint_layer_row(ui, &skin, shown, &view);
                    if renamed {
                        let indent = if masked[slot.index] { LAYER_NEST_INDENT } else { 0.0 };
                        let edit_rect = Rect::from_min_max(
                            Pos2::new(shown.left() + LAYER_NAME_LEFT + indent - 2.0, shown.center().y - LAYER_RENAME_HEIGHT / 2.0),
                            Pos2::new(layer_fold_rect(shown).left() - 2.0, shown.center().y + LAYER_RENAME_HEIGHT / 2.0),
                        );
                        if let Some((_, buffer)) = renaming.as_mut() {
                            let response = ui.put(edit_rect, egui::TextEdit::singleline(buffer).id(rename_id.with(slot.index)).desired_width(edit_rect.width()));
                            select_all_on_focus(ui, &response, buffer);
                            let clicked_away = ui.input(|input| input.pointer.any_pressed()) && !response.contains_pointer();
                            if response.lost_focus() || clicked_away {
                                if !ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                                    rename_commit = Some((slot.index, buffer.clone()));
                                }
                                renaming = None;
                            }
                        }
                    }
                    ui.set_clip_rect(list_clip);
                }
                if let (Some(layer), Some(y), Some(target), Some(intent)) = (held, pointer_y, target, intent) {
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                    let (Some(origin), Some(destination), Some(top_limit), Some(bottom_limit)) =
                        (slot_of(&slots, layer.index), slot_of(&slots, target), slots.first().map(|s| s.rect.top()), slots.last().map(|s| s.rect.bottom()))
                    else {
                        return;
                    };
                    let hint = match intent {
                        DropIntent::Nest(parent) => Some(format!("{} «{}»", t(Text::DropNestInto), rows[parent].0)),
                        DropIntent::Unnest => Some(t(Text::LayerMenuRelease).to_owned()),
                        DropIntent::Reorder(_) => None,
                    };
                    if let Some(hint) = hint {
                        egui::Tooltip::always_open(ctx.clone(), ui.layer_id(), rename_id.with("drop-hint"), egui::PopupAnchor::Pointer).show(|ui| {
                            ui.label(hint);
                        });
                    }
                    let top = snap_to_pixels((y - layer.grab).clamp(top_limit, bottom_limit - LAYER_ROW_HEIGHT), ctx.pixels_per_point());
                    let shown = Rect::from_min_size(Pos2::new(origin.min.x, top), vec2(origin.width(), LAYER_ROW_HEIGHT));
                    let held_masked = match intent {
                        DropIntent::Nest(_) => true,
                        DropIntent::Unnest => false,
                        DropIntent::Reorder(_) => masked[layer.index],
                    };
                    let marker = match intent {
                        DropIntent::Nest(parent) => slot_of(&slots, parent).map(|rect| (rect, true)),
                        DropIntent::Unnest => (layer.index + 1..masked.len()).find(|&above| !masked[above]).and_then(|parent| slot_of(&slots, parent)).map(|rect| (rect, false)),
                        DropIntent::Reorder(_) => None,
                    };
                    if let Some((rect, nest)) = marker {
                        paint_drop_marker(ui, full_row(rect), nest);
                    }
                    let view = LayerRowView { name: &rows[layer.index].0, visible: rows[layer.index].1, thumb: thumbs.get(layer.index).copied(), active: layer.index == active, hovered: true, masked: held_masked, is_mask: is_mask[layer.index], collapsed: collapsed_flags[layer.index], children: child_counts[layer.index] };
                    paint_held_layer(ui, &skin, shown, &view);
                    if !ui.input(|input| input.pointer.any_down()) {
                        let landed = full_row(destination).top();
                        let released = ui.input(|input| input.pointer.any_released());
                        match intent {
                            DropIntent::Nest(parent) if released => nest = Some((layer.index, parent)),
                            DropIntent::Unnest if released => unnest = Some(layer.index),
                            DropIntent::Reorder(row) if released && row != layer.index => {
                                reorder = Some(LayerMove { from: layer.index, to: row, held_offset: Some(top - landed) });
                            }
                            _ => glides[layer.index] = Glide::from_offset(top - full_row(origin).top()),
                        }
                        held = None;
                    }
                }
            }).with_fx(ui, skin::theme().field);
        });
        ui.data_mut(|data| {
            data.insert_temp(glides_id, (self.active, glides));
            if let Some(layer) = held {
                data.insert_temp(drag_id, layer);
            } else {
                data.remove::<HeldLayer>(drag_id);
            }
        });
        ui.data_mut(|data| match &renaming {
            Some(state) => {
                data.insert_temp(rename_id, state.clone());
            }
            None => {
                data.remove::<(usize, String)>(rename_id);
            }
        });
        ui.add_space(LIST_TO_BAR_GAP);
        if quick_settings {
            self.layer_props_bar(ui, active);
        }
        let mut command = if open_properties { Some(Command::LayerProperties) } else { None };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = INLINE_SPACING_TIGHT;
            let count = rows.len();
            let skin = &self.skin;
            let mut button = |ui: &mut Ui, enabled: bool, icon: Icon, tip: Text, cmd: Command| {
                if ui.add_enabled_ui(enabled, |ui| icon_button(ui, skin, icon, false, t(tip))).inner.clicked() {
                    command = Some(cmd);
                }
            };
            button(ui, true, Icon::Add, Text::AddLayer, Command::AddLayer);
            button(ui, count > 1, Icon::LayerRemove, Text::DeleteLayer, Command::DeleteLayer);
            button(ui, true, Icon::Duplicate, Text::DuplicateLayer, Command::DuplicateLayer);
            button(ui, active > 0, Icon::Merge, Text::MergeLayerDown, Command::MergeDown);
            button(ui, active + 1 < count, Icon::Up, Text::MoveLayerUp, Command::LayerUp);
            button(ui, active > 0, Icon::Down, Text::MoveLayerDown, Command::LayerDown);
            button(ui, true, Icon::Properties, Text::LayerProperties, Command::LayerProperties);
        });
        if let Some(layer_move) = reorder {
            self.edit_structure(|doc| {
                doc.move_layer_to(layer_move.from, layer_move.to);
            });
            self.layer_move = Some(layer_move);
            ctx.request_repaint();
        }
        if let Some((from, parent)) = nest {
            self.edit_structure(|doc| {
                doc.nest_layer(from, parent);
            });
            ctx.request_repaint();
        }
        if let Some(index) = unnest {
            self.edit_structure(|doc| {
                doc.unnest_layer(index);
            });
            ctx.request_repaint();
        }
        if let Some(index) = select {
            self.flush_pending();
            self.tab_mut().doc.set_active_layer(index);
        }
        if let Some(index) = toggle {
            self.toggle_layer_visibility(index);
        }
        if let Some(index) = fold {
            self.tab_mut().doc.toggle_layer_collapsed(index);
        }
        if let Some((index, name)) = rename_commit {
            let name = name.trim().to_owned();
            if !name.is_empty() {
                self.edit_structure(move |doc| {
                    let mut props = doc.layers()[index].props();
                    props.name = name;
                    doc.set_layer_props(index, props);
                });
            }
        }
        match menu_pick {
            Some(LayerMenuItem::Run(picked)) => command = Some(picked),
            Some(LayerMenuItem::AddMaskedBelow) => {
                let active = self.tab().doc.active_layer();
                let name = format!("{} {}", t(Text::LayerN), self.tab().doc.layers().len() + 1);
                self.edit_structure(move |doc| {
                    doc.add_masked_layer_below(active, name);
                });
                ctx.request_repaint();
            }
            Some(LayerMenuItem::Release) => {
                let active = self.tab().doc.active_layer();
                self.edit_structure(move |doc| {
                    doc.unnest_layer(active);
                });
                ctx.request_repaint();
            }
            None => {}
        }
        if let Some(command) = command {
            self.run(ctx, command);
        }
    }

    fn layer_props_bar(&mut self, ui: &mut Ui, index: usize) {
        let skin = self.skin.clone();
        let mut props = self.tab().doc.layers()[index].props();
        let before = props.clone();
        ui.spacing_mut().item_spacing = vec2(4.0, LIST_TO_BAR_GAP);
        let combo_width = ui.available_width();
        ui.allocate_ui(vec2(combo_width, LAYER_BLEND_COMBO_HEIGHT), |ui| choice_combo(ui, &skin, "layer-panel-blend", combo_width, &mut props.blend, &App::blend_labels()));
        ui.horizontal(|ui| {
            let mut percent = f32::from(props.opacity) * PERCENT_MAX / f32::from(u8::MAX);
            let mut edited = false;
            let slider_width = (ui.available_width() - LAYER_OPACITY_FIELD_WIDTH - ui.spacing().item_spacing.x).max(1.0);
            if let Some(unit) = plain_slider_stepped(ui, &skin, percent / PERCENT_MAX, slider_width, OPACITY_WHEEL_STEP) {
                percent = (unit * PERCENT_MAX).round();
                edited = true;
            }
            let spec = DragSpec { speed: 1.0, suffix: "%", max_decimals: Some(0), wheel_step: 1.0 };
            edited |= drag_value(ui, &skin, &mut percent, 0.0..=PERCENT_MAX, [LAYER_OPACITY_FIELD_WIDTH, SLIDER_ROW_HEIGHT], &spec).changed();
            if edited {
                props.opacity = (percent * f32::from(u8::MAX) / PERCENT_MAX).round() as u8;
            }
        });
        if props != before {
            self.edit_structure(move |doc| doc.set_layer_props(index, props));
        }
    }

    pub(crate) fn toggle_layer_visibility(&mut self, index: usize) {
        self.flush_pending();
        let tab = self.tab_mut();
        let mut props = tab.doc.layers()[index].props();
        props.visible = !props.visible;
        tab.doc.set_layer_props(index, props);
        tab.mark_all_dirty();
    }

    fn colors_content(&mut self, ui: &mut Ui, ctx: &Context) {
        self.colors_body(ui, ctx, LayoutMenu::Popup);
    }

    pub(crate) fn colors_body(&mut self, ui: &mut Ui, ctx: &Context, menu: LayoutMenu) {
        ui.spacing_mut().item_spacing = ITEM_SPACING;
        self.sync_hsv();
        let layout = self.settings.picker;
        ui.horizontal(|ui| {
            self.color_swatches(ui);
            if menu != LayoutMenu::None {
                ui.with_layout(egui::Layout::right_to_left(Align::TOP), |ui| self.layout_menu(ui, menu));
            }
        });
        if menu == LayoutMenu::Inline && self.inline_layout_menu(ui) {
            return;
        }
        let beside = layout.beside && layout.selector != ColorSelector::None;
        ui.horizontal_top(|ui| {
            match layout.selector {
                ColorSelector::Wheel => {
                    self.color_wheel(ui, ctx);
                    self.value_slider(ui);
                }
                ColorSelector::Square => {
                    self.color_square(ui, ctx);
                    self.hue_slider(ui);
                }
                ColorSelector::None => {}
            }
            if beside {
                ui.vertical(|ui| {
                    ui.set_width(BESIDE_FIELDS_WIDTH);
                    self.color_fields(ui, layout);
                });
            }
        });
        if !beside {
            self.color_fields(ui, layout);
        }
        if self.settings.palette_below {
            ui.separator();
            ui.allocate_ui(vec2(ui.available_width(), COLORS_PALETTE_HEIGHT), |ui| self.swatches_content(ui, ctx));
        }
    }

    fn color_fields(&mut self, ui: &mut Ui, layout: PickerLayout) {
        self.rgba_sliders(ui, layout.rgb, layout.alpha);
        if layout.hsv {
            self.hsv_sliders(ui);
        }
        if layout.hex {
            self.hex_field(ui);
        }
    }

    fn layout_menu(&mut self, ui: &mut Ui, menu: LayoutMenu) {
        let button = icon_button(ui, &self.skin, Icon::SettingsWindow, false, t(Text::PickerLayout));
        match menu {
            LayoutMenu::Popup => {
                let mut layout = self.settings.picker;
                let open_more = egui::Popup::menu(&button)
                    .show(|ui| {
                        skin::menu_style(ui);
                        ui.set_min_width(PICKER_MENU_WIDTH);
                        let open_more = layout_menu_content(ui, &mut layout);
                        if open_more {
                            ui.close();
                        }
                        open_more
                    })
                    .is_some_and(|shown| shown.inner);
                self.apply_picker_layout(layout, open_more);
            }
            LayoutMenu::Inline => {
                let open_id = inline_layout_menu_id();
                let open = ui.data(|data| data.get_temp::<bool>(open_id)).unwrap_or(false);
                if button.clicked() {
                    ui.data_mut(|data| data.insert_temp(open_id, !open));
                }
            }
            LayoutMenu::None => {}
        }
    }

    fn inline_layout_menu(&mut self, ui: &mut Ui) -> bool {
        let open_id = inline_layout_menu_id();
        if !ui.data(|data| data.get_temp::<bool>(open_id)).unwrap_or(false) {
            return false;
        }
        let mut layout = self.settings.picker;
        let open_more = ui
            .scope(|ui| {
                egui::containers::menu::menu_style(ui.style_mut());
                skin::menu_style(ui);
                ui.set_min_width(PICKER_MENU_WIDTH);
                layout_menu_content(ui, &mut layout)
            })
            .inner;
        if open_more {
            ui.data_mut(|data| data.insert_temp(open_id, false));
            egui::Popup::close_all(ui.ctx());
        }
        self.apply_picker_layout(layout, open_more);
        true
    }

    fn apply_picker_layout(&mut self, layout: PickerLayout, open_more: bool) {
        if open_more {
            self.dialog = Some(crate::dialogs::Dialog::ColorPicker { from_settings: false });
        }
        if layout != self.settings.picker {
            self.settings.picker = layout;
            crate::config::save(&self.settings);
        }
    }

    fn sync_hsv(&mut self) {
        let [r, g, b, _] = self.colors.active();
        if hsv_to_rgb(self.colors.hsv) != [r, g, b] {
            self.colors.hsv = rgb_to_hsv([r, g, b]);
        }
    }

    fn color_swatches(&mut self, ui: &mut Ui) {
        self.swatch_pair(ui, vec2(52.0, 44.0), 30.0, vec2(16.0, 14.0));
        let swap = ui.add_enabled_ui(true, |ui| icon_button(ui, &self.skin, Icon::Swap, false, "X")).inner;
        if swap.clicked() {
            self.colors.swap();
        }
    }

    fn swatch_pair(&mut self, ui: &mut Ui, size: Vec2, side: f32, shift: Vec2) -> [(ColorSlot, Response); 2] {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let secondary = Rect::from_min_size(rect.min + shift, vec2(side, side));
        let primary = Rect::from_min_size(rect.min, vec2(side, side));
        [
            (ColorSlot::Secondary, secondary, self.colors.secondary),
            (ColorSlot::Primary, primary, self.colors.primary),
        ]
        .map(|(slot, swatch, color)| {
            let response = ui.interact(swatch, ui.scope_id().with(("swatch", slot as u8)), Sense::click());
            self.skin.paint_plate(ui.painter(), swatch, &Plate::field(skin::theme().button));
            let inner = swatch.shrink(skin::plate_inset(ui.painter().pixels_per_point(), skin::FIELD_RINGS));
            if color[3] < u8::MAX {
                self.skin.paint_checker(&ui.painter().with_clip_rect(inner), inner, inner.min);
            }
            ui.painter().rect_filled(inner, 0.0, color32(color));
            if self.colors.editing == slot {
                ui.painter().rect_stroke(swatch.expand(1.0), 0.0, Stroke::new(1.0, skin::theme().line), egui::StrokeKind::Outside);
            }
            if response.clicked() {
                self.colors.editing = slot;
            }
            (slot, response)
        })
    }

    fn color_wheel(&mut self, ui: &mut Ui, ctx: &Context) {
        let hsv = lit_value(self.colors.hsv);
        let value = (hsv.v * 255.0).round() as u8;
        if self.colors.wheel_value != Some(value) {
            let image = make_wheel(hsv.v);
            match self.colors.wheel_texture.as_mut() {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => self.colors.wheel_texture = Some(ctx.load_texture("wheel", image, TextureOptions::NEAREST)),
            }
            self.colors.wheel_value = Some(value);
        }
        let (rect, response) = ui.allocate_exact_size(vec2(WHEEL_SIZE as f32, WHEEL_SIZE as f32), Sense::click_and_drag());
        if let Some(texture) = &self.colors.wheel_texture {
            ui.painter().image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let center = rect.center();
        let radius = rect.width() / 2.0;
        let mut hsv = hsv;
        if let Some(pos) = response.interact_pointer_pos().filter(|_| response.is_pointer_button_down_on() || response.dragged()) {
            let offset = pos - center;
            hsv.h = offset.y.atan2(offset.x).to_degrees().rem_euclid(360.0);
            hsv.s = (offset.length() / radius).min(1.0);
            self.apply_hsv(hsv);
        }
        let marker = center + vec2(hsv.h.to_radians().cos(), hsv.h.to_radians().sin()) * (hsv.s * radius);
        ui.painter().circle_stroke(marker, 4.0, Stroke::new(1.0, Color32::BLACK));
        ui.painter().circle_stroke(marker, 5.0, Stroke::new(1.0, Color32::WHITE));
    }

    fn value_slider(&mut self, ui: &mut Ui) {
        let hsv = self.colors.hsv;
        let stops = [(0.0, Color32::BLACK), (1.0, hsv_color(Hsv { v: 1.0, ..hsv }))];
        let size = vec2(VALUE_SLIDER_WIDTH, WHEEL_SIZE as f32);
        if let Some(value) = gradient_slider(ui, &self.skin, hsv.v, &Track { stops: &stops, checker: false }, size, true, crate::widgets::WHEEL_SLIDER_STEP) {
            self.apply_hsv(Hsv { v: value, ..hsv });
        }
    }

    fn color_square(&mut self, ui: &mut Ui, ctx: &Context) {
        let hsv = self.colors.hsv;
        let hue_degrees = hsv.h.round() as u16 % 360;
        if self.colors.square_hue != Some(hue_degrees) {
            let image = make_square(hue_degrees as f32);
            match self.colors.square_texture.as_mut() {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => self.colors.square_texture = Some(ctx.load_texture("square", image, TextureOptions::NEAREST)),
            }
            self.colors.square_hue = Some(hue_degrees);
        }
        let (rect, response) = ui.allocate_exact_size(vec2(WHEEL_SIZE as f32, WHEEL_SIZE as f32), Sense::click_and_drag());
        if let Some(texture) = &self.colors.square_texture {
            ui.painter().image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let mut hsv = hsv;
        if let Some(pos) = response.interact_pointer_pos().filter(|_| response.is_pointer_button_down_on() || response.dragged()) {
            hsv.s = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
            hsv.v = (1.0 - (pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
            self.apply_hsv(hsv);
        }
        let marker = Pos2::new(rect.min.x + hsv.s * rect.width(), rect.max.y - hsv.v * rect.height());
        ui.painter().circle_stroke(marker, 4.0, Stroke::new(1.0, Color32::BLACK));
        ui.painter().circle_stroke(marker, 5.0, Stroke::new(1.0, Color32::WHITE));
    }

    fn hue_slider(&mut self, ui: &mut Ui) {
        let hsv = self.colors.hsv;
        let stops: Vec<(f32, Color32)> = (0..=HUE_STOPS)
            .map(|step| (step as f32 / HUE_STOPS as f32, hsv_color(Hsv { h: step as f32 * DEGREES_PER_HUE_STOP, s: 1.0, v: 1.0 })))
            .collect();
        let size = vec2(VALUE_SLIDER_WIDTH, WHEEL_SIZE as f32);
        if let Some(fraction) = gradient_slider(ui, &self.skin, hsv.h / 360.0, &Track { stops: &stops, checker: false }, size, true, crate::widgets::WHEEL_SLIDER_STEP) {
            self.apply_hsv(Hsv { h: fraction * 360.0, ..hsv });
        }
    }

    fn apply_hsv(&mut self, hsv: Hsv) {
        let rgb = hsv_to_rgb(hsv);
        let alpha = self.colors.active()[3];
        self.colors.hsv = hsv;
        self.colors.set_active([rgb[0], rgb[1], rgb[2], alpha]);
    }

    fn channel_row(&mut self, ui: &mut Ui, label: &str, value: f32, max: f32, track: &Track) -> Option<f32> {
        let mut result = None;
        ui.horizontal(|ui| {
            let slider_width = (ui.available_width() - CHANNEL_LABEL_WIDTH - NUMBER_WIDTH - 2.0 * ui.spacing().item_spacing.x).max(40.0);
            ui.add_sized([CHANNEL_LABEL_WIDTH, SLIDER_HEIGHT], egui::Label::new(label));
            if let Some(fraction) = gradient_slider(ui, &self.skin, value / max, track, vec2(slider_width, SLIDER_HEIGHT), false, 1.0 / max) {
                result = Some((fraction * max).round());
            }
            let mut number = value.round();
            if number_field(ui, &self.skin, &mut number, 0.0..=max, NUMBER_WIDTH).changed() {
                result = Some(number);
            }
        });
        result
    }

    fn rgba_sliders(&mut self, ui: &mut Ui, show_rgb: bool, show_alpha: bool) {
        let color = self.colors.active();
        let mut edited = None;
        for (index, label) in ["R", "G", "B"].into_iter().enumerate().filter(|_| show_rgb) {
            let at = |level: u8| {
                let mut rgb = [color[0], color[1], color[2]];
                rgb[index] = level;
                Color32::from_rgb(rgb[0], rgb[1], rgb[2])
            };
            let stops = [(0.0, at(0)), (1.0, at(u8::MAX))];
            if let Some(value) = self.channel_row(ui, label, color[index] as f32, MAX_CHANNEL, &Track { stops: &stops, checker: false }) {
                edited = Some((index, value as u8));
            }
        }
        let clear = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 0);
        let opaque = Color32::from_rgb(color[0], color[1], color[2]);
        if show_alpha {
            if let Some(value) = self.channel_row(ui, "A", color[3] as f32, MAX_CHANNEL, &Track { stops: &[(0.0, clear), (1.0, opaque)], checker: true }) {
                edited = Some((3, value as u8));
            }
        }
        if let Some((index, value)) = edited {
            let mut color = self.colors.active();
            color[index] = value;
            self.colors.set_active(color);
        }
    }

    fn hsv_sliders(&mut self, ui: &mut Ui) {
        let hsv = self.colors.hsv;
        let for_hue = lit_saturation(lit_value(hsv));
        let for_saturation = lit_value(hsv);
        let hue_stops: Vec<(f32, Color32)> = (0..=HUE_STOPS)
            .map(|step| (step as f32 / HUE_STOPS as f32, hsv_color(Hsv { h: step as f32 * DEGREES_PER_HUE_STOP, ..for_hue })))
            .collect();
        let saturation_stops = [(0.0, hsv_color(Hsv { s: 0.0, ..for_saturation })), (1.0, hsv_color(Hsv { s: 1.0, ..for_saturation }))];
        let value_stops = [(0.0, hsv_color(Hsv { v: 0.0, ..hsv })), (1.0, hsv_color(Hsv { v: 1.0, ..hsv }))];
        if let Some(h) = self.channel_row(ui, "H", hsv.h, 360.0, &Track { stops: &hue_stops, checker: false }) {
            self.apply_hsv(Hsv { h, ..for_hue });
        }
        if let Some(s) = self.channel_row(ui, "S", hsv.s * 100.0, 100.0, &Track { stops: &saturation_stops, checker: false }) {
            self.apply_hsv(Hsv { s: s / 100.0, ..for_saturation });
        }
        if let Some(v) = self.channel_row(ui, "V", hsv.v * 100.0, 100.0, &Track { stops: &value_stops, checker: false }) {
            self.apply_hsv(Hsv { v: v / 100.0, ..hsv });
        }
    }

    fn hex_field(&mut self, ui: &mut Ui) {
        let color = self.colors.active();
        ui.horizontal(|ui| {
            ui.label(t(Text::Hex));
            let focused_id = ui.scope_id().with("hex");
            if self.colors.hex_synced != color && !ui.memory(|m| m.has_focus(focused_id)) {
                self.colors.hex_buffer = to_hex(color);
                self.colors.hex_synced = color;
            }
            let edit = egui::TextEdit::singleline(&mut self.colors.hex_buffer).id(focused_id).desired_width(78.0).char_limit(9);
            let response = ui.add(edit);
            crate::widgets::select_all_on_focus(ui, &response, &self.colors.hex_buffer);
            bevel_over(ui, &self.skin, response.rect, Bevel::Sunken);
            if response.changed() {
                if let Some(parsed) = parse_hex(&self.colors.hex_buffer, color[3]) {
                    self.colors.set_active(parsed);
                    self.colors.hex_synced = parsed;
                }
            }
        });
    }

    fn swatches_content(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.spacing_mut().item_spacing = ITEM_SPACING;
        let active = self.colors.active();
        let active_rgb = [active[0], active[1], active[2]];
        let has_color = self.colors.palette.contains(active_rgb);
        let full = self.colors.palette.colors.len() >= MAX_COLORS;
        let mut action = None;
        ui.horizontal(|ui| {
            let size = vec2(ICON_BUTTON_SIZE, ICON_BUTTON_SIZE);
            if icon_button_sized(ui, &self.skin, Icon::Add, false, !has_color && !full, size, t(Text::PaletteAddColor)).clicked() {
                action = Some(PaletteAction::Add(active_rgb));
            }
            if icon_button_sized(ui, &self.skin, Icon::Delete, false, has_color, size, t(Text::PaletteRemoveColor)).clicked() {
                action = Some(PaletteAction::Remove(active_rgb));
            }
            let menu = icon_button(ui, &self.skin, Icon::Palette, false, t(Text::PaletteMenu));
            egui::Popup::menu(&menu).show(|ui| {
                skin::menu_style(ui);
                for (candidate, label) in [
                    (PaletteAction::Load, Text::PaletteLoad),
                    (PaletteAction::Save, Text::PaletteSave),
                    (PaletteAction::FromImage, Text::PaletteFromImage),
                    (PaletteAction::Reset, Text::PaletteReset),
                ] {
                    if ui.button(t(label)).clicked() {
                        action = Some(candidate);
                        ui.close();
                    }
                }
                ui.separator();
                ui.label(t(Text::SwatchSize));
                let mut size = self.settings.picker.swatch;
                ui.horizontal(|ui| {
                    for (choice, label) in [(SwatchSize::Small, Text::SizeSmall), (SwatchSize::Medium, Text::SizeMedium), (SwatchSize::Large, Text::SizeLarge)] {
                        ui.selectable_value(&mut size, choice, t(label));
                    }
                });
                if size != self.settings.picker.swatch {
                    self.settings.picker.swatch = size;
                    crate::config::save(&self.settings);
                }
            });
            ui.label(egui::RichText::new(format!("{} ({})", self.colors.palette.name, self.colors.palette.colors.len())).color(skin::theme().ink_dim));
        });
        self.palette_grid(ui);
        if let Some(action) = action {
            self.run_palette_action(ctx, action);
        }
    }

    pub(crate) fn snapshot_palette(&mut self) {
        let colors = &mut self.colors;
        colors.palette_undo.push(colors.palette.clone());
        if colors.palette_undo.len() > PALETTE_UNDO_LIMIT {
            colors.palette_undo.remove(0);
        }
        colors.palette_redo.clear();
    }

    pub(crate) fn adopt_document_palette(&mut self, document: &mut chequer_core::document::Document) {
        let Some(embedded) = document.palette.take() else {
            return;
        };
        let palette = Palette::from(embedded);
        if palette != self.colors.palette {
            self.snapshot_palette();
            self.colors.palette = palette;
            crate::config::save_palette(&self.colors.palette);
        }
    }

    pub(crate) fn undo_palette(&mut self) {
        let Some(previous) = self.colors.palette_undo.pop() else {
            return;
        };
        let current = std::mem::replace(&mut self.colors.palette, previous);
        self.colors.palette_redo.push(current);
        crate::config::save_palette(&self.colors.palette);
    }

    pub(crate) fn redo_palette(&mut self) {
        let Some(next) = self.colors.palette_redo.pop() else {
            return;
        };
        let current = std::mem::replace(&mut self.colors.palette, next);
        self.colors.palette_undo.push(current);
        crate::config::save_palette(&self.colors.palette);
    }

    fn run_palette_action(&mut self, ctx: &Context, action: PaletteAction) {
        if matches!(action, PaletteAction::Add(_) | PaletteAction::Remove(_) | PaletteAction::Reset | PaletteAction::FromImage) {
            self.snapshot_palette();
        }
        let palette = &mut self.colors.palette;
        match action {
            PaletteAction::Add(color) => {
                palette.add(color);
            }
            PaletteAction::Remove(color) => {
                palette.remove(color);
            }
            PaletteAction::Reset => *palette = Palette::default(),
            PaletteAction::Load => return self.pick_palette_load(ctx),
            PaletteAction::Save => return self.pick_palette_save(ctx),
            PaletteAction::FromImage => return self.palette_from_image(),
        }
        crate::config::save_palette(&self.colors.palette);
    }

    fn palette_from_image(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        let tab = self.tab();
        let colors = colors_of_raster(&tab.doc.flattened(tab.doc.bounds()), MAX_COLORS);
        if colors.is_empty() {
            return self.fail(t(Text::PaletteNoColors));
        }
        let name = tab.name.clone();
        self.snapshot_palette();
        self.colors.palette = Palette { name, colors };
        crate::config::save_palette(&self.colors.palette);
    }

    pub(crate) fn load_palette(&mut self, path: &Path) {
        match Palette::load(path) {
            Ok(palette) => {
                self.snapshot_palette();
                self.colors.palette = palette;
                crate::config::save_palette(&self.colors.palette);
                self.say(t(Text::PaletteLoaded));
            }
            Err(e) => self.fail(format!("{}: {e}", path.display())),
        }
    }

    pub(crate) fn save_palette(&mut self, path: &Path) {
        match self.colors.palette.save(path).map_err(|e| e.to_string()).and_then(|()| crate::filepicker::sync_back(path)) {
            Ok(()) => self.say(t(Text::PaletteSaved)),
            Err(e) => self.fail(format!("{}: {e}", path.display())),
        }
    }

    fn pick_palette_load(&self, ctx: &Context) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let dialog = crate::filepicker::new_dialog().add_filter("Palettes", &palette::FILE_EXTENSIONS).add_filter("Images", &IMAGE_EXTENSIONS);
            if let Some(path) = dialog.pick_file() {
                let _ = tx.send(FileEvent::PaletteLoadPath(path));
                ctx.request_repaint();
            }
        });
    }

    fn pick_palette_save(&self, ctx: &Context) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        let name = format!("{}.gpl", self.colors.palette.name);
        std::thread::spawn(move || {
            if let Some(path) = crate::filepicker::new_dialog().set_file_name(name).add_filter("Palettes", &palette::FILE_EXTENSIONS).save_file() {
                let _ = tx.send(FileEvent::PaletteSavePath(path));
                ctx.request_repaint();
            }
        });
    }

    fn palette_grid(&mut self, ui: &mut Ui) {
        let swatch = self.settings.picker.swatch.pixels();
        let columns = ((ui.available_width() - PALETTE_FRAME_MARGIN) / swatch).floor().max(1.0) as usize;
        let colors = self.colors.palette.colors.clone();
        let active = self.colors.active();
        let active_rgb = [active[0], active[1], active[2]];
        let can_add = !self.colors.palette.contains(active_rgb) && colors.len() < MAX_COLORS;
        let cells = colors.len() + usize::from(can_add);
        let rows = cells.div_ceil(columns);
        let mut edit: Option<PaletteAction> = None;
        let frame = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), ui.available_height().max(swatch + PALETTE_FRAME_MARGIN)));
        ui.allocate_rect(frame, Sense::hover());
        let inner = frame.shrink(PALETTE_FRAME_MARGIN / 2.0);
        let mut picked = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let (rect, _) = ui.allocate_exact_size(vec2(columns as f32 * swatch, rows as f32 * swatch), Sense::hover());
                let grid = ui.interact(rect, ui.scope_id().with("palette-grid"), Sense::click_and_drag());
                let swatch_at = |pos: Pos2| {
                    let offset = pos - rect.min;
                    let (column, row) = ((offset.x / swatch).floor(), (offset.y / swatch).floor());
                    let in_grid = column >= 0.0 && row >= 0.0 && (column as usize) < columns;
                    in_grid.then(|| row as usize * columns + column as usize).filter(|&index| index < cells)
                };
                let pointer = ui.input(|input| input.pointer.latest_pos());
                let cell_under_pointer = pointer.and_then(swatch_at);
                let under_pointer = cell_under_pointer.filter(|&index| index < colors.len());
                let on_plus = can_add && cell_under_pointer == Some(colors.len());
                let alt = ui.input(|input| input.modifiers.alt);
                if grid.clicked() && on_plus {
                    edit = Some(PaletteAction::Add(active_rgb));
                } else if grid.clicked() && alt {
                    edit = under_pointer.map(|index| PaletteAction::Remove(colors[index]));
                }
                if grid.is_pointer_button_down_on() && !alt && !on_plus {
                    let slot = if ui.input(|input| input.pointer.button_down(PointerButton::Secondary)) { ColorSlot::Secondary } else { ColorSlot::Primary };
                    picked = under_pointer.map(|index| (slot, colors[index]));
                }
                for (index, rgb) in colors.iter().enumerate() {
                    let cell = Rect::from_min_size(rect.min + vec2((index % columns) as f32 * swatch, (index / columns) as f32 * swatch), vec2(swatch, swatch));
                    if !ui.is_rect_visible(cell) {
                        continue;
                    }
                    ui.painter().rect_filled(cell, 0.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                    if *rgb == active_rgb {
                        ui.painter().rect_stroke(cell, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
                        ui.painter().rect_stroke(cell.shrink(1.0), 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
                    }
                }
                if can_add {
                    let cell = Rect::from_min_size(rect.min + vec2((colors.len() % columns) as f32 * swatch, (colors.len() / columns) as f32 * swatch), vec2(swatch, swatch));
                    let preview = Color32::from_rgba_unmultiplied(active_rgb[0], active_rgb[1], active_rgb[2], if on_plus { u8::MAX } else { PLUS_CELL_ALPHA });
                    ui.painter().rect_filled(cell, 0.0, preview);
                    let cross = Stroke::new(1.0, Color32::from_gray(if on_plus { 0 } else { PLUS_CROSS_GRAY }));
                    let center = cell.center();
                    ui.painter().line_segment([center - vec2(PLUS_CROSS_HALF, 0.0), center + vec2(PLUS_CROSS_HALF, 0.0)], cross);
                    ui.painter().line_segment([center - vec2(0.0, PLUS_CROSS_HALF), center + vec2(0.0, PLUS_CROSS_HALF)], cross);
                }
                if on_plus && !grid.is_pointer_button_down_on() {
                    grid.on_hover_text(t(Text::PaletteAddColor));
                } else if let Some(index) = under_pointer.filter(|_| !grid.is_pointer_button_down_on()) {
                    let rgb = colors[index];
                    grid.on_hover_text(format!("#{}  ({})", to_hex([rgb[0], rgb[1], rgb[2], 0]), t(Text::PaletteAltDelete)));
                }
            }).with_fx(ui, skin::theme().window);
        });
        self.skin.paint_plate(ui.painter(), frame, &Plate::frame_only(Bevel::Sunken, skin::FIELD_RINGS));
        if ui.rect_contains_pointer(frame) {
            self.colors.palette_hovered = true;
        }
        if let Some(action) = edit {
            self.snapshot_palette();
            match action {
                PaletteAction::Add(color) => {
                    self.colors.palette.add(color);
                }
                PaletteAction::Remove(color) => {
                    self.colors.palette.remove(color);
                }
                _ => {}
            }
            crate::config::save_palette(&self.colors.palette);
        }
        if let Some((slot, rgb)) = picked {
            let alpha = if slot == ColorSlot::Primary { self.colors.primary[3] } else { self.colors.secondary[3] };
            match slot {
                ColorSlot::Primary => self.colors.primary = [rgb[0], rgb[1], rgb[2], alpha],
                ColorSlot::Secondary => self.colors.secondary = [rgb[0], rgb[1], rgb[2], alpha],
            }
            self.colors.editing = slot;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaletteAction {
    Add(Rgb),
    Remove(Rgb),
    Load,
    Save,
    FromImage,
    Reset,
}

fn lit_value(hsv: Hsv) -> Hsv {
    if hsv.v < MIN_VISIBLE_VALUE { Hsv { v: 1.0, ..hsv } } else { hsv }
}

fn lit_saturation(hsv: Hsv) -> Hsv {
    if hsv.s < MIN_VISIBLE_SATURATION { Hsv { s: 1.0, ..hsv } } else { hsv }
}

fn hsv_color(hsv: Hsv) -> Color32 {
    let [r, g, b] = hsv_to_rgb(hsv);
    Color32::from_rgb(r, g, b)
}

fn make_square(hue: f32) -> ColorImage {
    let last = (WHEEL_SIZE - 1) as f32;
    let mut pixels = Vec::with_capacity(WHEEL_SIZE * WHEEL_SIZE);
    for y in 0..WHEEL_SIZE {
        for x in 0..WHEEL_SIZE {
            let rgb = hsv_to_rgb(Hsv { h: hue, s: x as f32 / last, v: 1.0 - y as f32 / last });
            pixels.push(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        }
    }
    ColorImage::new([WHEEL_SIZE, WHEEL_SIZE], pixels)
}

fn make_wheel(value: f32) -> ColorImage {
    let size = WHEEL_SIZE as f32;
    let center = size / 2.0;
    let mut pixels = Vec::with_capacity(WHEEL_SIZE * WHEEL_SIZE);
    for y in 0..WHEEL_SIZE {
        for x in 0..WHEEL_SIZE {
            let (dx, dy) = (x as f32 + 0.5 - center, y as f32 + 0.5 - center);
            let distance = dx.hypot(dy) / center;
            if distance > 1.0 {
                pixels.push(Color32::TRANSPARENT);
                continue;
            }
            let rgb = hsv_to_rgb(Hsv { h: dy.atan2(dx).to_degrees().rem_euclid(360.0), s: distance, v: value });
            pixels.push(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        }
    }
    ColorImage::new([WHEEL_SIZE, WHEEL_SIZE], pixels)
}

pub fn history_icon(kind: HistoryKind) -> Icon {
    use HistoryKind::*;
    match kind {
        NewImage => Icon::New,
        Pencil => Icon::Pencil,
        Brush => Icon::Brush,
        Eraser => Icon::Eraser,
        CloneStamp => Icon::CloneStamp,
        Fill => Icon::Bucket,
        Gradient => Icon::Gradient,
        Line => Icon::Line,
        Rectangle | RoundedRectangle | Ellipse | FreeformShape | Shape => Icon::Shapes,
        TextInsert => Icon::TextTool,
        SelectRectangle | SelectAll => Icon::Rect,
        SelectEllipse => Icon::Ellipse,
        SelectLasso => Icon::Lasso,
        SelectWand => Icon::Wand,
        Deselect => Icon::Deselect,
        InvertSelection => Icon::SelectionInvert,
        PasteSelection => Icon::SelectionAreaPaste,
        MoveSelection | ModifySelection => Icon::MoveSelection,
        MoveSelectedPixels => Icon::MovePixels,
        Paste => Icon::Paste,
        Cut => Icon::Cut,
        Delete => Icon::Delete,
        FillSelection => Icon::Brush,
        AddLayer | DuplicateLayer | DeleteLayer | MergeLayerDown | MoveLayerUp | MoveLayerDown | MaskLayer | UnmaskLayer | LayerProperties | Flatten => Icon::LayersWindow,
        AddFrame => Icon::EmptyFrame,
        DuplicateFrame => Icon::FilmAdd,
        DeleteFrame => Icon::FilmRemove,
        MoveFrame => Icon::NextFrame,
        ReverseFrames => Icon::FlipHorizontal,
        ClearCels => Icon::Eraser,
        PasteCels => Icon::Paste,
        FrameDuration => Icon::Timeline,
        CropToSelection => Icon::Crop,
        ResizeImage => Icon::ResizeImage,
        ResizeCanvas => Icon::CanvasSize,
        FlipHorizontal | FlipVertical | OffsetHalf => Icon::Swap,
        RotateClockwise => Icon::RotateCw,
        RotateCounterClockwise => Icon::RotateCcw,
        RotateHalfTurn => Icon::Refresh,
        InvertColors => Icon::EffectInvert,
        BlackAndWhite => Icon::EffectMonochrome,
        Effect(effect) => crate::effects_ui::effect_icon(effect).unwrap_or(Icon::Contrast),
    }
}

pub fn history_text(kind: HistoryKind) -> Text {
    use HistoryKind::*;
    match kind {
        NewImage => Text::HNew,
        Pencil => Text::HPencil,
        Brush => Text::HBrush,
        Eraser => Text::HEraser,
        CloneStamp => Text::HCloneStamp,
        Fill => Text::HFill,
        Gradient => Text::HGradient,
        Line => Text::HLine,
        Rectangle => Text::HRectangle,
        RoundedRectangle => Text::HRounded,
        Ellipse => Text::HEllipse,
        FreeformShape => Text::HFreeform,
        Shape => Text::HShape,
        TextInsert => Text::HText,
        SelectRectangle => Text::HSelectRect,
        SelectEllipse => Text::HSelectEllipse,
        SelectLasso => Text::HSelectLasso,
        SelectWand => Text::HSelectWand,
        SelectAll => Text::HSelectAll,
        Deselect => Text::HDeselect,
        InvertSelection => Text::HInvertSelection,
        PasteSelection => Text::HPasteSelection,
        MoveSelection => Text::HMoveSelection,
        ModifySelection => Text::HModifySelection,
        MoveSelectedPixels => Text::HMovePixels,
        Paste => Text::HPaste,
        Cut => Text::HCut,
        Delete => Text::HErase,
        FillSelection => Text::HFillSelection,
        AddLayer => Text::HAddLayer,
        DuplicateLayer => Text::HDuplicateLayer,
        DeleteLayer => Text::HDeleteLayer,
        MergeLayerDown => Text::HMergeLayer,
        MoveLayerUp => Text::HMoveLayerUp,
        MoveLayerDown => Text::HMoveLayerDown,
        MaskLayer => Text::HMaskLayer,
        UnmaskLayer => Text::HUnmaskLayer,
        LayerProperties => Text::HLayerProps,
        CropToSelection => Text::HCrop,
        ResizeImage => Text::HResize,
        ResizeCanvas => Text::HCanvasSize,
        FlipHorizontal => Text::HFlipH,
        FlipVertical => Text::HFlipV,
        RotateClockwise => Text::HRotateCw,
        RotateCounterClockwise => Text::HRotateCcw,
        RotateHalfTurn => Text::HRotate180,
        OffsetHalf => Text::HOffsetHalf,
        Flatten => Text::HFlatten,
        AddFrame => Text::HAddFrame,
        DuplicateFrame => Text::HDuplicateFrame,
        DeleteFrame => Text::HDeleteFrame,
        MoveFrame => Text::HMoveFrame,
        FrameDuration => Text::HFrameDuration,
        ClearCels => Text::HClearCels,
        PasteCels => Text::HPasteCels,
        ReverseFrames => Text::HReverseFrames,
        InvertColors => Text::HInvertColors,
        BlackAndWhite => Text::HBlackWhite,
        Effect(_) => unreachable!("название эффекта берёт history_label"),
    }
}

pub fn history_label(kind: HistoryKind) -> &'static str {
    match kind {
        HistoryKind::Effect(effect) => crate::effects_ui::effect_label(effect),
        other => t(history_text(other)),
    }
}

#[derive(Clone)]
struct LayerWatch {
    tab: usize,
    active: usize,
    names: Vec<String>,
}

fn paint_closing_layer(ui: &mut Ui, skin: &Skin, motion: Option<(Slot, f32)>, name: Option<&str>, follower: Option<usize>) {
    let (Some((slot, extent)), Some(name)) = (motion, name) else {
        return;
    };
    if slot.slide != Slide::Out || slot.index.checked_sub(1) != follower {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), extent.clamp(0.0, LAYER_ROW_HEIGHT)), Sense::hover());
    let outer_clip = ui.clip_rect();
    ui.set_clip_rect(outer_clip.intersect(rect));
    paint_layer_row(ui, skin, full_row(rect), &LayerRowView { name, collapsed: false, children: 0, visible: true, thumb: None, active: false, hovered: false, masked: false, is_mask: false });
    ui.set_clip_rect(outer_clip);
}

#[derive(Clone, Copy)]
struct HeldLayer {
    index: usize,
    grab: f32,
    grab_x: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DropIntent {
    Reorder(usize),
    Nest(usize),
    Unnest,
}

fn drop_intent(held: usize, target: usize, drag_x: f32, masked: &[bool]) -> DropIntent {
    let nestable_target = target != held && !masked[target];
    if drag_x >= LAYER_NEST_DRAG_DISTANCE && nestable_target {
        DropIntent::Nest(target)
    } else if drag_x <= -LAYER_NEST_DRAG_DISTANCE && masked[held] {
        DropIntent::Unnest
    } else {
        DropIntent::Reorder(target)
    }
}

struct RowSlot {
    index: usize,
    rect: Rect,
    hovered: bool,
}

struct LayerRowView<'a> {
    name: &'a str,
    collapsed: bool,
    children: usize,
    visible: bool,
    thumb: Option<egui::TextureId>,
    active: bool,
    hovered: bool,
    masked: bool,
    is_mask: bool,
}

fn full_row(slot: Rect) -> Rect {
    Rect::from_min_size(Pos2::new(slot.min.x, slot.max.y - LAYER_ROW_HEIGHT), vec2(slot.width(), LAYER_ROW_HEIGHT))
}

fn layer_fold_rect(row: Rect) -> Rect {
    Rect::from_center_size(row.right_center() - vec2(LAYER_MASK_ICON_OFFSET, 0.0), vec2(16.0, 16.0))
}

fn layer_eye_rect(row: Rect) -> Rect {
    Rect::from_center_size(row.right_center() - vec2(14.0, 0.0), vec2(crate::icons::icon_side(), crate::icons::icon_side()))
}

fn slot_of(slots: &[RowSlot], index: usize) -> Option<Rect> {
    slots.iter().find(|slot| slot.index == index).map(|slot| slot.rect)
}

fn nearest_row(slots: &[RowSlot], y: f32) -> usize {
    slots.iter().min_by(|a, b| distance_to_row(y, a.rect).total_cmp(&distance_to_row(y, b.rect))).map_or(0, |slot| slot.index)
}

fn slide_target(held: Option<usize>, target: Option<usize>, index: usize) -> f32 {
    let (Some(from), Some(to)) = (held, target) else {
        return 0.0;
    };
    if from < index && index <= to {
        LAYER_ROW_HEIGHT
    } else if to <= index && index < from {
        -LAYER_ROW_HEIGHT
    } else {
        0.0
    }
}

fn paint_layer_row(ui: &mut Ui, skin: &Skin, rect: Rect, view: &LayerRowView) {
    if view.active {
        ui.painter().rect_filled(rect, 0.0, skin::theme().highlight);
    } else if view.hovered {
        ui.painter().rect_filled(rect, 0.0, skin::theme().list_hover);
    }
    let indent = if view.masked { LAYER_NEST_INDENT } else { 0.0 };
    let thumb_rect = Rect::from_center_size(rect.left_center() + vec2(LAYER_THUMB_CENTER_X + indent, 0.0), vec2(LAYER_THUMB_HALF * 2.0, LAYER_THUMB_HALF * 2.0));
    if view.masked {
        let stem_x = rect.left() + LAYER_THUMB_CENTER_X;
        let connector = Stroke::new(1.0, if view.active { skin::theme().highlight_text } else { skin::theme().ink });
        ui.painter().line_segment([Pos2::new(stem_x, rect.top()), Pos2::new(stem_x, rect.center().y)], connector);
        ui.painter().line_segment([Pos2::new(stem_x, rect.center().y), Pos2::new(thumb_rect.left() - 1.0, rect.center().y)], connector);
    }
    skin.paint_checker(&ui.painter().with_clip_rect(thumb_rect), thumb_rect, thumb_rect.min);
    if let Some(id) = view.thumb {
        ui.painter().image(id, thumb_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    ui.painter().rect_stroke(thumb_rect.expand(1.0), 0.0, Stroke::new(1.0, skin::theme().line), egui::StrokeKind::Outside);
    let color = if view.active { skin::theme().highlight_text } else { skin::theme().ink };
    let label = if view.is_mask && view.collapsed { format!("{} ({})", view.name, view.children) } else { view.name.to_owned() };
    skin::text_at(ui.painter(), rect.left_center() + vec2(LAYER_NAME_LEFT + indent, 0.0), Align2::LEFT_CENTER, &label, Font::Body, color);
    let tint = if view.visible { Color32::WHITE } else { Color32::from_white_alpha(50) };
    skin.icons.image(Icon::Eye).tint(tint).paint_at(ui, layer_eye_rect(rect));
    if view.is_mask {
        let badge = Rect::from_min_size(thumb_rect.right_bottom() - vec2(LAYER_MASK_BADGE_SIZE - 3.0, LAYER_MASK_BADGE_SIZE - 3.0), vec2(LAYER_MASK_BADGE_SIZE, LAYER_MASK_BADGE_SIZE));
        ui.painter().rect_filled(badge, 0.0, skin::theme().field);
        ui.painter().rect_stroke(badge, 0.0, Stroke::new(1.0, skin::theme().line), egui::StrokeKind::Inside);
        skin.icons.image(Icon::Contrast).tint(tint).paint_at(ui, badge.shrink(1.0));
        let center = layer_fold_rect(rect).center();
        let corners = if view.collapsed { [center + vec2(-2.0, -4.0), center + vec2(-2.0, 4.0), center + vec2(3.0, 0.0)] } else { [center + vec2(-4.0, -2.0), center + vec2(4.0, -2.0), center + vec2(0.0, 3.0)] };
        ui.painter().add(egui::Shape::convex_polygon(corners.to_vec(), color, Stroke::NONE));
    }
}

fn paint_drop_marker(ui: &Ui, row: Rect, nest: bool) {
    let stroke = Stroke::new(LAYER_DROP_MARKER_WIDTH, skin::theme().ink);
    let (y, start) = if nest { (row.bottom(), row.left() + LAYER_NEST_INDENT) } else { (row.top(), row.left()) };
    let (tip_x, arm_x) = if nest { (start + LAYER_DROP_ARROW, start) } else { (start, start + LAYER_DROP_ARROW) };
    let tip = Pos2::new(tip_x, y);
    let painter = ui.painter();
    painter.line_segment([tip, Pos2::new(row.right(), y)], stroke);
    painter.line_segment([Pos2::new(arm_x, y - LAYER_DROP_ARROW), tip], stroke);
    painter.line_segment([Pos2::new(arm_x, y + LAYER_DROP_ARROW), tip], stroke);
}

fn paint_held_layer(ui: &mut Ui, skin: &Skin, rect: Rect, view: &LayerRowView) {
    let pixel = 1.0 / skin::grid_ppp(ui.painter().pixels_per_point());
    ui.painter().rect_filled(rect.translate(vec2(HELD_SHADOW_PIXELS, HELD_SHADOW_PIXELS) * pixel), 0.0, Color32::from_black_alpha(HELD_SHADOW_ALPHA));
    ui.painter().rect_filled(rect, 0.0, skin::theme().field);
    paint_layer_row(ui, skin, rect, view);
    ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, skin::theme().line), egui::StrokeKind::Inside);
}

fn distance_to_row(y: f32, row: Rect) -> f32 {
    (row.top() - y).max(y - row.bottom()).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_is_lit_for_display_but_colors_are_untouched() {
        let black = Hsv { h: 40.0, s: 0.7, v: 0.0 };
        assert_eq!(lit_value(black), Hsv { h: 40.0, s: 0.7, v: 1.0 });
        let dark = Hsv { h: 40.0, s: 0.7, v: 0.3 };
        assert_eq!(lit_value(dark), dark);
    }

    #[test]
    fn gray_gets_full_saturation_for_the_hue_track_only() {
        let gray = Hsv { h: 0.0, s: 0.0, v: 0.5 };
        assert_eq!(lit_saturation(gray).s, 1.0);
        let tinted = Hsv { h: 0.0, s: 0.4, v: 0.5 };
        assert_eq!(lit_saturation(tinted), tinted);
    }

    #[test]
    fn hue_stops_cover_the_whole_circle_in_sixty_degree_steps() {
        assert_eq!(HUE_STOPS as f32 * DEGREES_PER_HUE_STOP, 360.0);
    }
}
