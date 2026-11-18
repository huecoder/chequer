use std::collections::HashMap;

use crate::device::AUTO_FOCUS_TEXT;
use chequer_core::brush::Brush;
use eframe::egui::{self, Align, Align2, Color32, Context, Layout, Rect, Response, ScrollArea, Sense, scroll_area::ScrollBarVisibility, Stroke, TextureHandle, Ui, pos2, vec2};

use crate::anim::{Glide, LayerMove, apply_layer_move, snap_to_pixels};
use crate::brush_library::BrushEntry;
use crate::brush_preview::{paint_texture, preview_texture};
use crate::i18n::{Text, lang_code, t};
use crate::icons::Icon;
use crate::skin::{self, Font};
use crate::widgets::{ICON_BUTTON_SIZE, bevel_over, fixed_box, icon_button};
use crate::scroll_fx::ScrollFx;

pub(crate) const LIST_WIDTH: f32 = 290.0;
const MODIFIED_MARK: &str = "•";
const SEARCH_ROW_HEIGHT: f32 = ICON_BUTTON_SIZE;
const ROW_PREVIEW_WIDTH: f32 = 118.0;
const ROW_PREVIEW_HEIGHT: f32 = 28.0;
const ROW_PREVIEW_BRUSH_SIZE: f32 = 14.0;
const ROW_VERTICAL_PADDING: f32 = 4.0;
const ROW_HEIGHT: f32 = ROW_PREVIEW_HEIGHT + 2.0 * ROW_VERTICAL_PADDING;
const ROW_PADDING: f32 = 8.0;
const GROUP_HEADER_HEIGHT: f32 = 22.0;
const GROUP_FILL_ALPHA: f32 = 0.18;
const SELECTED_FILL_ALPHA: f32 = 0.25;
const HOVER_FILL_ALPHA: f32 = 0.12;
const FRAME_STROKE: f32 = 1.0;
const DRAG_LIFT_DISTANCE: f32 = 22.0;
const HELD_SHADOW_OFFSET: f32 = 2.0;
const HELD_SHADOW_ALPHA: u8 = 90;

pub(crate) enum ListAction {
    New,
    Select(usize),
    Activate(usize),
    Edit(usize),
    Delete(usize),
    ToggleFavorite(usize),
    MoveFavorite { from: usize, to: usize },
}

struct HeldRow {
    position: usize,
    grab: f32,
    press_y: f32,
    lifted: bool,
}

struct BrushGroup {
    title: String,
    indices: Vec<usize>,
    favorites: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollRequest {
    Idle,
    Reveal,
    Center,
}

pub(crate) struct ListState {
    pub selected: usize,
    pub query: String,
    pub previews: HashMap<usize, TextureHandle>,
    pub scroll: ScrollRequest,
    pub focus_search: bool,
    held: Option<HeldRow>,
    glides: Vec<Glide>,
    pending_move: Option<LayerMove>,
}

impl ListState {
    pub fn new(selected: usize) -> Self {
        Self { selected, query: String::new(), previews: HashMap::new(), scroll: ScrollRequest::Center, focus_search: true, held: None, glides: Vec::new(), pending_move: None }
    }

    pub fn step(&mut self, entries: &[BrushEntry], favorites: &[usize], delta: isize) -> Option<usize> {
        let visible = visible_indices(entries, favorites, &self.query);
        let last = visible.len().checked_sub(1)?;
        let next = match visible.iter().position(|&index| index == self.selected) {
            Some(position) => position.saturating_add_signed(delta).min(last),
            None if delta > 0 => 0,
            None => last,
        };
        self.scroll = ScrollRequest::Reveal;
        visible.get(next).copied()
    }

    pub fn first_visible_if_selection_hidden(&self, entries: &[BrushEntry], favorites: &[usize]) -> Option<usize> {
        let visible = visible_indices(entries, favorites, &self.query);
        (!visible.contains(&self.selected)).then(|| visible.first().copied()).flatten()
    }
}

pub(crate) fn brush_list(ui: &mut Ui, skin: &skin::Skin, entries: &[BrushEntry], favorites: &[usize], state: &mut ListState) -> Option<ListAction> {
    let mut action = None;
    fixed_box(ui, vec2(ui.available_width(), SEARCH_ROW_HEIGHT), Layout::left_to_right(Align::Center), |ui| {
        let search_width = ui.available_width() - ICON_BUTTON_SIZE - ui.spacing().item_spacing.x;
        let search = ui.add(egui::TextEdit::singleline(&mut state.query).hint_text(t(Text::BrushSearchHint)).desired_width(search_width));
        if state.focus_search {
            if AUTO_FOCUS_TEXT {
                search.request_focus();
            }
            state.focus_search = AUTO_FOCUS_TEXT && !search.has_focus();
        }
        bevel_over(ui, skin, search.rect, skin::Bevel::Sunken);
        if icon_button(ui, skin, Icon::BrushAdd, false, t(Text::BrushNew)).clicked() {
            action = Some(ListAction::New);
        }
    });
    let scroll_target = std::mem::replace(&mut state.scroll, ScrollRequest::Idle);
    let height = ui.available_height() - 2.0 * FRAME_STROKE;
    let groups = brush_groups(entries, favorites, &state.query);
    let framed = egui::Frame::NONE.stroke(Stroke::new(FRAME_STROKE, skin::theme().line_soft)).show(ui, |ui| {
        ScrollArea::vertical().scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible).max_height(height).min_scrolled_height(height).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if groups.is_empty() {
                ui.add_space(ROW_PADDING);
                ui.label(t(Text::BrushNothingFound));
            }
            for group in &groups {
                group_header(ui, &group.title);
                let reorderable = group.favorites && group.indices.len() > 1 && state.query.trim().is_empty();
                if reorderable {
                    let moved = favorite_rows(ui, skin, entries, &group.indices, state, scroll_target);
                    action = moved.or(action.take());
                    continue;
                }
                for &index in &group.indices {
                    let row = brush_row(ui, skin, &entries[index], index, group.favorites, state, scroll_target);
                    action = row.or(action.take());
                }
            }
        }).with_fx(ui, skin::theme().window)
    });
    bevel_over(ui, skin, framed.inner.inner_rect, skin::Bevel::Sunken);
    action
}

fn group_header(ui: &mut Ui, title: &str) {
    let theme = skin::theme();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), GROUP_HEADER_HEIGHT), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme.highlight.gamma_multiply(GROUP_FILL_ALPHA));
    skin::text_at(ui.painter(), rect.left_center() + vec2(ROW_PADDING, 0.0), Align2::LEFT_CENTER, title, Font::Body, theme.ink);
}

fn brush_row(ui: &mut Ui, skin: &skin::Skin, entry: &BrushEntry, index: usize, favorite: bool, state: &mut ListState, scroll_target: ScrollRequest) -> Option<ListAction> {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let truncated = paint_row(ui, entry, index, rect, response.hovered(), state, scroll_target);
    row_actions(skin, entry, index, favorite, response, truncated)
}

fn paint_row(ui: &mut Ui, entry: &BrushEntry, index: usize, rect: Rect, hovered: bool, state: &mut ListState, scroll_target: ScrollRequest) -> bool {
    let theme = skin::theme();
    if index == state.selected {
        ui.painter().rect_filled(rect, 0.0, theme.highlight.gamma_multiply(SELECTED_FILL_ALPHA));
        match scroll_target {
            ScrollRequest::Idle => {}
            ScrollRequest::Reveal => ui.scroll_to_rect(rect, None),
            ScrollRequest::Center => ui.scroll_to_rect(rect, Some(Align::Center)),
        }
    } else if hovered {
        ui.painter().rect_filled(rect, 0.0, theme.highlight.gamma_multiply(HOVER_FILL_ALPHA));
    }
    let label = if entry.modified { format!("{} {MODIFIED_MARK}", brush_label(&entry.brush)) } else { brush_label(&entry.brush) };
    let name_rect = Rect::from_min_size(rect.min + vec2(ROW_PADDING, 0.0), vec2(rect.width() - 3.0 * ROW_PADDING - ROW_PREVIEW_WIDTH, ROW_HEIGHT));
    let name_painter = ui.painter().with_clip_rect(name_rect);
    skin::text_at(&name_painter, name_rect.left_center(), Align2::LEFT_CENTER, &label, Font::Body, theme.ink);
    let truncated = skin::text_width(&name_painter, &label, Font::Body) > name_rect.width();
    if ui.is_rect_visible(rect) {
        let preview_rect = Rect::from_min_size(
            pos2(rect.right() - ROW_PADDING - ROW_PREVIEW_WIDTH, rect.top() + ROW_VERTICAL_PADDING),
            vec2(ROW_PREVIEW_WIDTH, ROW_PREVIEW_HEIGHT),
        );
        let ctx = ui.ctx().clone();
        let preview = state
            .previews
            .entry(index)
            .or_insert_with(|| preview_texture(&ctx, "brush-row", &entry.brush, ROW_PREVIEW_BRUSH_SIZE, preview_rect.size()));
        paint_texture(ui, preview, preview_rect);
    }
    ui.painter().hline(rect.x_range(), rect.bottom() - FRAME_STROKE / 2.0, Stroke::new(FRAME_STROKE, theme.line_soft));
    truncated
}

fn row_actions(skin: &skin::Skin, entry: &BrushEntry, index: usize, favorite: bool, response: Response, truncated: bool) -> Option<ListAction> {
    let response = if truncated { response.on_hover_text(brush_label(&entry.brush)) } else { response };
    let mut action = if response.double_clicked() {
        Some(ListAction::Activate(index))
    } else if response.clicked() {
        Some(ListAction::Select(index))
    } else {
        None
    };
    let removable = entry.source.is_some();
    response.context_menu(|ui| {
        skin::menu_style(ui);
        let (star, star_text) = if favorite { (Icon::StarEmpty, Text::BrushFavoriteRemove) } else { (Icon::Star, Text::BrushFavoriteAdd) };
        if ui.add(egui::Button::image_and_text(skin.icons.image(star), t(star_text))).clicked() {
            action = Some(ListAction::ToggleFavorite(index));
            ui.close();
        }
        let edit_item = egui::Button::image_and_text(skin.icons.image(Icon::Pencil), t(Text::BrushEdit));
        if ui.add(edit_item).clicked() {
            action = Some(ListAction::Edit(index));
            ui.close();
        }
        let delete_item = ui.add_enabled(removable, egui::Button::new(t(Text::BrushDelete))).on_disabled_hover_text(t(Text::BrushDeleteLocked));
        if delete_item.clicked() {
            action = Some(ListAction::Delete(index));
            ui.close();
        }
    });
    action
}

fn slide_offset(held: Option<usize>, target: Option<usize>, position: usize) -> f32 {
    match (held, target) {
        (Some(from), Some(to)) if from < position && position <= to => -ROW_HEIGHT,
        (Some(from), Some(to)) if to <= position && position < from => ROW_HEIGHT,
        _ => 0.0,
    }
}

fn nearest_slot(slots: &[(Rect, Response)], y: f32) -> usize {
    let distance = |rect: Rect| (rect.top() - y).max(y - rect.bottom()).max(0.0);
    slots.iter().enumerate().min_by(|(_, a), (_, b)| distance(a.0).total_cmp(&distance(b.0))).map_or(0, |(position, _)| position)
}

fn favorite_rows(ui: &mut Ui, skin: &skin::Skin, entries: &[BrushEntry], indices: &[usize], state: &mut ListState, scroll_target: ScrollRequest) -> Option<ListAction> {
    let ctx = ui.ctx().clone();
    let count = indices.len();
    if let Some(layer_move) = state.pending_move.take().filter(|_| state.glides.len() == count) {
        apply_layer_move(&mut state.glides, layer_move, -ROW_HEIGHT);
    }
    if state.glides.len() != count {
        state.glides = vec![Glide::REST; count];
        state.held = None;
    }
    let mut slots: Vec<(Rect, Response)> = Vec::with_capacity(count);
    for position in 0..count {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click_and_drag());
        if response.drag_started() {
            let press = response.interact_pointer_pos().map_or(rect.top(), |pos| pos.y);
            state.held = Some(HeldRow { position, grab: press - rect.top(), press_y: press, lifted: false });
        }
        slots.push((rect, response));
    }
    let pointer_y = ctx.pointer_interact_pos().map(|pointer| pointer.y);
    if let (Some(held), Some(y)) = (state.held.as_mut(), pointer_y) {
        held.lifted |= (y - held.press_y).abs() >= DRAG_LIFT_DISTANCE;
    }
    let lifted = state.held.as_ref().filter(|held| held.lifted).map(|held| (held.position, held.grab));
    let target = lifted.zip(pointer_y).map(|(_, y)| nearest_slot(&slots, y));
    let mut truncated = vec![false; count];
    for (position, (rect, response)) in slots.iter().enumerate() {
        if lifted.is_some_and(|(held, _)| held == position) {
            continue;
        }
        state.glides[position].retarget(slide_offset(lifted.map(|(held, _)| held), target, position));
        let shown = rect.translate(vec2(0.0, state.glides[position].value(&ctx)));
        truncated[position] = paint_row(ui, &entries[indices[position]], indices[position], shown, response.hovered() && lifted.is_none(), state, scroll_target);
    }
    let mut action = None;
    if let (Some((from, grab)), Some(y), Some(to)) = (lifted, pointer_y, target) {
        action = drop_held_row(ui, &ctx, entries, indices, &slots, state, (from, to), (grab, y));
    } else if !ctx.input(|input| input.pointer.any_down()) {
        state.held = None;
    }
    for (position, (_, response)) in slots.into_iter().enumerate() {
        if lifted.is_some_and(|(held, _)| held == position) {
            continue;
        }
        let row = row_actions(skin, &entries[indices[position]], indices[position], true, response, truncated[position]);
        action = action.or(row);
    }
    action
}

fn drop_held_row(ui: &mut Ui, ctx: &Context, entries: &[BrushEntry], indices: &[usize], slots: &[(Rect, Response)], state: &mut ListState, (from, to): (usize, usize), (grab, pointer_y): (f32, f32)) -> Option<ListAction> {
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let (origin, destination) = (slots[from].0, slots[to].0);
    let (top_limit, bottom_limit) = (slots[0].0.top(), slots[slots.len() - 1].0.bottom());
    let top = snap_to_pixels((pointer_y - grab).clamp(top_limit, bottom_limit - ROW_HEIGHT), ctx.pixels_per_point());
    let shown = Rect::from_min_size(pos2(origin.min.x, top), origin.size());
    let theme = skin::theme();
    let pixel = 1.0 / skin::grid_ppp(ui.painter().pixels_per_point());
    ui.painter().rect_filled(shown.translate(vec2(HELD_SHADOW_OFFSET, HELD_SHADOW_OFFSET) * pixel), 0.0, Color32::from_black_alpha(HELD_SHADOW_ALPHA));
    ui.painter().rect_filled(shown, 0.0, theme.field);
    paint_row(ui, &entries[indices[from]], indices[from], shown, true, state, ScrollRequest::Idle);
    ui.painter().rect_stroke(shown, 0.0, Stroke::new(FRAME_STROKE, theme.line), egui::StrokeKind::Inside);
    if ctx.input(|input| input.pointer.any_down()) {
        return None;
    }
    state.held = None;
    if to == from || !ctx.input(|input| input.pointer.any_released()) {
        state.glides[from] = Glide::from_offset(top - origin.top());
        return None;
    }
    state.pending_move = Some(LayerMove { from, to, held_offset: Some(top - destination.top()) });
    Some(ListAction::MoveFavorite { from, to })
}

fn visible_indices(entries: &[BrushEntry], favorites: &[usize], query: &str) -> Vec<usize> {
    brush_groups(entries, favorites, query).into_iter().flat_map(|group| group.indices).collect()
}

pub(crate) fn brush_label(brush: &Brush) -> String {
    let localized = brush.meta.names.get(lang_code()).filter(|name| !name.is_empty());
    match (localized, brush.meta.name.is_empty()) {
        (Some(name), _) => name.clone(),
        (None, true) => t(Text::BrushUnnamed).to_owned(),
        (None, false) => brush.meta.name.clone(),
    }
}

fn category_label(key: &str) -> String {
    match key {
        "paint" => t(Text::BrushCategoryPaint).to_owned(),
        "basic" => t(Text::BrushCategoryBasic).to_owned(),
        "soft" => t(Text::BrushCategorySoft).to_owned(),
        "texture" => t(Text::BrushCategoryTexture).to_owned(),
        "decor" => t(Text::BrushCategoryDecor).to_owned(),
        "tracks" => t(Text::BrushCategoryTracks).to_owned(),
        "drawing" => t(Text::BrushCategoryDrawing).to_owned(),
        "lines" => t(Text::BrushCategoryLines).to_owned(),
        "painting" => t(Text::BrushCategoryPainting).to_owned(),
        "" => t(Text::BrushCategoryMine).to_owned(),
        custom => custom.to_owned(),
    }
}

fn brush_groups(entries: &[BrushEntry], favorites: &[usize], query: &str) -> Vec<BrushGroup> {
    let needle = query.trim().to_lowercase();
    let matches = |entry: &BrushEntry| {
        needle.is_empty() || category_label(&entry.brush.meta.category).to_lowercase().contains(&needle) || brush_label(&entry.brush).to_lowercase().contains(&needle)
    };
    let favorite_hits: Vec<usize> = favorites.iter().copied().filter(|&index| matches(&entries[index])).collect();
    let mut groups: Vec<BrushGroup> = Vec::new();
    if !favorite_hits.is_empty() {
        groups.push(BrushGroup { title: t(Text::BrushCategoryFavorites).to_owned(), indices: favorite_hits, favorites: true });
    }
    for (index, entry) in entries.iter().enumerate() {
        if favorites.contains(&index) || !matches(entry) {
            continue;
        }
        let title = category_label(&entry.brush.meta.category);
        match groups.iter_mut().find(|group| !group.favorites && group.title == title) {
            Some(group) => group.indices.push(index),
            None => groups.push(BrushGroup { title, indices: vec![index], favorites: false }),
        }
    }
    let own_brushes = category_label("");
    groups.sort_by_key(|group| (!group.favorites, group.title != own_brushes));
    groups
}
