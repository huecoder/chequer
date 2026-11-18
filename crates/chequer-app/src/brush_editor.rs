use std::sync::Arc;

use crate::device::AUTO_FOCUS_TEXT;
use chequer_core::brush::{BrushEngine, BrushSettings, BrushTip, MAX_TIP_VARIANTS, MaskTip, TIP_WEIGHT_RANGE, TipVariant, build_tip, tip_generator_ids};
use eframe::egui::{self, Align, Align2, Context, Layout, Margin, Rect, ScrollArea, Sense, scroll_area::ScrollBarVisibility, Stroke, TextureHandle, TextureOptions, Ui, UiBuilder, vec2};

use crate::metrics::PRESSED_OFFSET;
use crate::brush_dialog::BrushDialog;
use crate::brush_list::brush_label;
use crate::brush_preview::{paint_texture, preview_texture};
use crate::brush_presets::SETTINGS_PRESETS;
use crate::brush_settings::{BrushTab, BrushTargets, PERCENT, framed_section, tab_settings, visible_tabs};
use crate::i18n::{Text, t};
use crate::layer_editor::{LayerPick, layers_section};
use crate::texture_editor::TextureSlot;
use crate::tip_generator_editor::{default_spec, generator_label, generator_rows};
use crate::icons::{Icon, icon_side};
use crate::skin::{self, Font};
use crate::widgets::{tab_bar, DragSpec, ICON_BUTTON_SIZE, bevel_over, combo_box, drag_value, fixed_box, icon_button, icon_button_sized, raised_button_plate, text_button, text_button_sized};
use crate::scroll_fx::ScrollFx;

pub(crate) const PANE_WIDTH: f32 = 450.0;
const HEADER_HEIGHT: f32 = 28.0;
const EDIT_BUTTON_WIDTH: f32 = 96.0;
const PREVIEW_HEIGHT: f32 = 96.0;
const PREVIEW_BRUSH_SIZE: f32 = 26.0;
const TOOLBAR_HEIGHT: f32 = 28.0;
const PRESET_COMBO_WIDTH: f32 = 110.0;
const ENGINE_COMBO_WIDTH: f32 = 90.0;
const BRUSH_ENGINES: [BrushEngine; 2] = [BrushEngine::Stamp, BrushEngine::Ribbon];
const FRAME_STROKE: f32 = 1.0;
const FRAME_MARGIN: i8 = 4;
const TIP_TILE_SIZE: f32 = 56.0;
const TIP_TILE_GAP: f32 = 6.0;
const GENERATOR_COMBO_WIDTH: f32 = 200.0;
const TIP_CLOSE_SIZE: f32 = 16.0;
const TIP_CLOSE_INSET: f32 = 1.0;
const TIP_WEIGHT_HEIGHT: f32 = 20.0;
const TIP_ADD_ICON_SCALE: f32 = 2.0;
const TIP_GHOST_ALPHA: u8 = 200;
const TABS_ID: &str = "brush-tabs";
const TIP_DRAG_KEY: &str = "brush-tip-drag";

pub(crate) struct EditSession {
    pub target: Option<usize>,
    pub name: String,
    pub tips: Vec<TipVariant>,
    pub thumbs: Vec<TextureHandle>,
    pub focus_name: bool,
    pub initial_name: String,
    pub tips_changed: bool,
    pub selected: Option<usize>,
}

impl EditSession {
    pub fn new(target: Option<usize>, name: String, tips: Vec<TipVariant>) -> Self {
        Self { target, initial_name: name.clone(), name, tips, thumbs: Vec::new(), focus_name: true, tips_changed: false, selected: None }
    }
}

pub(crate) enum EditorEvent {
    BeginEdit,
    SaveEdit,
    CancelEdit,
    ResetBuiltin,
    ToggleFavorite,
    AddImages,
    PickTexture(TextureSlot),
    PickLayer(LayerPick),
}

#[derive(Default)]
struct TipsOutcome {
    changed: bool,
    add_images: bool,
}

pub(crate) fn brush_editor(ui: &mut Ui, skin: &skin::Skin, d: &mut BrushDialog, favorite: Option<bool>, resettable: bool) -> Option<EditorEvent> {
    let mut event = None;
    header(ui, skin, d, favorite, &mut event);
    preview(ui, skin, d);
    toolbar(ui, skin, d, resettable, &mut event);
    settings_area(ui, skin, d, &mut event);
    event
}

fn header(ui: &mut Ui, skin: &skin::Skin, d: &mut BrushDialog, favorite: Option<bool>, event: &mut Option<EditorEvent>) {
    fixed_box(ui, vec2(ui.available_width(), HEADER_HEIGHT), Layout::left_to_right(Align::Center), |ui| {
        if let Some(edit) = &mut d.edit {
            let save_enabled = !edit.name.trim().is_empty();
            let name_width = ui.available_width() - 2.0 * (EDIT_BUTTON_WIDTH + ui.spacing().item_spacing.x);
            let field = ui.add(egui::TextEdit::singleline(&mut edit.name).hint_text(t(Text::BrushNameHint)).desired_width(name_width));
            if edit.focus_name {
                if AUTO_FOCUS_TEXT {
                    field.request_focus();
                }
                edit.focus_name = AUTO_FOCUS_TEXT && !field.has_focus();
            }
            bevel_over(ui, skin, field.rect, skin::Bevel::Sunken);
            let size = vec2(EDIT_BUTTON_WIDTH, ICON_BUTTON_SIZE);
            if ui.add_enabled_ui(save_enabled, |ui| text_button_sized(ui, skin, t(Text::BrushSave), size)).inner.clicked() {
                *event = Some(EditorEvent::SaveEdit);
            }
            if text_button_sized(ui, skin, t(Text::Cancel), size).clicked() {
                *event = Some(EditorEvent::CancelEdit);
            }
            return;
        }
        let name_width = ui.available_width() - 2.0 * (ICON_BUTTON_SIZE + ui.spacing().item_spacing.x);
        let (name_rect, _) = ui.allocate_exact_size(vec2(name_width, HEADER_HEIGHT), Sense::hover());
        let painter = ui.painter().with_clip_rect(name_rect);
        skin::text_at(&painter, name_rect.left_center(), Align2::LEFT_CENTER, &brush_label(&d.working), Font::Title, skin::theme().ink);
        if icon_button(ui, skin, Icon::Pencil, false, t(Text::BrushEdit)).clicked() {
            *event = Some(EditorEvent::BeginEdit);
        }
        let (star, star_hint) = if favorite == Some(true) { (Icon::Star, Text::BrushFavoriteRemove) } else { (Icon::StarEmpty, Text::BrushFavoriteAdd) };
        let star_button = ui.add_enabled_ui(favorite.is_some(), |ui| icon_button(ui, skin, star, false, t(star_hint)));
        if star_button.inner.clicked() {
            *event = Some(EditorEvent::ToggleFavorite);
        }
    });
}

fn preview(ui: &mut Ui, skin: &skin::Skin, d: &mut BrushDialog) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PREVIEW_HEIGHT), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, skin::theme().field);
    let ctx = ui.ctx().clone();
    let texture = d.big_preview.get_or_insert_with(|| preview_texture(&ctx, "brush-big", &Arc::new(d.working.clone()), PREVIEW_BRUSH_SIZE, rect.size()));
    paint_texture(ui, texture, rect);
    bevel_over(ui, skin, rect, skin::Bevel::Sunken);
}

fn engine_text(engine: BrushEngine) -> Text {
    match engine {
        BrushEngine::Stamp => Text::BrushEngineStamp,
        BrushEngine::Ribbon => Text::BrushEngineRibbon,
    }
}

fn toolbar(ui: &mut Ui, skin: &skin::Skin, d: &mut BrushDialog, resettable: bool, event: &mut Option<EditorEvent>) {
    fixed_box(ui, vec2(ui.available_width(), TOOLBAR_HEIGHT), Layout::left_to_right(Align::Center), |ui| {
        let reset = ui.add_enabled_ui(resettable && d.edit.is_none(), |ui| text_button(ui, skin, t(Text::BrushReset)));
        if reset.inner.on_hover_text(t(Text::BrushResetHint)).clicked() {
            *event = Some(EditorEvent::ResetBuiltin);
        }
        let engine = d.working.settings.engine;
        let mut picked_engine = None;
        combo_box(ui, skin, "brush-engine", ENGINE_COMBO_WIDTH, t(engine_text(engine)), |ui| {
            for option in BRUSH_ENGINES {
                if ui.selectable_label(option == engine, t(engine_text(option))).clicked() {
                    picked_engine = Some(option);
                }
            }
        })
        .on_hover_text(t(Text::BrushEngineHint));
        if let Some(option) = picked_engine {
            d.replace_settings(BrushSettings { engine: option, ..d.working.settings.clone() }.clamped());
        }
        let mut picked = None;
        combo_box(ui, skin, "brush-presets", PRESET_COMBO_WIDTH, t(Text::BrushPresets), |ui| {
            for preset in SETTINGS_PRESETS {
                if ui.selectable_label(false, t(preset.label())).clicked() {
                    picked = Some(preset);
                }
            }
        });
        if let Some(preset) = picked {
            d.replace_settings(preset.settings());
        }
    });
}

fn settings_area(ui: &mut Ui, skin: &skin::Skin, d: &mut BrushDialog, event: &mut Option<EditorEvent>) {
    let editing = d.edit.is_some();
    let ribbon = d.working.settings.engine == BrushEngine::Ribbon;
    let soft_edge = matches!(d.working.tip, BrushTip::Round) && !ribbon;
    let settings_before = d.working.settings.clone();
    let tabs = visible_tabs(&d.working.settings, soft_edge);
    if !tabs.iter().any(|(tab, _)| *tab == d.tab) {
        d.tab = tabs.first().map_or(d.tab, |(tab, _)| *tab);
    }
    tab_bar(ui, ui.scope_id().with(TABS_ID), &mut d.tab, &tabs);
    let tab = d.tab;
    let height = ui.available_height() - 2.0 * (FRAME_STROKE + f32::from(FRAME_MARGIN));
    let ctx = ui.ctx().clone();
    let (edit, settings, textures) = (&mut d.edit, &mut d.working.settings, &mut d.working.textures);
    let mut pick_texture = None;
    let mut pick_layer = None;
    let framed = egui::Frame::NONE.stroke(Stroke::new(FRAME_STROKE, skin::theme().line_soft)).inner_margin(Margin::same(FRAME_MARGIN)).show(ui, |ui| {
        ScrollArea::vertical().scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible).id_salt(("brush-editor-scroll", editing, tab)).max_height(height).min_scrolled_height(height).auto_shrink([false, false]).show(ui, |ui| {
            let tips = edit.as_mut().filter(|_| !ribbon && tab == BrushTab::Shape).map(|edit| tips_section(ui, skin, &ctx, edit)).unwrap_or_default();
            let changed = if tab == BrushTab::Layers {
                let layers = layers_section(ui, skin, &mut textures.layers);
                pick_layer = layers.pick;
                layers.changed
            } else {
                let mut targets = BrushTargets { settings, textures, pick_texture: &mut pick_texture };
                tab_settings(ui, skin, tab, &mut targets, soft_edge)
            };
            (changed, tips)
        }).with_fx(ui, skin::theme().window)
    });
    bevel_over(ui, skin, framed.response.rect, skin::Bevel::Sunken);
    let (changed, tips) = framed.inner.inner;
    if tips.changed {
        d.sync_tips();
    }
    if tips.add_images {
        *event = Some(EditorEvent::AddImages);
    }
    if let Some(slot) = pick_texture {
        *event = Some(EditorEvent::PickTexture(slot));
    }
    if let Some(pick) = pick_layer {
        *event = Some(EditorEvent::PickLayer(pick));
    }
    if changed {
        d.working.settings = d.working.settings.clone().clamped();
        d.big_preview = None;
        if d.working.settings != settings_before {
            d.history.begin_step(settings_before);
        }
    }
    let pointer_down = ui.input(|input| input.pointer.any_down());
    d.history.seal_when_idle(pointer_down);
}

fn tip_thumbnail(ctx: &Context, mask: &MaskTip) -> TextureHandle {
    let ink = skin::theme().ink;
    let side = (TIP_TILE_SIZE * ctx.pixels_per_point()).round() as usize;
    let texel_px = side as f32 / mask.width().max(mask.height()) as f32;
    let (half_width, half_height) = (mask.width() as f32 / 2.0, mask.height() as f32 / 2.0);
    let alpha_at = |column: usize, row: usize| {
        let tip_x = (column as f32 + 0.5 - side as f32 / 2.0) / texel_px + half_width;
        let tip_y = (row as f32 + 0.5 - side as f32 / 2.0) / texel_px + half_height;
        let inside = (0.0..mask.width() as f32).contains(&tip_x) && (0.0..mask.height() as f32).contains(&tip_y);
        if inside { (mask.sample(tip_x, tip_y, texel_px) * f32::from(u8::MAX)).round() as u8 } else { 0 }
    };
    let rgba: Vec<u8> = (0..side * side).flat_map(|index| [ink.r(), ink.g(), ink.b(), alpha_at(index % side, index / side)]).collect();
    ctx.load_texture("brush-tip-thumb", egui::ColorImage::from_rgba_unmultiplied([side, side], &rgba), TextureOptions::LINEAR)
}

fn tips_section(ui: &mut Ui, skin: &skin::Skin, ctx: &Context, edit: &mut EditSession) -> TipsOutcome {
    let mut outcome = TipsOutcome::default();
    framed_section(ui, t(Text::BrushSectionTips), true, |ui| {
        let count = edit.tips.len();
        let several = count > 1;
        let total: f32 = edit.tips.iter().map(|variant| variant.weight).sum();
        let cell_height = if several { TIP_TILE_SIZE + TIP_TILE_GAP + TIP_WEIGHT_HEIGHT } else { TIP_TILE_SIZE };
        let (pitch_x, pitch_y) = (TIP_TILE_SIZE + TIP_TILE_GAP, cell_height + TIP_TILE_GAP);
        let columns = (((ui.available_width() + TIP_TILE_GAP) / pitch_x) as usize).max(1);
        let rows = (count + 1).div_ceil(columns);
        let (grid, _) = ui.allocate_exact_size(vec2(ui.available_width(), rows as f32 * pitch_y - TIP_TILE_GAP), Sense::hover());
        let cell = |slot: usize| Rect::from_min_size(grid.min + vec2((slot % columns) as f32 * pitch_x, (slot / columns) as f32 * pitch_y), vec2(TIP_TILE_SIZE, cell_height));

        let drag_id = ui.id().with(TIP_DRAG_KEY);
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let pointer_down = ui.input(|input| input.pointer.any_down());
        let mut dragging: Option<usize> = ui.data(|data| data.get_temp(drag_id)).filter(|&index| index < count && pointer_down);
        let target = dragging.zip(pointer).map(|(_, pos)| {
            let column = (((pos.x - grid.min.x) / pitch_x).floor().max(0.0) as usize).min(columns - 1);
            let row = ((pos.y - grid.min.y) / pitch_y).floor().max(0.0) as usize;
            (row * columns + column).min(count - 1)
        });
        let mut order: Vec<usize> = (0..count).collect();
        if let Some((from, to)) = dragging.zip(target) {
            let moved = order.remove(from);
            order.insert(to, moved);
        }

        let mut removed = None;
        let mut reorder = None;
        for (slot, &index) in order.iter().enumerate() {
            if edit.thumbs.len() <= index {
                edit.thumbs.resize_with(index + 1, || tip_thumbnail(ctx, &edit.tips[index].mask));
            }
            let tile = Rect::from_min_size(cell(slot).min, vec2(TIP_TILE_SIZE, TIP_TILE_SIZE));
            let response = ui.interact(tile, drag_id.with(index), Sense::click_and_drag());
            if response.clicked() {
                edit.selected = Some(index);
            }
            if response.drag_started() {
                ui.data_mut(|data| data.insert_temp(drag_id, index));
                dragging = Some(index);
            }
            if response.drag_stopped() {
                reorder = dragging.zip(target);
                ui.data_mut(|data| data.remove_temp::<usize>(drag_id));
            }
            if response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            ui.painter().rect_filled(tile, 0.0, skin::theme().field);
            if dragging == Some(index) {
                ui.painter().rect_stroke(tile, 0.0, Stroke::new(1.0, skin::theme().highlight), egui::StrokeKind::Inside);
            } else {
                paint_texture(ui, &edit.thumbs[index], tile);
                let close = Rect::from_min_size(tile.right_top() + vec2(-TIP_CLOSE_SIZE - TIP_CLOSE_INSET, TIP_CLOSE_INSET), vec2(TIP_CLOSE_SIZE, TIP_CLOSE_SIZE));
                let mut corner = ui.new_child(UiBuilder::new().max_rect(close));
                if icon_button_sized(&mut corner, skin, Icon::Close, false, true, vec2(TIP_CLOSE_SIZE, TIP_CLOSE_SIZE), t(Text::BrushTipRemove)).clicked() {
                    removed = Some(index);
                }
            }
            bevel_over(ui, skin, tile, skin::Bevel::Sunken);
            if edit.selected == Some(index) && dragging != Some(index) {
                ui.painter().rect_stroke(tile, 0.0, Stroke::new(1.0, skin::theme().highlight), egui::StrokeKind::Inside);
            }
            if several {
                let weight_rect = Rect::from_min_size(tile.left_bottom() + vec2(0.0, TIP_TILE_GAP), vec2(TIP_TILE_SIZE, TIP_WEIGHT_HEIGHT));
                let mut weight_ui = ui.new_child(UiBuilder::new().max_rect(weight_rect));
                let weight = &mut edit.tips[index].weight;
                let mut percent = *weight * PERCENT;
                let spec = DragSpec { speed: 0.5, suffix: "%", max_decimals: Some(0), ..DragSpec::default() };
                let range = TIP_WEIGHT_RANGE.0 * PERCENT..=TIP_WEIGHT_RANGE.1 * PERCENT;
                let field = drag_value(&mut weight_ui, skin, &mut percent, range, vec2(TIP_TILE_SIZE, TIP_WEIGHT_HEIGHT), &spec);
                if field.changed() {
                    *weight = percent / PERCENT;
                    outcome.changed = true;
                }
                field.on_hover_text(format!("{} {:.0}%", t(Text::BrushTipShare), *weight / total * PERCENT));
            }
        }
        if let Some((index, pos)) = dragging.zip(pointer) {
            let ghost = Rect::from_center_size(pos, vec2(TIP_TILE_SIZE, TIP_TILE_SIZE));
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, drag_id));
            painter.rect_filled(ghost, 0.0, skin::theme().field);
            painter.image(edit.thumbs[index].id(), ghost, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::from_white_alpha(TIP_GHOST_ALPHA));
        }
        let mut add_cell = ui.new_child(UiBuilder::new().max_rect(cell(count)));
        outcome.add_images = add_tile(&mut add_cell, skin, count < MAX_TIP_VARIANTS);
        if let Some(index) = removed {
            edit.tips.remove(index);
            edit.thumbs.clear();
            edit.selected = None;
            outcome.changed = true;
        }
        if let Some((from, to)) = reorder.filter(|(from, to)| from != to) {
            let moved = edit.tips.remove(from);
            edit.tips.insert(to, moved);
            edit.thumbs.clear();
            edit.selected = Some(to);
            outcome.changed = true;
        }
        ui.add_enabled_ui(edit.tips.len() < MAX_TIP_VARIANTS, |ui| {
            combo_box(ui, skin, "brush-generators", GENERATOR_COMBO_WIDTH, t(Text::BrushGenerate), |ui| {
                for id in tip_generator_ids() {
                    if ui.selectable_label(false, generator_label(id)).clicked()
                        && let Ok(mask) = build_tip(&default_spec(id))
                    {
                        edit.tips.push(TipVariant::new(mask));
                        edit.selected = Some(edit.tips.len() - 1);
                        edit.thumbs.clear();
                        outcome.changed = true;
                    }
                }
            });
        });
        if let Some(tip) = edit.selected.and_then(|index| edit.tips.get_mut(index)) {
            if generator_rows(ui, skin, tip) {
                edit.thumbs.clear();
                outcome.changed = true;
            }
        } else if edit.tips.iter().any(|tip| tip.mask.spec().is_some()) {
            ui.label(t(Text::BrushGeneratedTipHint));
        }
    });
    outcome
}

fn add_tile(ui: &mut Ui, skin: &skin::Skin, enabled: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(TIP_TILE_SIZE, TIP_TILE_SIZE), if enabled { Sense::click() } else { Sense::hover() });
    let plate = raised_button_plate(&response, false, enabled);
    skin.paint_plate(ui.painter(), rect, &plate);
    let pressed_offset = if plate.bevel == Some(skin::Bevel::Sunken) { PRESSED_OFFSET } else { egui::Vec2::ZERO };
    let icon_rect = Rect::from_center_size(rect.center(), vec2(icon_side(), icon_side()) * TIP_ADD_ICON_SCALE).translate(pressed_offset);
    skin.icons.image_sized(Icon::Add, icon_rect.width()).paint_at(ui, icon_rect);
    response.on_hover_text(t(Text::BrushAddImages)).clicked()
}
