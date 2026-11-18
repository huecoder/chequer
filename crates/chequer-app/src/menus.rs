
use chequer_core::document::{AlphaMode, Resample};
use chequer_core::effects::{EffectId, ids};
use chequer_core::gradient::GradientKind;
use chequer_core::sample::{AlphaMix, MixSpace};
use chequer_core::selection::CombineMode;
use chequer_core::shapes::StrokeAlign;
use chequer_core::shapes::ShapeKind;
use chequer_core::selection::LassoSmoothing;
use chequer_core::spline::LineMode;
use chequer_core::transform::RotationMethod;
use eframe::egui::scroll_area::{ScrollAreaOutput, ScrollBarVisibility, ScrollSource};
use eframe::egui::{self, Align, Align2, Color32, Context, Layout, Painter, Rect, Sense, Shape, Ui, UiBuilder, pos2, vec2};

use crate::metrics::{AUTO_SIZE, BarMargin, INLINE_SPACING, INLINE_SPACING_WIDE, OPTIONS_BAR_MARGIN, STATUS_BAR_MARGIN, TAB_BAR_MARGIN};
use crate::anim::{Slide, Slot};
use crate::app::{App, Command, PanelId, SystemFonts, default_panels};
use crate::canvas::tool_fill_style_label;
use crate::effects_ui::{effect_icon, effect_label};
use crate::command_registry::all_commands;
use crate::i18n::{Text, TextScope, t, text_or_key};
use crate::icons::Icon;
use crate::scroll_fx::ScrollFx;
use crate::skin::{self, Bevel, Font, Plate};
use crate::tools::{BRUSH_SIZE_MAX, BRUSH_SIZE_MIN, FillStyle, PICK_SIZES, TextUnit, MoveMethod, Sampling, TEXT_OUTLINE_WIDTH_MAX, TEXT_OUTLINE_WIDTH_MIN, TEXT_SIZE_MAX, TEXT_SIZE_MIN, Tool};
use chequer_core::text::{TextAlign, TextSmoothing};
use crate::view::{ZOOM_MAX, ZOOM_MIN};
use crate::widgets::{DragSpec, ICON_BUTTON_SIZE, choice_combo, combo_box, drag_value, glyph_button, icon_button, icon_button_sized, labeled, paint_cross, plain_slider, search_combo, separator, stepped_index, text_button, wheel_over, wheel_while};

const TAB_SIZE: f32 = 52.0;
const LEFT_COLUMN_WIDTH: f32 = 430.0;
const MIN_TAB_STRIP_WIDTH: f32 = 160.0;
const TAB_GAP: f32 = 3.0;
const TAB_THUMB: f32 = 38.0;
const TAB_CLOSE_SIZE: f32 = 16.0;
const TAB_ARROW_ZONE: f32 = 16.0;
const TAB_ARROW_COLUMNS: usize = 4;
const TAB_ARROW_HALF_HEIGHT: f32 = 5.0;
const TAB_ARROW_STEP_TABS: f32 = 3.0;
const TAB_SCROLL_EDGE: f32 = 0.5;
const WINDOW_TOGGLES: usize = 6;
const WINDOW_TOGGLE_SPACING: f32 = 2.0;
const STATUS_HEIGHT: f32 = 26.0;
const STATUS_SEPARATOR_HEIGHT: f32 = 14.0;
const MENU_MIN_WIDTH: f32 = 280.0;
const TOOL_OPTION_HEIGHT: f32 = 34.0;
const OPTIONS_SCROLL_ID: &str = "options-bar-scroll";
const BAR_EDGE_ROWS: f32 = 2.0;
const TOLERANCE_SLIDER_WIDTH: f32 = 110.0;
const PICK_SIZE_COMBO_WIDTH: f32 = 64.0;
const TEXT_UNIT_COMBO_WIDTH: f32 = 48.0;
const PICK_MIX_COMBO_WIDTH: f32 = 140.0;
const PICK_ALPHA_COMBO_WIDTH: f32 = 170.0;
const ZOOM_SLIDER_WIDTH: f32 = 130.0;

impl App {
    pub(crate) fn show_top_bars(&mut self, ui: &mut Ui, ctx: &Context) {
        let frame = egui::Frame::NONE.fill(skin::theme().chrome).inner_margin(TAB_BAR_MARGIN.frame_margin());
        egui::Panel::top("top-bar").frame(frame).show(ui, |ui| {
            self.paint_panel_grain(ui, TAB_BAR_MARGIN);
            let toggles_width = WINDOW_TOGGLES as f32 * (ICON_BUTTON_SIZE + 2.0) + (WINDOW_TOGGLES - 1) as f32 * WINDOW_TOGGLE_SPACING;
            let needed_width = LEFT_COLUMN_WIDTH + MIN_TAB_STRIP_WIDTH + toggles_width + 2.0 * ui.spacing().item_spacing.x;
            if ui.available_width() < needed_width {
                ui.vertical(|ui| {
                    self.menu_bar(ui, ctx);
                    self.actions_bar(ui, ctx);
                    let row = vec2(ui.available_width(), TAB_SIZE);
                    ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| self.tab_row(ui, ctx, toggles_width));
                });
                return;
            }
            let row = vec2(ui.available_width(), TAB_SIZE);
            ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.set_width(LEFT_COLUMN_WIDTH);
                    self.menu_bar(ui, ctx);
                    self.actions_bar(ui, ctx);
                });
                self.tab_row(ui, ctx, toggles_width);
            });
        });
        let frame = egui::Frame::NONE.fill(skin::theme().chrome).inner_margin(OPTIONS_BAR_MARGIN.frame_margin());
        egui::Panel::top("options-bar").frame(frame).show(ui, |ui| {
            self.paint_panel_grain(ui, OPTIONS_BAR_MARGIN);
            let bar = ui.max_rect();
            let edge = self.paint_bar_edge(ui, bar);
            ui.add_space(edge);
            let row = vec2(ui.available_width(), TOOL_OPTION_HEIGHT - edge);
            ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt(OPTIONS_SCROLL_ID)
                    .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
                    .auto_shrink([false, false])
                    .show(ui, |ui| ui.horizontal_centered(|ui| self.tool_options(ui, ctx)))
                    .with_fx(ui, skin::theme().chrome);
            });
        });
    }

    fn tab_row(&mut self, ui: &mut Ui, ctx: &Context, toggles_width: f32) {
        let strip_width = (ui.available_width() - toggles_width - 2.0 * ui.spacing().item_spacing.x).max(0.0);
        ui.allocate_ui_with_layout(vec2(strip_width, TAB_SIZE), Layout::left_to_right(Align::Center), |ui| self.tab_strip(ui, ctx));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.window_toggles(ui, ctx));
    }

    fn text_options(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.spacing_mut().item_spacing = INLINE_SPACING;
        let skin = self.skin.clone();
        if self.system_fonts == SystemFonts::NotLoaded {
            self.load_system_fonts(ctx);
        }
        ui.label(t(Text::TextFont));
        self.text_font_combo(ui, &skin);
        ui.label(t(Text::TextSize));
        let unit = self.settings.text_unit;
        let mut shown = unit.from_pixels(self.options.text.size);
        let spec = DragSpec { speed: 0.2, suffix: unit.suffix(), max_decimals: Some(1), wheel_step: 1.0 };
        let range = unit.from_pixels(TEXT_SIZE_MIN)..=unit.from_pixels(TEXT_SIZE_MAX);
        if drag_value(ui, &skin, &mut shown, range, AUTO_SIZE, &spec).changed() {
            self.options.text.size = unit.to_pixels(shown);
        }
        let units = [(TextUnit::Point, "pt"), (TextUnit::Pixel, "px")];
        choice_combo(ui, &skin, "text-unit", TEXT_UNIT_COMBO_WIDTH, &mut self.settings.text_unit, &units);
        separator(ui, ICON_BUTTON_SIZE);
        let text = &mut self.options.text;
        for (label, flag, tip) in [
            ("B", &mut text.bold, Text::TextBold),
            ("I", &mut text.italic, Text::TextItalic),
            ("U", &mut text.underline, Text::TextUnderline),
            ("S", &mut text.strikethrough, Text::TextStrikethrough),
        ] {
            if glyph_button(ui, &skin, label, *flag, t(tip)).clicked() {
                *flag = !*flag;
            }
        }
        separator(ui, ICON_BUTTON_SIZE);
        for (align, label, tip) in [
            (TextAlign::Left, t(Text::TextAlignLeftShort), Text::TextAlignLeft),
            (TextAlign::Center, t(Text::TextAlignCenterShort), Text::TextAlignCenter),
            (TextAlign::Right, t(Text::TextAlignRightShort), Text::TextAlignRight),
        ] {
            if glyph_button(ui, &skin, label, text.align == align, t(tip)).clicked() {
                text.align = align;
            }
        }
        let center_on_canvas = glyph_button(ui, &skin, "+", false, t(Text::TextCenterOnCanvas)).clicked();
        separator(ui, ICON_BUTTON_SIZE);
        ui.checkbox(&mut text.hinting, t(Text::TextHinting)).on_hover_text(t(Text::TextHintingTip));
        ui.label(t(Text::TextSmoothing));
        let smoothing = [
            (TextSmoothing::None, t(Text::SmoothNone)),
            (TextSmoothing::Grayscale, t(Text::SmoothGrayscale)),
            (TextSmoothing::LcdRgb, t(Text::SmoothLcdRgb)),
            (TextSmoothing::LcdBgr, t(Text::SmoothLcdBgr)),
        ];
        choice_combo(ui, &skin, "text-smoothing", 160.0, &mut text.smoothing, &smoothing);
        separator(ui, ICON_BUTTON_SIZE);
        ui.label(t(Text::OptFill));
        let styles = labeled([FillStyle::Filled, FillStyle::Outline, FillStyle::Both], |style| t(tool_fill_style_label(style)));
        choice_combo(ui, &skin, "text-fill-style", 110.0, &mut text.fill_style, &styles);
        if text.fill_style != FillStyle::Filled {
            ui.label(t(Text::TextOutlineWidth));
            let spec = DragSpec { speed: 0.1, suffix: " px", max_decimals: Some(1), wheel_step: 0.5 };
            drag_value(ui, &skin, &mut text.outline_width, TEXT_OUTLINE_WIDTH_MIN..=TEXT_OUTLINE_WIDTH_MAX, AUTO_SIZE, &spec);
            let aligns = [
                (StrokeAlign::Inside, t(Text::TextOutlineInside)),
                (StrokeAlign::Center, t(Text::TextOutlineCenter)),
                (StrokeAlign::Outside, t(Text::TextOutlineOutside)),
            ];
            choice_combo(ui, &skin, "text-outline-align", 110.0, &mut text.outline_align, &aligns);
        }
        if center_on_canvas {
            self.center_text_on_canvas();
        }
    }

    fn text_font_combo(&mut self, ui: &mut Ui, skin: &skin::Skin) {
        let families: &[String] = match &self.system_fonts {
            SystemFonts::Ready(families) => families,
            SystemFonts::NotLoaded | SystemFonts::Loading => &[],
        };
        let options: Vec<&str> = std::iter::once(t(Text::TextFontDefault)).chain(families.iter().map(String::as_str)).collect();
        let current = match &self.options.text.family {
            None => Some(0),
            Some(family) => families.iter().position(|f| f == family).map(|i| i + 1),
        };
        let (response, mut chosen) = search_combo(ui, skin, "text-font", 170.0, &options, current);
        let browse = glyph_button(ui, skin, "...", false, t(Text::TextFontBrowse)).clicked();
        if let Some(target) = stepped_index(options.len(), current, wheel_over(ui, &response).notches) {
            chosen = Some(target);
        }
        if let Some(index) = chosen {
            self.options.text.family = index.checked_sub(1).map(|i| families[i].clone());
        }
        if browse {
            self.open_font_picker(ui.ctx(), crate::font_picker::FontPickerTarget::TextTool);
        }
    }

    fn paint_panel_grain(&self, ui: &Ui, margin: BarMargin) {
        self.skin.paint_grain(ui.painter(), ui.max_rect().expand2(margin.grain_inset()));
    }

    fn paint_bar_edge(&self, ui: &Ui, bar: Rect) -> f32 {
        let ppp = skin::grid_ppp(ui.painter().pixels_per_point());
        let step = 1.0 / ppp;
        let y = (bar.min.y * ppp).round() / ppp;
        ui.painter().rect_filled(Rect::from_min_size(egui::pos2(bar.min.x - 8.0, y), vec2(bar.width() + 16.0, step)), 0.0, skin::theme().groove_dark);
        ui.painter().rect_filled(Rect::from_min_size(egui::pos2(bar.min.x - 8.0, y + step), vec2(bar.width() + 16.0, step)), 0.0, skin::theme().groove_light);
        step * BAR_EDGE_ROWS
    }

    fn menu_item(&mut self, ui: &mut Ui, ctx: &Context, command: Command, icon: Option<Icon>, text: &str, shortcut: &str, enabled: bool) {
        ui.set_min_width(MENU_MIN_WIDTH);
        let mut button = match icon {
            Some(icon) => egui::Button::image_and_text(self.skin.icons.image(icon), text),
            None => egui::Button::new(text),
        };
        if !shortcut.is_empty() {
            button = button.shortcut_text(shortcut);
        }
        if ui.add_enabled(enabled, button).clicked() {
            self.run(ctx, command);
            ui.close();
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui, ctx: &Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t(Text::MenuFile), |ui| self.file_menu(ui, ctx));
            ui.menu_button(t(Text::MenuEdit), |ui| self.edit_menu(ui, ctx));
            ui.menu_button(t(Text::MenuView), |ui| self.view_menu(ui, ctx));
            ui.menu_button(t(Text::MenuImage), |ui| self.image_menu(ui, ctx));
            ui.menu_button(t(Text::MenuLayers), |ui| self.layers_menu(ui, ctx));
            ui.menu_button(t(Text::MenuAdjustments), |ui| self.adjustments_menu(ui, ctx));
            ui.menu_button(t(Text::MenuEffects), |ui| self.effects_menu(ui, ctx));
            if !all_commands().is_empty() {
                ui.menu_button(t(Text::MenuPlugins), |ui| self.plugins_menu(ui, ctx));
            }
        });
    }

    fn file_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        skin::menu_style(ui);
        self.menu_item(ui, ctx, Command::New, Some(Icon::New), t(Text::New), "Ctrl+N", true);
        self.menu_item(ui, ctx, Command::Open, Some(Icon::Open), t(Text::Open), "Ctrl+O", true);
        let recent = self.recent.clone();
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_image_text_button(self.skin.icons.image(Icon::Clock), t(Text::OpenRecent), |ui| {
                for path in &recent {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                        self.open_path(path);
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        self.menu_item(ui, ctx, Command::Save, Some(Icon::Save), t(Text::Save), "Ctrl+S", true);
        self.menu_item(ui, ctx, Command::SaveAs, Some(Icon::Save), t(Text::SaveAs), "Ctrl+Shift+S", true);
        self.menu_item(ui, ctx, Command::SaveAll, Some(Icon::SaveAll), t(Text::SaveAll), "Ctrl+Alt+S", true);
        ui.separator();
        if cfg!(target_os = "android") {
            self.menu_item(ui, ctx, Command::Share, None, t(Text::Share), "", true);
        }
        self.menu_item(ui, ctx, Command::CloseTab, Some(Icon::Close), t(Text::Close), "Ctrl+W", true);
        self.menu_item(ui, ctx, Command::ReopenTab, None, t(Text::ReopenTab), "Ctrl+Shift+T", !self.closed_tabs.is_empty());
        ui.separator();
        self.menu_item(ui, ctx, Command::Welcome, Some(Icon::Picture), t(Text::MenuSplash), "", true);
        self.menu_item(ui, ctx, Command::Exit, Some(Icon::Exit), t(Text::Exit), "", true);
    }

    fn edit_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        skin::menu_style(ui);
        let (can_undo, can_redo) = {
            let doc = &self.tab().doc;
            (self.tab().can_undo(), doc.history().can_redo())
        };
        self.menu_item(ui, ctx, Command::Undo, Some(Icon::Undo), t(Text::Undo), "Ctrl+Z", can_undo);
        self.menu_item(ui, ctx, Command::Redo, Some(Icon::Redo), t(Text::Redo), "Ctrl+Y", can_redo);
        ui.separator();
        self.menu_item(ui, ctx, Command::Cut, Some(Icon::Cut), t(Text::Cut), "Ctrl+X", true);
        self.menu_item(ui, ctx, Command::Copy, Some(Icon::Copy), t(Text::Copy), "Ctrl+C", true);
        self.menu_item(ui, ctx, Command::CopyMerged, Some(Icon::Copy), t(Text::CopyMerged), "Ctrl+Shift+C", true);
        self.menu_item(ui, ctx, Command::Paste, Some(Icon::Paste), t(Text::Paste), "Ctrl+V", true);
        self.menu_item(ui, ctx, Command::PasteNewLayer, Some(Icon::PasteLayer), t(Text::PasteNewLayer), "Ctrl+Shift+V", true);
        ui.separator();
        self.menu_item(ui, ctx, Command::EraseSelection, Some(Icon::Delete), t(Text::EraseSelection), "Del", true);
        self.menu_item(ui, ctx, Command::FillSelection, Some(Icon::Brush), t(Text::FillSelection), "Alt+Backspace", true);
        self.menu_item(ui, ctx, Command::InvertSelection, Some(Icon::SelectionInvert), t(Text::InvertSelection), "Ctrl+I", true);
        self.menu_item(ui, ctx, Command::SelectAll, Some(Icon::Rect), t(Text::SelectAll), "Ctrl+A", true);
        self.menu_item(ui, ctx, Command::Deselect, Some(Icon::Deselect), t(Text::Deselect), "Ctrl+D", true);
    }

    fn view_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        skin::menu_style(ui);
        self.menu_item(ui, ctx, Command::ZoomIn, Some(Icon::ZoomIn), t(Text::ZoomIn), "Ctrl++", true);
        self.menu_item(ui, ctx, Command::ZoomOut, Some(Icon::ZoomOut), t(Text::ZoomOut), "Ctrl+-", true);
        self.menu_item(ui, ctx, Command::ZoomWindow, Some(Icon::Resize), t(Text::ZoomToWindow), "Ctrl+B", true);
        self.menu_item(ui, ctx, Command::ActualSize, Some(Icon::Picture), t(Text::ActualSize), "Ctrl+0", true);
        ui.separator();
        let label = if self.show_grid { format!("{}  {}", t(Text::PixelGrid), "✓") } else { t(Text::PixelGrid).to_owned() };
        self.menu_item(ui, ctx, Command::ToggleGrid, Some(Icon::PixelGrid), &label, "", true);
        let tiled = self.tab().view.tiled;
        let label = if tiled { format!("{}  {}", t(Text::SeamlessMode), "✓") } else { t(Text::SeamlessMode).to_owned() };
        self.menu_item(ui, ctx, Command::ToggleTiling, Some(Icon::SeamlessTiling), &label, "Ctrl+Shift+M", true);
    }

    fn image_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        skin::menu_style(ui);
        let has_selection = self.tab().doc.selection().is_some();
        self.menu_item(ui, ctx, Command::CropToSelection, Some(Icon::Crop), t(Text::CropToSelection), "Ctrl+Shift+X", has_selection);
        self.menu_item(ui, ctx, Command::ResizeImage, Some(Icon::ResizeImage), t(Text::ResizeImage), "Ctrl+R", true);
        self.menu_item(ui, ctx, Command::CanvasSize, Some(Icon::CanvasSize), t(Text::CanvasSize), "Ctrl+Shift+R", true);
        ui.separator();
        self.menu_item(ui, ctx, Command::OffsetHalf, Some(Icon::Move), t(Text::OffsetHalf), "", true);
        self.menu_item(ui, ctx, Command::FlipHorizontal, Some(Icon::FlipHorizontal), t(Text::FlipHorizontal), "", true);
        self.menu_item(ui, ctx, Command::FlipVertical, Some(Icon::FlipVertical), t(Text::FlipVertical), "", true);
        ui.separator();
        self.menu_item(ui, ctx, Command::RotateCw, Some(Icon::RotateCw), t(Text::RotateCw), "Ctrl+H", true);
        self.menu_item(ui, ctx, Command::RotateCcw, Some(Icon::RotateCcw), t(Text::RotateCcw), "Ctrl+G", true);
        self.menu_item(ui, ctx, Command::Rotate180, Some(Icon::Refresh), t(Text::Rotate180), "", true);
        ui.separator();
        let multi = self.tab().doc.layers().len() > 1;
        self.menu_item(ui, ctx, Command::Flatten, Some(Icon::Merge), t(Text::Flatten), "Ctrl+Shift+F", multi);
    }

    fn layers_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        skin::menu_style(ui);
        let (count, active) = (self.tab().doc.layers().len(), self.tab().doc.active_layer());
        self.menu_item(ui, ctx, Command::AddLayer, Some(Icon::Add), t(Text::AddLayer), "Ctrl+Shift+N", true);
        self.menu_item(ui, ctx, Command::DeleteLayer, Some(Icon::LayerRemove), t(Text::DeleteLayer), "Ctrl+Shift+Del", count > 1);
        self.menu_item(ui, ctx, Command::DuplicateLayer, Some(Icon::Duplicate), t(Text::DuplicateLayer), "Ctrl+Shift+D", true);
        self.menu_item(ui, ctx, Command::MergeDown, Some(Icon::Merge), t(Text::MergeLayerDown), "Ctrl+M", active > 0);
        ui.separator();
        self.menu_item(ui, ctx, Command::LayerUp, Some(Icon::Up), t(Text::MoveLayerUp), "", active + 1 < count);
        self.menu_item(ui, ctx, Command::LayerDown, Some(Icon::Down), t(Text::MoveLayerDown), "", active > 0);
        ui.separator();
        self.menu_item(ui, ctx, Command::LayerProperties, Some(Icon::Properties), t(Text::LayerProperties), "F4", true);
    }

    fn adjustments_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        skin::menu_style(ui);
        let adjust_item = |app: &mut Self, ui: &mut Ui, kind: EffectId, shortcut: &str| {
            let name = effect_label(kind);
            let label = if kind.params().is_empty() { name.to_string() } else { format!("{name}...") };
            app.menu_item(ui, ctx, Command::Effect(kind), effect_icon(kind), &label, shortcut, true);
        };
        adjust_item(self, ui, ids::AUTO_LEVEL, "");
        self.menu_item(ui, ctx, Command::InvertColors, Some(Icon::EffectInvert), t(Text::InvertColors), "Ctrl+Shift+I", true);
        adjust_item(self, ui, ids::CURVES, "");
        adjust_item(self, ui, ids::HUE_SATURATION, "");
        adjust_item(self, ui, ids::POSTERIZE, "");
        adjust_item(self, ui, ids::LEVELS, "");
        adjust_item(self, ui, ids::BRIGHTNESS_CONTRAST, "");
        self.menu_item(ui, ctx, Command::BlackAndWhite, Some(Icon::EffectMonochrome), t(Text::BlackAndWhite), "Ctrl+Shift+M", true);
        adjust_item(self, ui, ids::SEPIA, "Ctrl+Shift+E");
    }

    fn plugins_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        skin::menu_style(ui);
        ui.add_enabled_ui(!self.tabs.is_empty(), |ui| {
            for id in all_commands() {
                if ui.button(text_or_key(TextScope::Command, id.as_str())).clicked() {
                    self.run(ctx, Command::Plugin(id));
                    ui.close();
                }
            }
        });
    }

    fn effects_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        skin::menu_style(ui);
        let has_document = !self.tabs.is_empty();
        ui.add_enabled_ui(has_document, |ui| self.effects_menu_items(ui, ctx));
    }

    fn actions_bar(&mut self, ui: &mut Ui, ctx: &Context) {
        let (can_undo, can_redo, has_selection) = {
            let doc = &self.tab().doc;
            (self.tab().can_undo(), doc.history().can_redo(), doc.selection().is_some())
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
            let skin = &self.skin;
            let mut pressed = None;
            let mut button = |ui: &mut Ui, enabled: bool, icon: Icon, tip: &str, command: Command| {
                let response = ui.add_enabled_ui(enabled, |ui| icon_button(ui, skin, icon, false, tip)).inner;
                if response.clicked() {
                    pressed = Some(command);
                }
            };
            button(ui, true, Icon::New, t(Text::New), Command::New);
            button(ui, true, Icon::Open, t(Text::Open), Command::Open);
            button(ui, true, Icon::Save, t(Text::Save), Command::Save);
            separator(ui, ICON_BUTTON_SIZE);
            button(ui, true, Icon::Cut, t(Text::Cut), Command::Cut);
            button(ui, true, Icon::Copy, t(Text::Copy), Command::Copy);
            button(ui, true, Icon::Paste, t(Text::Paste), Command::Paste);
            button(ui, has_selection, Icon::Crop, t(Text::CropToSelection), Command::CropToSelection);
            button(ui, has_selection, Icon::Deselect, t(Text::Deselect), Command::Deselect);
            separator(ui, ICON_BUTTON_SIZE);
            button(ui, can_undo, Icon::Undo, t(Text::Undo), Command::Undo);
            button(ui, can_redo, Icon::Redo, t(Text::Redo), Command::Redo);
            separator(ui, ICON_BUTTON_SIZE);
            let grid = icon_button(ui, skin, Icon::PixelGrid, self.show_grid, t(Text::PixelGrid));
            if grid.clicked() {
                pressed = Some(Command::ToggleGrid);
            }
            if let Some(command) = pressed {
                self.run(ctx, command);
            }
        });
    }

    fn tab_strip(&mut self, ui: &mut Ui, ctx: &Context) {
        let mut switch_to = None;
        let mut close = None;
        let motion = self.tab_slot.and_then(|slot| slot.extent(ctx, TAB_SIZE + TAB_GAP).map(|extent| (slot, extent)));
        if motion.is_none() {
            self.tab_slot = None;
        }
        let closing = motion
            .filter(|(slot, _)| slot.slide == Slide::Out)
            .and_then(|_| self.closed_tabs.last_mut())
            .map(|tab| (tab.tab_thumbnail(ctx), vec2(tab.doc.width() as f32, tab.doc.height() as f32)));
        let skin = self.skin.clone();
        let strip_id = ui.scope_id().with("tab-strip");
        let arrow_step = ui.data_mut(|data| data.remove_temp::<f32>(strip_id)).unwrap_or(0.0);
        let followed_key = (self.active, self.tabs.len());
        let follow_active = ui.data(|data| data.get_temp::<(usize, usize)>(strip_id)) != Some(followed_key);
        ui.data_mut(|data| data.insert_temp(strip_id, followed_key));
        let scroll = egui::ScrollArea::horizontal()
            .id_salt(strip_id)
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
            .scroll_source(ScrollSource { scroll_bar: false, mouse_wheel: false, ..ScrollSource::default() })
            .auto_shrink([true, false])
            .show(ui, |ui| ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing = vec2(TAB_GAP, 0.0);
            if arrow_step != 0.0 {
                ui.scroll_with_delta(vec2(arrow_step, 0.0));
            }
            for index in 0..self.tabs.len() {
                closing_gap(ui, &skin, motion, closing, index);
                let active = index == self.active;
                let thumb = self.tabs[index].tab_thumbnail(ctx);
                let opening_width = motion.filter(|(slot, _)| slot.slide == Slide::In && slot.index == index).map(|(_, extent)| extent - TAB_GAP);
                let (slot_rect, response) = ui.allocate_exact_size(vec2(opening_width.unwrap_or(TAB_SIZE).clamp(0.0, TAB_SIZE), TAB_SIZE), Sense::click());
                let rect = Rect::from_min_size(slot_rect.right_top() - vec2(TAB_SIZE, 0.0), vec2(TAB_SIZE, TAB_SIZE));
                if active && follow_active {
                    ui.scroll_to_rect(slot_rect.expand2(vec2(TAB_ARROW_ZONE, 0.0)), None);
                }
                let outer_clip = ui.clip_rect();
                ui.set_clip_rect(outer_clip.intersect(slot_rect));
                let close_rect = Rect::from_min_size(rect.right_top() + vec2(-TAB_CLOSE_SIZE - 4.0, 4.0), vec2(TAB_CLOSE_SIZE, TAB_CLOSE_SIZE));
                let close_response = ui.interact(close_rect, ui.scope_id().with(("tab-close", index)), Sense::click());
                let hovered = response.contains_pointer() || close_response.contains_pointer();
                let fill = if active { skin::theme().button_on } else if hovered { skin::theme().button_hover } else { skin::theme().button };
                self.skin.paint_plate(ui.painter(), rect, &Plate::button(fill, if active { Bevel::Sunken } else { Bevel::Raised }));
                paint_tab_thumbnail(ui, &self.skin, rect, thumb, vec2(self.tabs[index].doc.width() as f32, self.tabs[index].doc.height() as f32));
                if self.tabs[index].is_modified() {
                    skin::text_at(ui.painter(), rect.left_top() + vec2(6.0, 5.0), Align2::LEFT_TOP, "*", Font::Body, skin::theme().ink);
                }
                if hovered {
                    let hot = close_response.hovered() || close_response.is_pointer_button_down_on();
                    if hot {
                        self.skin.paint_plate(ui.painter(), close_rect, &Plate { border: skin::theme().close_hover, bevel: None, ..Plate::button(skin::theme().close_hover, Bevel::Raised) });
                    }
                    paint_cross(ui.painter(), close_rect, if hot { skin::theme().close_hover_text } else { skin::theme().ink });
                }
                let tooltip = format!("{}{}", self.tabs[index].name, if self.tabs[index].is_modified() { " *" } else { "" });
                let response = response.on_hover_text(tooltip);
                if close_response.clicked() || response.middle_clicked() {
                    close = Some(index);
                } else if response.clicked() {
                    switch_to = Some(index);
                }
                let skin = self.skin.clone();
                response.context_menu(|ui| {
                    skin::menu_style(ui);
                    let item = egui::Button::image_and_text(skin.icons.image(Icon::Close), t(Text::Close)).shortcut_text("Ctrl+W");
                    if ui.add(item).clicked() {
                        close = Some(index);
                        ui.close();
                    }
                });
                ui.set_clip_rect(outer_clip);
            }
            closing_gap(ui, &skin, motion, closing, self.tabs.len());
        }));
        let scroll = scroll.with_fx(ui, skin::theme().chrome);
        if let Some(step) = tab_strip_arrows(ui, strip_id, &scroll) {
            ui.data_mut(|data| data.insert_temp(strip_id, step));
            ctx.request_repaint();
        }
        let wheel = wheel_while(ui, ui.scope_id().with("tab-strip-wheel"), ui.rect_contains_pointer(scroll.inner_rect));
        switch_to = switch_to.or(stepped_index(self.tabs.len(), Some(self.active), wheel.notches));
        if let Some(index) = switch_to {
            self.switch_tab(index);
        }
        if let Some(index) = close {
            self.close_tab(index);
        }
    }

    fn window_toggles(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.spacing_mut().item_spacing = vec2(WINDOW_TOGGLE_SPACING, 0.0);
        let mut pressed = None;
        let skin = &self.skin;
        let size = vec2(ICON_BUTTON_SIZE + 2.0, ICON_BUTTON_SIZE + 2.0);
        if icon_button_sized(ui, skin, Icon::SettingsWindow, false, true, size, t(Text::Settings)).clicked() {
            pressed = Some(Command::Settings);
        }
        for (id, icon, text, open) in [
            (PanelId::Swatches, Icon::Palette, Text::Swatches, self.panels.swatches.open),
            (PanelId::Colors, Icon::ColorsWindow, Text::Colors, self.panels.colors.open),
            (PanelId::Timeline, Icon::Timeline, Text::Timeline, self.panels.timeline.open),
            (PanelId::Layers, Icon::LayersWindow, Text::Layers, self.panels.layers.open),
            (PanelId::History, Icon::HistoryWindow, Text::History, self.panels.history.open),
            (PanelId::Tools, Icon::ToolsWindow, Text::Tools, self.panels.tools.open),
        ] {
            if icon_button_sized(ui, skin, icon, open, true, size, t(text)).clicked() {
                pressed = Some(Command::TogglePanel(id));
            }
        }
        if let Some(command) = pressed {
            self.run(ctx, command);
        }
    }

    fn tool_options(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.spacing_mut().item_spacing = INLINE_SPACING;
        self.current_tool_label(ui);
        separator(ui, ICON_BUTTON_SIZE);
        let tool = self.tool;
        if tool.is_selection_tool() {
            self.selection_mode_buttons(ui);
        }
        if matches!(tool, Tool::MagicWand | Tool::Bucket) {
            self.tolerance_controls(ui);
        }
        if matches!(tool, Tool::MagicWand | Tool::Picker) {
            self.sampling_combo(ui);
        }
        if tool == Tool::Picker {
            self.pick_area_controls(ui);
        }
        if tool.uses_brush_size() {
            self.width_control(ui);
        }
        if matches!(tool, Tool::Brush | Tool::Eraser | Tool::CloneStamp) {
            ui.label(t(Text::OptBrush));
            self.brush_chip(ui);
        }
        if tool == Tool::Shapes {
            self.shape_kind_combo(ui);
            self.shape_param_controls(ui);
        }
        if tool == Tool::Gradient {
            self.gradient_kind_combo(ui);
        }
        if tool.is_shape_tool() {
            self.fill_style_combo(ui);
        }
        if tool == Tool::MovePixels {
            self.move_method_combo(ui);
            self.float_mode_combo(ui);
        }
        if matches!(tool, Tool::MovePixels | Tool::MoveSelection | Tool::RectSelect | Tool::EllipseSelect) {
            self.move_resample_combo(ui);
        }
        if tool == Tool::MovePixels && self.options.move_resample == Resample::Nearest {
            self.rotation_combo(ui);
        }
        if tool == Tool::Lasso {
            self.lasso_smoothing_combo(ui);
        }
        if tool == Tool::Line {
            self.line_mode_combo(ui);
        }
        if matches!(tool, Tool::Brush | Tool::Eraser | Tool::CloneStamp | Tool::Line | Tool::Shapes) {
            ui.checkbox(&mut self.options.antialias, t(Text::OptAntialias));
        }
        if tool == Tool::Pencil {
            ui.checkbox(&mut self.options.pixel_perfect, t(Text::OptPixelPerfect));
        }
        if matches!(tool, Tool::Brush | Tool::Pencil) {
            ui.checkbox(&mut self.options.smooth_path, t(Text::OptSmoothPath));
        }
        if matches!(tool, Tool::Brush | Tool::CloneStamp | Tool::Pencil | Tool::Bucket | Tool::Gradient | Tool::Line | Tool::Shapes) {
            self.blend_combo(ui);
        }
        if tool == Tool::Text {
            separator(ui, ICON_BUTTON_SIZE);
            self.text_options(ui, ctx);
        }
        if self.tab().view.tiled {
            separator(ui, ICON_BUTTON_SIZE);
            self.tiling_controls(ui);
        }
        if self.has_pending() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if text_button(ui, &self.skin, &format!("✓ {}", t(Text::OptDone))).clicked() {
                    self.run(ctx, Command::Commit);
                }
            });
        }
    }

    fn current_tool_label(&self, ui: &mut Ui) {
        ui.add(self.skin.icons.image(self.tool.icon()));
        ui.label(self.tool.name());
    }

    fn selection_mode_buttons(&mut self, ui: &mut Ui) {
        for (mode, icon, tip) in [
            (CombineMode::Replace, Icon::SelectReplace, Text::ModeReplace),
            (CombineMode::Add, Icon::SelectAdd, Text::ModeAdd),
            (CombineMode::Subtract, Icon::SelectSubtract, Text::ModeSubtract),
            (CombineMode::Intersect, Icon::SelectIntersect, Text::ModeIntersect),
            (CombineMode::Xor, Icon::SelectXor, Text::ModeXor),
        ] {
            let selected = self.options.select_mode == mode;
            if icon_button(ui, &self.skin, icon, selected, t(tip)).clicked() {
                self.options.select_mode = mode;
            }
        }
        separator(ui, ICON_BUTTON_SIZE);
    }

    fn tolerance_controls(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptTolerance));
        if let Some(fraction) = plain_slider(ui, &self.skin, self.options.tolerance, TOLERANCE_SLIDER_WIDTH) {
            self.options.tolerance = fraction;
        }
        ui.label(format!("{:>3.0}%", self.options.tolerance * 100.0));
        ui.label(t(Text::OptFlood));
        let skin = self.skin.clone();
        choice_combo(ui, &skin, "flood-mode", 100.0, &mut self.options.flood_global, &[(false, t(Text::FloodContiguous)), (true, t(Text::FloodGlobal))]);
    }

    fn tiling_controls(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptTileCopies));
        if let Some(fraction) = plain_slider(ui, &self.skin, self.tile_opacity, TOLERANCE_SLIDER_WIDTH) {
            self.tile_opacity = fraction;
        }
        ui.label(format!("{:>3.0}%", self.tile_opacity * 100.0));
        ui.checkbox(&mut self.tile_frame, t(Text::OptTileFrame));
    }

    fn sampling_combo(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptSampling));
        let name = |s: Sampling| t(if s == Sampling::Layer { Text::SamplingLayer } else { Text::SamplingImage });
        let skin = self.skin.clone();
        choice_combo(ui, &skin, "sampling", 110.0, &mut self.options.sampling, &labeled([Sampling::Layer, Sampling::Image], name));
    }

    fn pick_area_controls(&mut self, ui: &mut Ui) {
        let skin = self.skin.clone();
        ui.label(t(Text::OptPickArea));
        choice_combo(ui, &skin, "pick-size", PICK_SIZE_COMBO_WIDTH, &mut self.options.pick_size, &PICK_SIZES);
        if self.options.pick_size == PICK_SIZES[0].0 {
            return;
        }
        let spaces = [(MixSpace::Linear, t(Text::PickMixLinear)), (MixSpace::Srgb, t(Text::PickMixSrgb))];
        choice_combo(ui, &skin, "pick-space", PICK_MIX_COMBO_WIDTH, &mut self.options.pick_space, &spaces);
        let alphas = [(AlphaMix::Weighted, t(Text::PickAlphaWeighted)), (AlphaMix::Plain, t(Text::PickAlphaPlain)), (AlphaMix::Opaque, t(Text::PickAlphaOpaque))];
        choice_combo(ui, &skin, "pick-alpha", PICK_ALPHA_COMBO_WIDTH, &mut self.options.pick_alpha, &alphas);
    }

    fn width_control(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptWidth));
        let skin = self.skin.clone();
        let spec = DragSpec { speed: 0.1, max_decimals: Some(2), ..DragSpec::default() };
        drag_value(ui, &skin, &mut self.options.size, BRUSH_SIZE_MIN..=BRUSH_SIZE_MAX, AUTO_SIZE, &spec);
    }

    fn shape_kind_combo(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptShape));
        let label = |kind: ShapeKind| t(shape_label(kind));
        let current = self.options.shape_kind;
        let skin = self.skin.clone();
        let same_kind = |a: &ShapeKind, b: &ShapeKind| std::mem::discriminant(a) == std::mem::discriminant(b);
        let response = combo_box(ui, &skin, "shape-kind", 190.0, label(current), |ui| {
            for kind in ShapeKind::ALL {
                if ui.selectable_label(same_kind(&kind, &current), label(kind)).clicked() {
                    self.options.shape_kind = kind.with_params(&self.options.shape_params);
                }
            }
        });
        let position = ShapeKind::ALL.iter().position(|kind| same_kind(kind, &current));
        if let Some(target) = stepped_index(ShapeKind::ALL.len(), position, wheel_over(ui, &response).notches) {
            self.options.shape_kind = ShapeKind::ALL[target].with_params(&self.options.shape_params);
        }
    }

    fn shape_param_controls(&mut self, ui: &mut Ui) {
        let skin = self.skin.clone();
        let before = self.options.shape_params;
        let params = &mut self.options.shape_params;
        match self.options.shape_kind {
            ShapeKind::RoundedRectangle { .. } => shape_param(ui, &skin, Text::OptCornerRadius, &mut params.corner_radius, 1.0, 0.0..=CORNER_RADIUS_MAX, 0.5, ""),
            ShapeKind::Triangle { .. } => shape_param(ui, &skin, Text::OptApex, &mut params.triangle_apex, PERCENT, 0.0..=PERCENT, 0.5, "%"),
            ShapeKind::Polygon { .. } => shape_count(ui, &skin, Text::OptSides, &mut params.polygon_sides),
            ShapeKind::Star { .. } => {
                shape_count(ui, &skin, Text::OptPoints, &mut params.star_points);
                shape_param(ui, &skin, Text::OptInnerRadius, &mut params.star_inner, PERCENT, SHAPE_PART_MIN..=PERCENT, 0.5, "%");
                shape_param(ui, &skin, Text::OptOuterRadius, &mut params.star_outer, PERCENT, SHAPE_PART_MIN..=PERCENT, 0.5, "%");
            }
            ShapeKind::Arrow { .. } => {
                shape_param(ui, &skin, Text::OptArrowHead, &mut params.arrow_head, PERCENT, SHAPE_PART_MIN..=SHAPE_PART_MAX, 0.5, "%");
                shape_param(ui, &skin, Text::OptArrowShaft, &mut params.arrow_shaft, PERCENT, SHAPE_PART_MIN..=SHAPE_PART_MAX, 0.5, "%");
            }
            ShapeKind::Rectangle | ShapeKind::Ellipse | ShapeKind::Diamond | ShapeKind::Heart => {}
        }
        if self.options.shape_params != before {
            self.options.shape_kind = self.options.shape_kind.with_params(&self.options.shape_params);
        }
    }

    fn gradient_kind_combo(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptGradient));
        let skin = self.skin.clone();
        choice_combo(ui, &skin, "gradient-kind", 190.0, &mut self.options.gradient_kind, &labeled(Self::gradient_kinds(), |kind| t(gradient_label(kind))));
    }

    fn fill_style_combo(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptFill));
        let skin = self.skin.clone();
        choice_combo(ui, &skin, "fill-style", 150.0, &mut self.options.fill_style, &labeled([FillStyle::Outline, FillStyle::Filled, FillStyle::Both], |style| t(tool_fill_style_label(style))));
    }

    fn move_method_combo(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptMoveMethod));
        let label = |method: MoveMethod| t(match method {
            MoveMethod::Transform => Text::MoveMethodTransform,
            MoveMethod::Perspective => Text::MoveMethodPerspective,
        });
        let skin = self.skin.clone();
        let before = self.options.move_method;
        choice_combo(ui, &skin, "move-method", 150.0, &mut self.options.move_method, &labeled([MoveMethod::Transform, MoveMethod::Perspective], label));
        if before != self.options.move_method && self.tab().doc.floating().is_some_and(|floating| floating.quad().is_some()) {
            self.flush_pending();
        }
    }

    fn float_mode_combo(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptMode));
        let skin = self.skin.clone();
        choice_combo(ui, &skin, "float-mode", 110.0, &mut self.options.float_mode, &labeled([AlphaMode::Replace, AlphaMode::Blend], |mode| t(Self::float_mode_label(mode))));
        let mode = self.options.float_mode;
        let tab = self.tab_mut();
        let area = tab.doc.set_floating_mode(mode);
        tab.mark_dirty(area);
    }

    fn move_resample_combo(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptResample));
        let skin = self.skin.clone();
        choice_combo(ui, &skin, "move-resample", 130.0, &mut self.options.move_resample, &labeled(RESAMPLE_METHODS, |method| t(resample_label(method))));
    }

    fn rotation_combo(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptRotation));
        let label = |method: RotationMethod| t(match method {
            RotationMethod::Plain => Text::RotationPlain,
            RotationMethod::FastRotation => Text::RotationFast,
            RotationMethod::RotSprite => Text::RotationRotSprite,
        });
        let skin = self.skin.clone();
        choice_combo(ui, &skin, "rotation-method", 130.0, &mut self.options.rotation, &labeled([RotationMethod::Plain, RotationMethod::FastRotation, RotationMethod::RotSprite], label));
    }

    fn lasso_smoothing_combo(&mut self, ui: &mut Ui) {
        ui.label(t(Text::OptLassoSmoothing));
        let label = |smoothing: LassoSmoothing| t(match smoothing {
            LassoSmoothing::Classic => Text::LassoClassic,
            LassoSmoothing::Full => Text::LassoFull,
            LassoSmoothing::Off => Text::LassoOff,
        });
        let skin = self.skin.clone();
        choice_combo(ui, &skin, "lasso-smoothing", 110.0, &mut self.options.lasso_smoothing, &labeled([LassoSmoothing::Classic, LassoSmoothing::Full, LassoSmoothing::Off], label));
    }

    fn line_mode_combo(&mut self, ui: &mut Ui) {
        let label = |mode: LineMode| t(match mode {
            LineMode::Straight => Text::LineStraight,
            LineMode::Smooth => Text::LineSmooth,
        });
        let skin = self.skin.clone();
        choice_combo(ui, &skin, "line-mode", 130.0, &mut self.options.line_mode, &labeled([LineMode::Straight, LineMode::Smooth], label));
    }

    fn blend_combo(&mut self, ui: &mut Ui) {
        let skin = self.skin.clone();
        choice_combo(ui, &skin, "blend", 120.0, &mut self.options.blend, &Self::blend_labels());
    }

    pub(crate) fn show_status_bar(&mut self, ui: &mut Ui) {
        let frame = egui::Frame::NONE.fill(skin::theme().chrome).inner_margin(STATUS_BAR_MARGIN.frame_margin());
        egui::Panel::bottom("status-bar").frame(frame).exact_size(STATUS_HEIGHT).show(ui, |ui| {
            self.paint_panel_grain(ui, STATUS_BAR_MARGIN);
            let bar = ui.max_rect();
            let edge = self.paint_bar_edge(ui, bar);
            ui.add_space(edge);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing = INLINE_SPACING_WIDE;
                let hint = self.status.clone().unwrap_or_else(|| self.tool.hint().to_owned());
                ui.add(egui::Label::new(egui::RichText::new(hint).small()).truncate());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.zoom_controls(ui));
            });
        });
    }

    fn zoom_controls(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing = INLINE_SPACING;
        let canvas = self.workspace;
        let zoom = self.tab().view.zoom;
        let span = (ZOOM_MAX / ZOOM_MIN).ln();
        let position = (zoom / ZOOM_MIN).ln() / span;
        if let Some(position) = plain_slider(ui, &self.skin, position, ZOOM_SLIDER_WIDTH) {
            let target = ZOOM_MIN * (position * span).exp();
            self.tab_mut().view.zoom_at(canvas, canvas.center(), target);
        }
        ui.label(egui::RichText::new(format!("{:>4.0}%", self.tab().view.zoom * 100.0)).monospace());
        separator(ui, STATUS_SEPARATOR_HEIGHT);
        let doc = &self.tab().doc;
        let size = match doc.selection() {
            Some(selection) => format!("{} × {}", selection.bounds().width(), selection.bounds().height()),
            None => format!("{} × {}", doc.width(), doc.height()),
        };
        ui.label(egui::RichText::new(format!("{size:>11}")).monospace());
        separator(ui, STATUS_SEPARATOR_HEIGHT);
        let cursor = self.hover_pixel.map(|p| format!("{}, {}", p.x, p.y)).unwrap_or_else(|| "-".to_owned());
        ui.label(egui::RichText::new(format!("{cursor:>11}")).monospace());
    }

    pub(crate) fn show_workspace(&mut self, ui: &mut Ui, ctx: &Context) {
        let response = egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let canvas = ui.max_rect();
            if self.layout_pending {
                self.panels = default_panels(canvas);
                self.layout_pending = false;
            }
            self.show_canvas(ui, ctx, canvas);
            ui.scope_builder(UiBuilder::new().max_rect(canvas), |_| {});
            canvas
        });
        self.show_panels(ctx, response.inner);
    }
}

pub(crate) const RESAMPLE_METHODS: [Resample; 5] = [Resample::Nearest, Resample::Bilinear, Resample::Bicubic, Resample::Lanczos, Resample::Anisotropic];

pub(crate) fn resample_label(method: Resample) -> Text {
    match method {
        Resample::Nearest => Text::ResampleNearest,
        Resample::Bilinear => Text::ResampleBilinear,
        Resample::Bicubic => Text::ResampleBicubic,
        Resample::Lanczos => Text::ResampleLanczos,
        Resample::Anisotropic => Text::ResampleAnisotropic,
    }
}

fn gradient_label(kind: GradientKind) -> Text {
    match kind {
        GradientKind::Linear => Text::GradientLinear,
        GradientKind::Reflected => Text::GradientReflected,
        GradientKind::Diamond => Text::GradientDiamond,
        GradientKind::Radial => Text::GradientRadial,
        GradientKind::Conical => Text::GradientConical,
    }
}

fn shape_label(kind: ShapeKind) -> Text {
    match kind {
        ShapeKind::Rectangle => Text::ShapeRectangle,
        ShapeKind::RoundedRectangle { .. } => Text::ShapeRounded,
        ShapeKind::Ellipse => Text::ShapeEllipse,
        ShapeKind::Triangle { .. } => Text::ShapeTriangle,
        ShapeKind::Diamond => Text::ShapeDiamond,
        ShapeKind::Polygon { .. } => Text::ShapePolygon,
                        ShapeKind::Star { .. } => Text::ShapeStar,
        ShapeKind::Arrow { .. } => Text::ShapeArrow,
        ShapeKind::Heart => Text::ShapeHeart,
    }
}

fn paint_tab_thumbnail(ui: &mut Ui, skin: &skin::Skin, tab: Rect, thumb: egui::TextureId, doc_size: egui::Vec2) {
    let scale = TAB_THUMB / doc_size.max_elem().max(1.0);
    let image_rect = Rect::from_center_size(tab.center(), (doc_size * scale.clamp(0.0, 1.0)).max(vec2(1.0, 1.0)));
    skin.paint_checker(&ui.painter().with_clip_rect(image_rect), image_rect, image_rect.min);
    ui.painter().image(thumb, image_rect, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
    ui.painter().rect_stroke(image_rect, 0.0, egui::Stroke::new(1.0, skin::theme().line), egui::StrokeKind::Outside);
}

fn closing_gap(ui: &mut Ui, skin: &skin::Skin, motion: Option<(Slot, f32)>, closing: Option<(egui::TextureId, egui::Vec2)>, index: usize) {
    let gap = motion.filter(|(slot, _)| slot.slide == Slide::Out && slot.index == index).map(|(_, extent)| extent - TAB_GAP);
    let Some(width) = gap.filter(|width| *width >= 0.0) else {
        return;
    };
    let (slot_rect, _) = ui.allocate_exact_size(vec2(width, TAB_SIZE), Sense::hover());
    let Some((thumb, doc_size)) = closing else {
        return;
    };
    let rect = Rect::from_min_size(slot_rect.right_top() - vec2(TAB_SIZE, 0.0), vec2(TAB_SIZE, TAB_SIZE));
    let outer_clip = ui.clip_rect();
    ui.set_clip_rect(outer_clip.intersect(slot_rect));
    skin.paint_plate(ui.painter(), rect, &Plate::button(skin::theme().button, Bevel::Raised));
    paint_tab_thumbnail(ui, skin, rect, thumb, doc_size);
    ui.set_clip_rect(outer_clip);
}

const PERCENT: f32 = 100.0;
const CORNER_RADIUS_MAX: f32 = 500.0;
const SHAPE_PART_MIN: f32 = 5.0;
const SHAPE_PART_MAX: f32 = 90.0;
const SHAPE_SIDES_MIN: u32 = 3;
const SHAPE_SIDES_MAX: u32 = 32;

fn shape_param(ui: &mut Ui, skin: &skin::Skin, label: Text, value: &mut f32, scale: f32, range: std::ops::RangeInclusive<f32>, speed: f64, suffix: &'static str) {
    ui.label(t(label));
    let mut shown = *value * scale;
    let spec = DragSpec { speed, suffix, max_decimals: Some(1), ..DragSpec::default() };
    if drag_value(ui, skin, &mut shown, range, AUTO_SIZE, &spec).changed() {
        *value = shown / scale;
    }
}

fn shape_count(ui: &mut Ui, skin: &skin::Skin, label: Text, value: &mut u32) {
    ui.label(t(label));
    let mut shown = *value as f32;
    let spec = DragSpec { speed: 0.1, max_decimals: Some(0), ..DragSpec::default() };
    if drag_value(ui, skin, &mut shown, SHAPE_SIDES_MIN as f32..=SHAPE_SIDES_MAX as f32, AUTO_SIZE, &spec).changed() {
        *value = shown.round() as u32;
    }
}

fn tab_strip_arrows<R>(ui: &mut Ui, strip_id: egui::Id, scroll: &ScrollAreaOutput<R>) -> Option<f32> {
    let area = scroll.inner_rect;
    let offset = scroll.state.clamped_offset().x;
    let hidden_after = (scroll.content_size.x - area.width()).max(0.0) - offset;
    let step = (TAB_SIZE + TAB_GAP) * TAB_ARROW_STEP_TABS;
    let mut requested = None;
    for (side, visible, delta) in [(-1.0, offset > TAB_SCROLL_EDGE, step), (1.0, hidden_after > TAB_SCROLL_EDGE, -step)] {
        if !visible {
            continue;
        }
        let zone = if side < 0.0 {
            Rect::from_min_size(area.min, vec2(TAB_ARROW_ZONE, area.height()))
        } else {
            Rect::from_min_size(pos2(area.max.x - TAB_ARROW_ZONE, area.min.y), vec2(TAB_ARROW_ZONE, area.height()))
        };
        let response = ui.interact(zone, strip_id.with(("arrow", side as i32)), Sense::click());
        let theme = skin::theme();
        paint_tab_arrow(ui.painter(), zone, side, if response.hovered() { theme.ink } else { theme.ink_dim });
        if response.clicked() {
            requested = Some(delta);
        }
    }
    requested
}

fn paint_tab_arrow(painter: &Painter, zone: Rect, side: f32, color: Color32) {
    let center = zone.center();
    if !skin::pixel_mode() {
        let depth = TAB_ARROW_COLUMNS as f32 * 1.5;
        let tip = pos2(center.x + side * depth / 2.0, center.y);
        let base_x = center.x - side * depth / 2.0;
        painter.add(Shape::convex_polygon(vec![tip, pos2(base_x, center.y - TAB_ARROW_HALF_HEIGHT), pos2(base_x, center.y + TAB_ARROW_HALF_HEIGHT)], color, egui::Stroke::NONE));
        return;
    }
    let grid = skin::grid_ppp(painter.pixels_per_point());
    let cell = 1.0 / grid;
    let snap = |value: f32| (value * grid).round() / grid;
    let tip_x = snap(center.x + side * TAB_ARROW_COLUMNS as f32 * cell / 2.0);
    for column in 0..TAB_ARROW_COLUMNS {
        let x = tip_x - side * (column as f32 + if side > 0.0 { 1.0 } else { 0.0 }) * cell;
        let height = (2 * column + 1) as f32 * cell;
        let top = snap(center.y - height / 2.0);
        painter.rect_filled(Rect::from_min_size(pos2(x, top), vec2(cell, height)), 0.0, color);
    }
}
