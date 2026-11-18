use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use chequer_core::document::{Document, Layer, LayerProps};
use chequer_core::frames::{CelClip, CelMark, MAX_FRAME_DURATION_MS, MIN_FRAME_DURATION_MS};
use chequer_core::raster::Raster;
use eframe::egui::{self, Align2, Color32, ColorImage, Context, Pos2, Rect, Sense, Stroke, TextureHandle, TextureId, TextureOptions, Ui, Vec2, pos2, vec2};

use crate::app::{App, Command};
use crate::canvas::Gesture;
use crate::dialogs::Dialog;
use crate::i18n::{Text, t};
use crate::icons::Icon;
use crate::skin::{self, Font, Skin};
use crate::view::{sample_down, to_color_image};
use crate::widgets::{icon_button, stepped_index, wheel_while};

const COMPACT_CELL: Vec2 = vec2(20.0, 20.0);
const FULL_CELL: Vec2 = vec2(46.0, 46.0);
const FULL_THUMB_INSET: f32 = 3.0;
const CEL_THUMB_SIDE: u32 = 40;
const HEADER_HEIGHT: f32 = 20.0;
const LAYER_COLUMN_WIDTH: f32 = 130.0;
const LAYER_COLUMN_PADDING: f32 = 6.0;
const EYE_SIDE: f32 = 16.0;
const DOT_RADIUS: f32 = 3.0;
const MOVE_MARKER_WIDTH: f32 = 2.0;
const BAND_ALPHA: u8 = 48;
const SELECTION_ALPHA: u8 = 110;
const SCROLL_BAR_THICKNESS: f32 = 3.0;
const SCROLL_BAR_ALPHA: u8 = 120;
const TOOLBAR_GAP: f32 = 2.0;
const GROUP_GAP: f32 = 8.0;
const PLUS_ARM: f32 = 4.0;
const NESTED_INDENT: f32 = 10.0;
const HIDDEN_EYE_ALPHA: u8 = 50;
const ONION_TINT_SHARE: f32 = 0.55;
pub const ONION_MAX_FRAMES: u8 = 5;
pub const PERCENT_MAX: u8 = 100;
const DEFAULT_ONION_OPACITY: u8 = 45;
const DEFAULT_ONION_BEFORE_COLOR: [u8; 3] = [255, 60, 60];
const DEFAULT_ONION_AFTER_COLOR: [u8; 3] = [60, 110, 255];
pub const MIN_FPS: u32 = 1;
pub const MAX_FPS: u32 = 100;
pub const DEFAULT_FPS: u32 = 12;
const SPACE_TAP_SECONDS: f64 = 0.35;
const DURATION_EDIT_KEY: &str = "frame-duration-edit";
const MS_PER_SECOND: f64 = 1000.0;
const FIRST_FRAME: usize = 0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimelineCells {
    #[default]
    Compact,
    Full,
}

impl TimelineCells {
    pub fn to_setting(self) -> &'static str {
        match self {
            TimelineCells::Compact => "compact",
            TimelineCells::Full => "full",
        }
    }

    pub fn from_setting(value: &str) -> Self {
        if value == "full" { TimelineCells::Full } else { TimelineCells::Compact }
    }

    fn size(self) -> Vec2 {
        match self {
            TimelineCells::Compact => COMPACT_CELL,
            TimelineCells::Full => FULL_CELL,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnionSettings {
    pub before: u8,
    pub after: u8,
    pub opacity: u8,
    pub before_color: [u8; 3],
    pub after_color: [u8; 3],
}

impl Default for OnionSettings {
    fn default() -> Self {
        Self { before: 1, after: 1, opacity: DEFAULT_ONION_OPACITY, before_color: DEFAULT_ONION_BEFORE_COLOR, after_color: DEFAULT_ONION_AFTER_COLOR }
    }
}

impl OnionSettings {
    pub fn clamped(self) -> Self {
        Self { before: self.before.min(ONION_MAX_FRAMES), after: self.after.min(ONION_MAX_FRAMES), opacity: self.opacity.min(PERCENT_MAX), ..self }
    }

    fn count(&self, side: OnionSide) -> u8 {
        match side {
            OnionSide::Before => self.before,
            OnionSide::After => self.after,
        }
    }

    fn color(&self, side: OnionSide) -> [u8; 3] {
        match side {
            OnionSide::Before => self.before_color,
            OnionSide::After => self.after_color,
        }
    }

    fn alpha(&self, side: OnionSide, distance: usize) -> f32 {
        let count = f32::from(self.count(side).max(1));
        f32::from(self.opacity) / f32::from(PERCENT_MAX) * (count - distance as f32 + 1.0) / count
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationKeys {
    pub space_plays: bool,
    pub arrows_navigate: bool,
    pub wheel_navigates: bool,
}

impl Default for AnimationKeys {
    fn default() -> Self {
        Self { space_plays: true, arrows_navigate: true, wheel_navigates: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnimationSettings {
    pub onion: OnionSettings,
    pub keys: AnimationKeys,
}

pub fn fps_to_duration_ms(fps: u32) -> u32 {
    (MS_PER_SECOND / f64::from(fps.clamp(MIN_FPS, MAX_FPS))).round() as u32
}

pub fn duration_ms_to_fps(duration_ms: u32) -> u32 {
    ((MS_PER_SECOND / f64::from(duration_ms.max(1))).round() as u32).clamp(MIN_FPS, MAX_FPS)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CellSelection {
    frames: BTreeSet<usize>,
    layers: Option<BTreeSet<usize>>,
    anchor_frame: usize,
    anchor_layer: usize,
    active_layer: usize,
}

fn span(from: usize, to: usize) -> BTreeSet<usize> {
    (from.min(to)..=from.max(to)).collect()
}

fn nearest_member(members: &BTreeSet<usize>, near: usize) -> usize {
    members.range(near..).next().or_else(|| members.iter().next_back()).copied().unwrap_or(near)
}

impl CellSelection {
    fn single(frame: usize, layer: usize) -> Self {
        Self { frames: BTreeSet::from([frame]), layers: Some(BTreeSet::from([layer])), anchor_frame: frame, anchor_layer: layer, active_layer: layer }
    }

    fn contains_frame(&self, frame: usize) -> bool {
        self.frames.contains(&frame)
    }

    fn contains_cel(&self, frame: usize, layer: usize) -> bool {
        self.contains_frame(frame) && self.layers.as_ref().is_none_or(|layers| layers.contains(&layer))
    }

    fn select_frame_column(&mut self, frame: usize) {
        self.frames = BTreeSet::from([frame]);
        self.layers = None;
        self.anchor_frame = frame;
    }

    fn extend_frames(&mut self, frame: usize) {
        self.frames = span(self.anchor_frame, frame);
        self.layers = None;
    }

    fn extend_cells(&mut self, frame: usize, layer: usize) {
        self.frames = span(self.anchor_frame, frame);
        self.layers = Some(span(self.anchor_layer, layer));
    }

    fn extend_frames_keeping_layers(&mut self, frame: usize) {
        self.frames = span(self.anchor_frame, frame);
    }

    fn toggle_frame(&mut self, frame: usize) -> usize {
        self.anchor_frame = frame;
        if self.frames.len() > 1 && self.frames.remove(&frame) {
            return nearest_member(&self.frames, frame);
        }
        self.frames.insert(frame);
        frame
    }

    fn select_layer(&mut self, layer: usize) {
        self.layers = Some(BTreeSet::from([layer]));
        self.anchor_layer = layer;
    }

    fn extend_layers(&mut self, layer: usize) {
        self.layers = Some(span(self.anchor_layer, layer));
    }

    fn toggle_layer(&mut self, layer: usize) -> usize {
        self.anchor_layer = layer;
        let layers = self.layers.get_or_insert_with(BTreeSet::new);
        if layers.len() > 1 && layers.remove(&layer) {
            return nearest_member(layers, layer);
        }
        layers.insert(layer);
        layer
    }

    fn first_frame(&self) -> usize {
        self.frames.first().copied().unwrap_or(FIRST_FRAME)
    }

    fn last_frame(&self) -> usize {
        self.frames.last().copied().unwrap_or(FIRST_FRAME)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectMode {
    Replace,
    Extend,
    Toggle,
}

impl SelectMode {
    fn of(modifiers: egui::Modifiers) -> Self {
        if modifiers.shift {
            SelectMode::Extend
        } else if modifiers.command {
            SelectMode::Toggle
        } else {
            SelectMode::Replace
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridDrag {
    Pan,
    Frames,
    Cells,
    Move { grab: usize },
}

pub struct TimelineState {
    pub playing: bool,
    pub looping: bool,
    pub onion: bool,
    next_tick: f64,
    playing_revision: u64,
    scroll: Vec2,
    seen_frame: usize,
    drag: Option<GridDrag>,
    selection: CellSelection,
    cel_clip: Option<CelClip>,
    hovered: bool,
    space_pressed_at: Option<f64>,
    space_used: bool,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            playing: false,
            looping: true,
            onion: false,
            next_tick: 0.0,
            playing_revision: 0,
            scroll: Vec2::ZERO,
            seen_frame: FIRST_FRAME,
            drag: None,
            selection: CellSelection::single(FIRST_FRAME, FIRST_FRAME),
            cel_clip: None,
            hovered: false,
            space_pressed_at: None,
            space_used: false,
        }
    }
}

type CacheStamp = (u64, usize, usize);

pub struct TimelineCache {
    stamp: CacheStamp,
    marks: Vec<Vec<CelMark>>,
    thumbs: Option<Vec<Vec<TextureHandle>>>,
}

impl TimelineCache {
    pub fn stamp(&self) -> CacheStamp {
        self.stamp
    }

    pub fn has_thumbs(&self) -> bool {
        self.thumbs.is_some()
    }

    pub fn build(document: &Document, stamp: CacheStamp, ctx: &Context, with_thumbs: bool) -> Self {
        let marks = document.layers().iter().map(Layer::cel_marks).collect();
        let thumbs = with_thumbs.then(|| document.layers().iter().map(|layer| cel_thumbnails(layer, ctx)).collect());
        Self { stamp, marks, thumbs }
    }
}

fn cel_thumbnails(layer: &Layer, ctx: &Context) -> Vec<TextureHandle> {
    let mut by_cel: HashMap<*const Raster, TextureHandle> = HashMap::new();
    layer
        .cels()
        .iter()
        .map(|cel| {
            by_cel
                .entry(Arc::as_ptr(cel))
                .or_insert_with(|| ctx.load_texture("cel-thumb", to_color_image(&sample_down(cel, CEL_THUMB_SIDE)), TextureOptions::NEAREST))
                .clone()
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OnionSide {
    Before,
    After,
}

struct OnionSource {
    side: OnionSide,
    distance: usize,
    frame: usize,
    against: usize,
    cels: Vec<(Arc<Raster>, bool)>,
}

pub struct OnionSignature {
    sources: Vec<OnionSource>,
    props: Vec<LayerProps>,
    settings: OnionSettings,
}

impl OnionSignature {
    pub fn of(document: &Document, settings: OnionSettings) -> Self {
        let settings = settings.clamped();
        let current = document.current_frame();
        let before = (1..=usize::from(settings.before)).filter_map(|distance| current.checked_sub(distance).map(|frame| (OnionSide::Before, distance, frame)));
        let after = (1..=usize::from(settings.after)).filter(|distance| current + distance < document.frame_count()).map(|distance| (OnionSide::After, distance, current + distance));
        let sources = before
            .chain(after)
            .map(|(side, distance, frame)| {
                let cels = document.layers().iter().map(|layer| (Arc::clone(layer.cel(frame)), !Arc::ptr_eq(layer.cel(frame), layer.cel(current)))).collect();
                OnionSource { side, distance, frame, against: current, cels }
            })
            .collect();
        Self { sources, props: document.layers().iter().map(Layer::props).collect(), settings }
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

impl PartialEq for OnionSignature {
    fn eq(&self, other: &Self) -> bool {
        self.props == other.props
            && self.settings == other.settings
            && self.sources.len() == other.sources.len()
            && self.sources.iter().zip(&other.sources).all(|(a, b)| {
                (a.side, a.distance, a.frame, a.against) == (b.side, b.distance, b.frame, b.against)
                    && a.cels.len() == b.cels.len()
                    && a.cels.iter().zip(&b.cels).all(|((left, left_differs), (right, right_differs))| Arc::ptr_eq(left, right) && left_differs == right_differs)
            })
    }
}

pub struct OnionLayer {
    pub texture: TextureHandle,
}

pub struct OnionCache {
    signature: OnionSignature,
    layers: Vec<OnionLayer>,
}

impl OnionCache {
    pub fn signature(&self) -> &OnionSignature {
        &self.signature
    }

    pub fn layers(&self) -> &[OnionLayer] {
        &self.layers
    }

    pub fn build(document: &Document, signature: OnionSignature, ctx: &Context) -> Self {
        let layers = signature
            .sources
            .iter()
            .map(|source| {
                let ghost = document.frame_changes_against(source.frame, source.against);
                let settings = &signature.settings;
                OnionLayer { texture: ctx.load_texture("onion", tinted(&ghost, settings.color(source.side), settings.alpha(source.side, source.distance)), TextureOptions::NEAREST) }
            })
            .collect();
        Self { signature, layers }
    }
}

fn tinted(raster: &Raster, tint: [u8; 3], alpha: f32) -> ColorImage {
    let pixels = raster
        .as_bytes()
        .chunks_exact(4)
        .map(|pixel| {
            let mix = |channel: usize| (f32::from(pixel[channel]) * (1.0 - ONION_TINT_SHARE) + f32::from(tint[channel]) * ONION_TINT_SHARE) as u8;
            Color32::from_rgba_unmultiplied(mix(0), mix(1), mix(2), (f32::from(pixel[3]) * alpha) as u8)
        })
        .collect();
    ColorImage::new([raster.width() as usize, raster.height() as usize], pixels)
}

struct RowView {
    index: usize,
    name: String,
    visible: bool,
    nested: bool,
}

struct GridLayout {
    layer_column: Rect,
    header: Rect,
    cells: Rect,
    cell: Vec2,
    frames: usize,
    rows: usize,
}

impl GridLayout {
    fn new(area: Rect, cell: Vec2, frames: usize, rows: usize) -> Self {
        let layer_column = Rect::from_min_size(area.min, vec2(LAYER_COLUMN_WIDTH.min(area.width()), area.height()));
        let header = Rect::from_min_max(pos2(layer_column.max.x, area.min.y), pos2(area.max.x, area.min.y + HEADER_HEIGHT));
        let cells = Rect::from_min_max(pos2(layer_column.max.x, header.max.y), area.max);
        Self { layer_column, header, cells, cell, frames, rows }
    }

    fn content(&self) -> Vec2 {
        vec2((self.frames + 1) as f32 * self.cell.x, self.rows as f32 * self.cell.y)
    }

    fn max_scroll(&self) -> Vec2 {
        (self.content() - self.cells.size()).max(Vec2::ZERO)
    }

    fn column_left(&self, frame: usize, scroll: Vec2) -> f32 {
        self.cells.min.x + frame as f32 * self.cell.x - scroll.x
    }

    fn row_top(&self, row: usize, scroll: Vec2) -> f32 {
        self.cells.min.y + row as f32 * self.cell.y - scroll.y
    }

    fn cell_rect(&self, row: usize, frame: usize, scroll: Vec2) -> Rect {
        Rect::from_min_size(pos2(self.column_left(frame, scroll), self.row_top(row, scroll)), self.cell)
    }

    fn header_rect(&self, frame: usize, scroll: Vec2) -> Rect {
        Rect::from_min_size(pos2(self.column_left(frame, scroll), self.header.min.y), vec2(self.cell.x, HEADER_HEIGHT))
    }

    fn frame_at(&self, x: f32, scroll: Vec2) -> usize {
        (((x - self.cells.min.x + scroll.x) / self.cell.x).floor().max(0.0) as usize).min(self.frames)
    }

    fn row_at(&self, y: f32, scroll: Vec2) -> Option<usize> {
        let row = ((y - self.cells.min.y + scroll.y) / self.cell.y).floor();
        (row >= 0.0 && (row as usize) < self.rows).then_some(row as usize)
    }

    fn visible_frames(&self, scroll: Vec2) -> std::ops::Range<usize> {
        let first = (scroll.x / self.cell.x).floor().max(0.0) as usize;
        let last = ((scroll.x + self.cells.width()) / self.cell.x).ceil() as usize;
        first..(last + 1).min(self.frames)
    }
}

struct CellPaint<'a> {
    skin: &'a Skin,
    painter: &'a egui::Painter,
    mode: TimelineCells,
}

#[derive(Clone, Copy)]
struct CellState {
    active_row: bool,
    current_column: bool,
    selected: bool,
}

impl CellPaint<'_> {
    fn paint(&self, rect: Rect, mark: CelMark, thumb: Option<TextureId>, state: CellState) {
        let theme = skin::theme();
        let current = state.active_row && state.current_column;
        let fill = if current {
            theme.highlight
        } else if state.active_row {
            theme.list_hover
        } else {
            theme.field
        };
        self.painter.rect_filled(rect, 0.0, fill);
        if state.selected && !current {
            self.painter.rect_filled(rect, 0.0, theme.highlight.gamma_multiply(f32::from(SELECTION_ALPHA) / f32::from(u8::MAX)));
        }
        if state.current_column && !current {
            self.painter.rect_filled(rect, 0.0, theme.highlight.gamma_multiply(f32::from(BAND_ALPHA) / f32::from(u8::MAX)));
        }
        match self.mode {
            TimelineCells::Compact => {
                if mark == CelMark::Drawn {
                    let ink = if current { theme.highlight_text } else { theme.ink };
                    self.painter.circle_filled(rect.center(), DOT_RADIUS, ink);
                }
            }
            TimelineCells::Full => {
                let inner = rect.shrink(FULL_THUMB_INSET);
                let clipped = self.painter.with_clip_rect(inner);
                self.skin.paint_checker(&clipped, inner, inner.min);
                if let Some(id) = thumb {
                    self.painter.image(id, inner, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
                }
                self.painter.rect_stroke(inner, 0.0, Stroke::new(1.0, theme.line_soft), egui::StrokeKind::Outside);
            }
        }
        self.painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme.line_soft), egui::StrokeKind::Inside);
    }
}

#[derive(Default)]
struct FrameKeys {
    frames: i32,
    layers: i32,
    first: bool,
    last: bool,
}

enum GridAction {
    Select { row: Option<usize>, frame: usize, mode: SelectMode },
    SelectLayer { row: usize, mode: SelectMode },
    GoTo(usize),
    MoveFrames(isize),
    ToggleVisibility(usize),
    NewFrame,
}

impl App {
    pub(crate) fn go_to_frame(&mut self, frame: usize) {
        self.flush_pending();
        self.tab_mut().go_to_frame(frame);
    }

    pub(crate) fn step_frame(&mut self, forward: bool) {
        let count = self.tab().doc.frame_count();
        let current = self.tab().doc.current_frame();
        let next = if forward { (current + 1) % count } else { (current + count - 1) % count };
        self.go_to_frame(next);
    }

    pub(crate) fn sync_selection(&mut self) {
        let (count, layer_count, current, active) = {
            let doc = &self.tab().doc;
            (doc.frame_count(), doc.layers().len(), doc.current_frame(), doc.active_layer())
        };
        let selection = &mut self.timeline.selection;
        selection.frames.retain(|&frame| frame < count);
        if let Some(layers) = selection.layers.as_mut() {
            layers.retain(|&layer| layer < layer_count);
        }
        let layers_stale = match &selection.layers {
            Some(layers) => !layers.contains(&active),
            None => selection.active_layer != active,
        };
        if !selection.contains_frame(current) || layers_stale {
            *selection = CellSelection::single(current, active);
        }
    }

    fn collapse_selection(&mut self) {
        let doc = &self.tab().doc;
        self.timeline.selection = CellSelection::single(doc.current_frame(), doc.active_layer());
    }

    fn remember_active_layer(&mut self) {
        self.timeline.selection.active_layer = self.tab().doc.active_layer();
    }

    fn selected_frames(&mut self) -> Vec<usize> {
        self.sync_selection();
        self.timeline.selection.frames.iter().copied().collect()
    }

    fn selected_layers(&mut self) -> Vec<usize> {
        self.sync_selection();
        match &self.timeline.selection.layers {
            Some(layers) => layers.iter().copied().collect(),
            None => (0..self.tab().doc.layers().len()).collect(),
        }
    }

    fn select_cells(&mut self, row: Option<usize>, frame: usize, mode: SelectMode) {
        self.flush_pending();
        self.sync_selection();
        let selection = &mut self.timeline.selection;
        let mut focus = frame;
        match (mode, row) {
            (SelectMode::Replace, Some(row)) => *selection = CellSelection::single(frame, row),
            (SelectMode::Replace, None) => selection.select_frame_column(frame),
            (SelectMode::Extend, Some(row)) => selection.extend_cells(frame, row),
            (SelectMode::Extend, None) => selection.extend_frames(frame),
            (SelectMode::Toggle, _) => focus = selection.toggle_frame(frame),
        }
        if let Some(row) = row.filter(|_| mode != SelectMode::Toggle) {
            self.tab_mut().doc.set_active_layer(row);
        }
        self.remember_active_layer();
        self.go_to_frame(focus);
    }

    fn select_layer(&mut self, row: usize, mode: SelectMode) {
        self.flush_pending();
        self.sync_selection();
        let selection = &mut self.timeline.selection;
        let active = match mode {
            SelectMode::Replace => {
                selection.select_layer(row);
                row
            }
            SelectMode::Extend => {
                selection.extend_layers(row);
                row
            }
            SelectMode::Toggle => selection.toggle_layer(row),
        };
        self.tab_mut().doc.set_active_layer(active);
        self.remember_active_layer();
    }

    pub(crate) fn select_all_frames(&mut self) {
        self.sync_selection();
        let count = self.tab().doc.frame_count();
        let selection = &mut self.timeline.selection;
        selection.frames = (FIRST_FRAME..count).collect();
        selection.layers = None;
        selection.anchor_frame = FIRST_FRAME;
    }

    pub(crate) fn extend_frame_selection(&mut self, forward: bool) {
        self.sync_selection();
        let (count, current) = (self.tab().doc.frame_count(), self.tab().doc.current_frame());
        let target = if forward { (current + 1).min(count - 1) } else { current.saturating_sub(1) };
        self.timeline.selection.extend_frames_keeping_layers(target);
        self.go_to_frame(target);
    }

    pub(crate) fn duplicate_selected_frames(&mut self) {
        let frames = self.selected_frames();
        let created = self.edit_structure(|doc| doc.duplicate_frames(&frames));
        if let Some(&first) = created.first() {
            let selection = &mut self.timeline.selection;
            selection.frames = created.into_iter().collect();
            selection.anchor_frame = first;
        }
    }

    pub(crate) fn delete_selected_frames(&mut self) {
        let frames = self.selected_frames();
        if self.edit_structure(|doc| doc.delete_frames(&frames)) {
            self.collapse_selection();
        }
    }

    pub(crate) fn shift_selected_frames(&mut self, delta: isize) {
        let frames = self.selected_frames();
        let moved = self.edit_structure(|doc| doc.shift_frames(&frames, delta));
        if let Some(&first) = moved.first() {
            let selection = &mut self.timeline.selection;
            selection.frames = moved.into_iter().collect();
            selection.anchor_frame = first;
        }
    }

    pub(crate) fn reverse_selected_frames(&mut self) {
        let mut frames = self.selected_frames();
        if frames.len() < 2 {
            frames = (FIRST_FRAME..self.tab().doc.frame_count()).collect();
        }
        self.edit_structure(|doc| doc.reverse_frames(&frames));
    }

    pub(crate) fn clear_selected_cels(&mut self) {
        let (frames, layers) = (self.selected_frames(), self.selected_layers());
        self.edit_structure(|doc| doc.clear_cels(&frames, &layers));
    }

    pub(crate) fn copy_selected_cels(&mut self) {
        let (frames, layers) = (self.selected_frames(), self.selected_layers());
        self.timeline.cel_clip = self.tab().doc.copy_cels(&frames, &layers);
    }

    pub(crate) fn paste_cels(&mut self) {
        let Some(clip) = self.timeline.cel_clip.clone() else {
            return;
        };
        let (frames, layers) = (self.selected_frames(), self.selected_layers());
        let (Some(&frame), Some(&layer)) = (frames.first(), layers.first()) else {
            return;
        };
        self.edit_structure(|doc| doc.paste_cels(&clip, frame, layer));
    }

    pub(crate) fn scope_to_timeline(&self, commands: Vec<Command>) -> Vec<Command> {
        if !(self.timeline_keys_active() && self.timeline.hovered) {
            return commands;
        }
        let pixels_selected = self.tab().doc.selection().is_some();
        commands
            .into_iter()
            .filter_map(|command| match command {
                Command::SelectAll => Some(Command::SelectAllFrames),
                Command::Copy => Some(Command::CopyCels),
                Command::Paste => Some(Command::PasteCels),
                Command::Deselect => Some(Command::NewFrame),
                Command::EraseSelection if !pixels_selected => Some(Command::ClearCels),
                Command::Cut => None,
                other => Some(other),
            })
            .collect()
    }

    pub(crate) fn timeline_key_commands(&mut self, ctx: &Context) -> Vec<Command> {
        let driving = self.arrows_drive_timeline();
        let scoped = self.timeline_keys_active() && self.timeline.hovered;
        let keys = [
            (egui::Modifiers::SHIFT, egui::Key::ArrowRight, Command::ExtendFramesRight, driving),
            (egui::Modifiers::SHIFT, egui::Key::ArrowLeft, Command::ExtendFramesLeft, driving),
            (egui::Modifiers::ALT, egui::Key::ArrowRight, Command::ShiftFramesRight, driving),
            (egui::Modifiers::ALT, egui::Key::ArrowLeft, Command::ShiftFramesLeft, driving),
            (egui::Modifiers::SHIFT, egui::Key::Delete, Command::DeleteFrame, scoped),
        ];
        ctx.input_mut(|input| keys.into_iter().filter(|&(modifiers, key, _, enabled)| enabled && input.consume_key(modifiers, key)).map(|(_, _, command, _)| command).collect())
    }

    fn current_duration_ms(&self) -> u32 {
        let doc = &self.tab().doc;
        doc.frames()[doc.current_frame()].duration_ms
    }

    pub(crate) fn toggle_playback(&mut self, ctx: &Context) {
        if self.timeline.playing {
            self.timeline.playing = false;
            return;
        }
        if self.tab().doc.frame_count() < 2 {
            return;
        }
        self.flush_pending();
        let last = self.tab().doc.frame_count() - 1;
        if !self.timeline.looping && self.tab().doc.current_frame() == last {
            self.go_to_frame(FIRST_FRAME);
        }
        self.timeline.playing = true;
        self.timeline.playing_revision = self.tab().doc.revision();
        self.timeline.next_tick = ctx.input(|input| input.time) + f64::from(self.current_duration_ms()) / MS_PER_SECOND;
    }

    pub(crate) fn play_from_start(&mut self, ctx: &Context) {
        if self.tab().doc.frame_count() < 2 {
            return;
        }
        self.timeline.playing = false;
        self.go_to_frame(FIRST_FRAME);
        self.toggle_playback(ctx);
    }

    pub(crate) fn open_animation_settings(&mut self) {
        let fps = self.tab().doc.uniform_duration().map_or(DEFAULT_FPS, duration_ms_to_fps);
        self.dialog = Some(Dialog::Animation { fps });
    }

    fn timeline_keys_active(&self) -> bool {
        self.panels.timeline.open && self.dialog.is_none() && !self.text_active()
    }

    pub(crate) fn enter_plays(&self) -> bool {
        self.timeline_keys_active() && !self.has_pending() && self.tab().doc.selection().is_none() && self.tab().doc.frame_count() > 1
    }

    pub(crate) fn arrows_drive_timeline(&self) -> bool {
        self.timeline_keys_active() && self.settings.animation.keys.arrows_navigate && !self.has_pending() && self.tab().doc.selection().is_none()
    }

    pub(crate) fn track_space_tap(&mut self, ctx: &Context, was_down: bool) {
        let now = ctx.input(|input| input.time);
        if self.space_down && !was_down {
            self.timeline.space_pressed_at = Some(now);
            self.timeline.space_used = false;
        }
        if self.space_down && ctx.input(|input| input.pointer.any_down()) {
            self.timeline.space_used = true;
        }
        if self.space_down || !was_down {
            return;
        }
        let quick = self.timeline.space_pressed_at.take().is_some_and(|pressed| now - pressed < SPACE_TAP_SECONDS);
        if quick && !self.timeline.space_used && self.timeline_keys_active() && self.settings.animation.keys.space_plays && !self.has_pending() {
            self.run(ctx, Command::PlayPause);
        }
    }

    pub(crate) fn handle_frame_keys(&mut self, ctx: &Context) {
        if !self.timeline_keys_active() || !self.settings.animation.keys.arrows_navigate {
            return;
        }
        let steps = ctx.input(|input| {
            if !input.modifiers.is_none() {
                return FrameKeys::default();
            }
            let pressed = |key| i32::from(input.num_presses(key) as u8);
            FrameKeys {
                frames: pressed(egui::Key::ArrowRight) - pressed(egui::Key::ArrowLeft),
                layers: pressed(egui::Key::ArrowUp) - pressed(egui::Key::ArrowDown),
                first: input.key_pressed(egui::Key::Home),
                last: input.key_pressed(egui::Key::End),
            }
        });
        if steps.first {
            self.go_to_frame(FIRST_FRAME);
        } else if steps.last {
            self.go_to_frame(self.tab().doc.frame_count() - 1);
        }
        if !self.arrows_drive_timeline() {
            return;
        }
        for _ in 0..steps.frames.abs() {
            self.step_frame(steps.frames > 0);
        }
        if steps.layers != 0 {
            self.step_layer(steps.layers);
        }
    }

    fn step_layer(&mut self, rows_up: i32) {
        let rows = self.timeline_rows();
        let active = self.tab().doc.active_layer();
        let Some(position) = rows.iter().position(|row| row.index == active) else {
            return;
        };
        if let Some(target) = stepped_index(rows.len(), Some(position), rows_up) {
            self.flush_pending();
            self.tab_mut().doc.set_active_layer(rows[target].index);
        }
    }

    pub(crate) fn advance_playback(&mut self, ctx: &Context) {
        if !self.timeline.playing {
            return;
        }
        let now = ctx.input(|input| input.time);
        let editing = self.has_pending() || !matches!(self.tab().gesture, Gesture::None | Gesture::Pan { .. } | Gesture::ZoomDrag { .. } | Gesture::Pick { .. });
        let doc = &self.tab().doc;
        let interrupted = editing || doc.revision() != self.timeline.playing_revision || doc.frame_count() < 2;
        if interrupted {
            self.timeline.playing = false;
            return;
        }
        if now >= self.timeline.next_tick {
            let next = doc.current_frame() + 1;
            if next >= doc.frame_count() && !self.timeline.looping {
                self.timeline.playing = false;
                return;
            }
            self.go_to_frame(next % self.tab().doc.frame_count());
            self.timeline.next_tick = now + f64::from(self.current_duration_ms()) / MS_PER_SECOND;
        }
        ctx.request_repaint_after(Duration::from_secs_f64((self.timeline.next_tick - now).max(0.0)));
    }

    pub(crate) fn timeline_content(&mut self, ui: &mut Ui, ctx: &Context) {
        let skin = self.skin.clone();
        let mut command = self.timeline_toolbar(ui, &skin);
        ui.add_space(TOOLBAR_GAP);
        let action = self.timeline_grid(ui, ctx, &skin, &mut command);
        match action {
            Some(GridAction::Select { row, frame, mode }) => self.select_cells(row, frame, mode),
            Some(GridAction::SelectLayer { row, mode }) => self.select_layer(row, mode),
            Some(GridAction::GoTo(frame)) => self.go_to_frame(frame),
            Some(GridAction::MoveFrames(delta)) => self.shift_selected_frames(delta),
            Some(GridAction::ToggleVisibility(index)) => self.toggle_layer_visibility(index),
            Some(GridAction::NewFrame) => command = Some(Command::NewFrame),
            None => {}
        }
        if let Some(command) = command {
            self.run(ctx, command);
        }
    }

    fn timeline_toolbar(&mut self, ui: &mut Ui, skin: &Skin) -> Option<Command> {
        self.sync_selection();
        let mut command = None;
        let playing = self.timeline.playing;
        let frames = self.tab().doc.frame_count();
        let selection = &self.timeline.selection;
        let (first, last, selected) = (selection.first_frame(), selection.last_frame(), selection.frames.len());
        let has_clip = self.timeline.cel_clip.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(TOOLBAR_GAP, TOOLBAR_GAP);
            if icon_button(ui, skin, Icon::FirstFrame, false, t(Text::FirstFrame)).clicked() {
                self.go_to_frame(FIRST_FRAME);
            }
            if icon_button(ui, skin, Icon::PrevFrame, false, t(Text::PrevFrame)).clicked() {
                command = Some(Command::PrevFrame);
            }
            let (icon, tip) = if playing { (Icon::Pause, Text::PauseAnimation) } else { (Icon::Play, Text::PlayAnimation) };
            if icon_button(ui, skin, icon, playing, t(tip)).clicked() {
                command = Some(Command::PlayPause);
            }
            if icon_button(ui, skin, Icon::NextFrame, false, t(Text::NextFrame)).clicked() {
                command = Some(Command::NextFrame);
            }
            if icon_button(ui, skin, Icon::LastFrame, false, t(Text::LastFrame)).clicked() {
                self.go_to_frame(frames - 1);
            }
            ui.add_space(GROUP_GAP);
            if icon_button(ui, skin, Icon::Refresh, self.timeline.looping, t(Text::LoopAnimation)).clicked() {
                self.timeline.looping = !self.timeline.looping;
            }
            if icon_button(ui, skin, Icon::OnionSkin, self.timeline.onion, t(Text::OnionSkin)).clicked() {
                self.timeline.onion = !self.timeline.onion;
            }
            ui.add_space(GROUP_GAP);
            let frame_tools = [
                (Icon::FilmAdd, Text::NewFrame, Command::NewFrame, true),
                (Icon::EmptyFrame, Text::NewEmptyFrame, Command::NewEmptyFrame, true),
                (Icon::FilmRemove, Text::DeleteFrame, Command::DeleteFrame, selected < frames),
                (Icon::PrevFrame, Text::MoveFrameLeft, Command::ShiftFramesLeft, first > FIRST_FRAME),
                (Icon::NextFrame, Text::MoveFrameRight, Command::ShiftFramesRight, last + 1 < frames),
                (Icon::FlipHorizontal, Text::ReverseFrames, Command::ReverseFrames, frames > 1),
            ];
            for (icon, tip, target, enabled) in frame_tools {
                if tool_button(ui, skin, icon, enabled, tip) {
                    command = Some(target);
                }
            }
            ui.add_space(GROUP_GAP);
            let cel_tools = [
                (Icon::Eraser, Text::ClearCels, Command::ClearCels, true),
                (Icon::Copy, Text::CopyCels, Command::CopyCels, true),
                (Icon::Paste, Text::PasteCels, Command::PasteCels, has_clip),
                (Icon::SelectAll, Text::SelectAllFrames, Command::SelectAllFrames, frames > 1),
            ];
            for (icon, tip, target, enabled) in cel_tools {
                if tool_button(ui, skin, icon, enabled, tip) {
                    command = Some(target);
                }
            }
            ui.add_space(GROUP_GAP);
            let (icon, tip, next) = match self.settings.timeline_cells {
                TimelineCells::Compact => (Icon::Grid, Text::CellsCompact, TimelineCells::Full),
                TimelineCells::Full => (Icon::Picture, Text::CellsFull, TimelineCells::Compact),
            };
            if icon_button(ui, skin, icon, false, t(tip)).clicked() {
                self.settings.timeline_cells = next;
                crate::config::save(&self.settings);
            }
            if icon_button(ui, skin, Icon::SettingsWindow, false, t(Text::AnimationSettings)).clicked() {
                command = Some(Command::AnimationSettings);
            }
            ui.add_space(GROUP_GAP);
            let duration = format!("{} мс", self.current_duration_ms());
            ui.label(if selected > 1 { format!("{selected}/{frames}  {duration}") } else { duration });
        });
        command
    }

    fn timeline_rows(&self) -> Vec<RowView> {
        let doc = &self.tab().doc;
        let layers = doc.layers();
        (0..layers.len())
            .rev()
            .filter(|&index| !doc.mask_parent(index).is_some_and(|parent| layers[parent].collapsed))
            .map(|index| RowView { index, name: layers[index].name.clone(), visible: layers[index].visible, nested: layers[index].masked })
            .collect()
    }

    fn timeline_grid(&mut self, ui: &mut Ui, ctx: &Context, skin: &Skin, command: &mut Option<Command>) -> Option<GridAction> {
        let mode = self.settings.timeline_cells;
        let rows = self.timeline_rows();
        let (frames, current, active_layer) = {
            let doc = &self.tab().doc;
            (doc.frame_count(), doc.current_frame(), doc.active_layer())
        };
        self.sync_selection();
        let selection = self.timeline.selection.clone();
        let durations: Vec<u32> = self.tab().doc.frames().iter().map(|frame| frame.duration_ms).collect();
        let (marks, thumbs) = {
            let cache = self.tab_mut().timeline_cache(ctx, mode == TimelineCells::Full);
            let thumbs: Vec<Vec<TextureId>> = cache.thumbs.as_ref().map(|all| all.iter().map(|row| row.iter().map(TextureHandle::id).collect()).collect()).unwrap_or_default();
            (cache.marks.clone(), thumbs)
        };
        let size = vec2(ui.available_width(), ui.available_height().max(HEADER_HEIGHT + mode.size().y));
        let (area, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let layout = GridLayout::new(area, mode.size(), frames, rows.len());
        let mut scroll = self.timeline.scroll;
        if self.timeline.seen_frame != current {
            self.timeline.seen_frame = current;
            scroll.x = scroll_to_show(scroll.x, layout.column_left(current, Vec2::ZERO) - layout.cells.min.x, layout.cell.x, layout.cells.width());
        }
        let hovered = response.hovered();
        self.timeline.hovered = hovered;
        let pointer = ctx.pointer_hover_pos();
        let select_mode = SelectMode::of(ctx.input(|input| input.modifiers));
        let mut action = None;
        let wheel_navigates = self.settings.animation.keys.wheel_navigates;
        let pan = if hovered { ctx.input(|input| input.smooth_scroll_delta) } else { Vec2::ZERO };
        if wheel_navigates {
            let shift = ctx.input(|input| input.modifiers.shift);
            if !shift {
                scroll.x -= pan.x;
            }
            let notches = wheel_while(ui, response.id.with("wheel"), hovered).notches;
            let over_layers = shift || pointer.is_some_and(|at| layout.layer_column.contains(at));
            if notches != 0 && over_layers {
                let position = rows.iter().position(|row| row.index == active_layer);
                action = stepped_index(rows.len(), position, notches).map(|target| GridAction::SelectLayer { row: rows[target].index, mode: SelectMode::Replace });
            } else if notches != 0 {
                action = stepped_index(frames, Some(current), notches).map(GridAction::GoTo);
            }
        } else {
            let vertical_overflow = layout.max_scroll().y > 0.0;
            scroll -= if vertical_overflow { pan } else { vec2(pan.x + pan.y, 0.0) };
        }
        if response.drag_started() {
            let touch = ctx.input(|input| input.events.iter().any(|event| matches!(event, egui::Event::Touch { .. })));
            let origin = ctx.input(|input| input.pointer.press_origin());
            self.timeline.drag = origin.map(|origin| self.begin_drag(&layout, &rows, origin, scroll, touch, select_mode));
        }
        let drag = self.timeline.drag.filter(|_| response.dragged() || response.drag_stopped());
        let drag_at = pointer.or_else(|| response.interact_pointer_pos());
        match (drag, drag_at) {
            (Some(GridDrag::Pan), _) => scroll -= response.drag_delta(),
            (Some(GridDrag::Frames), Some(at)) if response.dragged() => {
                action = Some(GridAction::Select { row: None, frame: layout.frame_at(at.x, scroll).min(frames - 1), mode: SelectMode::Extend });
            }
            (Some(GridDrag::Cells), Some(at)) if response.dragged() => {
                let row = layout.row_at(at.y.clamp(layout.cells.min.y, layout.cells.max.y - 1.0), scroll).and_then(|row| rows.get(row));
                action = row.map(|view| GridAction::Select { row: Some(view.index), frame: layout.frame_at(at.x, scroll).min(frames - 1), mode: SelectMode::Extend });
            }
            _ => {}
        }
        let move_delta = match (drag, drag_at) {
            (Some(GridDrag::Move { grab }), Some(at)) => {
                let wanted = layout.frame_at(at.x, scroll).min(frames - 1) as isize - grab as isize;
                Some(wanted.clamp(-(selection.first_frame() as isize), (frames - 1 - selection.last_frame()) as isize))
            }
            _ => None,
        };
        if response.drag_stopped() {
            self.timeline.drag = None;
            if let Some(delta) = move_delta.filter(|&delta| delta != 0) {
                action = Some(GridAction::MoveFrames(delta));
            }
        }
        scroll = scroll.clamp(Vec2::ZERO, layout.max_scroll());
        self.timeline.scroll = scroll;

        let painter = ui.painter().with_clip_rect(area);
        let theme = skin::theme();
        painter.rect_filled(area, 0.0, theme.field);
        painter.rect_filled(layout.header, 0.0, theme.chrome);
        painter.rect_filled(layout.layer_column, 0.0, theme.chrome);

        let cell_painter = CellPaint { skin, painter: &painter.with_clip_rect(layout.cells), mode };
        for (row, view) in rows.iter().enumerate() {
            let top = layout.row_top(row, scroll);
            if top + layout.cell.y < layout.cells.min.y || top > layout.cells.max.y {
                continue;
            }
            for frame in layout.visible_frames(scroll) {
                let rect = layout.cell_rect(row, frame, scroll);
                let thumb = thumbs.get(view.index).and_then(|frames| frames.get(frame)).copied();
                let mark = marks.get(view.index).and_then(|row| row.get(frame)).copied().unwrap_or(CelMark::Empty);
                let state = CellState { active_row: view.index == active_layer, current_column: frame == current, selected: selection.contains_cel(frame, view.index) };
                cell_painter.paint(rect, mark, thumb, state);
            }
        }

        let header_painter = painter.with_clip_rect(layout.header);
        for frame in layout.visible_frames(scroll) {
            let rect = layout.header_rect(frame, scroll);
            let selected = frame == current;
            if selected {
                header_painter.rect_filled(rect, 0.0, theme.highlight);
            } else if selection.contains_frame(frame) {
                header_painter.rect_filled(rect, 0.0, theme.highlight.gamma_multiply(f32::from(SELECTION_ALPHA) / f32::from(u8::MAX)));
            }
            let color = if selected { theme.highlight_text } else { theme.ink };
            skin::text_at(&header_painter, rect.center(), Align2::CENTER_CENTER, &(frame + 1).to_string(), Font::Body, color);
            header_painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme.line_soft), egui::StrokeKind::Inside);
        }
        if let Some(delta) = move_delta.filter(|&delta| delta != 0) {
            let edge = if delta > 0 { layout.column_left((selection.last_frame() as isize + delta) as usize + 1, scroll) } else { layout.column_left((selection.first_frame() as isize + delta) as usize, scroll) };
            painter.line_segment([pos2(edge, layout.header.min.y), pos2(edge, layout.cells.max.y)], Stroke::new(MOVE_MARKER_WIDTH, theme.ink));
        }
        let plus = layout.header_rect(frames, scroll);
        paint_plus(&header_painter, plus, theme.ink);

        let layer_painter = painter.with_clip_rect(layout.layer_column);
        let layer_clip = Rect::from_min_max(pos2(layout.layer_column.min.x, layout.cells.min.y), layout.layer_column.max);
        let names = layer_painter.with_clip_rect(layer_clip);
        let mut eye_hits = Vec::new();
        for (row, view) in rows.iter().enumerate() {
            let top = layout.row_top(row, scroll);
            let band = Rect::from_min_size(pos2(layout.layer_column.min.x, top), vec2(layout.layer_column.width(), layout.cell.y));
            if band.max.y < layout.cells.min.y || band.min.y > layout.cells.max.y {
                continue;
            }
            let active = view.index == active_layer;
            let picked = selection.layers.as_ref().is_some_and(|layers| layers.contains(&view.index));
            let band_fill = if active {
                theme.highlight
            } else if picked {
                theme.highlight.gamma_multiply(f32::from(SELECTION_ALPHA) / f32::from(u8::MAX))
            } else {
                theme.chrome
            };
            names.rect_filled(band, 0.0, band_fill);
            let color = if active { theme.highlight_text } else { theme.ink };
            let indent = if view.nested { NESTED_INDENT } else { 0.0 };
            let eye = Rect::from_center_size(pos2(band.min.x + LAYER_COLUMN_PADDING + EYE_SIDE / 2.0, band.center().y), vec2(EYE_SIDE, EYE_SIDE));
            let tint = if view.visible { Color32::WHITE } else { Color32::from_white_alpha(HIDDEN_EYE_ALPHA) };
            names.image(skin.icons.texture_id(Icon::Eye, EYE_SIDE), eye, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
            eye_hits.push((view.index, eye.intersect(layer_clip)));
            skin::text_at(&names, pos2(eye.max.x + LAYER_COLUMN_PADDING + indent, band.center().y), Align2::LEFT_CENTER, &view.name, Font::Body, color);
            names.line_segment([band.left_bottom(), band.right_bottom()], Stroke::new(1.0, theme.line_soft));
        }
        painter.line_segment([layout.layer_column.right_top(), layout.layer_column.right_bottom()], Stroke::new(1.0, theme.line));
        painter.line_segment([layout.header.left_bottom(), layout.header.right_bottom()], Stroke::new(1.0, theme.line));
        paint_scroll_bars(&painter, &layout, scroll);

        if response.secondary_clicked() {
            if let Some(at) = response.interact_pointer_pos() {
                action = self.pick_for_menu(&layout, &rows, &selection, at, scroll, frames);
            }
            let edit_id = response.id.with(DURATION_EDIT_KEY);
            ctx.data_mut(|data| data.insert_temp(edit_id, durations.get(current).copied().unwrap_or_default()));
        } else if response.clicked() {
            if let Some(at) = response.interact_pointer_pos() {
                action = self.click_target(&layout, &rows, &eye_hits, at, scroll, frames, select_mode);
            }
        }
        if let Some(menu) = self.frame_menu(&response, skin, frames, &durations) {
            *command = Some(menu);
        }
        action
    }

    fn begin_drag(&mut self, layout: &GridLayout, rows: &[RowView], origin: Pos2, scroll: Vec2, touch: bool, mode: SelectMode) -> GridDrag {
        let frames = layout.frames;
        let frame = layout.frame_at(origin.x, scroll).min(frames - 1);
        let selection = &mut self.timeline.selection;
        if layout.header.contains(origin) {
            if selection.contains_frame(frame) && mode == SelectMode::Replace {
                return GridDrag::Move { grab: frame };
            }
            if mode != SelectMode::Extend {
                selection.anchor_frame = frame;
            }
            return GridDrag::Frames;
        }
        let row = layout.row_at(origin.y, scroll).and_then(|row| rows.get(row));
        match row {
            Some(view) if layout.cells.contains(origin) && !touch => {
                if mode != SelectMode::Extend {
                    selection.anchor_frame = frame;
                    selection.anchor_layer = view.index;
                }
                GridDrag::Cells
            }
            _ => GridDrag::Pan,
        }
    }

    fn pick_for_menu(&self, layout: &GridLayout, rows: &[RowView], selection: &CellSelection, at: Pos2, scroll: Vec2, frames: usize) -> Option<GridAction> {
        let frame = layout.frame_at(at.x, scroll).min(frames - 1);
        let row = layout.row_at(at.y, scroll).and_then(|row| rows.get(row)).map(|view| view.index).filter(|_| layout.cells.contains(at));
        let inside = match row {
            Some(row) => selection.contains_cel(frame, row),
            None => selection.contains_frame(frame),
        };
        (!inside).then_some(GridAction::Select { row, frame, mode: SelectMode::Replace })
    }

    #[allow(clippy::too_many_arguments)]
    fn click_target(&self, layout: &GridLayout, rows: &[RowView], eyes: &[(usize, Rect)], at: Pos2, scroll: Vec2, frames: usize, mode: SelectMode) -> Option<GridAction> {
        if let Some(&(index, _)) = eyes.iter().find(|(_, rect)| rect.contains(at)) {
            return Some(GridAction::ToggleVisibility(index));
        }
        let row = layout.row_at(at.y, scroll).and_then(|row| rows.get(row)).map(|view| view.index);
        if layout.layer_column.contains(at) {
            return row.map(|row| GridAction::SelectLayer { row, mode });
        }
        let frame = layout.frame_at(at.x, scroll);
        if layout.header.contains(at) {
            return Some(if frame >= frames { GridAction::NewFrame } else { GridAction::Select { row: None, frame, mode } });
        }
        Some(GridAction::Select { row: row.filter(|_| layout.cells.contains(at)), frame: frame.min(frames - 1), mode })
    }

    fn frame_menu(&mut self, response: &egui::Response, skin: &Skin, frames: usize, durations: &[u32]) -> Option<Command> {
        let mut picked = None;
        let mut new_duration = None;
        let selection = &self.timeline.selection;
        let (first, last, selected) = (selection.first_frame(), selection.last_frame(), selection.frames.clone());
        let has_clip = self.timeline.cel_clip.is_some();
        egui::Popup::context_menu(response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            skin::menu_style(ui);
            let groups: [&[(Icon, Text, Command, bool)]; 3] = [
                &[
                    (Icon::FilmAdd, Text::NewFrame, Command::NewFrame, true),
                    (Icon::EmptyFrame, Text::NewEmptyFrame, Command::NewEmptyFrame, true),
                    (Icon::FilmRemove, Text::DeleteFrame, Command::DeleteFrame, selected.len() < frames),
                ],
                &[
                    (Icon::PrevFrame, Text::MoveFrameLeft, Command::ShiftFramesLeft, first > FIRST_FRAME),
                    (Icon::NextFrame, Text::MoveFrameRight, Command::ShiftFramesRight, last + 1 < frames),
                    (Icon::FlipHorizontal, Text::ReverseFrames, Command::ReverseFrames, frames > 1),
                ],
                &[
                    (Icon::Eraser, Text::ClearCels, Command::ClearCels, true),
                    (Icon::Copy, Text::CopyCels, Command::CopyCels, true),
                    (Icon::Paste, Text::PasteCels, Command::PasteCels, has_clip),
                    (Icon::SelectAll, Text::SelectAllFrames, Command::SelectAllFrames, frames > 1),
                ],
            ];
            for (position, group) in groups.into_iter().enumerate() {
                if position > FIRST_FRAME {
                    ui.separator();
                }
                for &(icon, label, command, enabled) in group {
                    if ui.add_enabled(enabled, egui::Button::image_and_text(skin.icons.image(icon), t(label))).clicked() {
                        picked = Some(command);
                        ui.close();
                    }
                }
            }
            ui.separator();
            let id = response.id.with(DURATION_EDIT_KEY);
            let stored = durations.get(last).copied().unwrap_or_default();
            let mut milliseconds: u32 = ui.data(|data| data.get_temp(id)).unwrap_or(stored);
            let edited = ui
                .horizontal(|ui| {
                    ui.label(t(Text::FrameDurationLabel));
                    ui.add(egui::DragValue::new(&mut milliseconds).range(MIN_FRAME_DURATION_MS..=MAX_FRAME_DURATION_MS).suffix(" мс"))
                })
                .inner;
            ui.data_mut(|data| data.insert_temp(id, milliseconds));
            if (edited.drag_stopped() || edited.lost_focus()) && milliseconds != stored {
                new_duration = Some(milliseconds);
            }
        });
        if let Some(milliseconds) = new_duration {
            let frames: Vec<usize> = selected.into_iter().collect();
            self.edit_structure(|doc| doc.set_frames_duration(&frames, milliseconds));
        }
        picked
    }
}

fn tool_button(ui: &mut Ui, skin: &Skin, icon: Icon, enabled: bool, tip: Text) -> bool {
    ui.add_enabled_ui(enabled, |ui| icon_button(ui, skin, icon, false, t(tip)).clicked()).inner
}

fn scroll_to_show(scroll: f32, left: f32, width: f32, view: f32) -> f32 {
    if left < scroll {
        left
    } else if left + width > scroll + view {
        left + width - view
    } else {
        scroll
    }
}

fn paint_plus(painter: &egui::Painter, rect: Rect, color: Color32) {
    let center = rect.center();
    let stroke = Stroke::new(1.0, color);
    painter.line_segment([center - vec2(PLUS_ARM, 0.0), center + vec2(PLUS_ARM, 0.0)], stroke);
    painter.line_segment([center - vec2(0.0, PLUS_ARM), center + vec2(0.0, PLUS_ARM)], stroke);
}

fn paint_scroll_bars(painter: &egui::Painter, layout: &GridLayout, scroll: Vec2) {
    let ink = skin::theme().ink.gamma_multiply(f32::from(SCROLL_BAR_ALPHA) / f32::from(u8::MAX));
    let (content, view) = (layout.content(), layout.cells.size());
    if content.x > view.x {
        let length = view.x * view.x / content.x;
        let left = layout.cells.min.x + scroll.x / content.x * view.x;
        painter.rect_filled(Rect::from_min_size(pos2(left, layout.cells.max.y - SCROLL_BAR_THICKNESS), vec2(length, SCROLL_BAR_THICKNESS)), 0.0, ink);
    }
    if content.y > view.y {
        let length = view.y * view.y / content.y;
        let top = layout.cells.min.y + scroll.y / content.y * view.y;
        painter.rect_filled(Rect::from_min_size(pos2(layout.cells.max.x - SCROLL_BAR_THICKNESS, top), vec2(SCROLL_BAR_THICKNESS, length)), 0.0, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 200.0));

    #[test]
    fn scroll_follows_the_current_column_only_when_it_leaves_the_view() {
        assert_eq!(scroll_to_show(0.0, 40.0, 20.0, 100.0), 0.0);
        assert_eq!(scroll_to_show(0.0, 120.0, 20.0, 100.0), 40.0);
        assert_eq!(scroll_to_show(80.0, 20.0, 20.0, 100.0), 20.0);
    }

    #[test]
    fn cells_map_back_to_rows_and_frames_under_scroll() {
        let layout = GridLayout::new(AREA, COMPACT_CELL, 30, 3);
        let scroll = vec2(40.0, 0.0);
        let rect = layout.cell_rect(1, 5, scroll);
        assert_eq!(layout.frame_at(rect.center().x, scroll), 5);
        assert_eq!(layout.row_at(rect.center().y, scroll), Some(1));
        assert_eq!(layout.row_at(layout.cells.min.y + 3.0 * COMPACT_CELL.y + 1.0, scroll), None);
    }

    #[test]
    fn scroll_range_leaves_room_for_the_plus_column() {
        let layout = GridLayout::new(AREA, COMPACT_CELL, 100, 2);
        assert_eq!(layout.max_scroll().x, 101.0 * COMPACT_CELL.x - layout.cells.width());
        assert_eq!(layout.max_scroll().y, 0.0);
    }

    #[test]
    fn fps_and_frame_duration_convert_both_ways() {
        assert_eq!(fps_to_duration_ms(10), 100);
        assert_eq!(fps_to_duration_ms(12), 83);
        assert_eq!(duration_ms_to_fps(83), 12);
        assert_eq!(duration_ms_to_fps(100), 10);
        assert_eq!(fps_to_duration_ms(0), 1000);
        assert_eq!(duration_ms_to_fps(1), MAX_FPS);
    }

    #[test]
    fn farther_onion_frames_are_fainter_and_a_single_frame_keeps_full_opacity() {
        let settings = OnionSettings { before: 3, after: 1, opacity: 60, ..OnionSettings::default() };
        let near = settings.alpha(OnionSide::Before, 1);
        let far = settings.alpha(OnionSide::Before, 3);
        assert!((near - 0.6).abs() < 1e-6);
        assert!(far < near && far > 0.0);
        assert!((settings.alpha(OnionSide::After, 1) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn onion_settings_are_clamped_to_the_allowed_range() {
        let wild = OnionSettings { before: 99, after: 99, opacity: 250, ..OnionSettings::default() }.clamped();
        assert_eq!((wild.before, wild.after, wild.opacity), (ONION_MAX_FRAMES, ONION_MAX_FRAMES, PERCENT_MAX));
    }

    #[test]
    fn cell_mode_survives_the_settings_round_trip() {
        for mode in [TimelineCells::Compact, TimelineCells::Full] {
            assert_eq!(TimelineCells::from_setting(mode.to_setting()), mode);
        }
    }

    #[test]
    fn shift_extends_from_the_anchor_in_both_directions() {
        let mut selection = CellSelection::single(4, 1);
        selection.extend_cells(6, 3);
        assert_eq!(selection.frames, BTreeSet::from([4, 5, 6]));
        assert_eq!(selection.layers, Some(BTreeSet::from([1, 2, 3])));
        selection.extend_frames_keeping_layers(2);
        assert_eq!(selection.frames, BTreeSet::from([2, 3, 4]));
        assert_eq!(selection.layers, Some(BTreeSet::from([1, 2, 3])));
    }

    #[test]
    fn ctrl_toggle_moves_focus_to_a_remaining_frame_and_never_empties_the_selection() {
        let mut selection = CellSelection::single(2, 0);
        assert_eq!(selection.toggle_frame(5), 5);
        assert_eq!(selection.toggle_frame(5), 2);
        assert_eq!(selection.frames, BTreeSet::from([2]));
        assert_eq!(selection.toggle_frame(2), 2);
        assert!(selection.contains_cel(2, 0) && !selection.contains_cel(3, 0));
    }

    #[test]
    fn a_frame_column_covers_every_layer() {
        let mut selection = CellSelection::single(1, 2);
        selection.select_frame_column(3);
        assert!(selection.contains_cel(3, 0) && selection.contains_cel(3, 9));
        assert!(!selection.contains_cel(1, 0));
    }
}
