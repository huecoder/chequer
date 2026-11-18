
use std::ops::RangeInclusive;

use eframe::egui::{self, Align2, Color32, Context, CursorIcon, Id, Order, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, UiBuilder, Vec2, vec2};

use crate::metrics::{ITEM_SPACING, PRESSED_OFFSET};
use crate::i18n::{Text, t};
use crate::icons::{Icon, icon_side};
use crate::skin::{self, Bevel, Font, Plate, Skin};
use crate::scroll_fx::ScrollFx;

pub const ICON_BUTTON_SIZE: f32 = 24.0;
const WHEEL_POINTS_PER_NOTCH: f32 = 40.0;
const WHEEL_FAST_FACTOR: i32 = 5;
const TAB_HEIGHT: f32 = 28.0;
const TAB_PADDING_X: f32 = 12.0;
const TAB_GAP: f32 = 2.0;
const TAB_LEAD: f32 = 4.0;
const TAB_INACTIVE_DROP: f32 = 3.0;
const TAB_LINE_WIDTH: f32 = 1.0;
pub const WHEEL_SLIDER_STEP: f32 = 0.01;
const CLOSE_BUTTON: f32 = 14.0;
const CLOSE_CROSS_HALF_PX: f32 = 3.0;
const CLOSE_CROSS_PADDING_PX: f32 = 1.0;
const RESIZE_EDGE: f32 = 5.0;
const RESIZE_OUTSIDE: f32 = skin::WINDOW_SHADOW_REACH;
const RESIZE_CORNER: f32 = 14.0;
const MIN_VISIBLE: f32 = 48.0;
const SNAP_DISTANCE: f32 = 10.0;
const TEXT_BUTTON_HEIGHT: f32 = 24.0;
const TEXT_BUTTON_MIN_WIDTH: f32 = 72.0;
const TEXT_BUTTON_PADDING: f32 = 24.0;
const TITLE_TEXT_INDENT: f32 = 8.0;
const TITLE_SLACK: f32 = 12.0;
const CLOSE_BUTTON_MARGIN: f32 = 3.0;
const TITLE_TO_CONTENT_GAP: f32 = 3.0;
const THUMB_WIDTH: f32 = 11.0;
const THUMB_RIDGE_OFFSETS: [f32; 2] = [-1.0, 1.0];
const THUMB_RIDGE_MARGIN_PX: f32 = 3.0;
pub const SLIDER_ROW_HEIGHT: f32 = 18.0;
const DISABLED_ICON_ALPHA: u8 = 140;
const ICON_FILL: f32 = 0.8;
const THUMB_OVERHANG: f32 = 2.0;
const ELLIPSIS: &str = "…";
pub const WINDOW_INSET: f32 = 2.0;
const LIST_FRAME_MARGIN: i8 = 1;

pub(crate) fn icon_scale(ppp: f32) -> f32 {
    if skin::pixel_mode() {
        return skin::ui_pixel() as f32;
    }
    (ppp + 0.25).floor().max(1.0)
}

fn icon_rect(rect: Rect, ppp: f32) -> Rect {
    let side = icon_side().min(rect.width().min(rect.height()) * ICON_FILL);
    let center = rect.center();
    let min = Pos2::new(((center.x * ppp) - side * ppp / 2.0).round() / ppp, ((center.y * ppp) - side * ppp / 2.0).round() / ppp);
    Rect::from_min_size(min, vec2(side, side))
}

pub fn fixed_box<R>(ui: &mut Ui, size: Vec2, layout: egui::Layout, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add_contents(&mut child)
}

pub fn icon_button(ui: &mut Ui, skin: &Skin, icon: Icon, selected: bool, tooltip: &str) -> Response {
    icon_button_sized(ui, skin, icon, selected, ui.is_enabled(), vec2(ICON_BUTTON_SIZE, ICON_BUTTON_SIZE), tooltip)
}

pub fn icon_button_sized(ui: &mut Ui, skin: &Skin, icon: Icon, selected: bool, enabled: bool, size: Vec2, tooltip: &str) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let plate = flat_button_plate(&response, selected, enabled);
    if let Some(plate) = plate {
        skin.paint_plate(ui.painter(), rect, &plate);
    }
    let target = icon_rect(rect, ui.painter().pixels_per_point());
    let tint = if enabled { Color32::WHITE } else { Color32::from_white_alpha(DISABLED_ICON_ALPHA) };
    skin.icons.image_sized(icon, target.width()).tint(tint).paint_at(ui, target);
    if tooltip.is_empty() { response } else { response.on_hover_text(tooltip) }
}

pub fn hover_text_with_hotkey(response: Response, text: &str, hotkey: Option<char>) -> Response {
    response.on_hover_ui(|ui| {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let mut job = egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat::simple(font.clone(), ui.visuals().text_color()));
        if let Some(key) = hotkey {
            job.append(&format!("({})", key.to_ascii_uppercase()), ITEM_SPACING.x, egui::TextFormat::simple(font, skin::theme().ink_dim));
        }
        ui.label(job);
    })
}

pub fn text_button(ui: &mut Ui, skin: &Skin, text: &str) -> Response {
    text_button_sized(ui, skin, text, vec2(0.0, TEXT_BUTTON_HEIGHT))
}

pub fn text_button_sized(ui: &mut Ui, skin: &Skin, text: &str, min_size: Vec2) -> Response {
    let text_width = skin::text_width(ui.painter(), text, Font::Body);
    let size = vec2((text_width + TEXT_BUTTON_PADDING).max(min_size.x.max(TEXT_BUTTON_MIN_WIDTH)), min_size.y.max(TEXT_BUTTON_HEIGHT));
    let enabled = ui.is_enabled();
    let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let plate = raised_button_plate(&response, false, enabled);
    skin.paint_plate(ui.painter(), rect, &plate);
    let color = if enabled { skin::theme().ink } else { skin::theme().ink_faint };
    let offset = if plate.bevel == Some(Bevel::Sunken) { PRESSED_OFFSET } else { Vec2::ZERO };
    skin::text_at(ui.painter(), (rect.center() + offset).round(), Align2::CENTER_CENTER, text, Font::Body, color);
    response
}

fn flat_button_plate(response: &Response, selected: bool, enabled: bool) -> Option<Plate> {
    if !enabled {
        return None;
    }
    if selected || response.is_pointer_button_down_on() {
        return Some(Plate::button(skin::theme().button_on, Bevel::Sunken));
    }
    response.hovered().then(|| Plate::button(skin::theme().button_hover, Bevel::Raised))
}

pub fn raised_button_plate(response: &Response, selected: bool, enabled: bool) -> Plate {
    if !enabled {
        return Plate::button(skin::theme().button, Bevel::Raised);
    }
    if selected || response.is_pointer_button_down_on() {
        return Plate::button(skin::theme().button_on, Bevel::Sunken);
    }
    Plate::button(if response.hovered() { skin::theme().button_hover } else { skin::theme().button }, Bevel::Raised)
}

pub fn glyph_button(ui: &mut Ui, skin: &Skin, label: &str, selected: bool, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ICON_BUTTON_SIZE, ICON_BUTTON_SIZE), Sense::click());
    let plate = raised_button_plate(&response, selected, true);
    skin.paint_plate(ui.painter(), rect, &plate);
    let nudge = if plate.bevel == Some(Bevel::Sunken) { PRESSED_OFFSET } else { Vec2::ZERO };
    skin::text_at(ui.painter(), (rect.center() + nudge).round(), Align2::CENTER_CENTER, label, Font::Body, skin::theme().ink);
    response.on_hover_text(tooltip)
}

pub fn bevel_over(ui: &Ui, skin: &Skin, rect: Rect, bevel: Bevel) {
    skin.paint_plate(ui.painter(), rect, &Plate::frame_only(bevel, skin::FIELD_RINGS));
}

pub fn list_frame<R>(ui: &mut Ui, skin: &Skin, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let ppp = ui.painter().pixels_per_point();
    let slot = ui.painter().add(Shape::Noop);
    let framed = egui::Frame::NONE.inner_margin(egui::Margin::same(LIST_FRAME_MARGIN)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add_contents(ui)
    });
    let plate = Plate { border: skin::theme().line_soft, bevel: None, ..Plate::field(skin::theme().field) };
    ui.painter().set(slot, Shape::Vec(skin.plate_shapes(ppp, framed.response.rect, &plate)));
    framed.inner
}

pub fn field_scope(ui: &mut Ui, skin: &Skin, add_widget: impl FnOnce(&mut Ui) -> Response) -> Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for state in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active] {
            state.bg_fill = skin::theme().field;
            state.weak_bg_fill = skin::theme().field;
            state.bg_stroke = Stroke::NONE;
        }
        let response = add_widget(ui);
        skin.paint_plate(ui.painter(), response.rect, &Plate { fill: Color32::TRANSPARENT, ..Plate::field(skin::theme().field) });
        response
    })
    .inner
}

pub fn select_all_on_focus(ui: &Ui, response: &Response, text: &str) {
    if !response.gained_focus() {
        return;
    }
    let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) else {
        return;
    };
    let everything = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
    state.cursor.set_char_range(Some(everything));
    state.store(ui.ctx(), response.id);
}

pub fn number_field(ui: &mut Ui, skin: &Skin, value: &mut f32, range: RangeInclusive<f32>, width: f32) -> Response {
    let spec = DragSpec { max_decimals: Some(0), ..DragSpec::default() };
    drag_value(ui, skin, value, range, [width, SLIDER_ROW_HEIGHT], &spec)
}

pub struct DragSpec {
    pub speed: f64,
    pub suffix: &'static str,
    pub max_decimals: Option<usize>,
    pub wheel_step: f64,
}

impl Default for DragSpec {
    fn default() -> Self {
        Self { speed: 1.0, suffix: "", max_decimals: None, wheel_step: 1.0 }
    }
}

pub fn drag_value<N: egui::emath::Numeric>(ui: &mut Ui, skin: &Skin, value: &mut N, range: RangeInclusive<N>, size: impl Into<Vec2>, spec: &DragSpec) -> Response {
    let (low, high) = (range.start().to_f64(), range.end().to_f64());
    let mut response = field_scope(ui, skin, |ui| {
        let mut field = egui::DragValue::new(value).range(range).speed(spec.speed).suffix(spec.suffix).custom_parser(crate::formula::evaluate);
        if let Some(decimals) = spec.max_decimals {
            field = field.max_decimals(decimals);
        }
        let size = size.into();
        if size == Vec2::ZERO { ui.add(field) } else { ui.add_sized(size, field) }
    });
    let mut scrolled = value.to_f64();
    if scroll_value(ui, &response, &mut scrolled, spec.wheel_step, low..=high) {
        *value = N::from_f64(scrolled);
        response.mark_changed();
    }
    response
}

const NUMBER_FIELD_WIDTH: f32 = 44.0;

#[derive(Clone, Copy)]
pub struct NumberFormat {
    pub decimals: usize,
    pub suffix: &'static str,
}

impl NumberFormat {
    pub const WHOLE: Self = Self { decimals: 0, suffix: "" };
}

pub fn slider_with_number<N: egui::emath::Numeric>(ui: &mut Ui, skin: &Skin, value: &mut N, range: RangeInclusive<N>, step: f64, format: NumberFormat) -> bool {
    let (low, high) = (range.start().to_f64(), range.end().to_f64());
    let span = (high - low).max(step);
    let mut changed = false;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - NUMBER_FIELD_WIDTH - ui.spacing().item_spacing.x).max(60.0);
        let fraction = ((value.to_f64() - low) / span) as f32;
        if let Some(fraction) = plain_slider_stepped(ui, skin, fraction, width, (step / span) as f32) {
            let snapped = low + ((fraction as f64 * span) / step).round() * step;
            *value = N::from_f64(snapped.clamp(low, high));
            changed = true;
        }
        let spec = DragSpec { max_decimals: Some(format.decimals), suffix: format.suffix, wheel_step: step, ..DragSpec::default() };
        changed |= drag_value(ui, skin, value, range, [NUMBER_FIELD_WIDTH, SLIDER_ROW_HEIGHT], &spec).changed();
    });
    changed
}

pub fn combo_box<R>(ui: &mut Ui, skin: &Skin, id: &str, width: f32, selected: &str, add_items: impl FnOnce(&mut Ui) -> R) -> Response {
    let response = egui::ComboBox::from_id_salt(id).width(width).selected_text(selected).show_ui(ui, |ui| {
        skin::menu_style(ui);
        add_items(ui)
    });
    bevel_over(ui, skin, response.response.rect, Bevel::Raised);
    response.response
}

#[derive(Clone, Default)]
struct SearchState {
    query: String,
    edited: bool,
}

const SEARCH_COMBO_MAX_HEIGHT: f32 = 320.0;
const SEARCH_ARROW_HALF_WIDTH: f32 = 4.0;
const SEARCH_ARROW_MARGIN: f32 = 6.0;

pub fn search_combo(ui: &mut Ui, skin: &Skin, id: &str, width: f32, options: &[&str], current: Option<usize>) -> (Response, Option<usize>) {
    let state_id = ui.make_persistent_id(id);
    let ctx = ui.ctx().clone();
    let current_name = current.map_or("", |index| options[index]);
    let mut state: SearchState = ctx.data(|data| data.get_temp(state_id)).unwrap_or_default();
    let popup_was_open = egui::Popup::is_id_open(&ctx, state_id.with("popup"));
    let mut shown = if popup_was_open { state.query.clone() } else { current_name.to_owned() };

    let output = egui::TextEdit::singleline(&mut shown).id(state_id.with("field")).desired_width(width).show(ui);
    let response: Response = output.response.response.clone();
    bevel_over(ui, skin, response.rect, Bevel::Sunken);
    paint_search_arrow(ui, response.rect);

    if response.gained_focus() {
        state = SearchState { query: current_name.to_owned(), edited: false };
        let mut field = output.state.clone();
        field.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(shown.chars().count()))));
        field.store(&ctx, response.id);
    } else if response.changed() {
        state = SearchState { query: shown, edited: true };
    }

    let needle = state.query.to_lowercase();
    let matches: Vec<usize> =
        (0..options.len()).filter(|&index| !state.edited || options[index].to_lowercase().contains(&needle)).collect();
    let mut chosen = None;
    let enter = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if enter {
        chosen = matches.first().copied();
    }
    let open_command = if response.gained_focus() || response.clicked() {
        Some(egui::SetOpenCommand::Bool(true))
    } else if enter || (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape))) {
        Some(egui::SetOpenCommand::Bool(false))
    } else {
        None
    };
    egui::Popup::from_response(&response)
        .id(state_id.with("popup"))
        .open_memory(open_command)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(width)
        .show(|ui| {
            skin::menu_style(ui);
            ui.spacing_mut().scroll.fade.strength = 0.0;
            egui::ScrollArea::vertical().max_height(SEARCH_COMBO_MAX_HEIGHT).show(ui, |ui| {
                for &index in &matches {
                    if ui.selectable_label(current == Some(index), options[index]).clicked() {
                        chosen = Some(index);
                        ui.close();
                    }
                }
            }).with_fx(ui, skin::theme().window);
        });
    ctx.data_mut(|data| data.insert_temp(state_id, state));
    (response, chosen)
}

fn paint_search_arrow(ui: &Ui, field: Rect) {
    let center = Pos2::new(field.max.x - SEARCH_ARROW_MARGIN - SEARCH_ARROW_HALF_WIDTH, field.center().y);
    let half = SEARCH_ARROW_HALF_WIDTH;
    let points = vec![center + vec2(-half, -half / 2.0), center + vec2(half, -half / 2.0), center + vec2(0.0, half / 2.0)];
    ui.painter().add(Shape::convex_polygon(points, ui.visuals().text_color(), Stroke::NONE));
}

pub fn plain_slider(ui: &mut Ui, skin: &Skin, value: f32, width: f32) -> Option<f32> {
    plain_slider_stepped(ui, skin, value, width, WHEEL_SLIDER_STEP)
}

pub fn plain_slider_stepped(ui: &mut Ui, skin: &Skin, value: f32, width: f32, wheel_step: f32) -> Option<f32> {
    let track = Track { stops: &[(0.0, skin::theme().field), (1.0, skin::theme().field)], checker: false };
    gradient_slider(ui, skin, value, &track, vec2(width, SLIDER_ROW_HEIGHT), false, wheel_step)
}

pub fn separator(ui: &mut Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(6.0, height), Sense::hover());
    let ppp = skin::grid_ppp(ui.painter().pixels_per_point());
    let x = (rect.center().x * ppp).round() / ppp;
    let step = 1.0 / ppp;
    let line = |dx: f32, color: Color32| Shape::rect_filled(Rect::from_min_size(Pos2::new(x + dx, rect.min.y + 2.0), vec2(step, rect.height() - 4.0)), 0.0, color);
    ui.painter().extend([line(-step, skin::theme().groove_dark), line(0.0, skin::theme().groove_light)]);
}

fn odd_cell_square(center: Pos2, side: f32, ppp: f32) -> Rect {
    let cells = (side * ppp).round().max(1.0) as i32 | 1;
    let min_cell = |axis: f32| (axis * ppp).floor() - ((cells - 1) / 2) as f32;
    let min = Pos2::new(min_cell(center.x) / ppp, min_cell(center.y) / ppp);
    Rect::from_min_size(min, Vec2::splat(cells as f32 / ppp))
}

pub fn paint_cross(painter: &egui::Painter, button: Rect, color: Color32) {
    let ppp = skin::grid_ppp(painter.pixels_per_point());
    let (cx, cy) = ((button.center().x * ppp).floor(), (button.center().y * ppp).floor());
    let frame_cells = (1 + skin::theme().bevel_rings) as f32;
    let room = (button.width().min(button.height()) * ppp - 2.0 * (frame_cells + CLOSE_CROSS_PADDING_PX)).floor();
    let reach = (((room - 1.0) / 2.0).floor() as i32).clamp(1, CLOSE_CROSS_HALF_PX as i32);
    let mut shapes = Vec::new();
    for step in -reach..=reach {
        for (dx, dy) in [(step, step), (step, -step)] {
            let at = Pos2::new((cx + dx as f32) / ppp, (cy + dy as f32) / ppp);
            shapes.push(Shape::rect_filled(Rect::from_min_size(at, vec2(1.0, 1.0) / ppp), 0.0, color));
        }
    }
    painter.extend(shapes);
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Move(Vec2),
    Resize { edges: Edges, origin: Rect, press: Pos2 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

impl Edges {
    fn cursor(self) -> CursorIcon {
        match (self.left || self.right, self.top || self.bottom) {
            (true, true) if self.left == self.top => CursorIcon::ResizeNwSe,
            (true, true) => CursorIcon::ResizeNeSw,
            (true, false) => CursorIcon::ResizeHorizontal,
            _ => CursorIcon::ResizeVertical,
        }
    }

    fn allowed(self, resize: Resize) -> bool {
        match resize {
            Resize::Fixed => false,
            Resize::Width => !(self.top || self.bottom),
            Resize::Both => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resize {
    Fixed,
    Width,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corner {
    pub right: bool,
    pub bottom: bool,
}

impl Corner {
    pub const TOP_LEFT: Corner = Corner { right: false, bottom: false };
    pub const TOP_RIGHT: Corner = Corner { right: true, bottom: false };
    pub const BOTTOM_LEFT: Corner = Corner { right: false, bottom: true };
    pub const BOTTOM_RIGHT: Corner = Corner { right: true, bottom: true };

    fn nearest(rect: Rect, workspace: Rect) -> Self {
        Self { right: rect.center().x > workspace.center().x, bottom: rect.center().y > workspace.center().y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelState {
    pub pos: Pos2,
    pub size: Vec2,
    pub open: bool,
    pub corner: Corner,
    pub gap: Vec2,
    grab: Option<Grab>,
}

impl PanelState {
    pub fn at_corner(corner: Corner, gap: Vec2, size: Vec2, workspace: Rect) -> Self {
        let mut state = Self { pos: workspace.min, size, open: true, corner, gap, grab: None };
        state.place(workspace);
        state
    }

    pub fn rect(&self) -> Rect {
        Rect::from_min_size(self.pos, self.size)
    }

    pub fn hit_rect(&self) -> Rect {
        self.rect().expand(RESIZE_OUTSIDE)
    }

    pub fn place(&mut self, workspace: Rect) {
        let x = if self.corner.right { workspace.max.x - self.gap.x - self.size.x } else { workspace.min.x + self.gap.x };
        let y = if self.corner.bottom { workspace.max.y - self.gap.y - self.size.y } else { workspace.min.y + self.gap.y };
        self.pos = clamp_position(Pos2::new(x, y), self.size, workspace);
    }

    fn reanchor(&mut self, workspace: Rect) {
        self.corner = Corner::nearest(self.rect(), workspace);
        let rect = self.rect();
        self.gap = vec2(
            if self.corner.right { workspace.max.x - rect.max.x } else { rect.min.x - workspace.min.x },
            if self.corner.bottom { workspace.max.y - rect.max.y } else { rect.min.y - workspace.min.y },
        );
    }
}

pub struct PanelSpec<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub titled: bool,
    pub resize: Resize,
    pub default_size: Option<Vec2>,
    pub min_size: Vec2,
    pub fit_height: bool,
    pub clip_title: bool,
    pub over_image: Rect,
}

fn follow_pointer(ctx: &Context, state: &mut PanelState, min_size: Vec2, workspace: Rect, neighbors: &[Rect]) -> bool {
    let Some(grab) = state.grab else {
        return false;
    };
    let (pointer, down) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down()));
    let Some(pointer) = pointer.filter(|_| down) else {
        state.grab = None;
        return false;
    };
    match grab {
        Grab::Move(offset) => state.pos = snapped(pointer - offset, state.size, workspace, neighbors, skin::grid_ppp(ctx.pixels_per_point())),
        Grab::Resize { edges, origin, press } => {
            let rect = resized(origin, edges, pointer - press, min_size);
            let rect = snapped_resize(
                rect,
                edges,
                min_size,
                workspace,
                neighbors,
                skin::grid_ppp(ctx.pixels_per_point()),
            );
            state.pos = rect.min;
            state.size = rect.size();
        }
    }
    ctx.set_cursor_icon(match grab {
        Grab::Move(_) => CursorIcon::Grabbing,
        Grab::Resize { edges, .. } => edges.cursor(),
    });
    ctx.request_repaint();
    true
}

fn resized(origin: Rect, edges: Edges, delta: Vec2, min_size: Vec2) -> Rect {
    let mut rect = origin;
    if edges.left {
        rect.min.x = (origin.min.x + delta.x).min(origin.max.x - min_size.x);
    }
    if edges.right {
        rect.max.x = (origin.max.x + delta.x).max(origin.min.x + min_size.x);
    }
    if edges.top {
        rect.min.y = (origin.min.y + delta.y).min(origin.max.y - min_size.y);
    }
    if edges.bottom {
        rect.max.y = (origin.max.y + delta.y).max(origin.min.y + min_size.y);
    }
    rect
}

fn snapped_resize(
    mut rect: Rect,
    edges: Edges,
    min_size: Vec2,
    workspace: Rect,
    neighbors: &[Rect],
    ppp: f32,
) -> Rect {
    let overlap = 1.0 / ppp;
    let beside = |other: &&Rect| {
        other.min.y - SNAP_DISTANCE <= rect.max.y && rect.min.y <= other.max.y + SNAP_DISTANCE
    };
    let stacked = |other: &&Rect| {
        other.min.x - SNAP_DISTANCE <= rect.max.x && rect.min.x <= other.max.x + SNAP_DISTANCE
    };
    let xs: Vec<f32> = [workspace.min.x, workspace.max.x]
        .into_iter()
        .chain(neighbors.iter().filter(beside).flat_map(|other| {
            [
                other.min.x,
                other.max.x,
                other.max.x - overlap,
                other.min.x + overlap,
            ]
        }))
        .collect();
    let ys: Vec<f32> = [workspace.min.y, workspace.max.y]
        .into_iter()
        .chain(neighbors.iter().filter(stacked).flat_map(|other| {
            [
                other.min.y,
                other.max.y,
                other.max.y - overlap,
                other.min.y + overlap,
            ]
        }))
        .collect();
    if edges.left {
        rect.min.x = nearest_within(rect.min.x, &xs, SNAP_DISTANCE).min(rect.max.x - min_size.x);
    }
    if edges.right {
        rect.max.x = nearest_within(rect.max.x, &xs, SNAP_DISTANCE).max(rect.min.x + min_size.x);
    }
    if edges.top {
        rect.min.y = nearest_within(rect.min.y, &ys, SNAP_DISTANCE).min(rect.max.y - min_size.y);
    }
    if edges.bottom {
        rect.max.y = nearest_within(rect.max.y, &ys, SNAP_DISTANCE).max(rect.min.y + min_size.y);
    }
    rect
}

fn snapped(pos: Pos2, size: Vec2, workspace: Rect, neighbors: &[Rect], ppp: f32) -> Pos2 {
    let overlap = 1.0 / ppp;
    let near_x = |range: Rect| range.min.y - SNAP_DISTANCE <= pos.y + size.y && pos.y <= range.max.y + SNAP_DISTANCE;
    let near_y = |range: Rect| range.min.x - SNAP_DISTANCE <= pos.x + size.x && pos.x <= range.max.x + SNAP_DISTANCE;
    let mut xs = vec![workspace.min.x, workspace.max.x - size.x];
    let mut ys = vec![workspace.min.y, workspace.max.y - size.y];
    for other in neighbors {
        if near_x(*other) {
            xs.extend([other.max.x - overlap, other.min.x - size.x + overlap, other.min.x, other.max.x - size.x]);
        }
        if near_y(*other) {
            ys.extend([other.max.y - overlap, other.min.y - size.y + overlap, other.min.y, other.max.y - size.y]);
        }
    }
    Pos2::new(nearest_within(pos.x, &xs, SNAP_DISTANCE), nearest_within(pos.y, &ys, SNAP_DISTANCE))
}

fn nearest_within(value: f32, candidates: &[f32], distance: f32) -> f32 {
    candidates.iter().copied().filter(|c| (c - value).abs() <= distance).min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs())).unwrap_or(value)
}

const OVER_IMAGE_OPACITY: f32 = 0.72;

pub fn floating_panel(
    ctx: &Context,
    skin: &Skin,
    spec: &PanelSpec,
    state: &mut PanelState,
    workspace: Rect,
    neighbors: &[Rect],
    add_contents: impl FnOnce(&mut Ui),
) {
    if !state.open {
        return;
    }
    state.place(workspace);
    if follow_pointer(ctx, state, spec.min_size, workspace, neighbors) {
        state.pos = clamp_position(state.pos, state.size, workspace);
        state.reanchor(workspace);
    }
    if spec.fit_height {
        state.pos.y = state.pos.y.min(workspace.max.y - state.size.y).max(workspace.min.y);
    }
    let area_id = Id::unique(("panel", spec.id));
    egui::Area::new(area_id)
        .order(Order::Middle)
        .fixed_pos(state.pos)
        .constrain(false)
        .show(ctx, |ui| {
            let ppp = ui.painter().pixels_per_point();
            let chrome = WINDOW_INSET + skin::PANEL_PADDING;
            if !spec.clip_title {
                let title_min_width = skin::text_width(ui.painter(), spec.title, Font::Title)
                    + CLOSE_BUTTON
                    + 2.0 * WINDOW_INSET
                    + TITLE_TEXT_INDENT
                    + TITLE_SLACK;
                state.size.x = state.size.x.max(title_min_width);
            }
            let outer = Rect::from_min_size(state.pos, state.size);
            if outer.intersects(spec.over_image)
                && state.grab.is_none()
                && !ui.rect_contains_pointer(outer)
            {
                ui.set_opacity(OVER_IMAGE_OPACITY);
            }
            ui.set_clip_rect(outer.expand(RESIZE_OUTSIDE));
            ui.allocate_rect(outer, Sense::hover());
            if ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(outer) {
                ctx.move_to_top(ui.layer_id());
            }
            let background = ui.painter().add(Shape::Noop);
            let body = (!spec.titled).then(|| body_drag(ui, spec, state, outer));
            let content_top = if spec.titled {
                state.pos.y + WINDOW_INSET + skin::TITLE_HEIGHT + TITLE_TO_CONTENT_GAP
            } else {
                state.pos.y + chrome
            };
            let content_bottom = if spec.fit_height {
                workspace.max.y.max(content_top)
            } else {
                outer.max.y - chrome
            };
            let content = Rect::from_min_max(
                Pos2::new(outer.min.x + chrome, content_top),
                Pos2::new(outer.max.x - chrome, content_bottom),
            );
            let used = ui
                .scope_builder(
                    UiBuilder::new().id_salt("content").max_rect(content),
                    |ui| {
                        ui.set_clip_rect(content);
                        add_contents(ui);
                    },
                )
                .response
                .rect;
            if spec.fit_height {
                let shown_at = state.pos;
                state.size.y = (used.max.y - outer.min.y + chrome).max(spec.min_size.y);
                if state.corner.bottom && state.grab.is_none() {
                    state.place(workspace);
                    state.pos.y = state
                        .pos
                        .y
                        .min(workspace.max.y - state.size.y)
                        .max(workspace.min.y);
                    if state.pos != shown_at {
                        ctx.request_discard("panel height changed");
                    }
                }
            }
            let outer = Rect::from_min_size(state.pos, state.size);
            ui.painter()
                .set(background, Shape::Vec(skin.window_shapes(ppp, outer)));
            resize_handles(ui, spec, state, outer);
            let drag = body.unwrap_or_else(|| paint_title_bar(ui, skin, spec, state, outer));
            panel_menu(skin, spec, state, &drag);
        });
}

fn panel_menu(skin: &Skin, spec: &PanelSpec, state: &mut PanelState, response: &Response) {
    let Some(default_size) = spec.default_size else {
        return;
    };
    response.context_menu(|ui| {
        skin::menu_style(ui);
        if ui.button(t(Text::ResetWindowSize)).clicked() {
            state.size = default_size;
            ui.close();
        }
        if ui.add(egui::Button::image_and_text(skin.icons.image(Icon::Close), t(Text::Close))).clicked() {
            state.open = false;
            ui.close();
        }
    });
}

fn body_drag(ui: &mut Ui, spec: &PanelSpec, state: &mut PanelState, outer: Rect) -> Response {
    let drag = ui.interact(outer, Id::unique(("panel-drag", spec.id)), Sense::click_and_drag());
    start_move(ui, state, &drag);
    drag
}

fn start_move(ui: &Ui, state: &mut PanelState, drag: &Response) {
    if drag.is_pointer_button_down_on()
        && state.grab.is_none()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        state.grab = Some(Grab::Move(origin - state.pos));
    }
}

fn paint_title_bar(ui: &mut Ui, skin: &Skin, spec: &PanelSpec, state: &mut PanelState, outer: Rect) -> Response {
    let raw_ppp = ui.painter().pixels_per_point();
    let ppp = skin::grid_ppp(raw_ppp);
    let bar = Rect::from_min_size(outer.min + vec2(WINDOW_INSET, WINDOW_INSET), vec2(outer.width() - 2.0 * WINDOW_INSET, skin::TITLE_HEIGHT));
    ui.painter().extend(skin::title_shapes(raw_ppp, bar, skin::theme().title));
    let step = 1.0 / ppp;
    let edge = Rect::from_min_size(Pos2::new(bar.min.x, (bar.max.y * ppp).round() / ppp), vec2(bar.width(), step));
    ui.painter().rect_filled(edge, 0.0, skin::theme().line_soft);

    let close = odd_cell_square(bar.right_center() - vec2(CLOSE_BUTTON / 2.0 + CLOSE_BUTTON_MARGIN, 0.0), CLOSE_BUTTON, skin::grid_ppp(ui.painter().pixels_per_point()));
    let text_room = close.min.x - bar.min.x - TITLE_TEXT_INDENT - CLOSE_BUTTON_MARGIN;
    let title = fit_text(ui.painter(), spec.title, Font::Title, text_room);
    skin::text_at(ui.painter(), bar.left_center() + vec2(TITLE_TEXT_INDENT, 0.0), Align2::LEFT_CENTER, &title, Font::Title, skin::theme().ink);

    let drag_area = Rect::from_min_max(bar.min, Pos2::new(close.min.x - CLOSE_BUTTON_MARGIN, bar.max.y));
    let drag = ui.interact(drag_area, Id::unique(("panel-drag", spec.id)), Sense::click_and_drag());
    start_move(ui, state, &drag);
    let close_response = ui.interact(close, Id::unique(("panel-close", spec.id)), Sense::click());
    let hot = close_response.hovered() || close_response.is_pointer_button_down_on();
    if hot {
        skin.paint_plate(ui.painter(), close, &Plate { border: skin::theme().close_hover, bevel: None, ..Plate::button(skin::theme().close_hover, Bevel::Raised) });
    }
    paint_cross(ui.painter(), close, if hot { skin::theme().close_hover_text } else { skin::theme().ink });
    if close_response.clicked() {
        state.open = false;
    }
    drag
}

fn fit_text(painter: &egui::Painter, text: &str, font: Font, room: f32) -> String {
    if skin::text_width(painter, text, font) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    (1..chars.len())
        .rev()
        .map(|keep| format!("{}{ELLIPSIS}", chars[..keep].iter().collect::<String>()))
        .find(|candidate| skin::text_width(painter, candidate, font) <= room)
        .unwrap_or_else(|| ELLIPSIS.to_owned())
}

fn resize_handles(ui: &mut Ui, spec: &PanelSpec, state: &mut PanelState, outer: Rect) {
    let edge = |left, right, top, bottom| Edges {
        left,
        right,
        top,
        bottom,
    };
    let hit = outer.expand(RESIZE_OUTSIDE);
    let band = RESIZE_EDGE + RESIZE_OUTSIDE;
    let reach = RESIZE_CORNER + RESIZE_OUTSIDE;
    let handles = [
        (
            edge(true, false, false, false),
            Rect::from_min_max(hit.min, Pos2::new(hit.min.x + band, hit.max.y)),
        ),
        (
            edge(false, true, false, false),
            Rect::from_min_max(Pos2::new(hit.max.x - band, hit.min.y), hit.max),
        ),
        (
            edge(false, false, true, false),
            Rect::from_min_max(hit.min, Pos2::new(hit.max.x, hit.min.y + band)),
        ),
        (
            edge(false, false, false, true),
            Rect::from_min_max(Pos2::new(hit.min.x, hit.max.y - band), hit.max),
        ),
        (
            edge(true, false, true, false),
            Rect::from_min_size(hit.min, vec2(reach, reach)),
        ),
        (
            edge(false, true, true, false),
            Rect::from_min_size(Pos2::new(hit.max.x - reach, hit.min.y), vec2(reach, reach)),
        ),
        (
            edge(true, false, false, true),
            Rect::from_min_size(Pos2::new(hit.min.x, hit.max.y - reach), vec2(reach, reach)),
        ),
        (
            edge(false, true, false, true),
            Rect::from_min_size(hit.max - vec2(reach, reach), vec2(reach, reach)),
        ),
    ];
    for (index, (edges, rect)) in handles.into_iter().enumerate().filter(|(_, (edges, _))| edges.allowed(spec.resize)) {
        let response = ui.interact(rect, Id::unique(("panel-resize", spec.id, index)), Sense::drag());
        if response.is_pointer_button_down_on()
            && state.grab.is_none()
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
        {
            state.grab = Some(Grab::Resize { edges, origin: state.rect(), press });
        }
        if response.hovered() && state.grab.is_none() {
            ui.ctx().set_cursor_icon(edges.cursor());
        }
    }
    if spec.resize == Resize::Both {
        paint_grip_dots(ui, outer);
    }
}

fn paint_grip_dots(ui: &mut Ui, outer: Rect) {
    let ppp = skin::grid_ppp(ui.painter().pixels_per_point());
    let step = (3.0 * ppp).round() / ppp;
    let dot = 1.0 / ppp;
    let corner = outer.max - vec2(WINDOW_INSET + 4.0, WINDOW_INSET + 4.0);
    for row in 0..3 {
        for column in 0..=row {
            let at = corner - vec2(column as f32 * step, (2 - row) as f32 * step);
            ui.painter().rect_filled(Rect::from_min_size(at, vec2(dot, dot)), 0.0, skin::theme().ink_faint);
        }
    }
}

fn clamp_position(pos: Pos2, size: Vec2, workspace: Rect) -> Pos2 {
    let min_x = workspace.min.x - size.x + MIN_VISIBLE;
    let max_x = workspace.max.x - MIN_VISIBLE;
    let min_y = workspace.min.y;
    let max_y = workspace.max.y - skin::TITLE_HEIGHT;
    Pos2::new(pos.x.clamp(min_x, max_x.max(min_x)), pos.y.clamp(min_y, max_y.max(min_y)))
}

pub struct Track<'a> {
    pub stops: &'a [(f32, Color32)],
    pub checker: bool,
}

fn thumb_remap(stops: &[(f32, Color32)], margin: f32) -> Vec<(f32, Color32)> {
    let (first, last) = (stops[0].1, stops[stops.len() - 1].1);
    let mut out = vec![(0.0, first)];
    out.extend(stops.iter().map(|&(at, color)| (margin + at * (1.0 - 2.0 * margin), color)));
    out.push((1.0, last));
    out
}

fn value_from_pointer(along: f32, track_start: f32, track_length: f32, thumb: f32) -> f32 {
    let span = (track_length - thumb).max(1.0);
    ((along - track_start - thumb / 2.0) / span).clamp(0.0, 1.0)
}

fn paint_thumb_ridges(painter: &egui::Painter, thumb: Rect, vertical: bool, pressed: bool) {
    let grid = skin::grid_ppp(painter.pixels_per_point());
    let pixel = 1.0 / grid;
    let nudge = if pressed { pixel } else { 0.0 };
    let margin = THUMB_RIDGE_MARGIN_PX * pixel;
    let (along_min, along_max) = if vertical { (thumb.min.y, thumb.max.y) } else { (thumb.min.x, thumb.max.x) };
    let center_pixel = (((along_min * grid).round() + (along_max * grid).round()) / 2.0).floor();
    for offset in THUMB_RIDGE_OFFSETS {
        let along = (center_pixel + offset) * pixel + nudge;
        let ridge = if vertical {
            Rect::from_min_max(Pos2::new(thumb.min.x + margin + nudge, along), Pos2::new(thumb.max.x - margin + nudge, along + pixel))
        } else {
            Rect::from_min_max(Pos2::new(along, thumb.min.y + margin + nudge), Pos2::new(along + pixel, thumb.max.y - margin + nudge))
        };
        painter.rect_filled(ridge, 0.0, skin::theme().groove_dark);
    }
}

pub fn gradient_slider(ui: &mut Ui, skin: &Skin, value: f32, track: &Track, size: Vec2, vertical: bool, wheel_step: f32) -> Option<f32> {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let ppp = ui.painter().pixels_per_point();
    let overhang = THUMB_OVERHANG;
    let bed = if vertical { rect.shrink2(vec2(overhang, 0.0)) } else { rect.shrink2(vec2(0.0, overhang)) };
    skin.paint_plate(ui.painter(), bed, &Plate::field(skin::theme().field));
    let well = bed.shrink(skin::plate_inset(ppp, skin::FIELD_RINGS));
    let (length, start) = if vertical { (well.height(), well.min.y) } else { (well.width(), well.min.x) };
    let thumb_len = THUMB_WIDTH;
    let margin = thumb_len / 2.0 / length.max(1.0);
    let painter = ui.painter().with_clip_rect(well);
    if track.checker {
        skin.paint_checker(&painter, well, well.min);
    }
    let remapped = thumb_remap(track.stops, margin);
    let stops: Vec<(f32, Color32)> = if vertical { remapped.iter().rev().map(|&(at, c)| (1.0 - at, c)).collect() } else { remapped };
    painter.add(Shape::mesh(skin::gradient_mesh(well, &stops, !vertical)));

    let changed = response
        .interact_pointer_pos()
        .filter(|_| response.is_pointer_button_down_on() || response.dragged())
        .map(|pos| {
            if vertical {
                1.0 - value_from_pointer(pos.y, start, length, thumb_len)
            } else {
                value_from_pointer(pos.x, start, length, thumb_len)
            }
        });
    let scrolled = Some(value + wheel_over(ui, &response).amount(wheel_step as f64) as f32).filter(|scrolled| *scrolled != value).map(|scrolled| scrolled.clamp(0.0, 1.0));
    let changed = changed.or(scrolled);
    let shown = changed.unwrap_or(value).clamp(0.0, 1.0);
    let travel = (length - thumb_len).max(0.0);
    let thumb = if vertical {
        let center = well.max.y - thumb_len / 2.0 - shown * travel;
        Rect::from_center_size(Pos2::new(rect.center().x, center), vec2(rect.width(), thumb_len))
    } else {
        let center = well.min.x + thumb_len / 2.0 + shown * travel;
        Rect::from_center_size(Pos2::new(center, rect.center().y), vec2(thumb_len, rect.height()))
    };
    let plate = raised_button_plate(&response, false, true);
    skin.paint_plate(ui.painter(), thumb, &plate);
    paint_thumb_ridges(ui.painter(), thumb, vertical, plate.bevel == Some(Bevel::Sunken));
    changed.filter(|new| (new - value).abs() > f32::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_up_moves_to_earlier_items_and_stops_at_the_ends() {
        assert_eq!(stepped_index(4, Some(2), 1), Some(1));
        assert_eq!(stepped_index(4, Some(2), -1), Some(3));
        assert_eq!(stepped_index(4, Some(0), 1), None);
        assert_eq!(stepped_index(4, Some(3), -1), None);
        assert_eq!(stepped_index(4, Some(1), 0), None);
    }

    #[test]
    fn ctrl_multiplies_the_wheel_step_by_five() {
        assert_eq!(Wheel { notches: 1, fast: false }.amount(1.0), 1.0);
        assert_eq!(Wheel { notches: -2, fast: true }.amount(1.0), -10.0);
    }

    #[test]
    fn pointer_at_track_ends_gives_zero_and_one() {
        assert_eq!(value_from_pointer(100.0, 100.0, 200.0, 10.0), 0.0);
        assert_eq!(value_from_pointer(300.0, 100.0, 200.0, 10.0), 1.0);
    }

    #[test]
    fn pointer_in_the_middle_gives_half() {
        assert!((value_from_pointer(200.0, 100.0, 200.0, 10.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn thumb_remap_keeps_end_colors_flat_and_inner_stops_inside() {
        let stops = [(0.0, Color32::BLACK), (1.0, Color32::WHITE)];
        let remapped = thumb_remap(&stops, 0.1);
        assert_eq!(remapped.len(), 4);
        assert_eq!(remapped[0], (0.0, Color32::BLACK));
        assert!((remapped[1].0 - 0.1).abs() < 1e-6 && remapped[1].1 == Color32::BLACK);
        assert!((remapped[2].0 - 0.9).abs() < 1e-6 && remapped[2].1 == Color32::WHITE);
        assert_eq!(remapped[3], (1.0, Color32::WHITE));
    }

    #[test]
    fn panel_cannot_leave_the_workspace_entirely() {
        let workspace = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0));
        let pos = clamp_position(Pos2::new(5000.0, 5000.0), vec2(200.0, 200.0), workspace);
        assert!(pos.x <= workspace.max.x - MIN_VISIBLE && pos.y <= workspace.max.y - skin::TITLE_HEIGHT);
        let pos = clamp_position(Pos2::new(-5000.0, -5000.0), vec2(200.0, 200.0), workspace);
        assert!(pos.x + 200.0 >= MIN_VISIBLE && pos.y >= 0.0);
    }

    #[test]
    fn icon_scale_is_whole_and_follows_the_system_scale() {
        assert_eq!(icon_scale(1.0), 1.0);
        assert_eq!(icon_scale(1.25), 1.0);
        assert_eq!(icon_scale(1.5), 1.0);
        assert_eq!(icon_scale(1.75), 2.0);
        assert_eq!(icon_scale(2.0), 2.0);
    }

    #[test]
    fn window_near_the_workspace_edge_snaps_flush() {
        let workspace = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0));
        let size = vec2(100.0, 100.0);
        assert_eq!(snapped(Pos2::new(6.0, 4.0), size, workspace, &[], 1.0), Pos2::new(0.0, 0.0));
        assert_eq!(snapped(Pos2::new(895.0, 495.0), size, workspace, &[], 1.0), Pos2::new(900.0, 500.0));
        assert_eq!(snapped(Pos2::new(300.0, 300.0), size, workspace, &[], 1.0), Pos2::new(300.0, 300.0));
    }

    #[test]
    fn windows_snap_together_sharing_one_physical_pixel() {
        let workspace = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0));
        let neighbor = Rect::from_min_size(Pos2::new(200.0, 200.0), vec2(100.0, 100.0));
        let pos = snapped(Pos2::new(305.0, 210.0), vec2(80.0, 80.0), workspace, &[neighbor], 1.0);
        assert_eq!(pos.x, 299.0);
        let pos = snapped(Pos2::new(120.0, 202.0), vec2(80.0, 80.0), workspace, &[neighbor], 2.0);
        assert_eq!(pos.x, 200.5 - 80.0);
        assert_eq!(pos.y, 200.0);
    }

    #[test]
    fn far_neighbors_do_not_attract() {
        let workspace = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0));
        let neighbor = Rect::from_min_size(Pos2::new(800.0, 400.0), vec2(100.0, 100.0));
        assert_eq!(snapped(Pos2::new(300.0, 100.0), vec2(80.0, 80.0), workspace, &[neighbor], 1.0), Pos2::new(300.0, 100.0));
    }
}

#[derive(Clone, Copy, Default)]
pub struct Wheel {
    pub notches: i32,
    pub fast: bool,
}

impl Wheel {
    pub fn amount(self, step: f64) -> f64 {
        let factor = if self.fast { WHEEL_FAST_FACTOR } else { 1 };
        (self.notches * factor) as f64 * step
    }
}

pub fn wheel_over(ui: &Ui, response: &Response) -> Wheel {
    wheel_while(ui, response.id, response.hovered())
}

pub fn wheel_while(ui: &Ui, id: egui::Id, pointer_over: bool) -> Wheel {
    if !pointer_over || egui::Popup::is_any_open(ui.ctx()) {
        return Wheel::default();
    }
    let (delta, fast) = ui.input(|input| {
        let delta: f32 = input
            .events
            .iter()
            .map(|event| match event {
                egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, .. } => delta.y / WHEEL_POINTS_PER_NOTCH,
                egui::Event::MouseWheel { delta, .. } => delta.y,
                _ => 0.0,
            })
            .sum();
        (delta, input.modifiers.command)
    });
    ui.ctx().input_mut(|input| {
        input.events.retain(|event| !matches!(event, egui::Event::MouseWheel { .. }));
        input.smooth_scroll_delta = Vec2::ZERO;
    });
    if delta == 0.0 {
        return Wheel::default();
    }
    let accumulated = ui.data_mut(|data| {
        let remainder = data.get_temp_mut_or_default::<f32>(id);
        *remainder += delta;
        let notches = remainder.trunc();
        *remainder -= notches;
        notches
    });
    Wheel { notches: accumulated as i32, fast }
}

pub fn stepped_index(len: usize, current: Option<usize>, notches: i32) -> Option<usize> {
    let current = current?;
    let target = (current as i64 - notches as i64).clamp(0, len as i64 - 1) as usize;
    (target != current && notches != 0).then_some(target)
}

pub fn tab_bar<T: Copy + PartialEq>(ui: &mut Ui, id: Id, current: &mut T, tabs: &[(T, &'static str)]) -> bool {
    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), TAB_HEIGHT), Sense::hover());
    let theme = skin::theme();
    let painter = ui.painter().clone();
    let grid = skin::grid_ppp(painter.pixels_per_point());
    let snap = |value: f32| if skin::pixel_mode() { (value * grid).round() / grid } else { value };
    let line_width = if skin::pixel_mode() { 1.0 / grid } else { TAB_LINE_WIDTH };
    let line = Stroke::new(line_width, theme.groove_dark);
    let baseline = snap(row.bottom() - line_width / 2.0);
    let mut x = snap(row.left() + TAB_LEAD);
    let mut selected = None;
    let mut active_span = None;
    for (index, (value, label)) in tabs.iter().enumerate() {
        let width = snap(skin::text_width(&painter, label, Font::Body) + 2.0 * TAB_PADDING_X);
        let active = value == current;
        let top = snap(if active { row.top() } else { row.top() + TAB_INACTIVE_DROP });
        let rect = Rect::from_min_max(egui::pos2(x, top), egui::pos2(x + width, row.bottom()));
        let tab_response = ui.interact(rect, id.with(index), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let fill = if active { theme.window } else if tab_response.hovered() { theme.button_hover } else { theme.button };
        painter.rect_filled(Rect::from_min_max(rect.min, egui::pos2(rect.right(), baseline)), 0.0, fill);
        let inset = line_width;
        painter.hline(rect.left() + inset..=rect.right() - inset, rect.top() + line_width / 2.0, line);
        painter.vline(rect.left() + line_width / 2.0, rect.top() + inset..=baseline, line);
        painter.vline(rect.right() - line_width / 2.0, rect.top() + inset..=baseline, line);
        let ink = if active { theme.ink } else if tab_response.hovered() { theme.ink } else { theme.ink_dim };
        skin::text_at(&painter, egui::pos2(snap(rect.center().x), snap(rect.center().y + line_width)), Align2::CENTER_CENTER, label, Font::Body, ink);
        if active {
            active_span = Some((rect.left(), rect.right()));
        }
        if tab_response.clicked() {
            selected = Some(*value);
        }
        x += width + snap(TAB_GAP);
    }
    match active_span {
        Some((left, right)) => {
            painter.hline(row.left()..=left, baseline, line);
            painter.hline(right..=row.right(), baseline, line);
        }
        None => {
            painter.hline(row.x_range(), baseline, line);
        }
    }
    let position = tabs.iter().position(|(value, _)| value == current);
    let wheel = wheel_while(ui, id, response.contains_pointer());
    if let Some(target) = stepped_index(tabs.len(), position, wheel.notches) {
        selected = Some(tabs[target].0);
    }
    match selected {
        Some(value) if value != *current => {
            *current = value;
            true
        }
        _ => false,
    }
}

pub fn choice_combo<T: PartialEq + Copy>(ui: &mut Ui, skin: &Skin, id: &str, width: f32, value: &mut T, items: &[(T, &str)]) -> bool {
    let position = items.iter().position(|(item, _)| item == value);
    let selected = position.map_or("", |i| items[i].1);
    let mut changed = false;
    let response = combo_box(ui, skin, id, width, selected, |ui| {
        for (item, label) in items {
            changed |= ui.selectable_value(value, *item, *label).changed();
        }
    });
    if let Some(target) = stepped_index(items.len(), position, wheel_over(ui, &response).notches) {
        *value = items[target].0;
        changed = true;
    }
    changed
}

pub fn labeled<T: Copy>(values: impl IntoIterator<Item = T>, label: impl Fn(T) -> &'static str) -> Vec<(T, &'static str)> {
    values.into_iter().map(|value| (value, label(value))).collect()
}

fn scroll_value(ui: &Ui, response: &Response, value: &mut f64, step: f64, range: RangeInclusive<f64>) -> bool {
    let amount = wheel_over(ui, response).amount(step);
    let scrolled = (*value + amount).clamp(*range.start(), *range.end());
    let changed = amount != 0.0 && scrolled != *value;
    *value = scrolled;
    changed
}
