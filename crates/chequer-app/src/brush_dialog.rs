use std::sync::Arc;

use chequer_core::brush::{Brush, BrushMeta, BrushSettings, BrushTip, LayerSpec, MAX_TIP_VARIANTS, MAX_TRACK_LAYERS, MaskTip, Texture, TipVariant, TrackLayer};
use eframe::egui::{self, Align, Context, Layout, Sense, TextureHandle, Ui, vec2};

use crate::metrics::PRESSED_OFFSET;
use crate::app::{App, FileEvent};
use crate::brush_settings::BrushTab;
use crate::brush_editor::{EditSession, EditorEvent, PANE_WIDTH, brush_editor};
use crate::brush_library::BrushLibrary;
use crate::brush_list::{LIST_WIDTH, ListAction, ListState, ScrollRequest, brush_label, brush_list};
use crate::brush_presets::SettingsHistory;
use crate::brush_preview::{paint_texture, preview_texture};
use crate::dialogs::Dialog;
use crate::modal::{BUTTON_ROW_LEAD, DIALOG_MARGIN, DialogKeys, NameInput, confirm_modal, dialog_keys, input_modal, ok_cancel_with, take_dialog_escape, title_bar};
use crate::i18n::{Text, t};
use crate::skin;
use crate::layer_editor::LayerPick;
use crate::texture_editor::TextureSlot;
use crate::widgets::{fixed_box, raised_button_plate, text_button};

const COLUMN_GAP: f32 = 8.0;
pub(crate) const BRUSH_DIALOG_WIDTH: f32 = LIST_WIDTH + COLUMN_GAP + PANE_WIDTH;
const DIALOG_NON_BODY_HEIGHT: f32 = 164.0;
const MIN_BODY_HEIGHT: f32 = 320.0;
const MAX_BODY_HEIGHT: f32 = 520.0;
const STACKED_LIST_HEIGHT: f32 = 220.0;
const CHIP_BRUSH_SIZE: f32 = 12.0;

#[derive(Clone, Copy)]
pub(crate) enum NamePurpose {
    NewBrush,
    Duplicate,
}

pub(crate) enum Prompt {
    Name { purpose: NamePurpose, input: NameInput },
    ResetBuiltin,
    DiscardEdit { commit: bool, close: bool },
}

pub struct BrushDialog {
    pub(crate) list: ListState,
    pub(crate) working: Brush,
    pub(crate) original: Brush,
    pub(crate) tab: BrushTab,
    pub(crate) big_preview: Option<TextureHandle>,
    pub(crate) edit: Option<EditSession>,
    pub(crate) pending_delete: Option<usize>,
    pub(crate) prompt: Option<Prompt>,
    pub(crate) message: Option<String>,
    pub(crate) history: SettingsHistory,
}

impl BrushDialog {
    pub fn new(library: &BrushLibrary, current: &Brush) -> Self {
        Self {
            list: ListState::new(library.find(current).unwrap_or(0)),
            working: current.clone(),
            original: current.clone(),
            tab: BrushTab::Shape,
            big_preview: None,
            edit: None,
            pending_delete: None,
            prompt: None,
            message: library.problems().first().cloned(),
            history: SettingsHistory::default(),
        }
    }

    pub fn working(&self) -> &Brush {
        &self.working
    }

    pub fn library_changed(&mut self, library: &BrushLibrary, message: Option<String>) {
        self.list.previews.clear();
        self.big_preview = None;
        self.list.selected = library
            .find(&self.working)
            .unwrap_or(self.list.selected.min(library.entries().len().saturating_sub(1)));
        self.list.scroll = ScrollRequest::Center;
        self.message = message.or_else(|| library.problems().first().cloned());
    }

    pub(crate) fn select(&mut self, library: &BrushLibrary, index: usize) {
        if let Some(entry) = library.entries().get(index) {
            self.list.selected = index;
            self.working = (*entry.brush).clone();
            self.original = self.working.clone();
            self.big_preview = None;
            self.message = None;
            self.history.clear();
        }
    }

    fn begin_new(&mut self, name: String) {
        self.working = Brush { meta: BrushMeta::default(), ..Brush::default() };
        self.original = self.working.clone();
        self.big_preview = None;
        self.message = None;
        self.pending_delete = None;
        self.history.clear();
        self.edit = Some(EditSession { focus_name: false, ..EditSession::new(None, name, Vec::new()) });
    }

    fn edit_is_dirty(&self) -> bool {
        self.edit.as_ref().is_some_and(|edit| {
            edit.name != edit.initial_name || edit.tips_changed || self.working.settings != self.original.settings || self.working.textures != self.original.textures
        })
    }

    fn request_cancel_edit(&mut self, library: &BrushLibrary) {
        if self.edit_is_dirty() {
            self.prompt = Some(Prompt::DiscardEdit { commit: false, close: false });
        } else {
            self.cancel_edit(library);
        }
    }

    fn begin_edit(&mut self, library: &BrushLibrary, index: usize) {
        self.pending_delete = None;
        self.select(library, index);
        let Some(entry) = library.entries().get(index) else {
            return;
        };
        self.edit = Some(EditSession::new(entry.source.is_some().then_some(index), brush_label(&entry.brush), entry.brush.tip.variants()));
    }

    pub(crate) fn cancel_edit(&mut self, library: &BrushLibrary) {
        if self.edit.take().is_some() {
            self.select(library, self.list.selected);
        }
    }

    pub(crate) fn sync_tips(&mut self) {
        if let Some(edit) = &mut self.edit {
            edit.tips_changed = true;
            self.working.tip = BrushTip::from_variants(edit.tips.clone());
            self.big_preview = None;
        }
    }

    pub(crate) fn replace_settings(&mut self, settings: BrushSettings) {
        self.history.record_step(self.working.settings.clone());
        self.working.settings = settings;
        self.big_preview = None;
    }

    fn step_history(&mut self, ctx: &Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (redo_shortcut, undo_shortcut) = ctx.input_mut(|input| {
            let redo = input.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z)
                || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Y);
            (redo, input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z))
        });
        let restored = if redo_shortcut {
            self.history.redo(&self.working.settings)
        } else if undo_shortcut {
            self.history.undo(&self.working.settings)
        } else {
            None
        };
        if let Some(settings) = restored {
            self.working.settings = settings;
            self.big_preview = None;
        }
    }
}

fn body_height(ctx: &Context) -> f32 {
    (ctx.content_rect().height() - DIALOG_NON_BODY_HEIGHT).clamp(MIN_BODY_HEIGHT, MAX_BODY_HEIGHT)
}

impl App {
    pub(crate) fn open_brush_dialog(&mut self) {
        self.dialog = Some(Dialog::Brushes(BrushDialog::new(
            &self.brush_library,
            self.options.brush_for(self.tool),
        )));
    }

    pub(crate) fn brush_chip(&mut self, ui: &mut Ui) {
        let size = vec2(ui.available_width().min(120.0), 22.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let key = (Arc::as_ptr(self.options.brush_for(self.tool)) as usize, skin::theme().ink);
        if self
            .brush_chip_cache
            .as_ref()
            .is_none_or(|(cached, _)| *cached != key)
        {
            let texture = preview_texture(
                ui.ctx(),
                "brush-chip",
                self.options.brush_for(self.tool),
                CHIP_BRUSH_SIZE,
                size,
            );
            self.brush_chip_cache = Some((key, texture));
        }
        let plate = raised_button_plate(&response, false, true);
        self.skin.paint_plate(ui.painter(), rect, &plate);
        let pressed_offset = if plate.bevel == Some(skin::Bevel::Sunken) { PRESSED_OFFSET } else { egui::Vec2::ZERO };
        if let Some((_, texture)) = &self.brush_chip_cache {
            paint_texture(ui, texture, rect.translate(pressed_offset));
        }
        if response
            .on_hover_text(brush_label(self.options.brush_for(self.tool)))
            .clicked()
        {
            self.open_brush_dialog();
        }
    }

    pub(crate) fn pick_brush_import(&self, ctx: &Context) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(path) = crate::filepicker::new_dialog()
                .add_filter(
                    "Brushes",
                    &["chqbrush", "gbr", "abr", "kpp", "png", "jpg", "jpeg", "bmp", "gif", "webp"],
                )
                .pick_file()
            {
                let _ = tx.send(FileEvent::BrushImportPath(path));
                ctx.request_repaint();
            }
        });
    }

    fn pick_brush_tips(&self, ctx: &Context) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(paths) = crate::filepicker::new_dialog()
                .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "webp"])
                .pick_files()
            {
                let _ = tx.send(FileEvent::BrushTipPaths(paths));
                ctx.request_repaint();
            }
        });
    }

    fn pick_brush_layer(&self, ctx: &Context, pick: LayerPick) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(path) = crate::filepicker::new_dialog().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "webp"]).pick_file() {
                let _ = tx.send(FileEvent::BrushLayerPath(pick, path));
                ctx.request_repaint();
            }
        });
    }

    pub(crate) fn load_brush_layer(&mut self, pick: LayerPick, path: &std::path::Path) {
        let Some(Dialog::Brushes(d)) = &mut self.dialog else {
            return;
        };
        let loaded = image::open(path).map_err(|error| error.to_string()).and_then(|image| Texture::from_ink_image(&image).map_err(|error| error.to_string()));
        match loaded {
            Ok(texture) => {
                let layers = &mut d.working.textures.layers;
                match pick {
                    LayerPick::New if layers.len() < MAX_TRACK_LAYERS => layers.push(TrackLayer { spec: LayerSpec::default(), texture }),
                    LayerPick::New => {}
                    LayerPick::Replace(index) => {
                        if let Some(layer) = layers.get_mut(index) {
                            layer.texture = texture;
                            layer.spec.marks.clear();
                        }
                    }
                }
                d.big_preview = None;
                d.message = None;
            }
            Err(error) => d.message = Some(format!("{}: {error}", path.display())),
        }
    }

    fn pick_brush_texture(&self, ctx: &Context, slot: TextureSlot) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(path) = crate::filepicker::new_dialog().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "webp"]).pick_file() {
                let _ = tx.send(FileEvent::BrushTexturePath(slot, path));
                ctx.request_repaint();
            }
        });
    }

    pub(crate) fn load_brush_texture(&mut self, slot: TextureSlot, path: &std::path::Path) {
        let Some(Dialog::Brushes(d)) = &mut self.dialog else {
            return;
        };
        let loaded = image::open(path).map_err(|error| error.to_string()).and_then(|image| {
            match slot {
                TextureSlot::Paper => Texture::from_image(&image),
                TextureSlot::Hairs => Texture::from_image_row(&image),
            }
            .map_err(|error| error.to_string())
        });
        match loaded {
            Ok(texture) => {
                match slot {
                    TextureSlot::Paper => d.working.textures.paper = Some(texture),
                    TextureSlot::Hairs => d.working.textures.hairs = Some(texture),
                }
                d.big_preview = None;
                d.message = None;
            }
            Err(error) => d.message = Some(format!("{}: {error}", path.display())),
        }
    }

    pub(crate) fn add_brush_tips(&mut self, paths: &[std::path::PathBuf]) {
        let Some(Dialog::Brushes(d)) = &mut self.dialog else {
            return;
        };
        let Some(edit) = &mut d.edit else {
            return;
        };
        let mut failure = None;
        for path in paths.iter().take(MAX_TIP_VARIANTS.saturating_sub(edit.tips.len())) {
            let loaded = image::open(path)
                .map_err(|error| error.to_string())
                .and_then(|image| MaskTip::from_image(&image).map_err(|error| error.to_string()));
            match loaded {
                Ok(mask) => edit.tips.push(TipVariant::new(mask)),
                Err(error) => failure = Some(format!("{}: {error}", path.display())),
            }
        }
        if edit.name.is_empty() {
            edit.name = paths
                .first()
                .and_then(|path| path.file_stem())
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        d.sync_tips();
        d.message = failure;
    }

    pub(crate) fn pick_brush_export(&self, ctx: &Context, file_name: String) {
        let (tx, ctx) = (self.file_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(path) = crate::filepicker::new_dialog()
                .set_file_name(file_name)
                .add_filter("Chequer brush", &["chqbrush"])
                .save_file()
            {
                let _ = tx.send(FileEvent::BrushExportPath(path));
                ctx.request_repaint();
            }
        });
    }

    pub(crate) fn import_brush(&mut self, path: &std::path::Path) {
        let message = self.brush_library.import(path).err();
        if let Some(Dialog::Brushes(d)) = &mut self.dialog {
            d.library_changed(&self.brush_library, message);
        }
    }

    pub(crate) fn export_brush(&mut self, path: &std::path::Path) {
        let Some(Dialog::Brushes(d)) = &mut self.dialog else {
            return;
        };
        let written = crate::brush_library::write_pack(path, std::slice::from_ref(d.working()))
            .and_then(|()| crate::filepicker::sync_back(path));
        d.message = written.err();
    }

    pub(crate) fn brush_dialog(&mut self, ui: &mut Ui, skin: &skin::Skin, d: &mut BrushDialog) -> bool {
        let modal_open = d.pending_delete.is_some() || d.prompt.is_some();
        let accept = dialog_keys(ui).accept && modal_open;
        let escape = (modal_open || d.edit.is_some()) && take_dialog_escape(ui);
        let modal_keys = DialogKeys { accept, cancel: escape && modal_open };
        if escape && !modal_open {
            d.request_cancel_edit(&self.brush_library);
        }
        title_bar(ui, t(Text::BrushDialogTitle));
        d.step_history(ui.ctx());
        self.navigate_brush_list(ui.ctx(), d);
        ui.spacing_mut().item_spacing.x = COLUMN_GAP;
        let height = body_height(ui.ctx());
        let (list_action, editor_event) = if ui.available_width() < BRUSH_DIALOG_WIDTH {
            egui::ScrollArea::vertical()
                .max_height(height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let width = ui.available_width();
                    self.brush_panes(ui, skin, d, vec2(width, STACKED_LIST_HEIGHT), vec2(width, height))
                })
                .inner
        } else {
            ui.horizontal_top(|ui| self.brush_panes(ui, skin, d, vec2(LIST_WIDTH, height), vec2(PANE_WIDTH, height))).inner
        };
        let activated = self.apply_list_action(d, list_action);
        match editor_event {
            Some(EditorEvent::BeginEdit) => d.begin_edit(&self.brush_library, d.list.selected),
            Some(EditorEvent::SaveEdit) => self.save_brush_edit(d),
            Some(EditorEvent::CancelEdit) => d.request_cancel_edit(&self.brush_library),
            Some(EditorEvent::ResetBuiltin) => d.prompt = Some(Prompt::ResetBuiltin),
            Some(EditorEvent::ToggleFavorite) => self.brush_library.toggle_favorite(&d.working),
            Some(EditorEvent::AddImages) => self.pick_brush_tips(ui.ctx()),
            Some(EditorEvent::PickTexture(slot)) => self.pick_brush_texture(ui.ctx(), slot),
            Some(EditorEvent::PickLayer(pick)) => self.pick_brush_layer(ui.ctx(), pick),
            None => {}
        }
        self.autosave_quick_edit(ui.ctx(), d);
        ui.spacing_mut().item_spacing.y = f32::from(DIALOG_MARGIN) - BUTTON_ROW_LEAD;
        let keep_open = self.brush_footer(ui, skin, d, activated);
        self.confirm_brush_delete(ui.ctx(), skin, d, modal_keys.cancel && d.pending_delete.is_some());
        let close_confirmed = self.show_brush_prompt(ui.ctx(), skin, d, modal_keys);
        keep_open && !close_confirmed
    }

    fn brush_panes(&mut self, ui: &mut Ui, skin: &skin::Skin, d: &mut BrushDialog, list_size: egui::Vec2, pane_size: egui::Vec2) -> (Option<ListAction>, Option<EditorEvent>) {
        let favorites = self.brush_library.favorite_indices();
        let list_action = fixed_box(ui, list_size, Layout::top_down(Align::Min), |ui| brush_list(ui, skin, self.brush_library.entries(), &favorites, &mut d.list));
        let favorite = self.brush_library.favorite_state(&d.working);
        let resettable = self.brush_library.entries().get(d.list.selected).is_some_and(|entry| entry.modified);
        let editor_event = fixed_box(ui, pane_size, Layout::top_down(Align::Min), |ui| brush_editor(ui, skin, d, favorite, resettable));
        (list_action, editor_event)
    }

    fn autosave_quick_edit(&mut self, ctx: &Context, d: &mut BrushDialog) {
        if d.edit.is_some() || ctx.input(|input| input.pointer.any_down()) {
            return;
        }
        let index = d.list.selected;
        let Some(stored) = self.brush_library.entries().get(index).map(|entry| &entry.brush) else {
            return;
        };
        if stored.settings == d.working.settings && stored.textures == d.working.textures {
            return;
        }
        let mut brush = (**stored).clone();
        brush.settings = d.working.settings.clone();
        brush.textures = d.working.textures.clone();
        match self.brush_library.replace(index, brush) {
            Ok(()) => {
                d.list.previews.remove(&index);
            }
            Err(error) => {
                d.select(&self.brush_library, index);
                d.message = Some(error);
            }
        }
    }

    fn show_brush_prompt(&mut self, ctx: &Context, skin: &skin::Skin, d: &mut BrushDialog, keys: DialogKeys) -> bool {
        let Some(prompt) = d.prompt.take() else {
            return false;
        };
        match prompt {
            Prompt::Name { purpose, mut input } => {
                let name = input.text.trim().to_owned();
                let problem = self.brush_library.name_taken(&name, None).then(|| t(Text::BrushNameTaken));
                let title = match purpose {
                    NamePurpose::NewBrush => t(Text::BrushNew),
                    NamePurpose::Duplicate => t(Text::BrushDuplicate),
                };
                match input_modal(ctx, skin, title, &mut input, problem, keys) {
                    Some(true) => self.apply_brush_name(d, purpose, name),
                    Some(false) => {}
                    None => d.prompt = Some(Prompt::Name { purpose, input }),
                }
                false
            }
            Prompt::ResetBuiltin => {
                match confirm_modal(ctx, skin, t(Text::BrushReset), t(Text::BrushResetQuestion), keys.cancel) {
                    Some(true) => self.reset_selected_builtin(d),
                    Some(false) => {}
                    None => d.prompt = Some(Prompt::ResetBuiltin),
                }
                false
            }
            Prompt::DiscardEdit { commit, close } => match confirm_modal(ctx, skin, t(Text::BrushDiscardTitle), t(Text::BrushDiscardQuestion), keys.cancel) {
                Some(true) => {
                    d.cancel_edit(&self.brush_library);
                    if close && commit {
                        if let Some(error) = self.commit_active_brush(d.working.clone()) {
                            eprintln!("{error}");
                        }
                    }
                    close
                }
                Some(false) => false,
                None => {
                    d.prompt = Some(Prompt::DiscardEdit { commit, close });
                    false
                }
            },
        }
    }

    fn apply_brush_name(&mut self, d: &mut BrushDialog, purpose: NamePurpose, name: String) {
        match purpose {
            NamePurpose::NewBrush => d.begin_new(name),
            NamePurpose::Duplicate => {
                let mut brush = d.working.clone();
                brush.meta.name = name;
                brush.meta.id.clear();
                brush.meta.category.clear();
                match self.brush_library.save(brush.clone()) {
                    Ok(()) => self.adopt_saved_brush(d, brush),
                    Err(error) => d.message = Some(error),
                }
            }
        }
    }

    fn reset_selected_builtin(&mut self, d: &mut BrushDialog) {
        let index = d.list.selected;
        match self.brush_library.reset_builtin(index) {
            Ok(()) => {
                d.list.previews.remove(&index);
                d.select(&self.brush_library, index);
            }
            Err(error) => d.message = Some(error),
        }
    }

    fn adopt_saved_brush(&mut self, d: &mut BrushDialog, brush: Brush) {
        let commit_error = self.commit_active_brush(brush.clone());
        d.edit = None;
        d.original = brush.clone();
        d.working = brush;
        d.history.clear();
        d.library_changed(&self.brush_library, commit_error);
    }

    fn confirm_brush_delete(&mut self, ctx: &Context, skin: &skin::Skin, d: &mut BrushDialog, escape: bool) {
        let Some(entry) = d.pending_delete.and_then(|index| self.brush_library.entries().get(index)) else {
            return;
        };
        let question = format!("{} «{}»?", t(Text::BrushDeleteQuestion), brush_label(&entry.brush));
        match confirm_modal(ctx, skin, t(Text::BrushDelete), &question, escape) {
            Some(true) => self.delete_pending_brush(d),
            Some(false) => d.pending_delete = None,
            None => {}
        }
    }

    fn navigate_brush_list(&mut self, ctx: &Context, d: &mut BrushDialog) {
        if d.edit.is_some() || d.pending_delete.is_some() {
            return;
        }
        let (down, up) = ctx.input_mut(|input| (input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown), input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)));
        let entries = self.brush_library.entries();
        let favorites = self.brush_library.favorite_indices();
        let target = if down {
            d.list.step(entries, &favorites, 1)
        } else if up {
            d.list.step(entries, &favorites, -1)
        } else {
            d.list.first_visible_if_selection_hidden(entries, &favorites)
        };
        if let Some(index) = target.filter(|&index| index != d.list.selected) {
            d.select(&self.brush_library, index);
        }
    }

    fn apply_list_action(&mut self, d: &mut BrushDialog, action: Option<ListAction>) -> bool {
        match action {
            Some(ListAction::ToggleFavorite(index)) => {
                if let Some(brush) = self.brush_library.entries().get(index).map(|entry| entry.brush.clone()) {
                    self.brush_library.toggle_favorite(&brush);
                }
                return false;
            }
            Some(ListAction::MoveFavorite { from, to }) => {
                self.brush_library.move_favorite(from, to);
                return false;
            }
            _ => {}
        }
        let library = &self.brush_library;
        match action {
            None => false,
            Some(ListAction::New) => {
                d.prompt = Some(Prompt::Name { purpose: NamePurpose::NewBrush, input: NameInput::new(String::new()) });
                false
            }
            Some(ListAction::Select(index)) | Some(ListAction::Activate(index)) if d.edit.is_some() || d.pending_delete.is_some() => {
                d.edit = None;
                d.pending_delete = None;
                d.select(library, index);
                false
            }
            Some(ListAction::Select(index)) => {
                d.select(library, index);
                false
            }
            Some(ListAction::Activate(index)) => {
                d.select(library, index);
                true
            }
            Some(ListAction::Edit(index)) => {
                d.begin_edit(library, index);
                false
            }
            Some(ListAction::Delete(index)) => {
                d.cancel_edit(library);
                d.pending_delete = Some(index);
                false
            }
            Some(ListAction::ToggleFavorite(_) | ListAction::MoveFavorite { .. }) => false,
        }
    }

    fn brush_footer(&mut self, ui: &mut Ui, skin: &skin::Skin, d: &mut BrushDialog, activated: bool) -> bool {
        let (primary, secondary) = ok_cancel_with(ui, skin, true, |ui| self.footer_actions(ui, skin, d));
        let closing = primary || activated || secondary;
        if closing && d.edit_is_dirty() {
            d.prompt = Some(Prompt::DiscardEdit { commit: primary || activated, close: true });
            return true;
        }
        if primary || activated {
            if let Some(error) = self.commit_active_brush(d.working.clone()) {
                eprintln!("{error}");
            }
        }
        !closing
    }

    fn footer_actions(&mut self, ui: &mut Ui, skin: &skin::Skin, d: &mut BrushDialog) {
        let ctx = ui.ctx().clone();
        if text_button(ui, skin, t(Text::BrushImport)).clicked() {
            d.cancel_edit(&self.brush_library);
            self.pick_brush_import(&ctx);
        }
        if text_button(ui, skin, t(Text::BrushExport)).clicked() {
            self.pick_brush_export(&ctx, format!("{}.chqbrush", brush_label(&d.working)));
        }
        let duplicate = ui.add_enabled_ui(d.edit.is_none(), |ui| text_button(ui, skin, t(Text::BrushDuplicate)));
        if duplicate.inner.clicked() {
            let name = format!("{} {}", brush_label(&d.working), t(Text::BrushCopySuffix));
            d.prompt = Some(Prompt::Name { purpose: NamePurpose::Duplicate, input: NameInput::new(name) });
        }
        let removable = self.brush_library.entries().get(d.list.selected).is_some_and(|entry| entry.source.is_some());
        let delete = ui.add_enabled_ui(removable, |ui| text_button(ui, skin, t(Text::BrushDelete)));
        if delete.inner.clicked() {
            d.cancel_edit(&self.brush_library);
            d.pending_delete = Some(d.list.selected);
        }
        if let Some(message) = &d.message {
            ui.add(egui::Label::new(egui::RichText::new(message).color(skin::theme().ink_dim)).truncate()).on_hover_text(message);
        }
    }

    fn save_brush_edit(&mut self, d: &mut BrushDialog) {
        let Some(edit) = &d.edit else {
            return;
        };
        let (name, target) = (edit.name.trim().to_owned(), edit.target);
        if name.is_empty() {
            return;
        }
        if self.brush_library.name_taken(&name, target) {
            d.message = Some(t(Text::BrushNameTaken).to_owned());
            return;
        }
        let mut brush = d.working.clone();
        brush.meta.name = name;
        let saved = match target {
            Some(index) => self.brush_library.replace(index, brush.clone()),
            None => {
                brush.meta.id.clear();
                brush.meta.category.clear();
                self.brush_library.save(brush.clone())
            }
        };
        match saved {
            Ok(()) => self.adopt_saved_brush(d, brush),
            Err(error) => d.message = Some(error),
        }
    }

    fn commit_active_brush(&mut self, brush: Brush) -> Option<String> {
        *self.options.brush_for_mut(self.tool) = Arc::new(brush);
        crate::brush_library::save_active(&self.options.brush, &self.options.eraser_brush).err()
    }

    fn delete_pending_brush(&mut self, d: &mut BrushDialog) {
        let Some(index) = d.pending_delete.take() else {
            return;
        };
        let message = self.brush_library.remove(index).err();
        let neighbour = index.saturating_sub(1).min(self.brush_library.entries().len().saturating_sub(1));
        d.select(&self.brush_library, neighbour);
        d.library_changed(&self.brush_library, message);
    }
}
