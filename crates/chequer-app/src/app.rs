use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use chequer_core::blend::BlendMode;
use chequer_core::document::{AlphaMode, Document, MAX_SIDE, Resample};
use chequer_core::geom::IPoint;
use chequer_core::io::{SaveOptions, load_raster};
use chequer_core::raster::{Raster, Rgba};
use chequer_core::selection::CombineMode;
use eframe::egui::{self, Context, Event, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Vec2, vec2};

use crate::metrics::DEFAULT_WORKSPACE_SIZE;
use crate::new_document::NewDocumentSetup;
use crate::anim::{LayerMove, Slide, Slot};
use crate::clipboard::Clipboard;
use crate::color::Hsv;
use crate::dialogs::{ConfirmAction, Dialog, NewImageDialog, SaveConfigDialog, SettingsTab};
use crate::fonts::{FONT_FILE_EXTENSIONS, FontChoice};
use crate::i18n::{self, Lang, Text, t};
use crate::skin::Skin;
use crate::tools::{Tool, ToolOptions};
use crate::view::{DocTab, Fit};
use crate::widgets::{Corner, PanelState};

pub const APP_NAME: &str = "Chequer";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_NEW_SIZE: (u32, u32) = (800, 600);
pub const IMAGE_EXTENSIONS: [&str; 14] = [
    "png", "jpg", "jpeg", "bmp", "gif", "webp", "tga", "tif", "tiff", "jpe", "ico", "chq", "ora", "pdn",
];
const RECENT_LIMIT: usize = 10;
const AUTOSAVE_INTERVAL_SECONDS: f64 = 30.0;
const CLOSED_TABS_LIMIT: usize = 10;
const DEFAULT_PRIMARY: Rgba = [0, 0, 0, 255];
const DEFAULT_SECONDARY: Rgba = [255, 255, 255, 255];
const NUDGE_SMALL: i32 = 1;
const NUDGE_LARGE: i32 = 10;
const SETTINGS_FILE_NAME: &str = "chequer-settings.ini";
const SETTINGS_FILE_EXTENSIONS: [&str; 1] = ["ini"];
const PASTE_MARGIN: i32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelId {
    Tools,
    History,
    Layers,
    Colors,
    Swatches,
    Timeline,
}

impl PanelId {
    pub const ALL: [PanelId; 6] = [Self::Tools, Self::History, Self::Layers, Self::Colors, Self::Swatches, Self::Timeline];
}

pub struct Panels {
    pub tools: PanelState,
    pub history: PanelState,
    pub layers: PanelState,
    pub colors: PanelState,
    pub swatches: PanelState,
    pub timeline: PanelState,
}

impl Panels {
    pub fn get(&self, id: PanelId) -> Option<&PanelState> {
        Some(match id {
            PanelId::Tools => &self.tools,
            PanelId::History => &self.history,
            PanelId::Layers => &self.layers,
            PanelId::Colors => &self.colors,
            PanelId::Swatches => &self.swatches,
            PanelId::Timeline => &self.timeline,
        })
    }

    pub fn get_mut(&mut self, id: PanelId) -> &mut PanelState {
        match id {
            PanelId::Tools => &mut self.tools,
            PanelId::History => &mut self.history,
            PanelId::Layers => &mut self.layers,
            PanelId::Colors => &mut self.colors,
            PanelId::Swatches => &mut self.swatches,
            PanelId::Timeline => &mut self.timeline,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSlot {
    Primary,
    Secondary,
}

pub struct ColorState {
    pub primary: Rgba,
    pub secondary: Rgba,
    pub editing: ColorSlot,
    pub hsv: Hsv,
    pub hex_buffer: String,
    pub hex_synced: Rgba,
    pub wheel_value: Option<u8>,
    pub wheel_texture: Option<egui::TextureHandle>,
    pub square_hue: Option<u16>,
    pub square_texture: Option<egui::TextureHandle>,
    pub palette: crate::palette::Palette,
    pub palette_undo: Vec<crate::palette::Palette>,
    pub palette_redo: Vec<crate::palette::Palette>,
    pub palette_hovered: bool,
}

impl ColorState {
    pub fn active(&self) -> Rgba {
        match self.editing {
            ColorSlot::Primary => self.primary,
            ColorSlot::Secondary => self.secondary,
        }
    }

    pub fn set_active(&mut self, color: Rgba) {
        match self.editing {
            ColorSlot::Primary => self.primary = color,
            ColorSlot::Secondary => self.secondary = color,
        }
    }

    pub fn swap(&mut self) {
        std::mem::swap(&mut self.primary, &mut self.secondary);
    }

    pub fn reset_to_defaults(&mut self) {
        self.primary = DEFAULT_PRIMARY;
        self.secondary = DEFAULT_SECONDARY;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemFonts {
    NotLoaded,
    Loading,
    Ready(Vec<String>),
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub lang: Lang,
    pub ui_scale: Option<f32>,
    pub grain: f32,
    pub grain_enabled: bool,
    pub ui_pixel: u32,
    pub theme_id: String,
    pub theme_values: crate::themes::SettingValues,
    pub theme_colors: crate::themes::ColorOverrides,
    pub font: FontChoice,
    pub user_fonts: Vec<PathBuf>,
    pub trackpad: crate::trackpad::TrackpadSettings,
    pub pen_wayland: bool,
    pub haptics: bool,
    pub pen_only: bool,
    pub volume_undo: bool,
    pub pen_response: crate::pen::PenResponse,
    pub picker: crate::panels::PickerLayout,
    pub compact_colors: bool,
    pub high_contrast_icons: bool,
    pub palette_below: bool,
    pub layer_quick_settings: bool,
    pub layer_double_click: crate::panels::LayerDoubleClick,
    pub timeline_cells: crate::timeline::TimelineCells,
    pub animation: crate::timeline::AnimationSettings,
    pub new_document: NewDocumentSetup,
    pub zoom_out_filter: crate::view::ZoomOutFilter,
    pub text_unit: crate::tools::TextUnit,
    pub checker_light: Option<eframe::egui::Color32>,
    pub checker_dark: Option<eframe::egui::Color32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: Lang::Ru,
            ui_scale: None,
            grain: crate::skin::GRAIN_DEFAULT_STRENGTH,
            grain_enabled: false,
            ui_pixel: 1,
            theme_id: crate::themes::DEFAULT_LIGHT_ID.to_owned(),
            theme_values: Default::default(),
            theme_colors: Default::default(),
            font: FontChoice::Builtin,
            user_fonts: Vec::new(),
            trackpad: Default::default(),
            pen_wayland: false,
            haptics: true,
            pen_only: false,
            volume_undo: false,
            pen_response: Default::default(),
            picker: Default::default(),
            compact_colors: false,
            high_contrast_icons: false,
            palette_below: false,
            layer_quick_settings: false,
            layer_double_click: Default::default(),
            timeline_cells: Default::default(),
            animation: Default::default(),
            new_document: Default::default(),
            zoom_out_filter: Default::default(),
            text_unit: Default::default(),
            checker_light: None,
            checker_dark: None,
        }
    }
}

impl Settings {
    pub fn reset_tab(&mut self, tab: SettingsTab) {
        let defaults = Self::default();
        match tab {
            SettingsTab::General => self.lang = defaults.lang,
            SettingsTab::Interface => {
                self.ui_scale = defaults.ui_scale;
                self.picker = defaults.picker;
                self.compact_colors = defaults.compact_colors;
                self.high_contrast_icons = defaults.high_contrast_icons;
                crate::icons::set_high_contrast(defaults.high_contrast_icons);
                self.palette_below = defaults.palette_below;
                self.layer_quick_settings = defaults.layer_quick_settings;
                self.layer_double_click = defaults.layer_double_click;
                self.timeline_cells = defaults.timeline_cells;
                self.font = defaults.font;
                self.zoom_out_filter = defaults.zoom_out_filter;
                self.checker_light = defaults.checker_light;
                self.checker_dark = defaults.checker_dark;
                self.grain = defaults.grain;
                self.grain_enabled = defaults.grain_enabled;
            }
            SettingsTab::Gestures => {
                self.trackpad = defaults.trackpad;
                self.pen_wayland = defaults.pen_wayland;
                self.haptics = defaults.haptics;
                self.pen_only = defaults.pen_only;
                self.volume_undo = defaults.volume_undo;
                self.pen_response = defaults.pen_response;
            }
            SettingsTab::Themes => {
                self.theme_id = defaults.theme_id;
                self.theme_values = defaults.theme_values;
                self.theme_colors = defaults.theme_colors;
            }
        }
    }

    pub fn resolved_theme(&self, spec: &crate::themes::ThemeSpec) -> crate::skin::Theme {
        let mut theme = spec.resolve(&self.theme_values, &self.theme_colors);
        theme.checker_light = self.checker_light.unwrap_or(theme.checker_light);
        theme.checker_dark = self.checker_dark.unwrap_or(theme.checker_dark);
        theme
    }
}

#[cfg(target_os = "android")]
const SYSTEM_SCALE_FACTOR: f32 = 0.85;
#[cfg(not(target_os = "android"))]
const SYSTEM_SCALE_FACTOR: f32 = 1.0;
const SCALE_EPSILON: f32 = 0.001;
const WINDOW_SCREEN_SHARE: f32 = 0.92;
const WINDOW_RESIZE_EPSILON: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
enum WindowFit {
    Pending,
    Resizing { from: Vec2 },
    Done,
}

pub enum FileEvent {
    Opened(Vec<PathBuf>),
    OpenedFromOutside(Vec<(PathBuf, bool)>),
    SavePath { tab: usize, path: PathBuf },
    FontImport(PathBuf),
    SystemFonts(Vec<String>),
    SettingsExportPath(PathBuf),
    SettingsImportPath(PathBuf),
    PaletteLoadPath(PathBuf),
    PaletteSavePath(PathBuf),
    BrushImportPath(PathBuf),
BrushTipPaths(Vec<PathBuf>),
    BrushTexturePath(crate::texture_editor::TextureSlot, PathBuf),
    BrushLayerPath(crate::layer_editor::LayerPick, PathBuf),
    BrushExportPath(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Welcome,
    New,
    Open,
    Save,
    SaveAs,
    SaveAll,
    Share,
    CloseTab,
    Exit,
    Undo,
    Redo,
    Cut,
    Copy,
    CopyMerged,
    Paste,
    PasteNewLayer,
    PasteNewImage,
    EraseSelection,
    FillSelection,
    InvertSelection,
    SelectAll,
    Deselect,
    ZoomIn,
    ZoomOut,
    ZoomWindow,
    ZoomSelection,
    ActualSize,
    ToggleGrid,
    ToggleTiling,
    CropToSelection,
    ResizeImage,
    CanvasSize,
    FlipHorizontal,
    OffsetHalf,
    FlipVertical,
    RotateCw,
    RotateCcw,
    Rotate180,
    Flatten,
    NewFrame,
    NewEmptyFrame,
    DeleteFrame,
    ShiftFramesLeft,
    ShiftFramesRight,
    ExtendFramesLeft,
    ExtendFramesRight,
    SelectAllFrames,
    ReverseFrames,
    ClearCels,
    CopyCels,
    PasteCels,
    PrevFrame,
    NextFrame,
    PlayPause,
    PlayFromStart,
    AnimationSettings,
    AddLayer,
    DeleteLayer,
    DuplicateLayer,
    MergeDown,
    LayerUp,
    LayerDown,
    LayerProperties,
    InvertColors,
    BlackAndWhite,
    TogglePanel(PanelId),
    ResetLayout,
    ReopenTab,
    Settings,
    NextTab,
    PrevTab,
    Commit,
    Effect(chequer_core::effects::EffectId),
    Plugin(crate::command_registry::CommandId),
    RepeatEffect,
}

pub struct App {
    recovery: Option<crate::recovery::Session>,
    last_autosave: f64,
    pub(crate) skin: Arc<Skin>,
    pub(crate) tabs: Vec<DocTab>,
    pub(crate) active: usize,
    pub(crate) tool: Tool,
    pub(crate) options: ToolOptions,
    pub(crate) colors: ColorState,
    pub(crate) panels: Panels,
    pub(crate) timeline: crate::timeline::TimelineState,
    saved_layout: String,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) clipboard: Clipboard,
    pub(crate) settings: Settings,
    pub(crate) status: Option<String>,
    pub(crate) file_tx: Sender<FileEvent>,
    file_rx: Receiver<FileEvent>,
    pub(crate) untitled_counter: u32,
    pub(crate) hover_pixel: Option<IPoint>,
    pub(crate) show_grid: bool,
    pub(crate) tile_opacity: f32,
    pub(crate) tile_frame: bool,
    pub(crate) recent: Vec<PathBuf>,
    pub(crate) workspace: Rect,
    pub(crate) image_rect: Rect,
    pub(crate) layout_pending: bool,
    pub(crate) space_down: bool,
    pub(crate) auto_move_origin: Option<Tool>,
    pub(crate) zoom_notches: f32,
    applied_scale: f32,
    applied_pixel: u32,
    pub(crate) themes: crate::themes::ThemeLibrary,
    pub(crate) applied_icons: Option<PathBuf>,
    applied_font: FontChoice,
    pub(crate) system_fonts: SystemFonts,
    pub(crate) brush_library: crate::brush_library::BrushLibrary,
    pub(crate) brush_chip_cache: Option<((usize, egui::Color32), egui::TextureHandle)>,
    pub(crate) trackpad: crate::trackpad::Trackpad,
    pub(crate) pen: crate::pen::Pen,
    pub(crate) touch: crate::touch::Touches,
    pub(crate) stylus: crate::stylus::Stylus,
    pub(crate) keyboard_visible: bool,
pub(crate) keyboard_dismissed: bool,
    pub(crate) deferred_press: Option<crate::canvas::DeferredPress>,
    pub(crate) text_faces: std::collections::HashMap<(Option<String>, bool, bool), crate::fonts::TextFace>,
    pub(crate) closed_tabs: Vec<DocTab>,
    pub(crate) tab_slot: Option<Slot>,
    pub(crate) layer_move: Option<LayerMove>,
    pub(crate) export_flash: Option<f64>,
    pub(crate) floating_resample: (Resample, chequer_core::transform::RotationMethod),
    window_fit: WindowFit,
    window_title: String,
    pub(crate) welcome_banner: Option<egui::TextureHandle>,
    pub(crate) effect: Option<crate::effects_ui::EffectWindow>,
    pub(crate) last_effect: Option<chequer_core::effects::EffectParams>,
    pub(crate) effect_memory: std::collections::HashMap<chequer_core::effects::EffectId, chequer_core::effects::EffectParams>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        crate::sound::start();
        let recovered = crate::recovery::restore_abandoned();
        let recovery = crate::recovery::Session::open();
        let mut settings = Settings::default();
        crate::config::load(&mut settings);
        crate::icons::set_high_contrast(settings.high_contrast_icons);
        crate::fonts::register_user_fonts(&settings.user_fonts);
        let themes = crate::themes::ThemeLibrary::load();
        let spec = themes.find(&settings.theme_id);
        let theme = settings.resolved_theme(spec);
        let applied_icons = spec.icons_dir.clone();
        settings.ui_pixel = spec.pixel;
        crate::skin::set_ui_pixel(settings.ui_pixel);
        crate::skin::set_font_choice(settings.font.clone());
        let (skin, font_error) = Skin::install(&cc.egui_ctx, theme, applied_icons.as_deref());
let skin = Arc::new(skin);
        if font_error.is_some() {
            settings.font = FontChoice::Builtin;
            crate::skin::set_font_choice(FontChoice::Builtin);
        }
        crate::device::set_repaint(cc.egui_ctx.clone());
        let (file_tx, file_rx) = channel();
        let (outside_tx, repaint) = (file_tx.clone(), cc.egui_ctx.clone());
        crate::filepicker::watch_view_requests(move |files| {
            let _ = outside_tx.send(FileEvent::OpenedFromOutside(files));
            repaint.request_repaint();
        });
        let mut panels = default_panels(Rect::from_min_size(Pos2::ZERO, DEFAULT_WORKSPACE_SIZE));
        let layout_pending = !crate::config::load_panels(&mut panels);
        let saved_layout = crate::config::panels_text(&panels);
        i18n::set_lang(settings.lang);
        i18n::register_texts(crate::effect_texts::EFFECT_TEXTS).expect("ключи текстов встроенных эффектов не повторяются");
        i18n::register_texts(crate::brush_texts::BRUSH_TEXTS).expect("ключи текстов встроенных кистей не повторяются");
        let (applied_pixel, applied_font) = (settings.ui_pixel, settings.font.clone());
        let mut app = Self {
            recovery,
            last_autosave: 0.0,
            skin,
            tabs: Vec::new(),
            active: 0,
            tool: Tool::Pencil,
            options: crate::brush_library::restore_active(ToolOptions::default()),
            colors: ColorState {
                primary: DEFAULT_PRIMARY,
                secondary: DEFAULT_SECONDARY,
                editing: ColorSlot::Primary,
                hsv: Hsv { h: 0.0, s: 0.0, v: 0.0 },
                hex_buffer: String::new(),
                hex_synced: [0; 4],
                wheel_value: None,
                wheel_texture: None,
                square_hue: None,
                square_texture: None,
                palette: crate::config::load_palette(),
                palette_undo: Vec::new(),
                palette_redo: Vec::new(),
                palette_hovered: false,
            },
            panels,
            timeline: Default::default(),
            saved_layout,
            dialog: None,
            clipboard: Clipboard::new(),
            settings,
            status: None,
            file_tx,
            file_rx,
            untitled_counter: 0,
            hover_pixel: None,
            show_grid: false,
            tile_opacity: 1.0,
            tile_frame: true,
            recent: crate::config::load_recent(),
            workspace: Rect::from_min_size(Pos2::ZERO, DEFAULT_WORKSPACE_SIZE),
            image_rect: Rect::NOTHING,
            layout_pending,
            space_down: false,
            auto_move_origin: None,
            zoom_notches: 0.0,
            applied_scale: cc.egui_ctx.pixels_per_point(),
            applied_pixel,
            themes,
            applied_icons,
            applied_font,
            system_fonts: SystemFonts::NotLoaded,
            brush_library: crate::brush_library::BrushLibrary::load(),
            brush_chip_cache: None,
            trackpad: crate::trackpad::Trackpad::new(crate::trackpad::display_address(cc)),
            pen: crate::pen::Pen::new(crate::pen::native_handle(cc)),
            touch: Default::default(),
            stylus: crate::stylus::Stylus::detect(),
            keyboard_visible: false,
keyboard_dismissed: false,
            deferred_press: None,
            text_faces: std::collections::HashMap::new(),
            closed_tabs: Vec::new(),
            tab_slot: None,
            layer_move: None,
            export_flash: None,
            floating_resample: (ToolOptions::default().move_resample, ToolOptions::default().rotation),
            window_fit: WindowFit::Pending,
            window_title: String::new(),
            welcome_banner: None,
            effect: None,
            last_effect: None,
            effect_memory: Default::default(),
        };
        for path in files {
            app.open_path(&path);
        }
        let recovered_count = recovered.len();
        for crate::recovery::Recovered { mut document, origin } in recovered {
            app.adopt_document_palette(&mut document);
            let name = origin.as_deref().and_then(Path::file_name).map_or_else(|| t(Text::RecoveredName).to_owned(), |name| name.to_string_lossy().into_owned());
            let mut tab = DocTab::new(document, name, origin);
            tab.mark_modified();
            app.push_tab(tab);
        }
        if recovered_count > 0 {
            app.say(format!("{} {recovered_count}", t(Text::RecoveredStatus)));
        }
        if app.tabs.is_empty() {
            app.new_document(DEFAULT_NEW_SIZE.0, DEFAULT_NEW_SIZE.1);
            app.dialog = Some(Dialog::Welcome);
        }
        if let Some(message) = font_error {
            app.fail(message);
        }
        app
    }

    pub(crate) fn tab(&self) -> &DocTab {
        &self.tabs[self.active]
    }

    pub(crate) fn tab_mut(&mut self) -> &mut DocTab {
        &mut self.tabs[self.active]
    }

    pub(crate) fn say(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
    }

    pub(crate) fn fail(&mut self, message: impl Into<String>) {
        self.dialog = Some(Dialog::Error(message.into()));
    }

    pub fn new_document(&mut self, width: u32, height: u32) {
        self.new_document_with(width, height, self.settings.new_document);
    }

    pub(crate) fn new_document_with(&mut self, width: u32, height: u32, setup: NewDocumentSetup) {
        match setup.build(width, height) {
            Ok(doc) => {
                self.discard_blank_placeholder();
                self.untitled_counter += 1;
                let suffix = if self.untitled_counter > 1 { format!(" {}", self.untitled_counter) } else { String::new() };
                let name = format!("{}{suffix}", t(Text::Untitled));
                self.push_tab(DocTab::new(doc, name, None));
            }
            Err(e) => self.fail(e.to_string()),
        }
    }

    pub(crate) fn is_blank_placeholder(&self) -> bool {
        matches!(self.tabs.as_slice(), [tab] if tab.path.is_none() && !tab.is_modified() && !tab.can_undo())
    }

    fn discard_blank_placeholder(&mut self) {
        if self.is_blank_placeholder() {
            self.tabs.clear();
            self.untitled_counter = 0;
        }
    }

    pub(crate) fn push_tab(&mut self, tab: DocTab) {
        self.discard_blank_placeholder();
        self.flush_pending();
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.tab_slot = Some(Slot::start(Slide::In, self.active));
    }

    fn load_document(path: &Path) -> Result<Document, String> {
        let has_extension = |name: &str| path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case(name));
        if has_extension(chequer_core::chq::FILE_EXTENSION) {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            return chequer_core::chq::decode_document(&bytes).map_err(|e| e.to_string());
        }
        if has_extension(chequer_core::ora::FILE_EXTENSION) {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            return chequer_core::ora::decode_document(&bytes).map_err(|e| e.to_string());
        }
        if has_extension(chequer_core::pdn::FILE_EXTENSION) {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            return chequer_core::pdn::decode_document(&bytes).map_err(|e| e.to_string());
        }
        chequer_core::io::load_document(path, t(Text::Background).to_owned()).map_err(|e| e.to_string())
    }

    pub fn open_path(&mut self, path: &Path) {
        if let Some(index) = self.tabs.iter().position(|t| t.path.as_deref() == Some(path)) {
            self.switch_tab(index);
            return;
        }
        match Self::load_document(path) {
            Ok(mut doc) => {
                self.adopt_document_palette(&mut doc);
                let has_history = !doc.history().is_empty();
                self.panels.timeline.open |= doc.frame_count() > 1;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut tab = DocTab::new(doc, name, Some(path.to_owned()));
                let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
                tab.save_options = match SaveOptions::for_extension(&extension) {
                    Some(SaveOptions::Chq { .. }) => Some(SaveOptions::Chq { history: has_history }),
                    Some(SaveOptions::Ora) => Some(SaveOptions::Ora),
                    _ => None,
                };
                self.push_tab(tab);
                self.remember_recent(path);
            }
            Err(e) => self.fail(format!("{}: {e}", path.display())),
        }
    }

    fn open_from_outside(&mut self, files: &[(PathBuf, bool)]) {
        if matches!(self.dialog, Some(Dialog::Welcome)) {
            self.dialog = None;
        }
        for (path, writable) in files {
            self.open_path(path);
            if !writable {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.path.as_deref() == Some(path.as_path())) {
                    tab.path = None;
                }
            }
        }
    }

    fn remember_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_owned());
        self.recent.truncate(RECENT_LIMIT);
        crate::config::save_recent(&self.recent);
    }

    pub(crate) fn switch_tab(&mut self, index: usize) {
        if index < self.tabs.len() && index != self.active {
            self.flush_pending();
            self.active = index;
            self.tabs[index].mark_all_dirty();
        }
    }

    pub(crate) fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        if self.tabs[index].is_modified() {
            self.dialog = Some(Dialog::Confirm { tab: index, action: ConfirmAction::CloseTab });
            return;
        }
        self.remove_tab(index);
    }

    pub(crate) fn remove_tab(&mut self, index: usize) {
        let mut closed = self.tabs.remove(index);
        closed.end_gesture();
        if let Some(session) = &self.recovery {
            session.forget(closed.id);
        }
        closed.autosaved_revision = None;
        self.tab_slot = Some(Slot::start(Slide::Out, index));
        self.closed_tabs.push(closed);
        if self.closed_tabs.len() > CLOSED_TABS_LIMIT {
            self.closed_tabs.remove(0);
        }
        if self.tabs.is_empty() {
            self.active = 0;
            self.new_document(DEFAULT_NEW_SIZE.0, DEFAULT_NEW_SIZE.1);
            self.dialog = Some(Dialog::Welcome);
            return;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.tab_mut().mark_all_dirty();
    }

    fn reopen_closed_tab(&mut self) {
        let Some(tab) = self.closed_tabs.pop() else {
            return;
        };
        self.push_tab(tab);
        self.tab_mut().mark_all_dirty();
        self.tab_mut().view.needs_initial_fit = true;
        self.flush_pending();
    }

    pub(crate) fn flush_pending(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.close_effect(true);
        self.commit_shape();
        self.commit_text();
        self.commit_live();
        let tab = &mut self.tabs[self.active];
        let area = tab.doc.settle();
        tab.mark_dirty(area);
    }

    pub(crate) fn pick_files_to_open(&self, ctx: &Context) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let picked = crate::filepicker::new_dialog().add_filter("Images", &IMAGE_EXTENSIONS).pick_files();
            if let Some(paths) = picked {
                let _ = tx.send(FileEvent::Opened(paths));
                ctx.request_repaint();
            }
        });
    }

    pub(crate) fn load_system_fonts(&mut self, ctx: &Context) {
        self.system_fonts = SystemFonts::Loading;
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(FileEvent::SystemFonts(crate::fonts::system_families()));
            ctx.request_repaint();
        });
    }

    pub(crate) fn pick_font_to_import(&self, ctx: &Context) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(path) = crate::filepicker::new_dialog().add_filter("Fonts", &FONT_FILE_EXTENSIONS).pick_file() {
                let _ = tx.send(FileEvent::FontImport(path));
                ctx.request_repaint();
            }
        });
    }

    fn import_font(&mut self, path: PathBuf) {
        let families = match crate::fonts::register_font_file(&path) {
            Ok(families) => families,
            Err(message) => return self.fail(message),
        };
        if !self.settings.user_fonts.contains(&path) {
            self.settings.user_fonts.push(path);
            crate::config::save(&self.settings);
        }
        self.system_fonts = SystemFonts::Ready(crate::fonts::system_families());
        if let (Some(crate::dialogs::Dialog::FontPicker(picker)), Some(family)) = (&mut self.dialog, families.into_iter().next()) {
            picker.choose(FontChoice::System(family));
        }
    }

    pub(crate) fn pick_settings_export_path(&self, ctx: &Context) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(path) = crate::filepicker::new_dialog().set_file_name(SETTINGS_FILE_NAME).add_filter("Chequer settings", &SETTINGS_FILE_EXTENSIONS).save_file() {
                let _ = tx.send(FileEvent::SettingsExportPath(path));
                ctx.request_repaint();
            }
        });
    }

    pub(crate) fn pick_settings_import_path(&self, ctx: &Context) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(path) = crate::filepicker::new_dialog().add_filter("Chequer settings", &SETTINGS_FILE_EXTENSIONS).pick_file() {
                let _ = tx.send(FileEvent::SettingsImportPath(path));
                ctx.request_repaint();
            }
        });
    }

    fn export_settings(&mut self, path: &Path) {
        let written = std::fs::write(path, crate::config::to_text(&self.settings)).map_err(|e| e.to_string());
        match written.and_then(|()| crate::filepicker::sync_back(path)) {
            Ok(()) => self.say(t(Text::SettingsExported)),
            Err(e) => self.fail(format!("{}: {e}", path.display())),
        }
    }

    fn import_settings(&mut self, ctx: &Context, path: &Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => return self.fail(format!("{}: {e}", path.display())),
        };
        let mut settings = self.settings.clone();
        if crate::config::apply_text(&mut settings, &text) == 0 {
            return self.fail(t(Text::SettingsInvalid));
        }
        i18n::set_lang(settings.lang);
        self.settings = settings;
        self.apply_theme(ctx);
        crate::config::save(&self.settings);
        self.say(t(Text::SettingsImported));
    }

    fn pick_save_path(&self, ctx: &Context, tab: usize) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        let suggested = self.tabs[tab].path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
        let name = suggested.unwrap_or_else(|| format!("{}.{}", self.tabs[tab].name, chequer_core::chq::FILE_EXTENSION));
        std::thread::spawn(move || {
            if let Some(path) = crate::filepicker::new_dialog().set_file_name(name).save_file() {
                let _ = tx.send(FileEvent::SavePath { tab, path });
                ctx.request_repaint();
            }
        });
    }

    fn poll_file_events(&mut self, ctx: &Context) {
        if let Some(message) = crate::filepicker::take_failure() {
            self.fail(message);
        }
        while let Ok(event) = self.file_rx.try_recv() {
            match event {
                FileEvent::Opened(paths) => paths.iter().for_each(|p| self.open_path(p)),
                FileEvent::OpenedFromOutside(files) => self.open_from_outside(&files),
                FileEvent::SavePath { tab, path } => self.begin_save_to(ctx, tab, path),
                FileEvent::FontImport(path) => self.import_font(path),
                FileEvent::SystemFonts(families) => self.system_fonts = SystemFonts::Ready(families),
                FileEvent::SettingsExportPath(path) => self.export_settings(&path),
                FileEvent::SettingsImportPath(path) => self.import_settings(ctx, &path),
                FileEvent::PaletteLoadPath(path) => self.load_palette(&path),
                FileEvent::PaletteSavePath(path) => self.save_palette(&path),
                FileEvent::BrushImportPath(path) => self.import_brush(&path),
                FileEvent::BrushTipPaths(paths) => self.add_brush_tips(&paths),
                FileEvent::BrushTexturePath(slot, path) => self.load_brush_texture(slot, &path),
                FileEvent::BrushLayerPath(pick, path) => self.load_brush_layer(pick, &path),
                FileEvent::BrushExportPath(path) => self.export_brush(&path),
            }
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_owned()).collect());
        if self.tabs.is_empty() && !dropped.is_empty() {
            dropped.iter().for_each(|path| self.open_path(path));
            return;
        }
        if !dropped.is_empty() && self.dialog.is_none() {
            self.dialog = Some(Dialog::Dropped(dropped));
        }
    }

    fn save(&mut self, ctx: &Context, as_new: bool) {
        self.flush_pending();
        let index = self.active;
        let known = self.tabs[index].path.clone().zip(self.tabs[index].save_options);
        match known {
            Some((path, options)) if !as_new => self.save_known(ctx, index, path, options, None),
            _ => self.pick_save_path(ctx, index),
        }
    }

    fn save_all(&mut self, ctx: &Context) {
        let modified: Vec<usize> = (0..self.tabs.len()).filter(|&i| self.tabs[i].is_modified()).collect();
        for index in modified {
            self.switch_tab(index);
            self.save(ctx, false);
        }
    }

    fn begin_save_to(&mut self, ctx: &Context, tab_index: usize, mut path: PathBuf) {
        if tab_index >= self.tabs.len() {
            return;
        }
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_owned();
        let options = SaveOptions::for_extension(&extension).unwrap_or_else(|| {
            if !cfg!(target_os = "android") {
                path.set_extension("png");
            }
            SaveOptions::for_extension("png").expect("png поддерживается")
        });
        let options = match self.tabs[tab_index].save_options {
            Some(previous) if std::mem::discriminant(&previous) == std::mem::discriminant(&options) => previous,
            _ => options,
        };
        let doc = &self.tabs[tab_index].doc;
        let raster = doc.flattened(doc.bounds());
        self.dialog = Some(Dialog::SaveConfig(SaveConfigDialog::new(tab_index, path, options, raster, doc.snapshot(), ctx)));
    }

    pub(crate) fn write_tab(&mut self, index: usize, path: PathBuf, options: SaveOptions) {
        let written = match options {
            SaveOptions::Chq { history } => {
                self.tabs[index].doc.palette = Some((&self.colors.palette).into());
                chequer_core::chq::save_document(&path, &self.tabs[index].doc, chequer_core::chq::WriteMode::Small, history).map_err(|e| e.to_string())
            }
            SaveOptions::Ora => chequer_core::ora::encode_document(&self.tabs[index].doc)
                .map_err(|e| e.to_string())
                .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string())),
            _ => chequer_core::io::save_document(&path, &self.tabs[index].doc, &options).map_err(|e| e.to_string()),
        };
        match written.and_then(|()| crate::filepicker::sync_back(&path)) {
            Ok(()) => {
                let tab = &mut self.tabs[index];
                tab.name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                tab.path = Some(path.clone());
                tab.save_options = Some(options);
                tab.mark_saved();
                self.remember_recent(&path);
                self.say(t(Text::Saved));
                crate::sound::play_export();
                crate::device::vibrate(crate::device::Haptic::Click);
                self.export_flash = Some(f64::NEG_INFINITY);
            }
            Err(e) => self.fail(format!("{}: {e}", path.display())),
        }
    }

    fn copy(&mut self, merged: bool) {
        self.flush_pending();
        let raster = self.tab_mut().doc.copy_selection(merged);
        if let Err(e) = self.clipboard.set(&raster) {
            self.say(e);
        }
    }

    fn cut(&mut self) {
        self.copy(false);
        let tab = self.tab_mut();
        let area = tab.doc.erase_selection(chequer_core::history::HistoryKind::Cut);
        tab.mark_dirty(area);
    }

    fn paste(&mut self, new_layer: bool, new_image: bool) {
        self.flush_pending();
        let raster = match self.clipboard.get() {
            Ok(Some(raster)) => raster,
            Ok(None) => {
                self.say(t(Text::ClipboardEmpty));
                return;
            }
            Err(e) => {
                self.say(format!("{} ({e})", t(Text::ClipboardEmpty)));
                return;
            }
        };
        if new_image {
            let doc = Document::from_raster(t(Text::Background).to_owned(), raster);
            self.discard_blank_placeholder();
            self.untitled_counter += 1;
            let name = format!("{} {}", t(Text::Untitled), self.untitled_counter);
            self.push_tab(DocTab::new(doc, name, None));
            return;
        }
        self.paste_raster(raster, new_layer, None);
    }

    pub(crate) fn add_files_as_layers(&mut self, paths: Vec<PathBuf>) {
        let single = paths.len() == 1;
        let mut errors = Vec::new();
        for path in paths {
            match load_raster(&path) {
                Ok(raster) => {
                    let name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned());
                    if single {
                        self.paste_raster(raster, true, name);
                    } else {
                        self.flush_pending();
                        self.finish_paste(raster, true, name);
                    }
                }
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        if !errors.is_empty() {
            self.fail(errors.join("\n"));
        }
    }

    pub(crate) fn paste_raster(&mut self, raster: Raster, new_layer: bool, layer_name: Option<String>) {
        self.flush_pending();
        let tab = self.tab();
        let too_big = raster.width() > tab.doc.width() || raster.height() > tab.doc.height();
        if too_big {
            self.dialog = Some(Dialog::PasteTooBig { raster, new_layer, layer_name });
        } else {
            self.finish_paste(raster, new_layer, layer_name);
        }
    }

    pub(crate) fn finish_paste(&mut self, raster: Raster, new_layer: bool, layer_name: Option<String>) {
        self.set_tool(Tool::MovePixels);
        let (viewport, float_mode) = (self.workspace, self.options.float_mode);
        let tab = &mut self.tabs[self.active];
        if new_layer {
            let name = layer_name.unwrap_or_else(|| format!("{} {}", t(Text::LayerN), tab.doc.layers().len() + 1));
            tab.doc.add_layer(name);
            tab.mark_all_dirty();
        }
        let corner = visible_corner(tab, viewport);
        let mut area = tab.doc.paste(raster, corner);
        area = area.union(tab.doc.set_floating_mode(float_mode));
        tab.mark_dirty(area);
    }

    pub(crate) fn set_tool(&mut self, tool: Tool) {
        if tool != self.tool {
            self.flush_pending();
            self.tool = tool;
            self.auto_move_origin = None;
            self.tab_mut().end_gesture();
        }
    }

    fn cycle_tool(&mut self, key: char) {
        let group = Tool::hotkey_group(key);
        let Some(first) = group.first() else {
            return;
        };
        let next = group.iter().position(|&t| t == self.tool).map(|i| group[(i + 1) % group.len()]).unwrap_or(*first);
        self.set_tool(next);
    }

    pub fn run(&mut self, ctx: &Context, command: Command) {
        use chequer_core::history::HistoryKind as H;
        if self.effect.is_some() && !Self::effect_allows(command) {
            let undo = command == Command::Undo;
            self.close_effect(!undo);
            if undo {
                return;
            }
        }
        match command {
            Command::Effect(kind) => self.open_effect(ctx, kind),
            Command::Plugin(id) => {
                if let Some(def) = crate::command_registry::lookup_command(id) {
                    self.edit_structure(def.run);
                }
            }
            Command::RepeatEffect => self.repeat_effect(),
            Command::New => {
                let (width, height) = self.clipboard_image_size().unwrap_or(DEFAULT_NEW_SIZE);
                self.dialog = Some(Dialog::New(NewImageDialog::new(width, height, self.settings.new_document)));
            }
            Command::Welcome => self.dialog = Some(Dialog::Welcome),
            Command::Open => self.pick_files_to_open(ctx),
            Command::Save => self.save(ctx, false),
            Command::SaveAs => self.save(ctx, true),
            Command::Share => self.share_image(),
            Command::SaveAll => self.save_all(ctx),
            Command::CloseTab => self.close_tab(self.active),
            Command::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::NextTab => self.switch_tab((self.active + 1) % self.tabs.len()),
            Command::PrevTab => self.switch_tab((self.active + self.tabs.len() - 1) % self.tabs.len()),
            Command::Undo if self.colors.palette_hovered => self.undo_palette(),
            Command::Redo if self.colors.palette_hovered => self.redo_palette(),
            Command::Undo if self.text_active() => {
                let nothing_to_undo = self.tab_mut().text.as_mut().is_some_and(|session| !session.undo_edit());
                if nothing_to_undo {
                    self.cancel_text();
                }
            }
            Command::Redo if self.text_active() => {
                if let Some(session) = self.tab_mut().text.as_mut() {
                    session.redo_edit();
                }
            }
            Command::Undo => {
                let tab = self.tab_mut();
                if !tab.can_undo() {
                    crate::device::vibrate(crate::device::Haptic::Reject);
                }
                tab.end_gesture();
                let dropped_pending = tab.shape.take().is_some() | tab.live.take().is_some();
                let area = tab.doc.set_preview(None);
                tab.mark_dirty(area);
                tab.mark_all_dirty();
                if !dropped_pending && !self.reopen_committed_text() && !self.reopen_committed_shape() {
                    let (method, rotation) = (self.options.move_resample, self.options.rotation);
                    let tab = self.tab_mut();
                    let area = tab.doc.undo_floating_step(method, rotation).unwrap_or_else(|| tab.doc.undo());
                    tab.mark_dirty(area);
                    tab.mark_all_dirty();
                }
            }
            Command::Redo => {
                let (method, rotation) = (self.options.move_resample, self.options.rotation);
                let floating_step = self.tab_mut().doc.redo_floating_step(method, rotation);
                if floating_step.is_none() {
                    self.flush_pending();
                }
                let tab = self.tab_mut();
                let area = floating_step.unwrap_or_else(|| tab.doc.redo());
                tab.mark_dirty(area);
                tab.mark_all_dirty();
            }
            Command::Cut => self.cut(),
            Command::Copy => self.copy(false),
            Command::CopyMerged => self.copy(true),
            Command::Paste => self.paste(false, false),
            Command::PasteNewLayer => self.paste(true, false),
            Command::PasteNewImage => self.paste(false, true),
            Command::EraseSelection => self.edit_doc(|d| d.erase_selection(H::Delete)),
            Command::FillSelection => {
                let color = self.colors.primary;
                self.edit_doc(|d| d.fill_selection(color));
            }
            Command::InvertSelection => self.edit_selection(|d| d.invert_selection()),
            Command::SelectAll => {
                self.edit_selection(|d| d.select_all());
                self.tab_mut().sel_origin = None;
                if self.options.select_mode == CombineMode::Replace {
                    self.enter_move_mode(Tool::RectSelect);
                }
            }
            Command::Deselect => self.edit_selection(|d| d.deselect()),
            Command::Commit => {
                self.flush_pending();
                self.edit_selection(|d| d.deselect());
            }
            Command::ZoomIn => self.zoom_step(true),
            Command::ZoomOut => self.zoom_step(false),
            Command::ZoomWindow => self.zoom_fit(),
            Command::ZoomSelection => self.zoom_to_selection(),
            Command::ActualSize => self.zoom_to(1.0),
            Command::ToggleGrid => self.show_grid = !self.show_grid,
            Command::ToggleTiling => {
                self.flush_pending();
                let tab = self.tab_mut();
                tab.view.tiled = !tab.view.tiled;
                tab.doc.set_tiled(tab.view.tiled);
            }
            Command::CropToSelection => self.edit_structure(|d| {
                d.crop_to_selection();
            }),
            Command::ResizeImage => self.open_resize_dialog(),
            Command::CanvasSize => self.open_canvas_dialog(),
            Command::FlipHorizontal => self.edit_structure(|d| d.flip_horizontal()),
            Command::FlipVertical => self.edit_structure(|d| d.flip_vertical()),
            Command::RotateCw => self.edit_structure(|d| d.rotate_clockwise()),
            Command::RotateCcw => self.edit_structure(|d| d.rotate_counter_clockwise()),
            Command::OffsetHalf => self.edit_structure(|d| d.offset_half()),
            Command::Rotate180 => self.edit_structure(|d| d.rotate_half_turn()),
            Command::Flatten => {
                let name = t(Text::Background).to_owned();
                self.edit_structure(move |d| {
                    d.flatten(name);
                });
            }
            Command::AddLayer => {
                let name = format!("{} {}", t(Text::LayerN), self.tab().doc.layers().len() + 1);
                self.edit_structure(move |d| d.add_layer(name));
            }
            Command::DuplicateLayer => {
                let name = format!("{} {}", self.tab().doc.layers()[self.tab().doc.active_layer()].name, t(Text::Copy));
                self.edit_structure(move |d| d.duplicate_layer(name));
            }
            Command::DeleteLayer => self.edit_structure(|d| {
                d.delete_layer();
            }),
            Command::MergeDown => self.edit_structure(|d| {
                d.merge_layer_down();
            }),
            Command::LayerUp => self.step_active_layer(ctx, true),
            Command::LayerDown => self.step_active_layer(ctx, false),
            Command::LayerProperties => {
                let index = self.tab().doc.active_layer();
                self.open_layer_properties(index);
            }
            Command::InvertColors => self.adjust(H::InvertColors, |c| [255 - c[0], 255 - c[1], 255 - c[2], c[3]]),
            Command::BlackAndWhite => self.adjust(H::BlackAndWhite, |c| {
                let luma = luminance(c);
                [luma, luma, luma, c[3]]
            }),
            Command::TogglePanel(id) => {
                let state = self.panels.get_mut(id);
                state.open = !state.open;
            }
            Command::NewFrame => self.duplicate_selected_frames(),
            Command::NewEmptyFrame => self.edit_structure(|d| {
                d.add_empty_frame();
            }),
            Command::DeleteFrame => self.delete_selected_frames(),
            Command::ShiftFramesLeft => self.shift_selected_frames(-1),
            Command::ShiftFramesRight => self.shift_selected_frames(1),
            Command::ExtendFramesLeft => self.extend_frame_selection(false),
            Command::ExtendFramesRight => self.extend_frame_selection(true),
            Command::SelectAllFrames => self.select_all_frames(),
            Command::ReverseFrames => self.reverse_selected_frames(),
            Command::ClearCels => self.clear_selected_cels(),
            Command::CopyCels => self.copy_selected_cels(),
            Command::PasteCels => self.paste_cels(),
            Command::PrevFrame => self.step_frame(false),
            Command::NextFrame => self.step_frame(true),
            Command::PlayPause => self.toggle_playback(ctx),
            Command::PlayFromStart => self.play_from_start(ctx),
            Command::AnimationSettings => self.open_animation_settings(),
            Command::ResetLayout => self.layout_pending = true,
            Command::ReopenTab => self.reopen_closed_tab(),
            Command::Settings => self.dialog = Some(Dialog::Settings(SettingsTab::General)),
        }
    }

    fn edit_doc(&mut self, f: impl FnOnce(&mut Document) -> chequer_core::geom::IRect) {
        self.flush_pending();
        let tab = self.tab_mut();
        let area = f(&mut tab.doc);
        tab.mark_dirty(area);
    }

    fn edit_selection(&mut self, f: impl FnOnce(&mut Document)) {
        self.flush_pending();
        f(&mut self.tab_mut().doc);
    }

    fn step_active_layer(&mut self, ctx: &Context, up: bool) {
        let from = self.tab().doc.active_layer();
        let mut moved = false;
        self.edit_structure(|doc| moved = doc.move_layer(up));
        if moved && self.panels.layers.open {
            let to = if up { from + 1 } else { from - 1 };
            self.layer_move = Some(LayerMove { from, to, held_offset: None });
            ctx.request_repaint();
        }
    }

    pub(crate) fn edit_structure<R>(&mut self, f: impl FnOnce(&mut Document) -> R) -> R {
        self.flush_pending();
        let tab = self.tab_mut();
        let result = f(&mut tab.doc);
        tab.mark_all_dirty();
        result
    }

    fn adjust(&mut self, kind: chequer_core::history::HistoryKind, f: impl Fn(Rgba) -> Rgba) {
        self.edit_doc(|d| d.adjust_colors(kind, f));
    }

    fn canvas_rect(&self) -> Rect {
        self.workspace
    }

    fn zoom_step(&mut self, zoom_in: bool) {
        let canvas = self.canvas_rect();
        self.tab_mut().view.step_zoom(canvas, canvas.center(), zoom_in);
    }

    pub(crate) fn zoom_to(&mut self, zoom: f32) {
        let canvas = self.canvas_rect();
        self.tab_mut().view.zoom_at(canvas, canvas.center(), zoom);
    }

    fn zoom_fit(&mut self) {
        let canvas = self.canvas_rect();
        let tab = self.tab_mut();
        let (w, h) = (tab.doc.width(), tab.doc.height());
        tab.view.fit(canvas, w, h, Fit::Exact);
    }

    fn zoom_to_selection(&mut self) {
        let canvas = self.canvas_rect();
        let tab = self.tab_mut();
        let Some(bounds) = tab.doc.selection().map(|s| s.bounds()) else {
            return;
        };
        let fit_scale = (canvas.width() / bounds.width() as f32).min(canvas.height() / bounds.height() as f32);
        tab.view.zoom = (fit_scale * tab.view.ppp).clamp(crate::view::ZOOM_MIN, crate::view::ZOOM_MAX);
        let center = vec2((bounds.x0 + bounds.x1) as f32 / 2.0, (bounds.y0 + bounds.y1) as f32 / 2.0);
        tab.view.origin = canvas.size() / 2.0 - center * tab.view.scale();
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        if ctx.egui_wants_keyboard_input() || self.dialog.is_some() {
            return;
        }
        let typing = self.text_active();
        if typing {
            self.handle_text_input(ctx);
        }
        let space_was_down = self.space_down;
        self.space_down = !typing && ctx.input(|i| i.key_down(Key::Space));
        self.track_space_tap(ctx, space_was_down);
        let mut triggered = self.timeline_key_commands(ctx);
        ctx.input_mut(|input| {
            for (shortcut, command) in shortcut_table() {
                if input.consume_shortcut(&shortcut) {
                    triggered.push(command);
                }
            }
        });
        let (clipboard_commands, pasted_images) = ctx.input(|i| {
            if typing {
                return (Vec::new(), Vec::new());
            }
            let mut commands = Vec::new();
            let mut images = Vec::new();
            for event in &i.events {
                match event {
                    Event::Copy => commands.push(Command::Copy),
                    Event::Cut => commands.push(Command::Cut),
                    Event::Paste(_) => commands.push(Command::Paste),
                    Event::PasteImage(image) => images.push(raster_from_color_image(image)),
                    _ => {}
                }
            }
            (commands, images)
        });
        triggered.extend(clipboard_commands);
        let triggered = self.scope_to_timeline(triggered);
        for command in triggered {
            if typing && matches!(command, Command::FillSelection | Command::EraseSelection | Command::SelectAll | Command::PrevFrame | Command::NextFrame) {
                continue;
            }
            self.run(ctx, command);
        }
        for raster in pasted_images {
            match raster {
                Ok(raster) => self.paste_raster(raster, false, None),
                Err(e) => self.say(e.to_string()),
            }
        }
        if !typing && self.effect.is_none() {
            self.handle_tool_keys(ctx);
            self.handle_frame_keys(ctx);
            self.handle_nudge(ctx);
        }
    }

    fn handle_tool_keys(&mut self, ctx: &Context) {
        let pressed: Vec<(Key, Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (key, modifiers) in pressed {
            if modifiers.ctrl || modifiers.alt || modifiers.command {
                continue;
            }
            match key {
                Key::OpenBracket => self.options.adjust_size(-crate::tools::BRUSH_SIZE_STEP),
                Key::CloseBracket => self.options.adjust_size(crate::tools::BRUSH_SIZE_STEP),
                Key::BrowserBack => self.go_back(ctx),
                Key::Enter if self.enter_plays() => self.run(ctx, Command::PlayFromStart),
                Key::Enter | Key::Escape => self.run(ctx, if self.has_pending() { Command::Commit } else { Command::Deselect }),
                Key::X => self.colors.swap(),
                other => {
                    if let Some(letter) = key_letter(other) {
                        self.cycle_tool(letter);
                    }
                }
            }
        }
        let size_steps: Vec<(Key, Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key: key @ (Key::OpenBracket | Key::CloseBracket), pressed: true, modifiers, .. } if modifiers.ctrl => {
                        Some((*key, *modifiers))
                    }
                    _ => None,
                })
                .collect()
        });
        for (key, _) in size_steps {
            let sign = if key == Key::CloseBracket { 1.0 } else { -1.0 };
            self.options.adjust_size(sign * crate::tools::BRUSH_SIZE_STEP_LARGE);
        }
    }

    fn go_back(&mut self, ctx: &Context) {
        let closes_itself = self.dialog.is_some() && !(matches!(self.dialog, Some(Dialog::Welcome)) && self.tabs.is_empty());
        if closes_itself {
            return;
        }
        if self.has_pending() {
            self.run(ctx, Command::Commit);
        } else if self.tab().doc.selection().is_some() {
            self.run(ctx, Command::Deselect);
        } else {
            crate::device::move_to_back();
        }
    }

    fn share_image(&mut self) {
        self.flush_pending();
        let tab = self.tab();
        let raster = tab.doc.flattened(tab.doc.bounds());
        let name = Path::new(&tab.name).file_stem().map_or_else(|| tab.name.clone(), |stem| stem.to_string_lossy().into_owned());
        if let Err(message) = crate::filepicker::share_image(&raster, &name) {
            self.fail(message);
        }
    }

    pub(crate) fn has_pending(&self) -> bool {
        let tab = self.tab();
        tab.shape.is_some() || tab.text.is_some() || tab.live.is_some() || tab.doc.floating().is_some()
    }

    fn handle_nudge(&mut self, ctx: &Context) {
        if !matches!(self.tool, Tool::MovePixels | Tool::MoveSelection) || self.arrows_drive_timeline() {
            return;
        }
        let (mut dx, mut dy) = (0, 0);
        ctx.input(|i| {
            let step = if i.modifiers.ctrl { NUDGE_LARGE } else { NUDGE_SMALL };
            for (key, (kx, ky)) in [(Key::ArrowLeft, (-1, 0)), (Key::ArrowRight, (1, 0)), (Key::ArrowUp, (0, -1)), (Key::ArrowDown, (0, 1))] {
                if i.key_pressed(key) {
                    dx += kx * step;
                    dy += ky * step;
                }
            }
        });
        if dx == 0 && dy == 0 {
            return;
        }
        let tool = self.tool;
        let tab = self.tab_mut();
        if tool == Tool::MovePixels {
            let lifted = tab.doc.begin_move_pixels();
            tab.doc.seal_floating_step();
            let moved = tab.doc.move_floating(dx, dy);
            tab.mark_dirty(lifted.union(moved));
        } else if let Some(before) = tab.doc.begin_selection_move() {
            tab.doc.move_selection(dx, dy);
            tab.doc.finish_selection_move(Some(before));
        }
    }
}

fn key_letter(key: Key) -> Option<char> {
    Some(match key {
        Key::S => 's',
        Key::M => 'm',
        Key::O => 'o',
        Key::F => 'f',
        Key::G => 'g',
        Key::B => 'b',
        Key::P => 'p',
        Key::E => 'e',
        Key::K => 'k',
        Key::L => 'l',
        Key::Z => 'z',
        Key::H => 'h',
        _ => return None,
    })
}

fn visible_corner(tab: &DocTab, canvas: Rect) -> IPoint {
    let corner = tab.view.screen_to_image(canvas, canvas.min);
    let x = (corner.x.ceil() as i32).clamp(0, tab.doc.width() as i32 - 1) + PASTE_MARGIN;
    let y = (corner.y.ceil() as i32).clamp(0, tab.doc.height() as i32 - 1) + PASTE_MARGIN;
    IPoint::new(x, y)
}

pub fn luminance(c: Rgba) -> u8 {
    ((c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114) / 1000) as u8
}

fn raster_from_color_image(image: &egui::ColorImage) -> Result<Raster, chequer_core::raster::RasterError> {
    let bytes = image.pixels.iter().flat_map(|pixel| pixel.to_srgba_unmultiplied()).collect();
    Raster::from_rgba(image.width() as u32, image.height() as u32, bytes)
}

fn shortcut_table() -> Vec<(KeyboardShortcut, Command)> {
    let ctrl = Modifiers::CTRL;
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    let ctrl_alt = Modifiers::CTRL | Modifiers::ALT;
    let key = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
    vec![
        (key(ctrl_alt, Key::S), Command::SaveAll),
        (key(ctrl_alt, Key::V), Command::PasteNewImage),
        (key(ctrl_shift, Key::S), Command::SaveAs),
        (key(ctrl_shift, Key::C), Command::CopyMerged),
        (key(ctrl_shift, Key::V), Command::PasteNewLayer),
        (key(ctrl_shift, Key::X), Command::CropToSelection),
        (key(ctrl_shift, Key::R), Command::CanvasSize),
        (key(ctrl_shift, Key::F), Command::Flatten),
        (key(ctrl_shift, Key::N), Command::AddLayer),
        (key(ctrl_shift, Key::T), Command::ReopenTab),
        (key(ctrl_shift, Key::M), Command::ToggleTiling),
        (key(ctrl_shift, Key::D), Command::DuplicateLayer),
        (key(ctrl_shift, Key::Delete), Command::DeleteLayer),
        (key(ctrl_shift, Key::B), Command::ZoomSelection),
        (key(ctrl_shift, Key::I), Command::InvertColors),
        (key(ctrl_shift, Key::G), Command::BlackAndWhite),
        (key(ctrl_shift, Key::E), Command::Effect(chequer_core::effects::ids::SEPIA)),
        (key(ctrl_shift, Key::F5), Command::ResetLayout),
        (key(ctrl_shift, Key::F6), Command::ResetLayout),
        (key(ctrl_shift, Key::F7), Command::ResetLayout),
        (key(ctrl_shift, Key::F8), Command::ResetLayout),
        (key(ctrl_shift, Key::Tab), Command::PrevTab),
        (key(ctrl, Key::N), Command::New),
        (key(ctrl, Key::T), Command::New),
        (key(ctrl, Key::O), Command::Open),
        (key(ctrl, Key::S), Command::Save),
        (key(ctrl, Key::W), Command::CloseTab),
        (key(ctrl, Key::F), Command::RepeatEffect),
        (key(ctrl, Key::Z), Command::Undo),
        (key(ctrl, Key::Y), Command::Redo),
        (key(ctrl, Key::X), Command::Cut),
        (key(ctrl, Key::C), Command::Copy),
        (key(ctrl, Key::V), Command::Paste),
        (key(ctrl, Key::A), Command::SelectAll),
        (key(ctrl, Key::D), Command::Deselect),
        (key(ctrl, Key::I), Command::InvertSelection),
        (key(ctrl, Key::R), Command::ResizeImage),
        (key(ctrl, Key::H), Command::RotateCw),
        (key(ctrl, Key::G), Command::RotateCcw),
        (key(ctrl, Key::M), Command::MergeDown),
        (key(ctrl, Key::B), Command::ZoomWindow),
        (key(ctrl, Key::Num0), Command::ActualSize),
        (key(ctrl, Key::Plus), Command::ZoomIn),
        (key(ctrl, Key::Equals), Command::ZoomIn),
        (key(ctrl, Key::Minus), Command::ZoomOut),
        (key(ctrl, Key::Tab), Command::NextTab),
        (key(Modifiers::NONE, Key::F4), Command::LayerProperties),
        (key(Modifiers::NONE, Key::F5), Command::TogglePanel(PanelId::Tools)),
        (key(Modifiers::NONE, Key::F6), Command::TogglePanel(PanelId::History)),
        (key(Modifiers::NONE, Key::F7), Command::TogglePanel(PanelId::Layers)),
        (key(Modifiers::NONE, Key::F8), Command::TogglePanel(PanelId::Colors)),
        (key(Modifiers::NONE, Key::F9), Command::TogglePanel(PanelId::Swatches)),
        (key(Modifiers::NONE, Key::F10), Command::TogglePanel(PanelId::Timeline)),
        (key(Modifiers::NONE, Key::Comma), Command::PrevFrame),
        (key(Modifiers::NONE, Key::Period), Command::NextFrame),
        (key(Modifiers::NONE, Key::Delete), Command::EraseSelection),
        (key(Modifiers::ALT, Key::Backspace), Command::FillSelection),
    ]
}

pub(crate) const TOOLS_PANEL_SIZE: Vec2 = vec2(62.0, 270.0);
pub(crate) const COLORS_PANEL_SIZE: Vec2 = vec2(250.0, 380.0);
pub(crate) const SWATCHES_PANEL_SIZE: Vec2 = vec2(236.0, 190.0);
pub(crate) const TIMELINE_PANEL_SIZE: Vec2 = vec2(560.0, 230.0);
const SIDE_PANEL_WIDTH: f32 = 300.0;
pub(crate) const HISTORY_PANEL_SIZE: Vec2 = vec2(SIDE_PANEL_WIDTH, HISTORY_PANEL_HEIGHT);
pub(crate) const LAYERS_PANEL_SIZE: Vec2 = vec2(SIDE_PANEL_WIDTH, LAYERS_PANEL_HEIGHT);
const HISTORY_PANEL_HEIGHT: f32 = 190.0;
const LAYERS_PANEL_HEIGHT: f32 = 340.0;

pub fn default_panels(workspace: Rect) -> Panels {
    let at = |corner, size| PanelState::at_corner(corner, Vec2::ZERO, size, workspace);
    let closed = |mut state: PanelState| {
        state.open = false;
        state
    };
    Panels {
        tools: at(Corner::TOP_LEFT, TOOLS_PANEL_SIZE),
        history: at(Corner::TOP_RIGHT, HISTORY_PANEL_SIZE),
        layers: at(Corner::BOTTOM_RIGHT, LAYERS_PANEL_SIZE),
        colors: at(Corner::BOTTOM_LEFT, COLORS_PANEL_SIZE),
        swatches: PanelState::at_corner(Corner::BOTTOM_LEFT, vec2(COLORS_PANEL_SIZE.x, 0.0), SWATCHES_PANEL_SIZE, workspace),
        timeline: closed(at(Corner::BOTTOM_LEFT, TIMELINE_PANEL_SIZE)),
    }
}

impl eframe::App for App {
    fn on_exit(&mut self) {
        self.recovery = None;
        crate::config::save_panels(&self.panels);
    }

    fn raw_input_hook(&mut self, ctx: &Context, raw_input: &mut egui::RawInput) {
        if !self.keyboard_visible {
            return;
        }
        for event in crate::device::take_ime_events() {
            if event == crate::device::ImeEvent::Hide {
                self.keyboard_visible = false;
                self.keyboard_dismissed = true;
                ctx.memory_mut(|memory| memory.stop_text_input());
            } else {
                raw_input.events.extend(ime_events(event));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        crate::filepicker::remember_window(frame);
        let ctx = ui.ctx().clone();
        self.skin.set_grain_strength(if self.settings.grain_enabled { self.settings.grain } else { 0.0 });
        crate::scroll_fx::boost_fast_scroll(&ctx);
        self.apply_scale(&ctx);
        self.fit_window_to_monitor(&ctx);
        self.poll_file_events(&ctx);
        crate::device::set_haptics_enabled(self.settings.haptics);
        self.stylus.set_pen_only(self.settings.pen_only);
        crate::device::capture_volume_keys(self.settings.volume_undo);
        for key in crate::device::take_volume_keys() {
            self.run(&ctx, if key == crate::device::VolumeKey::Up { Command::Redo } else { Command::Undo });
        }
        self.trackpad.set_mode(self.settings.trackpad.mode, &ctx);
        self.pen.update(&ctx, self.settings.pen_wayland, self.settings.pen_response, |device| self.stylus.draws(device));
        self.apply_gestures(&ctx);
        self.sync_auto_tool();
        self.handle_shortcuts(&ctx);
        self.handle_close_request(&ctx);
        self.show_top_bars(ui, &ctx);
        self.show_status_bar(ui);
        self.advance_playback(&ctx);
        self.show_workspace(ui, &ctx);
        self.show_effect_window(&ctx);
        self.show_dialogs(&ctx);
        self.update_window_title(&ctx);
        self.sync_keyboard(&ctx);
        self.autosave(&ctx);
        self.persist_panels(&ctx);
    }
}

impl App {
    fn persist_panels(&mut self, ctx: &Context) {
        if ctx.input(|input| input.pointer.any_down()) {
            return;
        }
        let layout = crate::config::panels_text(&self.panels);
        if layout != self.saved_layout {
            crate::config::save_panels_text(&layout);
            self.saved_layout = layout;
        }
    }

    fn autosave(&mut self, ctx: &Context) {
        let now = ctx.input(|input| input.time);
        if now - self.last_autosave >= AUTOSAVE_INTERVAL_SECONDS {
            self.last_autosave = now;
            if let Some(session) = &self.recovery {
                session.sync(&mut self.tabs, &self.colors.palette);
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(AUTOSAVE_INTERVAL_SECONDS));
    }

    fn clipboard_image_size(&mut self) -> Option<(u32, u32)> {
        let raster = self.clipboard.get().ok().flatten()?;
        Some((raster.width().min(MAX_SIDE), raster.height().min(MAX_SIDE)))
    }

    fn apply_scale(&mut self, ctx: &Context) {
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        let pixel = self.settings.ui_pixel;
        crate::skin::set_ui_pixel(pixel);
        ctx.set_pixels_per_point(self.settings.ui_scale.unwrap_or(native * SYSTEM_SCALE_FACTOR));
        let scale = ctx.pixels_per_point();
        let layout_changed = (scale - self.applied_scale).abs() > SCALE_EPSILON || pixel != self.applied_pixel;
        if !layout_changed && self.settings.font == self.applied_font {
            return;
        }
        self.applied_scale = scale;
        self.applied_pixel = pixel;
        self.applied_font = self.settings.font.clone();
        crate::skin::set_font_choice(self.applied_font.clone());
        if let Some(message) = crate::skin::refresh_style(ctx) {
            self.settings.font = FontChoice::Builtin;
            self.applied_font = FontChoice::Builtin;
            crate::skin::set_font_choice(FontChoice::Builtin);
            crate::skin::refresh_style(ctx);
            self.fail(message);
        }
        if layout_changed {
            self.tabs.iter_mut().for_each(|tab| tab.view.needs_initial_fit = true);
        }
    }

    fn fit_window_to_monitor(&mut self, ctx: &Context) {
        let (monitor, inner) = ctx.input(|i| (i.viewport().monitor_size, i.viewport().inner_rect));
        let (Some(monitor), Some(inner)) = (monitor, inner) else {
            return;
        };
        match self.window_fit {
            WindowFit::Pending => {
                let limit = monitor * WINDOW_SCREEN_SHARE;
                if inner.width() > limit.x || inner.height() > limit.y {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner.size().min(limit)));
                    self.window_fit = WindowFit::Resizing { from: inner.size() };
                } else {
                    self.window_fit = WindowFit::Done;
                }
            }
            WindowFit::Resizing { from } if (inner.size() - from).length() > WINDOW_RESIZE_EPSILON => {
                self.layout_pending = true;
                self.tabs.iter_mut().for_each(|tab| tab.view.needs_initial_fit = true);
                self.window_fit = WindowFit::Done;
            }
            WindowFit::Resizing { .. } | WindowFit::Done => {}
        }
    }

    fn handle_close_request(&mut self, ctx: &Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if let Some(index) = self.tabs.iter().position(DocTab::is_modified) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.switch_tab(index);
            self.dialog = Some(Dialog::Confirm { tab: index, action: ConfirmAction::Exit });
        }
    }

    fn update_window_title(&mut self, ctx: &Context) {
        let tab = self.tab();
        let mark = if tab.is_modified() { "*" } else { "" };
        let title = format!("{mark}{} - {APP_NAME} {APP_VERSION}", tab.name);
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    pub(crate) fn blend_options() -> [(BlendMode, Text); BlendMode::ALL.len()] {
        BlendMode::ALL.map(|mode| {
            let text = match mode {
                BlendMode::Normal => Text::BlendNormal,
                BlendMode::Multiply => Text::BlendMultiply,
                BlendMode::Additive => Text::BlendAdditive,
                BlendMode::ColorBurn => Text::BlendColorBurn,
                BlendMode::ColorDodge => Text::BlendColorDodge,
                BlendMode::Reflect => Text::BlendReflect,
                BlendMode::Glow => Text::BlendGlow,
                BlendMode::Overlay => Text::BlendOverlay,
                BlendMode::Difference => Text::BlendDifference,
                BlendMode::Negation => Text::BlendNegation,
                BlendMode::Lighten => Text::BlendLighten,
                BlendMode::Darken => Text::BlendDarken,
                BlendMode::Screen => Text::BlendScreen,
                BlendMode::Xor => Text::BlendXor,
                BlendMode::HardLight => Text::BlendHardLight,
                BlendMode::SoftLight => Text::BlendSoftLight,
                BlendMode::Color => Text::BlendColor,
                BlendMode::Luminosity => Text::BlendLuminosity,
                BlendMode::Hue => Text::BlendHue,
                BlendMode::Saturation => Text::BlendSaturation,
            };
            (mode, text)
        })
    }

    pub(crate) fn blend_labels() -> Vec<(BlendMode, &'static str)> {
        Self::blend_options().into_iter().map(|(mode, text)| (mode, t(text))).collect()
    }

    pub(crate) fn float_mode_label(mode: AlphaMode) -> Text {
        match mode {
            AlphaMode::Blend => Text::PasteBlend,
            AlphaMode::Replace => Text::PasteReplace,
        }
    }
}

fn ime_events(event: crate::device::ImeEvent) -> Vec<egui::Event> {
    use crate::device::ImeEvent;
    let key = |key| {
        [true, false].map(|pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE })
    };
    match event {
        ImeEvent::Text(text) => vec![egui::Event::Text(text)],
        ImeEvent::Backspace => key(egui::Key::Backspace).into(),
        ImeEvent::Enter => key(egui::Key::Enter).into(),
        ImeEvent::Hide => Vec::new(),
    }
}
