
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use chequer_core::chq::{self, WriteMode};
use chequer_core::document::{Anchor, Document, LayerProps, MAX_SIDE, Resample};
use chequer_core::io::{
    JPEG_QUALITY_MAX, JPEG_QUALITY_MIN, PngColor, PngCompression, SaveOptions, decode,
};
use chequer_core::ora;
use chequer_core::raster::Raster;
use eframe::egui::{self, Align, Color32, Context, Id, Layout, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2, vec2};

use crate::metrics::{FORM_GRID_SPACING, GAP_ROW, GAP_TIGHT, GRID_SPACING_TIGHT};
use crate::app::{APP_NAME, APP_VERSION, App, Command};
use crate::app::SystemFonts;
use crate::fonts::FontChoice;
use crate::i18n::{Lang, Text, set_lang, t};
use crate::pen;
use crate::new_document::{NewBackground, NewDocumentSetup};
use crate::panels::LayerDoubleClick;
use crate::timeline::{MAX_FPS, MIN_FPS, ONION_MAX_FRAMES, PERCENT_MAX, fps_to_duration_ms};
use crate::view::ZoomOutFilter;
use crate::skin::{self, Bevel, Plate};
use crate::trackpad::{PINCH_SENSITIVITY_MAX, PINCH_SENSITIVITY_MIN, TrackpadMode};
use crate::widgets::{DragSpec, NumberFormat, SLIDER_ROW_HEIGHT, slider_with_number, choice_combo, tab_bar, combo_box, drag_value, field_scope, glyph_button, labeled, text_button, text_button_sized};
use crate::scroll_fx::ScrollFx;
use crate::modal::{BUTTON_ROW_LEAD, ModalSpec, pinned_footer, clear_dialog_keys, confirm_modal, dialog_keys, ok_cancel, ok_cancel_with, read_dialog_keys, show_modal, take_dialog_escape, title_bar};

const BYTES_PER_MEGABYTE: f64 = 1024.0 * 1024.0;
const SETTINGS_BODY_HEIGHT: f32 = 340.0;
const SAVE_DIALOG_WIDTH: f32 = 760.0;
const FONT_PICKER_WIDTH: f32 = 560.0;
const WELCOME_WIDTH: f32 = 640.0;
const SAVE_SETTINGS_WIDTH: f32 = 340.0;
const SAVE_PREVIEW_SIZE: egui::Vec2 = vec2(380.0, 300.0);
const SAVE_PREVIEW_MAX_SIDE: u32 = 760;
const COMBO_WIDTH: f32 = 140.0;
const NEW_BACKGROUND_COMBO_WIDTH: f32 = 140.0;
const FONT_COMBO_WIDTH: f32 = 170.0;
const CHOICE_BUTTON_WIDTH: f32 = 220.0;
const PNG_COLOR_COMBO_WIDTH: f32 = 250.0;
const FPS_FIELD_WIDTH: f32 = 56.0;
const PIXEL_FIELD: DragSpec = DragSpec { speed: 1.0, suffix: " px", max_decimals: None, wheel_step: 1.0 };
const SAVE_EXTENSIONS: [&str; 10] = ["chq", "png", "jpg", "bmp", "tga", "gif", "webp", "tif", "ico", "ora"];
const THEME_COMBO_WIDTH: f32 = 220.0;
const GESTURE_MODE_COMBO_WIDTH: f32 = 220.0;
const SETTINGS_LABEL_WIDTH: f32 = 190.0;
const SETTINGS_WIDTH: f32 = 520.0;
const PICKER_DIALOG_WIDTH: f32 = 560.0;
const PICKER_CONTROLS_WIDTH: f32 = 190.0;
const PICKER_PREVIEW_WIDTH: f32 = 236.0;
const PENDING_RESET_KEY: &str = "settings-pending-reset";
const PICKER_DIVIDER_GAP: f32 = 8.0;
const PICKER_PREVIEW_WIDTH_BESIDE: f32 = 340.0;
const PICKER_HEIGHT_KEY: &str = "picker-reserved-height";
const SETTINGS_TAB_GAP: f32 = 4.0;
const PEN_CURVE_STEP: f64 = 0.1;
const PINCH_SENSITIVITY_STEP_PERCENT: f64 = 5.0;
const UI_SCALE_MIN_PERCENT: f32 = 50.0;
const UI_SCALE_MAX_PERCENT: f32 = 300.0;
const UI_SCALE_STEP_PERCENT: f64 = 5.0;
const UI_SCALE_DRAFT_ID: &str = "ui_scale_draft_percent";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    CloseTab,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsReset {
    Tab(SettingsTab),
    All,
}

impl SettingsReset {
    fn question(self) -> Text {
        match self {
            SettingsReset::Tab(_) => Text::ResetTabConfirm,
            SettingsReset::All => Text::ResetAllConfirm,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsTab {
    General,
    Interface,
    Gestures,
    Themes,
}

impl SettingsTab {
    const ALL: [(SettingsTab, Text); 4] = [
        (SettingsTab::General, Text::SettingsGeneral),
        (SettingsTab::Interface, Text::SettingsInterface),
        (SettingsTab::Gestures, Text::SettingsGestures),
        (SettingsTab::Themes, Text::SettingsThemes),
    ];
}

pub struct ThemeOptionsDialog {
    values_before: crate::themes::SettingValues,
    colors_before: crate::themes::ColorOverrides,
    confirming_reset: bool,
}

pub enum Dialog {
    New(NewImageDialog),
    Resize(SizeDialog),
    CanvasSize(SizeDialog),
    LayerProps { index: usize, props: LayerProps, original: LayerProps },
    SaveConfig(SaveConfigDialog),
    PasteTooBig { raster: Raster, new_layer: bool, layer_name: Option<String> },
    Dropped(Vec<PathBuf>),
    Confirm { tab: usize, action: ConfirmAction },
    FlattenWarning { tab: usize, path: PathBuf, options: SaveOptions, then: Option<ConfirmAction> },
    About,
    Welcome,
    FontPicker(crate::font_picker::FontPickerDialog),
    Brushes(crate::brush_dialog::BrushDialog),
    Settings(SettingsTab),
    ColorPicker { from_settings: bool },
    ThemeOptions(ThemeOptionsDialog),
    Animation { fps: u32 },
    Error(String),
}

impl Dialog {
    fn modal_spec(&self) -> ModalSpec {
        match self {
            Dialog::SaveConfig(_) => ModalSpec::default().width(SAVE_DIALOG_WIDTH),
            Dialog::FontPicker(_) => ModalSpec::default().width(FONT_PICKER_WIDTH),
            Dialog::Brushes(_) => ModalSpec::default().width(crate::brush_dialog::BRUSH_DIALOG_WIDTH).unscrolled(),
            Dialog::Settings(_) => ModalSpec::default().width(SETTINGS_WIDTH),
            Dialog::LayerProps { .. } | Dialog::Animation { .. } => ModalSpec::default().backdrop(Color32::TRANSPARENT),
            Dialog::Welcome => ModalSpec::default().width(WELCOME_WIDTH).backdrop(Color32::TRANSPARENT).unscrolled(),
            Dialog::ColorPicker { .. } => ModalSpec::default().width(PICKER_DIALOG_WIDTH),
            _ => ModalSpec::default(),
        }
    }
}

pub struct NewImageDialog {
    pub size: SizeFields,
    pub setup: NewDocumentSetup,
}

impl NewImageDialog {
    pub fn new(width: u32, height: u32, setup: NewDocumentSetup) -> Self {
        Self { size: SizeFields::new(width, height), setup }
    }
}

#[derive(Clone, Copy)]
pub struct SizeFields {
    pub width: u32,
    pub height: u32,
    pub keep_aspect: bool,
    ratio: f32,
}

impl SizeFields {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, keep_aspect: true, ratio: width as f32 / height.max(1) as f32 }
    }

    fn show(&mut self, ui: &mut Ui, skin: &skin::Skin) {
        egui::Grid::new("size-fields").num_columns(2).spacing(FORM_GRID_SPACING).show(ui, |ui| {
            ui.label(t(Text::Width));
            let width_changed = drag_value(ui, skin, &mut self.width, 1..=MAX_SIDE, Vec2::ZERO, &PIXEL_FIELD).changed();
            ui.end_row();
            ui.label(t(Text::Height));
            let height_changed = drag_value(ui, skin, &mut self.height, 1..=MAX_SIDE, Vec2::ZERO, &PIXEL_FIELD).changed();
            ui.end_row();
            if self.keep_aspect && width_changed {
                self.height = ((self.width as f32 / self.ratio).round() as u32).clamp(1, MAX_SIDE);
            } else if self.keep_aspect && height_changed {
                self.width = ((self.height as f32 * self.ratio).round() as u32).clamp(1, MAX_SIDE);
            }
        });
        if ui.checkbox(&mut self.keep_aspect, t(Text::KeepAspect)).changed() && self.keep_aspect {
            self.ratio = self.width as f32 / self.height.max(1) as f32;
        }
    }

    fn memory_label(&self) -> String {
        let megabytes = self.width as f64 * self.height as f64 * 4.0 / BYTES_PER_MEGABYTE;
        format!("{}: {megabytes:.1} MB", t(Text::MemoryEstimate))
    }
}

pub struct SizeDialog {
    pub size: SizeFields,
    pub resample: Resample,
    pub anchor: Anchor,
}

pub struct SaveConfigDialog {
    tab: usize,
    path: PathBuf,
    options: SaveOptions,
    raster: Arc<Raster>,
    document: Arc<Document>,
    size_bytes: Option<usize>,
    requested: Option<SaveOptions>,
    encoding: bool,
    tx: Sender<(SaveOptions, Option<Encoded>)>,
    rx: Receiver<(SaveOptions, Option<Encoded>)>,
    preview: Option<TextureHandle>,
}

struct Encoded {
    size: usize,
    preview: Option<Raster>,
}

fn shrunk_for_preview(raster: Raster) -> Raster {
    let longest = raster.width().max(raster.height());
    if longest <= SAVE_PREVIEW_MAX_SIDE {
        return raster;
    }
    let factor = SAVE_PREVIEW_MAX_SIDE as f32 / longest as f32;
    let (width, height) = (((raster.width() as f32 * factor).round() as u32).max(1), ((raster.height() as f32 * factor).round() as u32).max(1));
    raster.resized(width, height, Resample::Anisotropic)
}

fn encode_for_preview(document: &Document, flattened: &Raster, options: &SaveOptions) -> Option<Encoded> {
    let bytes = match *options {
        SaveOptions::Chq { history } => chq::encode_document(document, WriteMode::Small, history).ok()?,
        SaveOptions::Ora => ora::encode_document(document).ok()?,
        _ => chequer_core::io::encode_document(document, options).ok()?,
    };
    let preview = if options.keeps_layers() { Some(flattened.clone()) } else { decode(&bytes).ok() };
    Some(Encoded { size: bytes.len(), preview: preview.map(shrunk_for_preview) })
}

impl SaveConfigDialog {
    pub fn new(tab: usize, path: PathBuf, options: SaveOptions, raster: Raster, document: Document, _ctx: &Context) -> Self {
        let (tx, rx) = channel();
        Self {
            tab,
            path,
            options,
            raster: Arc::new(raster),
            document: Arc::new(document),
            size_bytes: None,
            requested: None,
            encoding: false,
            tx,
            rx,
            preview: None,
        }
    }

    fn refresh_size(&mut self, ctx: &Context) {
        while let Ok((options, encoded)) = self.rx.try_recv() {
            self.encoding = false;
            if options == self.options {
                self.size_bytes = encoded.as_ref().map(|e| e.size);
            }
            if let Some(raster) = encoded.and_then(|e| e.preview) {
                let image = egui::ColorImage::from_rgba_unmultiplied([raster.width() as usize, raster.height() as usize], raster.as_bytes());
                self.preview = Some(ctx.load_texture("save-preview", image, TextureOptions::LINEAR));
            }
        }
        if !self.encoding && self.requested != Some(self.options) {
            self.requested = Some(self.options);
            self.encoding = true;
            self.size_bytes = None;
            let (options, raster, document, tx, ctx) = (self.options, self.raster.clone(), self.document.clone(), self.tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send((options, encode_for_preview(&document, &raster, &options)));
                ctx.request_repaint();
            });
        }
    }
}

fn save_preview(ui: &mut Ui, skin: &skin::Skin, d: &SaveConfigDialog, size: egui::Vec2) {
    let (frame, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(frame);
    painter.rect_filled(frame, 0.0, skin::theme().field);
    let Some(texture) = &d.preview else {
        return;
    };
    let size = texture.size_vec2();
    let fit = (frame.width() / size.x).min(frame.height() / size.y);
    let shown = Rect::from_center_size(frame.center(), size * fit);
    skin.paint_checker(&painter, shown, shown.min);
    painter.image(texture.id(), shown, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
}

fn picker_height_id() -> egui::Id {
    egui::Id::NULL.with(PICKER_HEIGHT_KEY)
}

impl App {
    pub(crate) fn open_resize_dialog(&mut self) {
        let doc = &self.tab().doc;
        let size = SizeFields::new(doc.width(), doc.height());
        self.dialog = Some(Dialog::Resize(SizeDialog { size, resample: Resample::Bicubic, anchor: Anchor::Center }));
    }

    pub(crate) fn open_canvas_dialog(&mut self) {
        let doc = &self.tab().doc;
        let mut size = SizeFields::new(doc.width(), doc.height());
        size.keep_aspect = false;
        self.dialog = Some(Dialog::CanvasSize(SizeDialog { size, resample: Resample::Bicubic, anchor: Anchor::Center }));
    }

    pub(crate) fn open_layer_properties(&mut self, index: usize) {
        if let Some(layer) = self.tab().doc.layers().get(index) {
            let original = layer.props();
            self.dialog = Some(Dialog::LayerProps { index, props: original.clone(), original });
        }
    }

    pub(crate) fn show_dialogs(&mut self, ctx: &Context) {
        let Some(mut dialog) = self.dialog.take() else {
            clear_dialog_keys(ctx);
            return;
        };
        let keep_open = self.show_dialog(ctx, &mut dialog);
        if !keep_open {
            clear_dialog_keys(ctx);
            ctx.data_mut(|data| data.remove_temp::<f32>(picker_height_id()));
        }
        if matches!(dialog, Dialog::Settings(_) | Dialog::ColorPicker { .. } | Dialog::Animation { .. }) && (!keep_open || self.dialog.is_some()) {
            crate::config::save(&self.settings);
        }
        if keep_open && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }

    fn show_dialog(&mut self, ctx: &Context, dialog: &mut Dialog) -> bool {
        let skin = self.skin.clone();
        let skin = skin.as_ref();
        let mut keep_open = true;
        let keys = read_dialog_keys(ctx);
        let modal = show_modal(ctx, skin, dialog.modal_spec(), Some(keys), |ui| {
            match dialog {
                Dialog::New(d) => keep_open = self.new_dialog(ui, skin, d),
                Dialog::Resize(d) => keep_open = self.resize_dialog(ui, skin, d),
                Dialog::CanvasSize(d) => keep_open = self.canvas_dialog(ui, skin, d),
                Dialog::LayerProps { index, props, original } => {
                    keep_open = self.layer_dialog(ui, skin, *index, props, original)
                }
                Dialog::SaveConfig(d) => keep_open = self.save_dialog(ui, ctx, skin, d),
                Dialog::PasteTooBig {
                    raster,
                    new_layer,
                    layer_name,
                } => keep_open = self.paste_dialog(ui, skin, raster, *new_layer, layer_name),
                Dialog::Dropped(paths) => keep_open = self.dropped_dialog(ui, skin, paths),
                Dialog::Confirm { tab, action } => {
                    keep_open = self.confirm_dialog(ui, ctx, skin, *tab, *action)
                }
                Dialog::FlattenWarning { tab, path, options, then } => {
                    keep_open = self.flatten_warning_dialog(ui, ctx, skin, *tab, path, *options, *then)
                }
                Dialog::Animation { fps } => keep_open = self.animation_dialog(ui, skin, fps),
                Dialog::About => keep_open = self.about_dialog(ui, skin),
                Dialog::Welcome => keep_open = self.welcome_dialog(ui, ctx, skin),
                Dialog::FontPicker(d) => keep_open = self.font_picker_dialog(ui, skin, d),
                Dialog::Brushes(d) => keep_open = self.brush_dialog(ui, skin, d),
                Dialog::Settings(tab) => keep_open = self.settings_dialog(ui, skin, tab),
                Dialog::ColorPicker { from_settings } => keep_open = self.color_picker_dialog(ui, ctx, skin, *from_settings),
                Dialog::ThemeOptions(d) => keep_open = self.theme_options_dialog(ui, skin, d),
                Dialog::Error(message) => keep_open = self.error_dialog(ui, skin, message),
            }
        });
        keep_open && !(matches!(dialog, Dialog::Welcome) && !self.tabs.is_empty() && (keys.cancel || modal.should_close()))
    }

    fn new_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, d: &mut NewImageDialog) -> bool {
        title_bar(ui, &t(Text::New).replace("...", ""));
        ui.horizontal_top(|ui| {
            let size_height = ui.vertical(|ui| d.size.show(ui, skin)).response.rect.height();
            let layout = Layout::top_down(Align::Center).with_main_align(Align::Center);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), size_height), layout, |ui| background_picker(ui, skin, &mut d.setup));
        });
        let memory_label = d.size.memory_label();
        let (ok, cancel) = ok_cancel_with(ui, skin, true, |ui| {
            let size = vec2(ui.available_width(), ui.available_height());
            ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| ui.label(egui::RichText::new(memory_label).color(skin::theme().ink_faint)));
        });
        if ok {
            self.settings.new_document = d.setup;
            self.new_document_with(d.size.width, d.size.height, d.setup);
        }
        !(ok || cancel)
    }

    fn resize_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, d: &mut SizeDialog) -> bool {
        title_bar(ui, &t(Text::ResizeImage).replace("...", ""));
        d.size.show(ui, skin);
        ui.horizontal(|ui| {
            ui.label(t(Text::Resampling));
            let name = |r: Resample| match r {
                Resample::Nearest => Text::ResampleNearest,
                Resample::Bilinear => Text::ResampleBilinear,
                Resample::Bicubic => Text::ResampleBicubic,
                Resample::Lanczos => Text::ResampleLanczos,
                Resample::Anisotropic => Text::ResampleAnisotropic,
            };
            choice_combo(ui, skin, "resample", COMBO_WIDTH, &mut d.resample, &labeled(crate::menus::RESAMPLE_METHODS, |r| t(name(r))));
        });
        ui.label(d.size.memory_label());
        let (ok, cancel) = ok_cancel(ui, skin, true);
        if ok {
            let (w, h, resample) = (d.size.width, d.size.height, d.resample);
            let mut error = None;
            self.edit_structure(|doc| error = doc.resize_image(w, h, resample).err());
            if let Some(e) = error {
                self.fail(e.to_string());
            }
        }
        !(ok || cancel)
    }

    fn canvas_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, d: &mut SizeDialog) -> bool {
        title_bar(ui, &t(Text::CanvasSize).replace("...", ""));
        d.size.show(ui, skin);
        ui.label(t(Text::Anchor));
        egui::Grid::new("anchor-grid").spacing(GRID_SPACING_TIGHT).show(ui, |ui| {
            let anchors = [
                [Anchor::TopLeft, Anchor::Top, Anchor::TopRight],
                [Anchor::Left, Anchor::Center, Anchor::Right],
                [Anchor::BottomLeft, Anchor::Bottom, Anchor::BottomRight],
            ];
            for row in anchors {
                for anchor in row {
                    let selected = d.anchor == anchor;
                    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                    let plate = if selected { Plate::button(skin::theme().button_on, Bevel::Sunken) } else { Plate::button(skin::theme().button, Bevel::Raised) };
                    skin.paint_plate(ui.painter(), rect, &plate);
                    if selected {
                        ui.painter().rect_filled(rect.shrink(9.0), 0.0, skin::theme().ink);
                    }
                    if response.clicked() {
                        d.anchor = anchor;
                    }
                }
                ui.end_row();
            }
        });
        let (ok, cancel) = ok_cancel(ui, skin, true);
        if ok {
            let (w, h, anchor) = (d.size.width, d.size.height, d.anchor);
            let mut error = None;
            self.edit_structure(|doc| error = doc.resize_canvas(w, h, anchor).err());
            if let Some(e) = error {
                self.fail(e.to_string());
            }
        }
        !(ok || cancel)
    }

    fn layer_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, index: usize, props: &mut LayerProps, original: &LayerProps) -> bool {
        title_bar(ui, &t(Text::LayerProperties).replace("...", ""));
        ui.label(t(Text::LayerName));
        field_scope(ui, skin, |ui| ui.add(egui::TextEdit::singleline(&mut props.name).desired_width(f32::INFINITY)));
        ui.label(t(Text::LayerOpacity));
        let mut opacity = props.opacity;
        if slider_with_number(ui, skin, &mut opacity, 0..=u8::MAX, 1.0, NumberFormat::WHOLE) {
            props.opacity = opacity;
        }
        ui.horizontal(|ui| {
            ui.label(t(Text::LayerBlendMode));
            choice_combo(ui, skin, "layer-blend", COMBO_WIDTH, &mut props.blend, &Self::blend_labels());
        });
        ui.checkbox(&mut props.visible, t(Text::LayerVisible));
        let (ok, cancel) = ok_cancel(ui, skin, true);
        let applied = if ok || cancel { original } else { &*props };
        let tab = self.tab_mut();
        tab.doc.preview_layer_props(index, applied);
        tab.mark_all_dirty();
        if ok {
            let props = props.clone();
            self.edit_structure(|doc| doc.set_layer_props(index, props));
        }
        !(ok || cancel)
    }

    fn save_dialog(&mut self, ui: &mut Ui, ctx: &Context, skin: &skin::Skin, d: &mut SaveConfigDialog) -> bool {
        title_bar(ui, t(Text::SaveConfig));
        d.refresh_size(ctx);
        let side_by_side_width = SAVE_SETTINGS_WIDTH + SAVE_PREVIEW_SIZE.x + ui.spacing().item_spacing.x;
        if ui.available_width() >= side_by_side_width {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(SAVE_SETTINGS_WIDTH);
                    Self::save_settings(ui, skin, d);
                });
                save_preview(ui, skin, d, SAVE_PREVIEW_SIZE);
            });
        } else {
            Self::save_settings(ui, skin, d);
            let width = ui.available_width();
            save_preview(ui, skin, d, vec2(width, width * SAVE_PREVIEW_SIZE.y / SAVE_PREVIEW_SIZE.x));
        }
        let (ok, cancel) = ok_cancel(ui, skin, true);
        if ok {
            self.write_tab(d.tab, d.path.clone(), d.options);
        }
        !(ok || cancel)
    }

    fn save_settings(ui: &mut Ui, skin: &skin::Skin, d: &mut SaveConfigDialog) {
        ui.horizontal(|ui| {
            ui.label(t(Text::FileFormat));
            let mut extension = d.options.extension();
            let formats = labeled(SAVE_EXTENSIONS, |extension| extension_label(extension));
            if choice_combo(ui, skin, "format", COMBO_WIDTH, &mut extension, &formats) {
                if let Some(options) = SaveOptions::for_extension(extension) {
                    d.options = options;
                    d.path.set_extension(extension);
                }
            }
        });
        ui.label(d.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        ui.separator();
        format_options(ui, skin, &mut d.options);
        let size = d.size_bytes.map(format_bytes).unwrap_or_else(|| "...".to_owned());
        ui.label(format!("{}: {size}", t(Text::FileSize)));
    }

    fn paste_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, raster: &Raster, new_layer: bool, layer_name: &Option<String>) -> bool {
        title_bar(ui, t(Text::Paste));
        let doc = &self.tab().doc;
        ui.label(t(Text::PasteTooBig));
        ui.label(format!("{}: {}×{}    {}: {}×{}", t(Text::PasteImageSize), raster.width(), raster.height(), t(Text::PasteCanvasSize), doc.width(), doc.height()));
        let keys = dialog_keys(ui);
        let as_is = choice(ui, skin, t(Text::PasteAsIs), t(Text::PasteAsIsHint)) || keys.accept;
        let expand = choice(ui, skin, t(Text::PasteExpand), t(Text::PasteExpandHint));
        let cancel = cancel_button(ui, skin) || keys.cancel;
        if expand {
            let (w, h) = (raster.width().max(self.tab().doc.width()), raster.height().max(self.tab().doc.height()));
            self.edit_structure(|doc| {
                let _ = doc.resize_canvas(w, h, Anchor::TopLeft);
            });
        }
        if as_is || expand {
            self.finish_paste(raster.clone(), new_layer, layer_name.clone());
        }
        !(as_is || expand || cancel)
    }

    fn dropped_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, paths: &[PathBuf]) -> bool {
        title_bar(ui, t(Text::DropTitle));
        let names: Vec<String> = paths.iter().filter_map(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned()).collect();
        ui.label(if let [only] = names.as_slice() { only.clone() } else { format!("{}: {}", t(Text::DropFilesCount), names.len()) });
        let keys = dialog_keys(ui);
        let open = choice(ui, skin, t(Text::DropOpenTab), t(Text::DropOpenTabHint)) || keys.accept;
        let layer = choice(ui, skin, t(Text::DropAddLayer), t(Text::DropAddLayerHint));
        let cancel = cancel_button(ui, skin) || keys.cancel;
        if open {
            paths.iter().for_each(|path| self.open_path(path));
        }
        if layer {
            self.add_files_as_layers(paths.to_vec());
        }
        !(open || layer || cancel)
    }

    fn confirm_dialog(&mut self, ui: &mut Ui, ctx: &Context, skin: &skin::Skin, tab: usize, action: ConfirmAction) -> bool {
        title_bar(ui, t(Text::UnsavedTitle));
        ui.label(format!("{}\n{}", self.tabs.get(tab).map(|t| t.name.as_str()).unwrap_or_default(), t(Text::UnsavedBody)));
        let (mut save, mut discard, mut cancel) = (false, false, false);
        let escape = dialog_keys(ui).cancel;
        ui.add_space(GAP_ROW);
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            cancel = text_button(ui, skin, t(Text::Cancel)).clicked();
            discard = text_button(ui, skin, t(Text::DontSave)).clicked();
            save = text_button(ui, skin, t(Text::Save)).clicked();
        });
        cancel |= escape;
        if tab >= self.tabs.len() {
            return false;
        }
        if save {
            let known = self.tabs[tab].path.clone().zip(self.tabs[tab].save_options);
            match known {
                Some((path, options)) => self.save_known(ctx, tab, path, options, Some(action)),
                None => self.run(ctx, Command::SaveAs),
            }
        }
        if discard {
            self.tabs[tab].mark_saved();
            self.finish_confirmed(ctx, tab, action);
        }
        !(save || discard || cancel)
    }

    #[allow(clippy::too_many_arguments)]
    fn flatten_warning_dialog(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        skin: &skin::Skin,
        tab: usize,
        path: &Path,
        options: SaveOptions,
        then: Option<ConfirmAction>,
    ) -> bool {
        let doc = &self.tabs[tab].doc;
        let flattens = doc.layers().len() > 1 && !options.keeps_layers();
        let drops_frames = doc.frame_count() > 1 && !options.keeps_frames();
        title_bar(ui, t(if flattens { Text::FlattenTitle } else { Text::FramesTitle }));
        if flattens {
            ui.label(t(Text::FlattenBody));
        }
        if drops_frames {
            ui.label(t(Text::SaveFramesWarning));
        }
        let (ok, cancel) = ok_cancel(ui, skin, true);
        if ok && tab < self.tabs.len() {
            self.write_tab(tab, path.to_owned(), options);
            if let Some(action) = then {
                self.finish_confirmed(ctx, tab, action);
            }
        }
        !(ok || cancel)
    }

    pub(crate) fn save_known(&mut self, ctx: &Context, tab: usize, path: PathBuf, options: SaveOptions, then: Option<ConfirmAction>) {
        let doc = &self.tabs[tab].doc;
        if (doc.layers().len() > 1 && !options.keeps_layers()) || (doc.frame_count() > 1 && !options.keeps_frames()) {
            self.dialog = Some(Dialog::FlattenWarning { tab, path, options, then });
            return;
        }
        self.write_tab(tab, path, options);
        if let Some(action) = then {
            self.finish_confirmed(ctx, tab, action);
        }
    }

    fn finish_confirmed(&mut self, ctx: &Context, tab: usize, action: ConfirmAction) {
        match action {
            ConfirmAction::CloseTab => self.remove_tab(tab),
            ConfirmAction::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn about_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin) -> bool {
        title_bar(ui, t(Text::About));
        let (rect, _) = ui.allocate_exact_size(skin.logo_size(ui.painter().pixels_per_point()), Sense::hover());
        skin.paint_logo(ui.painter(), rect);
        ui.label(format!("{APP_NAME} {APP_VERSION}"));
        ui.label(t(Text::AboutBody));
        ui.label(egui::RichText::new(t(Text::AboutIcons)).color(skin::theme().ink_dim).small());
        let (ok, _) = ok_cancel_single(ui, skin);
        if ok {
            self.dialog = Some(Dialog::Settings(SettingsTab::General));
        }
        !ok
    }

    fn settings_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, tab: &mut SettingsTab) -> bool {
        title_bar(ui, t(Text::Settings));
        tab_bar(ui, ui.scope_id().with("settings-tabs"), tab, &SettingsTab::ALL.map(|(value, label)| (value, t(label))));
        ui.add_space(SETTINGS_TAB_GAP);
        let body_height = SETTINGS_BODY_HEIGHT;
        egui::ScrollArea::vertical().id_salt(*tab).max_height(body_height).min_scrolled_height(body_height).auto_shrink([false, false]).show(ui, |ui| match tab {
            SettingsTab::General => self.general_settings(ui, skin),
            SettingsTab::Interface => self.interface_settings(ui, skin),
            SettingsTab::Gestures => self.gesture_settings(ui, skin),
            SettingsTab::Themes => self.theme_list_settings(ui, skin),
        }).with_fx(ui, skin::theme().window);
        let pending_id = ui.scope_id().with(PENDING_RESET_KEY);
        let mut pending: Option<SettingsReset> = ui.data(|data| data.get_temp(pending_id)).flatten();
        if let Some(scope) = pending {
            let escape = take_dialog_escape(ui);
            if let Some(confirmed) = confirm_modal(ui.ctx(), skin, t(Text::Reset), t(scope.question()), escape) {
                if confirmed {
                    self.reset_settings(ui.ctx(), scope);
                }
                pending = None;
            }
        }
        let keys = dialog_keys(ui);
        let mut ok = keys.accept || keys.cancel;
        ui.add_space(BUTTON_ROW_LEAD);
        pinned_footer(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ok |= text_button(ui, skin, t(Text::Ok)).clicked();
                if text_button(ui, skin, t(Text::ResetAll)).clicked() {
                    pending = Some(SettingsReset::All);
                }
                if text_button(ui, skin, t(Text::ResetTab)).clicked() {
                    pending = Some(SettingsReset::Tab(*tab));
                }
            });
        });
        ui.data_mut(|data| data.insert_temp(pending_id, pending));
        !ok
    }

    fn reset_settings(&mut self, ctx: &Context, scope: SettingsReset) {
        match scope {
            SettingsReset::Tab(tab) => self.settings.reset_tab(tab),
            SettingsReset::All => self.settings = crate::app::Settings::default(),
        }
        set_lang(self.settings.lang);
        self.apply_theme(ctx);
        crate::config::save(&self.settings);
    }

    fn color_picker_dialog(&mut self, ui: &mut Ui, ctx: &Context, skin: &skin::Skin, from_settings: bool) -> bool {
        title_bar(ui, t(Text::ColorPickerTitle));
        let mut divider_x = 0.0;
        let columns = ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(PICKER_CONTROLS_WIDTH);
                let selectors = crate::panels::SELECTOR_LABELS.map(|(selector, label)| (selector, t(label)));
                let width = ui.available_width();
                choice_combo(ui, skin, "picker-selector", width, &mut self.settings.picker.selector, &selectors);
                ui.separator();
                crate::panels::picker_channel_controls(ui, &mut self.settings.picker);
                ui.separator();
                ui.checkbox(&mut self.settings.compact_colors, t(Text::CompactColors));
                ui.checkbox(&mut self.settings.palette_below, t(Text::PaletteBelow));
            });
            divider_x = ui.allocate_exact_size(vec2(PICKER_DIVIDER_GAP, 0.0), Sense::hover()).0.center().x;
            ui.vertical(|ui| {
                ui.set_width(if self.settings.picker.beside { PICKER_PREVIEW_WIDTH_BESIDE } else { PICKER_PREVIEW_WIDTH });
                ui.label(egui::RichText::new(t(Text::ColorPickerPreview)).color(skin::theme().ink_dim));
                self.colors_body(ui, ctx, crate::panels::LayoutMenu::None);
            });
        });
        let content_height = ui.min_rect().height();
        let reserved_height = ui.data_mut(|data| {
            let reserved = data.get_temp_mut_or_insert_with::<f32>(picker_height_id(), || content_height);
            *reserved = reserved.max(content_height);
            *reserved
        });
        let padding = reserved_height - content_height;
        ui.add_space(padding);
        let divider_stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        ui.painter().vline(divider_x, columns.response.rect.top()..=columns.response.rect.bottom() + padding, divider_stroke);
        let (ok, _) = ok_cancel_single(ui, skin);
        if ok && from_settings {
            self.dialog = Some(Dialog::Settings(SettingsTab::Interface));
        }
        !ok
    }

    fn general_settings(&mut self, ui: &mut Ui, skin: &skin::Skin) {
        settings_group(ui, t(Text::GroupLanguageWindows), |ui| {
            form_row(ui, t(Text::Language), |ui| {
                let mut lang = self.settings.lang;
                if choice_combo(ui, skin, "lang", COMBO_WIDTH, &mut lang, &[(Lang::Ru, "Русский"), (Lang::En, "English")]) {
                    self.settings.lang = lang;
                    set_lang(lang);
                }
            });
            form_row(ui, t(Text::WindowLayout), |ui| {
                if text_button(ui, skin, t(Text::Reset)).clicked() {
                    self.layout_pending = true;
                }
            });
        });
        settings_group(ui, t(Text::GroupSettingsFile), |ui| {
            form_row(ui, t(Text::Settings), |ui| {
                if text_button(ui, skin, t(Text::ExportSettings)).clicked() {
                    self.pick_settings_export_path(ui.ctx());
                }
                if text_button(ui, skin, t(Text::ImportSettings)).clicked() {
                    self.pick_settings_import_path(ui.ctx());
                }
            });
        });
        settings_group(ui, t(Text::GroupApp), |ui| {
            form_row(ui, t(Text::AboutDetails), |ui| {
                if text_button(ui, skin, t(Text::About)).clicked() {
                    self.dialog = Some(Dialog::About);
                }
            });
        });
    }

    fn gesture_settings(&mut self, ui: &mut Ui, skin: &skin::Skin) {
        settings_group(ui, t(Text::GroupTrackpad), |ui| {
            let trackpad = &mut self.settings.trackpad;
            form_row(ui, t(Text::TrackpadMode), |ui| {
                let modes = [(TrackpadMode::Auto, t(Text::TrackpadAuto)), (TrackpadMode::System, t(Text::TrackpadSystem))];
                choice_combo(ui, skin, "trackpad-mode", GESTURE_MODE_COMBO_WIDTH, &mut trackpad.mode, &modes);
            });
            let mut percent = (trackpad.pinch_sensitivity * 100.0).round();
            let range = PINCH_SENSITIVITY_MIN * 100.0..=PINCH_SENSITIVITY_MAX * 100.0;
            if form_slider(ui, skin, t(Text::PinchSensitivity), &mut percent, range, PINCH_SENSITIVITY_STEP_PERCENT) {
                trackpad.pinch_sensitivity = percent / 100.0;
            }
            form_row(ui, t(Text::GestureRotate), |ui| ui.checkbox(&mut trackpad.rotate, ""));
            form_row(ui, t(Text::GestureSwipes), |ui| ui.checkbox(&mut trackpad.swipes, ""));
        });
        self.pen_settings(ui, skin);
    }

    fn pen_settings(&mut self, ui: &mut Ui, skin: &skin::Skin) {
        settings_group(ui, t(Text::GroupPen), |ui| {
            form_row(ui, t(Text::PenOnly), |ui| ui.checkbox(&mut self.settings.pen_only, ""));
            form_row(ui, t(Text::Haptics), |ui| ui.checkbox(&mut self.settings.haptics, ""));
            form_row(ui, t(Text::VolumeUndo), |ui| ui.checkbox(&mut self.settings.volume_undo, ""));
            form_row(ui, t(Text::PenWayland), |ui| ui.checkbox(&mut self.settings.pen_wayland, "").on_hover_text(t(Text::PenWaylandHint)));
            let response = &mut self.settings.pen_response;
            form_slider(ui, skin, t(Text::PenCurve), &mut response.curve, pen::CURVE_RANGE.0..=pen::CURVE_RANGE.1, PEN_CURVE_STEP);
            for (label, value, max) in [
                (Text::PenDeadZone, &mut response.dead_zone, pen::DEAD_ZONE_MAX),
                (Text::PenSmoothing, &mut response.smoothing, pen::SMOOTHING_MAX),
                (Text::PenMinPressure, &mut response.min_pressure, pen::MIN_PRESSURE_MAX),
            ] {
                let mut percent = *value * 100.0;
                if form_slider(ui, skin, t(label), &mut percent, 0.0..=max * 100.0, 1.0) {
                    *value = percent / 100.0;
                }
            }
        });
    }

    fn interface_settings(&mut self, ui: &mut Ui, skin: &skin::Skin) {
        settings_group(ui, t(Text::GroupAppearance), |ui| {
            self.scale_settings(ui, skin);
            self.font_settings(ui, skin);
            form_row(ui, t(Text::HighContrastIcons), |ui| {
                if ui.checkbox(&mut self.settings.high_contrast_icons, "").changed() {
                    crate::icons::set_high_contrast(self.settings.high_contrast_icons);
                    skin.icons.clear_rendered();
                }
            });
            form_row(ui, t(Text::ZoomOutFilter), |ui| {
                let filters = [(ZoomOutFilter::Smooth, t(Text::ZoomOutSmooth)), (ZoomOutFilter::Pixel, t(Text::ZoomOutPixel))];
                choice_combo(ui, skin, "zoom-out-filter", GESTURE_MODE_COMBO_WIDTH, &mut self.settings.zoom_out_filter, &filters);
            });
            for (label, color, theme_color) in [
                (Text::CheckerLight, &mut self.settings.checker_light, skin::theme().checker_light),
                (Text::CheckerDark, &mut self.settings.checker_dark, skin::theme().checker_dark),
            ] {
                checker_color_row(ui, skin, t(label), color, theme_color);
            }
            form_row(ui, t(Text::GrainEnabled), |ui| ui.checkbox(&mut self.settings.grain_enabled, ""));
            ui.add_enabled_ui(self.settings.grain_enabled, |ui| {
                let mut percent = self.settings.grain * 100.0;
                if form_slider(ui, skin, t(Text::Grain), &mut percent, 0.0..=100.0, 1.0) {
                    self.settings.grain = percent / 100.0;
                }
            });
        });
        settings_group(ui, t(Text::GroupPanels), |ui| {
            form_row(ui, t(Text::GroupColorPicker), |ui| {
                if text_button(ui, skin, t(Text::ColorPickerCustomize)).clicked() {
                    self.dialog = Some(Dialog::ColorPicker { from_settings: true });
                }
            });
            ui.checkbox(&mut self.settings.layer_quick_settings, t(Text::LayerQuickSettings));
            form_row(ui, t(Text::LayerDoubleClick), |ui| {
                let actions = [(LayerDoubleClick::Properties, t(Text::LayerProperties)), (LayerDoubleClick::Rename, t(Text::LayerDoubleClickRename))];
                choice_combo(ui, skin, "layer-double-click", GESTURE_MODE_COMBO_WIDTH, &mut self.settings.layer_double_click, &actions);
            });
        });
    }

    fn scale_settings(&mut self, ui: &mut Ui, skin: &skin::Skin) {
        let mut system = self.settings.ui_scale.is_none();
        let toggled = form_row(ui, t(Text::UiScale), |ui| ui.checkbox(&mut system, t(Text::UiScaleAuto)).changed());
        if toggled {
            let current = ui.ctx().pixels_per_point();
            self.settings.ui_scale = (!system).then_some(current);
        }
        if let Some(scale) = self.settings.ui_scale {
            let draft_id = Id::NULL.with(UI_SCALE_DRAFT_ID);
            let mut percent = ui.data(|data| data.get_temp::<f32>(draft_id)).unwrap_or_else(|| (scale * 100.0).round());
            if form_slider(ui, skin, "", &mut percent, UI_SCALE_MIN_PERCENT..=UI_SCALE_MAX_PERCENT, UI_SCALE_STEP_PERCENT) {
                ui.data_mut(|data| data.insert_temp(draft_id, percent));
            }
            let released = !ui.input(|input| input.pointer.any_down());
            if let Some(draft) = ui.data(|data| data.get_temp::<f32>(draft_id)).filter(|_| released) {
                self.settings.ui_scale = Some(draft / 100.0);
                ui.data_mut(|data| data.remove_temp::<f32>(draft_id));
                ui.ctx().request_repaint();
            }
        }
    }

    fn font_settings(&mut self, ui: &mut Ui, skin: &skin::Skin) {
        if self.system_fonts == SystemFonts::NotLoaded {
            self.load_system_fonts(ui.ctx());
        }
        let current = match &self.settings.font {
            FontChoice::Builtin => t(Text::FontBuiltin).to_owned(),
            FontChoice::System(family) => family.clone(),
            FontChoice::File(path) => path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned()),
        };
        let mut chosen = None;
        form_row(ui, t(Text::Font), |ui| {
            let combo = combo_box(ui, skin, "font", FONT_COMBO_WIDTH, &current, |ui| {
                if ui.selectable_label(self.settings.font == FontChoice::Builtin, t(Text::FontBuiltin)).clicked() {
                    chosen = Some(FontChoice::Builtin);
                }
                match &self.system_fonts {
                    SystemFonts::Ready(families) => {
                        for family in families {
                            let selected = self.settings.font == FontChoice::System(family.clone());
                            if ui.selectable_label(selected, family).clicked() {
                                chosen = Some(FontChoice::System(family.clone()));
                            }
                        }
                    }
                    SystemFonts::NotLoaded | SystemFonts::Loading => {
                        ui.label(t(Text::FontLoading));
                    }
                }
            });
            if let SystemFonts::Ready(families) = &self.system_fonts {
                let choices: Vec<FontChoice> = std::iter::once(FontChoice::Builtin).chain(families.iter().cloned().map(FontChoice::System)).collect();
                let position = choices.iter().position(|choice| *choice == self.settings.font);
                let wheel = crate::widgets::wheel_over(ui, &combo);
                if let Some(target) = crate::widgets::stepped_index(choices.len(), position, wheel.notches) {
                    chosen = Some(choices[target].clone());
                }
            }
            if glyph_button(ui, skin, "...", false, t(Text::TextFontBrowse)).clicked() {
                self.open_font_picker(ui.ctx(), crate::font_picker::FontPickerTarget::Interface);
            }
        });
        if let Some(choice) = chosen {
            self.settings.font = choice;
        }
    }

    pub(crate) fn apply_theme(&mut self, ctx: &Context) {
        let spec = self.themes.find(&self.settings.theme_id);
        let theme = self.settings.resolved_theme(spec);
        let icons_dir = spec.icons_dir.clone();
        self.settings.theme_id = spec.id.clone();
        self.settings.ui_pixel = spec.pixel;
        let ink_changed = theme.ink != skin::theme().ink;
        if theme != skin::theme() {
            skin::set_theme(theme);
            skin::apply_style(ctx);
        }
        if icons_dir != self.applied_icons || ink_changed {
            self.skin = std::sync::Arc::new(self.skin.with_icons(ctx, icons_dir.as_deref()));
            self.applied_icons = icons_dir;
        }
    }

    fn theme_list_settings(&mut self, ui: &mut Ui, skin: &skin::Skin) {
        let ctx = ui.ctx().clone();
        settings_group(ui, t(Text::SettingsThemes), |ui| {
            let current = self.themes.find(&self.settings.theme_id).name.get().to_owned();
            let mut chosen = None;
            form_row(ui, t(Text::GroupTheme), |ui| {
                let combo = combo_box(ui, skin, "theme", THEME_COMBO_WIDTH, &current, |ui| {
                    for spec in &self.themes.specs {
                        if ui.selectable_label(spec.id == self.settings.theme_id, spec.name.get()).clicked() {
                            chosen = Some(spec.id.clone());
                        }
                    }
                });
                let position = self.themes.specs.iter().position(|spec| spec.id == self.settings.theme_id);
                let wheel = crate::widgets::wheel_over(ui, &combo);
                if let Some(target) = crate::widgets::stepped_index(self.themes.specs.len(), position, wheel.notches) {
                    chosen = Some(self.themes.specs[target].id.clone());
                }
                if text_button(ui, skin, t(Text::ThemeSettings)).clicked() {
                    self.dialog = Some(Dialog::ThemeOptions(ThemeOptionsDialog {
                        values_before: self.settings.theme_values.clone(),
                        colors_before: self.settings.theme_colors.clone(),
                        confirming_reset: false,
                    }));
                }
            });
            if let Some(id) = chosen {
                self.settings.theme_id = id;
            }
            form_row(ui, t(Text::ThemesCustom), |ui| {
                if text_button(ui, skin, t(Text::ThemesReload)).clicked() {
                    self.themes = crate::themes::ThemeLibrary::load();
                }
            });
            if let Some(directory) = crate::config::themes_dir() {
                ui.label(egui::RichText::new(format!("{} {}", t(Text::ThemesFolder), directory.display())).color(skin::theme().ink_dim).small());
            }
            for problem in &self.themes.problems {
                ui.label(egui::RichText::new(problem).color(skin::theme().close_hover).small());
            }
        });
        self.apply_theme(&ctx);
    }

    fn theme_options_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, d: &mut ThemeOptionsDialog) -> bool {
        let ctx = ui.ctx().clone();
        let spec = self.themes.find(&self.settings.theme_id).clone();
        title_bar(ui, spec.name.get());
        egui::ScrollArea::vertical().max_height(SETTINGS_BODY_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
            if !spec.settings.is_empty() {
                settings_group(ui, t(Text::GroupThemeOptions), |ui| self.theme_value_controls(ui, skin, &spec));
            }
            settings_group(ui, t(Text::GroupThemeColors), |ui| self.theme_color_controls(ui, &spec));
        }).with_fx(ui, skin::theme().window);
        self.apply_theme(&ctx);
        let keys = dialog_keys(ui);
        let (mut ok, mut cancel, mut reset) = (false, false, false);
        ui.add_space(GAP_ROW);
        if d.confirming_reset {
            ui.label(t(Text::ResetThemeConfirm));
            pinned_footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    d.confirming_reset = !(text_button(ui, skin, t(Text::Cancel)).clicked() || keys.cancel);
                    reset = text_button(ui, skin, t(Text::Reset)).clicked();
                });
            });
        } else {
            pinned_footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    ok = text_button(ui, skin, t(Text::Ok)).clicked() || keys.accept;
                    cancel = text_button(ui, skin, t(Text::Cancel)).clicked() || keys.cancel;
                    d.confirming_reset = text_button(ui, skin, t(Text::Reset)).clicked();
                });
            });
        }
        if reset {
            let prefix = crate::themes::scoped_key(&spec.id, "");
            self.settings.theme_values.retain(|key, _| !key.starts_with(&prefix));
            self.settings.theme_colors.retain(|key, _| !key.starts_with(&prefix));
            d.confirming_reset = false;
        }
        if cancel {
            self.settings.theme_values = d.values_before.clone();
            self.settings.theme_colors = d.colors_before.clone();
        }
        if ok || cancel {
            self.dialog = Some(Dialog::Settings(SettingsTab::Themes));
        }
        !(ok || cancel)
    }

    fn theme_value_controls(&mut self, ui: &mut Ui, skin: &skin::Skin, spec: &crate::themes::ThemeSpec) {
        use crate::themes::{SettingKind, scoped_key};
        for setting in &spec.settings {
            let mut value = spec.value(setting, &self.settings.theme_values);
            let changed = match &setting.kind {
                SettingKind::Toggle => {
                    let mut on = value > 0.0;
                    let changed = ui.checkbox(&mut on, setting.label.get()).changed();
                    value = f64::from(on);
                    changed
                }
                SettingKind::Number { min, max, step } => {
                    ui.label(setting.label.get());
                    slider_with_number(ui, skin, &mut value, *min..=*max, *step, NumberFormat::WHOLE)
                }
                SettingKind::Choice(options) => {
                    let items: Vec<(usize, &str)> = options.iter().enumerate().map(|(index, (label, _))| (index, label.get())).collect();
                    let mut index = options.iter().position(|(_, option)| *option == value).unwrap_or(0);
                    let mut changed = false;
                    settings_row(ui, &format!("theme-choice-{}", setting.id), setting.label.get(), |ui| {
                        changed = choice_combo(ui, skin, &format!("theme-choice-combo-{}", setting.id), COMBO_WIDTH, &mut index, &items);
                    });
                    value = options[index].1;
                    changed
                }
            };
            if changed {
                self.settings.theme_values.insert(scoped_key(&spec.id, &setting.id), value);
                for driven in &setting.drives {
                    self.settings.theme_values.insert(scoped_key(&spec.id, driven), value);
                }
            }
        }
    }

    fn theme_color_controls(&mut self, ui: &mut Ui, spec: &crate::themes::ThemeSpec) {
        let mut theme = skin::theme();
        for (key, ru, en, color) in theme.colors_mut() {
            ui.horizontal(|ui| {
                let mut rgba = color.to_srgba_unmultiplied();
                if crate::color_popup::color_button(ui, &self.skin, &mut rgba, false).changed() {
                    *color = Color32::from_rgb(rgba[0], rgba[1], rgba[2]);
                    self.settings.theme_colors.insert(crate::themes::scoped_key(&spec.id, key), *color);
                }
                ui.label(if crate::i18n::lang() == Lang::Ru { ru } else { en });
            });
        }
    }

    fn animation_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, fps: &mut u32) -> bool {
        title_bar(ui, t(Text::AnimationSettings));
        let mixed = self.tab().doc.uniform_duration().is_none();
        settings_group(ui, t(Text::AnimationPlayback), |ui| {
            let mut apply = false;
            form_row(ui, t(Text::AnimationFps), |ui| {
                let spec = DragSpec { max_decimals: Some(0), ..DragSpec::default() };
                drag_value(ui, skin, fps, MIN_FPS..=MAX_FPS, [FPS_FIELD_WIDTH, SLIDER_ROW_HEIGHT], &spec);
                apply = text_button(ui, skin, t(Text::ApplyToAllFrames)).clicked();
            });
            if mixed {
                ui.label(egui::RichText::new(t(Text::AnimationMixedFps)).color(skin::theme().ink_dim).small());
            }
            if apply {
                let duration_ms = fps_to_duration_ms(*fps);
                self.edit_structure(|doc| {
                    doc.set_all_frame_durations(duration_ms);
                });
            }
        });
        settings_group(ui, t(Text::OnionSection), |ui| {
            ui.checkbox(&mut self.timeline.onion, t(Text::OnionEnabled));
            let onion = &mut self.settings.animation.onion;
            form_slider(ui, skin, t(Text::OnionBefore), &mut onion.before, 0..=ONION_MAX_FRAMES, 1.0);
            form_slider(ui, skin, t(Text::OnionAfter), &mut onion.after, 0..=ONION_MAX_FRAMES, 1.0);
            form_slider(ui, skin, t(Text::OnionOpacity), &mut onion.opacity, 0..=PERCENT_MAX, 1.0);
            onion_color_row(ui, skin, t(Text::OnionColorBefore), &mut onion.before_color);
            onion_color_row(ui, skin, t(Text::OnionColorAfter), &mut onion.after_color);
        });
        settings_group(ui, t(Text::AnimationKeysSection), |ui| {
            let keys = &mut self.settings.animation.keys;
            ui.checkbox(&mut keys.space_plays, t(Text::SpacePlays));
            ui.checkbox(&mut keys.arrows_navigate, t(Text::ArrowsNavigate));
            ui.checkbox(&mut keys.wheel_navigates, t(Text::WheelNavigates));
        });
        let (ok, _) = ok_cancel_single(ui, skin);
        !ok
    }

    fn error_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, message: &str) -> bool {
        title_bar(ui, t(Text::ErrorTitle));
        ui.label(message);
        let (ok, _) = ok_cancel_single(ui, skin);
        !ok
    }
}

fn settings_group<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(GAP_TIGHT);
    ui.label(egui::RichText::new(title).color(skin::theme().ink_dim));
    ui.separator();
    add_contents(ui)
}

fn background_picker(ui: &mut Ui, skin: &skin::Skin, setup: &mut NewDocumentSetup) {
    choice_combo(ui, skin, "new-background", NEW_BACKGROUND_COMBO_WIDTH, &mut setup.background, &NewBackground::labeled());
    match setup.background {
        NewBackground::Color => {
            crate::color_popup::color_button(ui, skin, &mut setup.color, false);
        }
        NewBackground::Transparent => {
            ui.allocate_space(vec2(0.0, crate::color_popup::SWATCH_SIZE[1]));
        }
    }
}

fn onion_color_row(ui: &mut Ui, skin: &skin::Skin, label: &str, color: &mut [u8; 3]) {
    form_row(ui, label, |ui| {
        let mut rgba = [color[0], color[1], color[2], u8::MAX];
        if crate::color_popup::color_button(ui, skin, &mut rgba, false).changed() {
            *color = [rgba[0], rgba[1], rgba[2]];
        }
    });
}

fn checker_color_row(ui: &mut Ui, skin: &skin::Skin, label: &str, slot: &mut Option<Color32>, shown: Color32) {
    form_row(ui, label, |ui| {
        let mut rgba = shown.to_srgba_unmultiplied();
        if crate::color_popup::color_button(ui, skin, &mut rgba, false).changed() {
            *slot = Some(Color32::from_rgb(rgba[0], rgba[1], rgba[2]));
        }
        if slot.is_some() && text_button(ui, skin, t(Text::CheckerFromTheme)).clicked() {
            *slot = None;
        }
    });
}

fn settings_row(ui: &mut Ui, id: &str, label: &str, add_control: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id).num_columns(2).spacing(FORM_GRID_SPACING).show(ui, |ui| {
        ui.label(label);
        add_control(ui);
        ui.end_row();
    });
}

fn choice(ui: &mut Ui, skin: &skin::Skin, label: &str, hint: &str) -> bool {
    ui.add_space(GAP_TIGHT);
    let clicked = text_button_sized(ui, skin, label, vec2(CHOICE_BUTTON_WIDTH, 0.0)).clicked();
    ui.label(egui::RichText::new(hint).color(skin::theme().ink_dim).small());
    clicked
}

fn cancel_button(ui: &mut Ui, skin: &skin::Skin) -> bool {
    ui.add_space(GAP_ROW);
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| text_button(ui, skin, t(Text::Cancel)).clicked()).inner
}

fn ok_cancel_single(ui: &mut Ui, skin: &skin::Skin) -> (bool, bool) {
    let mut ok = false;
    let keys = dialog_keys(ui);
    ui.add_space(GAP_ROW);
    pinned_footer(ui, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| ok = text_button(ui, skin, t(Text::Ok)).clicked());
    });
    (ok || keys.accept || keys.cancel, false)
}

fn format_options(ui: &mut Ui, skin: &skin::Skin, options: &mut SaveOptions) {
    match options {
        SaveOptions::Png { color, compression } => {
            ui.horizontal(|ui| {
                ui.label(t(Text::BitDepth));
                let name = |c: PngColor| match c {
                    PngColor::Auto => Text::PngAuto,
                    PngColor::Rgba32 => Text::Png32,
                    PngColor::Rgb24 => Text::Png24,
                };
                choice_combo(ui, skin, "png-color", PNG_COLOR_COMBO_WIDTH, color, &labeled([PngColor::Auto, PngColor::Rgba32, PngColor::Rgb24], |c| t(name(c))));
            });
            ui.horizontal(|ui| {
                ui.label(t(Text::Compression));
                let name = |c: PngCompression| match c {
                    PngCompression::Fast => Text::CompressFast,
                    PngCompression::Default => Text::CompressDefault,
                    PngCompression::Best => Text::CompressBest,
                };
                choice_combo(ui, skin, "png-compression", COMBO_WIDTH, compression, &labeled([PngCompression::Fast, PngCompression::Default, PngCompression::Best], |c| t(name(c))));
            });
        }
        SaveOptions::Jpeg { quality } => {
            ui.label(t(Text::JpegQuality));
            slider_with_number(ui, skin, quality, JPEG_QUALITY_MIN..=JPEG_QUALITY_MAX, 1.0, NumberFormat::WHOLE);
        }
        SaveOptions::Bmp { alpha } | SaveOptions::Tga { alpha } => {
            ui.checkbox(alpha, t(Text::WithAlpha));
        }
        SaveOptions::WebpLossless => {
            ui.label(t(Text::WebpNote));
        }
        SaveOptions::Chq { history } => {
            ui.checkbox(history, t(Text::SaveHistory));
        }
        SaveOptions::Gif | SaveOptions::Tiff | SaveOptions::Ico | SaveOptions::Ora => {
            ui.label(t(Text::NoOptions));
        }
    }
}

fn form_row<R>(ui: &mut Ui, label: &str, add_control: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(SETTINGS_LABEL_WIDTH, SLIDER_ROW_HEIGHT), Layout::left_to_right(Align::Center), |ui| {
            ui.add(egui::Label::new(label).truncate());
        });
        add_control(ui)
    })
    .inner
}

fn form_slider<N: egui::emath::Numeric>(ui: &mut Ui, skin: &skin::Skin, label: &str, value: &mut N, range: RangeInclusive<N>, step: f64) -> bool {
    form_row(ui, label, |ui| slider_with_number(ui, skin, value, range, step, NumberFormat::WHOLE))
}

fn extension_label(extension: &str) -> &'static str {
    match extension {
        "png" => "PNG",
        "jpg" => "JPG",
        "bmp" => "BMP",
        "tga" => "TGA",
        "gif" => "GIF",
        "webp" => "WEBP",
        "ico" => "ICO",
        "ora" => "ORA",
        "chq" => "CHQ",
        _ => "TIF",
    }
}

fn format_bytes(bytes: usize) -> String {
    const KILOBYTE: f64 = 1024.0;
    let value = bytes as f64;
    if value < KILOBYTE {
        format!("{bytes} B")
    } else if value < KILOBYTE * KILOBYTE {
        format!("{:.1} KB", value / KILOBYTE)
    } else {
        format!("{:.2} MB", value / BYTES_PER_MEGABYTE)
    }
}
