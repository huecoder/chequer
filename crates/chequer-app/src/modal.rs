use eframe::egui::{self, Align, Align2, Color32, Context, Id, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::i18n::{Text, t};
use crate::skin::{self, Font};
use crate::device::AUTO_FOCUS_TEXT;
use crate::widgets::{bevel_over, text_button};

pub(crate) const DIALOG_MARGIN: i8 = 8;
pub(crate) const BUTTON_ROW_LEAD: f32 = 6.0;
pub(crate) const DIALOG_WIDTH: f32 = 380.0;
pub(crate) const MODAL_BACKDROP: Color32 = Color32::from_black_alpha(100);
const MODAL_ITEM_SPACING: egui::Vec2 = vec2(8.0, 6.0);
const SCREEN_GUTTER: f32 = 8.0;
const TITLE_BOTTOM_GAP: f32 = 8.0;
const DEFAULT_MODAL_ID: &str = "modal";
const CONFIRM_MODAL_ID: &str = "confirm-modal";
const INPUT_MODAL_ID: &str = "input-modal";
const MIN_MODAL_WIDTH: f32 = 160.0;
const MIN_SCROLL_HEIGHT: f32 = 80.0;
const DEFAULT_FOOTER_HEIGHT: f32 = 32.0;
const FOOTER_HEIGHT_EPSILON: f32 = 0.5;
const CANCEL_KEYS: [egui::Key; 2] = [egui::Key::Escape, egui::Key::BrowserBack];

#[derive(Clone, Copy)]
pub(crate) struct ModalSpec {
    pub id: &'static str,
    pub width: f32,
    pub backdrop: Color32,
    pub scrolls: bool,
}

impl Default for ModalSpec {
    fn default() -> Self {
        Self { id: DEFAULT_MODAL_ID, width: DIALOG_WIDTH, backdrop: MODAL_BACKDROP, scrolls: true }
    }
}

impl ModalSpec {
    pub(crate) fn id(self, id: &'static str) -> Self {
        Self { id, ..self }
    }

    pub(crate) fn width(self, width: f32) -> Self {
        Self { width, ..self }
    }

    pub(crate) fn backdrop(self, backdrop: Color32) -> Self {
        Self { backdrop, ..self }
    }

    pub(crate) fn unscrolled(self) -> Self {
        Self { scrolls: false, ..self }
    }
}

pub(crate) fn show_modal<R>(ctx: &Context, skin: &skin::Skin, spec: ModalSpec, keys: Option<DialogKeys>, content: impl FnOnce(&mut Ui) -> R) -> egui::ModalResponse<R> {
    let screen = ctx.viewport_rect();
    let frame_reach = 2.0 * (f32::from(DIALOG_MARGIN) + SCREEN_GUTTER);
    let max_width = (screen.width() - frame_reach).max(MIN_MODAL_WIDTH);
    let scroll_height = modal_content_height(ctx);
    let response = egui::Modal::new(Id::unique(spec.id)).backdrop_color(spec.backdrop).frame(dialog_frame()).show(ctx, |ui| {
        if let Some(keys) = keys {
            ui.data_mut(|data| data.insert_temp(dialog_keys_id(), keys));
        }
        let slot = ui.painter().add(egui::Shape::Noop);
        ui.set_width(spec.width.min(max_width));
        ui.set_max_height(scroll_height + TITLE_RESERVE);
        ui.spacing_mut().item_spacing = MODAL_ITEM_SPACING;
        let (_, title_area) = ui.allocate_space(vec2(ui.available_width(), TITLE_RESERVE));
        let outer = ui.data(|data| (data.get_temp::<Option<Rect>>(viewport_id()).flatten(), data.get_temp::<String>(title_id())));
        let result = if spec.scrolls {
            egui::ScrollArea::vertical().max_height(scroll_height).auto_shrink([false, true]).show_viewport(ui, |ui, viewport| {
                let visible = Rect::from_min_size(ui.max_rect().min + viewport.min.to_vec2(), viewport.size());
                ui.data_mut(|data| data.insert_temp(viewport_id(), Some(visible)));
                content(ui)
            }).inner
        } else {
            content(ui)
        };
        let title = ui.data_mut(|data| data.remove_temp::<String>(title_id())).unwrap_or_default();
        ui.data_mut(|data| {
            data.insert_temp(viewport_id(), outer.0);
            if let Some(title) = outer.1 {
                data.insert_temp(title_id(), title);
            }
        });
        paint_title(ui, Rect::from_min_size(title_area.min, vec2(title_area.width(), skin::TITLE_HEIGHT)), &title);
        let outline = ui.min_rect().expand(DIALOG_MARGIN as f32);
        ui.painter().set(slot, egui::Shape::Vec(skin.window_shapes(ui.painter().pixels_per_point(), outline)));
        result
    });
    if !screen.contains_rect(response.response.rect) {
        ctx.request_repaint();
    }
    if response.is_top_modal && !response.any_popup_open && response.backdrop_response.clicked() {
        ctx.data_mut(|data| data.insert_temp(backdrop_dismiss_id(), true));
        ctx.request_repaint();
    }
    response
}

const TITLE_RESERVE: f32 = skin::TITLE_HEIGHT + TITLE_BOTTOM_GAP;

pub(crate) fn modal_content_height(ctx: &Context) -> f32 {
    let frame_reach = 2.0 * (f32::from(DIALOG_MARGIN) + SCREEN_GUTTER);
    (ctx.viewport_rect().height() - frame_reach - TITLE_RESERVE).max(MIN_SCROLL_HEIGHT)
}

fn viewport_id() -> Id {
    Id::NULL.with("modal-viewport")
}

fn title_id() -> Id {
    Id::NULL.with("modal-title")
}

pub(crate) fn pinned_footer<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let Some(visible) = ui.data(|data| data.get_temp::<Option<Rect>>(viewport_id()).flatten()) else {
        return add(ui);
    };
    let height_id = ui.scope_id().with("footer-height");
    let height = ui.data(|data| data.get_temp::<f32>(height_id)).unwrap_or(DEFAULT_FOOTER_HEIGHT);
    let (_, natural) = ui.allocate_space(vec2(visible.width(), height));
    let lift = (visible.bottom() - natural.bottom()).min(0.0);
    let area = Rect::from_min_size(pos2(visible.left(), natural.top() + lift), vec2(visible.width(), height));
    let mut footer = ui.new_child(UiBuilder::new().id_salt("pinned-footer").max_rect(area));
    if lift < 0.0 {
        footer.painter().rect_filled(area.expand2(vec2(f32::from(DIALOG_MARGIN), 0.0)), 0.0, skin::theme().window);
    }
    footer.interact(area, footer.id().with("blocker"), Sense::click_and_drag());
    let result = add(&mut footer);
    let measured = footer.min_rect().height();
    if (measured - height).abs() > FOOTER_HEIGHT_EPSILON {
        ui.data_mut(|data| data.insert_temp(height_id, measured));
        ui.ctx().request_repaint();
    }
    result
}

fn paint_title(ui: &Ui, rect: Rect, title: &str) {
    let reach = DIALOG_MARGIN as f32 - crate::widgets::WINDOW_INSET;
    let bar = Rect::from_min_max(rect.min - vec2(reach, reach), pos2(rect.max.x + reach, rect.max.y));
    let ppp = skin::grid_ppp(ui.painter().pixels_per_point());
    ui.painter().extend(skin::title_shapes(ui.painter().pixels_per_point(), bar, skin::theme().title));
    let line = (bar.max.y * ppp).round() / ppp;
    ui.painter().rect_filled(Rect::from_min_size(pos2(bar.min.x, line), vec2(bar.width(), 1.0 / ppp)), 0.0, skin::theme().line_soft);
    skin::text_at(ui.painter(), bar.center(), Align2::CENTER_CENTER, title, Font::Title, skin::theme().ink);
}

fn backdrop_dismiss_id() -> Id {
    Id::NULL.with("modal-backdrop-dismiss")
}

fn dialog_frame() -> egui::Frame {
    egui::Frame::NONE.inner_margin(egui::Margin::same(DIALOG_MARGIN))
}

pub(crate) fn title_bar(ui: &mut Ui, title: &str) {
    ui.data_mut(|data| data.insert_temp(title_id(), title.to_owned()));
}

#[derive(Clone, Copy, Default)]
pub(crate) struct DialogKeys {
    pub accept: bool,
    pub cancel: bool,
}

fn dialog_keys_id() -> Id {
    Id::NULL.with("dialog-keys")
}

pub(crate) fn read_dialog_keys(ctx: &Context) -> DialogKeys {
    if egui::Popup::is_any_open(ctx) {
        return DialogKeys::default();
    }
    let fresh_press = |key: egui::Key| ctx.input(|i| i.events.iter().any(|event| matches!(event, egui::Event::Key { key: k, pressed: true, repeat: false, .. } if *k == key)));
    let backdrop_clicked = ctx.data_mut(|data| data.remove_temp::<bool>(backdrop_dismiss_id())).unwrap_or(false);
    let accept = fresh_press(egui::Key::Enter);
    let cancel = CANCEL_KEYS.into_iter().any(fresh_press) || backdrop_clicked;
    ctx.input_mut(|i| {
        i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
        for key in CANCEL_KEYS {
            i.consume_key(egui::Modifiers::NONE, key);
        }
    });
    DialogKeys { accept, cancel }
}

pub(crate) fn clear_dialog_keys(ctx: &Context) {
    ctx.data_mut(|data| data.remove_temp::<DialogKeys>(dialog_keys_id()));
}

pub(crate) fn dialog_keys(ui: &Ui) -> DialogKeys {
    ui.data(|data| data.get_temp(dialog_keys_id())).unwrap_or_default()
}

pub(crate) fn take_dialog_escape(ui: &Ui) -> bool {
    let escape = dialog_keys(ui).cancel;
    ui.data_mut(|data| data.insert_temp(dialog_keys_id(), DialogKeys::default()));
    escape
}

pub(crate) fn confirm_modal(ctx: &Context, skin: &skin::Skin, title: &str, question: &str, escape: bool) -> Option<bool> {
    let mut answer = escape.then_some(false);
    show_modal(ctx, skin, ModalSpec::default().id(CONFIRM_MODAL_ID), None, |ui| {
        title_bar(ui, title);
        ui.label(question);
        ui.add_space(BUTTON_ROW_LEAD);
        pinned_footer(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if text_button(ui, skin, t(Text::No)).clicked() {
                    answer = Some(false);
                }
                if text_button(ui, skin, t(Text::Yes)).clicked() {
                    answer = Some(true);
                }
            });
        });
    });
    answer
}

pub(crate) struct NameInput {
    pub text: String,
    focus: bool,
}

impl NameInput {
    pub(crate) fn new(text: String) -> Self {
        Self { text, focus: true }
    }
}

pub(crate) fn input_modal(ctx: &Context, skin: &skin::Skin, title: &str, input: &mut NameInput, problem: Option<&str>, keys: DialogKeys) -> Option<bool> {
    let valid = !input.text.trim().is_empty() && problem.is_none();
    let mut answer = keys.cancel.then_some(false);
    show_modal(ctx, skin, ModalSpec::default().id(INPUT_MODAL_ID), None, |ui| {
        title_bar(ui, title);
        let field = ui.add(egui::TextEdit::singleline(&mut input.text).hint_text(t(Text::BrushNameHint)).desired_width(f32::INFINITY));
        if input.focus {
            if AUTO_FOCUS_TEXT {
                field.request_focus();
            }
            input.focus = AUTO_FOCUS_TEXT && !field.has_focus();
        }
        bevel_over(ui, skin, field.rect, skin::Bevel::Sunken);
        if let Some(problem) = problem {
            ui.label(egui::RichText::new(problem).color(skin::theme().ink_dim));
        }
        ui.add_space(BUTTON_ROW_LEAD);
        pinned_footer(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if text_button(ui, skin, t(Text::Cancel)).clicked() {
                    answer = Some(false);
                }
                if ui.add_enabled_ui(valid, |ui| text_button(ui, skin, t(Text::Ok))).inner.clicked() {
                    answer = Some(true);
                }
            });
        });
    });
    if keys.accept && valid {
        answer = Some(true);
    }
    answer
}

pub(crate) fn ok_cancel(ui: &mut Ui, app_skin: &skin::Skin, ok_enabled: bool) -> (bool, bool) {
    ok_cancel_with(ui, app_skin, ok_enabled, |_| {})
}

pub(crate) fn ok_cancel_with(ui: &mut Ui, app_skin: &skin::Skin, ok_enabled: bool, leading: impl FnOnce(&mut Ui)) -> (bool, bool) {
    ok_cancel_labeled(ui, app_skin, t(Text::Ok), ok_enabled, leading)
}

pub(crate) fn ok_cancel_labeled(ui: &mut Ui, app_skin: &skin::Skin, ok_label: &str, ok_enabled: bool, leading: impl FnOnce(&mut Ui)) -> (bool, bool) {
    let (mut ok, mut cancel) = (false, false);
    let keys = dialog_keys(ui);
    ui.add_space(BUTTON_ROW_LEAD);
    pinned_footer(ui, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            cancel = text_button(ui, app_skin, t(Text::Cancel)).clicked();
            ok = ui.add_enabled_ui(ok_enabled, |ui| text_button(ui, app_skin, ok_label)).inner.clicked();
            ui.with_layout(Layout::left_to_right(Align::Min), leading);
        });
    });
    (ok || (keys.accept && ok_enabled), cancel || keys.cancel)
}

