
use crate::device::AUTO_FOCUS_TEXT;
use std::collections::HashMap;
use std::sync::Arc;

use chequer_core::blend::BlendMode;
use chequer_core::geom::{FPoint, IRect};
use chequer_core::shapes::StrokeAlign;
use chequer_core::text::{FontFace, Layout, TextAlign, TextSmoothing, TextStyle, render};
use eframe::egui::{self, Align2, Context, Sense, TextureHandle, TextureOptions, Ui, vec2};

use crate::app::{App, SystemFonts};
use crate::dialogs::{Dialog, SettingsTab};
use crate::modal::{ok_cancel_with, title_bar};
use crate::fonts::{FIRST_FACE, FontChoice, text_face};
use crate::i18n::{Text, t};
use crate::skin::{self, Font};
use crate::view::to_color_image;
use crate::widgets::{bevel_over, text_button};
use crate::scroll_fx::ScrollFx;

const ROW_HEIGHT: f32 = 56.0;
const LIST_HEIGHT: f32 = 420.0;
const SAMPLE_SIZE: f32 = 26.0;
const NAME_OFFSET: egui::Vec2 = vec2(8.0, 4.0);
const SAMPLE_OFFSET: egui::Vec2 = vec2(8.0, 20.0);
const SELECTED_FILL_ALPHA: f32 = 0.25;
const HOVER_FILL_ALPHA: f32 = 0.12;
const SAMPLE_CANVAS: IRect = IRect { x0: 0, y0: 0, x1: 1600, y1: 120 };

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FontPickerTarget {
    TextTool,
    Interface,
}

impl FontPickerTarget {
    fn default_label(self) -> &'static str {
        match self {
            Self::TextTool => t(Text::TextFontDefault),
            Self::Interface => t(Text::FontBuiltin),
        }
    }
}

pub struct FontPickerDialog {
    target: FontPickerTarget,
    query: String,
    selected: FontChoice,
    samples: HashMap<String, Option<TextureHandle>>,
    scroll_to: Option<f32>,
    selection_revealed: bool,
    search_focused: bool,
}

impl FontPickerDialog {
    pub fn new(target: FontPickerTarget, current: FontChoice) -> Self {
        Self { target, query: String::new(), selected: current, samples: HashMap::new(), scroll_to: None, selection_revealed: false, search_focused: false }
    }

    pub fn choose(&mut self, choice: FontChoice) {
        self.selected = choice;
        self.query.clear();
        self.selection_revealed = false;
    }
}

fn is_selected(selected: &FontChoice, row: Option<&str>) -> bool {
    match (selected, row) {
        (FontChoice::Builtin, None) => true,
        (FontChoice::System(family), Some(name)) => family == name,
        _ => false,
    }
}

fn row_choice(row: Option<&str>) -> FontChoice {
    row.map_or(FontChoice::Builtin, |name| FontChoice::System(name.to_owned()))
}

fn text_family(choice: &FontChoice) -> Option<String> {
    match choice {
        FontChoice::System(family) => Some(family.clone()),
        FontChoice::Builtin | FontChoice::File(_) => None,
    }
}

fn sample_face(target: FontPickerTarget, family: Option<&str>) -> Option<FontFace> {
    match (target, family) {
        (FontPickerTarget::Interface, None) => FontFace::new(Arc::from(skin::builtin_font_bytes()), FIRST_FACE).ok(),
        _ => text_face(family, false, false).ok().map(|text| text.face),
    }
}

fn sample_texture(ctx: &Context, target: FontPickerTarget, family: Option<&str>) -> Option<TextureHandle> {
    let face = sample_face(target, family)?;
    let ink = skin::theme().ink;
    let style = TextStyle {
        size: SAMPLE_SIZE,
        synthetic_bold: false,
        synthetic_italic: false,
        underline: false,
        strikethrough: false,
        hinting: false,
        smoothing: TextSmoothing::Grayscale,
        align: TextAlign::Left,
        outline: None,
        fill: Some([ink.r(), ink.g(), ink.b(), u8::MAX]),
        outline_width: 0.0,
        outline_align: StrokeAlign::Center,
    };
    let layout = Layout::build(&face, t(Text::FontSampleText), &style);
    let overlay = render(&face, &layout, &style, FPoint::new(0.0, 0.0), BlendMode::Normal, SAMPLE_CANVAS)?;
    Some(ctx.load_texture(format!("font-sample-{}", family.unwrap_or_default()), to_color_image(&overlay.raster), TextureOptions::NEAREST))
}

impl App {
    pub(crate) fn open_font_picker(&mut self, ctx: &Context, target: FontPickerTarget) {
        if self.system_fonts == SystemFonts::NotLoaded {
            self.load_system_fonts(ctx);
        }
        let current = match target {
            FontPickerTarget::TextTool => row_choice(self.options.text.family.as_deref()),
            FontPickerTarget::Interface => self.settings.font.clone(),
        };
        self.dialog = Some(Dialog::FontPicker(FontPickerDialog::new(target, current)));
    }

    pub(crate) fn font_picker_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, d: &mut FontPickerDialog) -> bool {
        title_bar(ui, t(Text::FontPickerTitle));
        let search = egui::TextEdit::singleline(&mut d.query).hint_text(t(Text::FontSearchHint)).desired_width(f32::INFINITY);
        let search = ui.add(search);
        bevel_over(ui, skin, search.rect, skin::Bevel::Sunken);
        if !d.search_focused {
            if AUTO_FOCUS_TEXT {
                search.request_focus();
            }
            d.search_focused = true;
        }
        if search.changed() {
            d.scroll_to = Some(0.0);
        }
        let families: &[String] = match &self.system_fonts {
            SystemFonts::Ready(families) => families,
            SystemFonts::NotLoaded | SystemFonts::Loading => &[],
        };
        let needle = d.query.to_lowercase();
        let default_name = d.target.default_label();
        let mut rows: Vec<Option<&str>> = Vec::with_capacity(families.len() + 1);
        if needle.is_empty() || default_name.to_lowercase().contains(&needle) {
            rows.push(None);
        }
        rows.extend(families.iter().map(String::as_str).filter(|family| family.to_lowercase().contains(&needle)).map(Some));
        if !d.selection_revealed && !families.is_empty() {
            d.selection_revealed = true;
            d.scroll_to = rows.iter().position(|row| is_selected(&d.selected, *row)).map(|index| index as f32 * ROW_HEIGHT);
        }
        let mut accept = false;
        let mut scroll = egui::ScrollArea::vertical().max_height(LIST_HEIGHT).min_scrolled_height(LIST_HEIGHT).auto_shrink([false, false]);
        if let Some(offset) = d.scroll_to.take() {
            scroll = scroll.vertical_scroll_offset(offset);
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        let area = scroll.show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
            for row in &rows[range] {
                let name = row.unwrap_or(default_name);
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
                let selected = is_selected(&d.selected, *row);
                let theme = skin::theme();
                if selected {
                    ui.painter().rect_filled(rect, 0.0, theme.highlight.gamma_multiply(SELECTED_FILL_ALPHA));
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 0.0, theme.highlight.gamma_multiply(HOVER_FILL_ALPHA));
                }
                skin::text_at(ui.painter(), rect.min + NAME_OFFSET, Align2::LEFT_TOP, name, Font::Body, theme.ink_dim);
                let ctx = ui.ctx().clone();
                let target = d.target;
                let sample = d.samples.entry(row.unwrap_or_default().to_owned()).or_insert_with(|| sample_texture(&ctx, target, *row));
                if let Some(texture) = sample {
                    let ppp = ui.painter().pixels_per_point();
                    let size = texture.size_vec2() / ppp;
                    ui.painter().with_clip_rect(rect).image(texture.id(), egui::Rect::from_min_size(rect.min + SAMPLE_OFFSET, size), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                }
                if response.clicked() || response.double_clicked() {
                    d.selected = row_choice(*row);
                }
                accept |= response.double_clicked();
            }
        }).with_fx(ui, skin::theme().window);
        bevel_over(ui, skin, area.inner_rect, skin::Bevel::Sunken);
        if rows.is_empty() {
            ui.label(if families.is_empty() { t(Text::FontLoading) } else { t(Text::FontNoMatches) });
        }
        let mut import = false;
        let (ok, cancel) = ok_cancel_with(ui, skin, true, |ui| import = text_button(ui, skin, t(Text::FontImport)).clicked());
        if import {
            self.pick_font_to_import(ui.ctx());
        }
        let accepted = ok || accept;
        if accepted {
            match d.target {
                FontPickerTarget::TextTool => self.options.text.family = text_family(&d.selected),
                FontPickerTarget::Interface => self.settings.font = d.selected.clone(),
            }
        }
        let closing = accepted || cancel;
        if closing && d.target == FontPickerTarget::Interface {
            self.dialog = Some(Dialog::Settings(SettingsTab::Interface));
        }
        !closing
    }
}
