
use std::time::Duration;

use chequer_core::blend::BlendMode;
use chequer_core::geom::{FPoint, IRect};
use chequer_core::history::HistoryKind;
use chequer_core::raster::Rgba;
use chequer_core::text::{self, Layout, TextStyle};
use eframe::egui::{self, Context, Event, Key, Pos2, Rect, Stroke, vec2};

use crate::app::App;
use crate::fonts::{TextFace, text_face};
use crate::skin;
use crate::handles::{ScreenBox, paint_move_icon};
use crate::tools::{TextOptions, Tool};

const MIN_BOX_WIDTH: f32 = 8.0;
const CARET_BLINK: Duration = Duration::from_millis(500);
const CARET_WIDTH: f32 = 1.0;
const KEYBOARD_POLL: Duration = Duration::from_millis(40);
const TEXT_HIT_PADDING: f32 = 10.0;
const NEWLINE_SELECTION_WIDTH: f32 = 4.0;
const SELECTION_ALPHA: f32 = 0.5;

#[derive(Clone, PartialEq)]
struct RenderKey {
    content: String,
    options: TextOptions,
    primary: Rgba,
    secondary: Rgba,
    origin: FPoint,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WordClass {
    Space,
    Word,
    Punctuation,
}

impl WordClass {
    fn of(character: char) -> Self {
        if character.is_whitespace() {
            Self::Space
        } else if character.is_alphanumeric() || character == '_' {
            Self::Word
        } else {
            Self::Punctuation
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum EditKind {
    Typing,
    Deleting,
}

struct TextSnapshot {
    content: String,
    caret: usize,
}

pub struct TextSession {
    pub content: String,
    pub caret: usize,
    pub anchor: Option<usize>,
    pub origin: FPoint,
    pub secondary: bool,
    layout: Option<Layout>,
    rendered: Option<RenderKey>,
    undo: Vec<TextSnapshot>,
    redo: Vec<TextSnapshot>,
    last_edit: Option<EditKind>,
}

pub struct CommittedText {
    session: TextSession,
    options: TextOptions,
    history_cursor: usize,
}

impl TextSession {
    pub fn new(origin: FPoint, secondary: bool) -> Self {
        Self { content: String::new(), caret: 0, anchor: None, origin, secondary, layout: None, rendered: None, undo: Vec::new(), redo: Vec::new(), last_edit: None }
    }

    fn char_count(&self) -> usize {
        self.content.chars().count()
    }

    fn byte_index(&self, char_index: usize) -> usize {
        self.content.char_indices().nth(char_index).map_or(self.content.len(), |(byte, _)| byte)
    }

    fn selection(&self) -> Option<std::ops::Range<usize>> {
        let anchor = self.anchor.filter(|&anchor| anchor != self.caret)?;
        Some(anchor.min(self.caret)..anchor.max(self.caret))
    }

    fn snapshot(&self) -> TextSnapshot {
        TextSnapshot { content: self.content.clone(), caret: self.caret }
    }

    fn record(&mut self, kind: Option<EditKind>) {
        if kind.is_none() || kind != self.last_edit {
            self.undo.push(self.snapshot());
        }
        self.redo.clear();
        self.last_edit = kind;
    }

    fn restore(&mut self, snapshot: TextSnapshot) {
        self.content = snapshot.content;
        self.caret = snapshot.caret;
        self.anchor = None;
        self.last_edit = None;
    }

    pub fn undo_edit(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(previous);
        true
    }

    pub fn redo_edit(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(next);
        }
    }

    fn move_to(&mut self, caret: usize) {
        self.move_caret(caret, false);
    }

    fn move_caret(&mut self, caret: usize, extend: bool) {
        self.last_edit = None;
        self.anchor = if extend { self.anchor.or(Some(self.caret)) } else { None };
        self.caret = caret;
    }

    fn word_boundary(&self, from: usize, forward: bool) -> usize {
        let chars: Vec<char> = self.content.chars().collect();
        let class = |index: usize| WordClass::of(chars[index]);
        let step = |index: usize| if forward { index + 1 } else { index - 1 };
        let probe = |index: usize| if forward { index } else { index - 1 };
        let in_range = |index: usize| if forward { index < chars.len() } else { index > 0 };
        let mut index = from;
        while in_range(index) && class(probe(index)) == WordClass::Space {
            index = step(index);
        }
        if !in_range(index) {
            return index;
        }
        let run = class(probe(index));
        while in_range(index) && class(probe(index)) == run {
            index = step(index);
        }
        index
    }

    fn delete_word(&mut self, forward: bool) {
        if self.selection().is_none() {
            self.anchor = Some(self.word_boundary(self.caret, forward));
        }
        if self.selection().is_none() {
            return;
        }
        self.record(None);
        self.delete_selection();
    }

    fn select_all(&mut self) {
        self.last_edit = None;
        self.anchor = Some(0);
        self.caret = self.char_count();
    }

    fn delete_selection(&mut self) -> bool {
        let range = self.selection();
        self.anchor = None;
        let Some(range) = range else {
            return false;
        };
        let bytes = self.byte_index(range.start)..self.byte_index(range.end);
        self.content.replace_range(bytes, "");
        self.caret = range.start;
        true
    }

    fn insert(&mut self, text: &str, coalesce: bool) {
        self.record((coalesce && self.selection().is_none()).then_some(EditKind::Typing));
        self.delete_selection();
        let at = self.byte_index(self.caret);
        self.content.insert_str(at, text);
        self.caret += text.chars().count();
    }

    fn delete_before(&mut self) {
        if self.caret == 0 && self.selection().is_none() {
            return;
        }
        self.record(self.selection().is_none().then_some(EditKind::Deleting));
        if self.delete_selection() {
            return;
        }
        if self.caret > 0 {
            self.caret -= 1;
            let at = self.byte_index(self.caret);
            self.content.remove(at);
        }
    }

    fn delete_after(&mut self) {
        if self.caret == self.char_count() && self.selection().is_none() {
            return;
        }
        self.record(self.selection().is_none().then_some(EditKind::Deleting));
        if self.delete_selection() {
            return;
        }
        if self.caret < self.char_count() {
            let at = self.byte_index(self.caret);
            self.content.remove(at);
        }
    }

    fn move_vertically(&mut self, lines: i32, extend: bool) {
        let Some(layout) = &self.layout else {
            return;
        };
        let stop = layout.caret(self.caret);
        let y = stop.top + stop.height / 2.0 + lines as f32 * layout.line_height();
        let target = layout.index_at(stop.x, y);
        self.move_caret(target, extend);
    }

    fn move_to_line_edge(&mut self, end: bool, extend: bool) {
        let Some(layout) = &self.layout else {
            return;
        };
        let stop = layout.caret(self.caret);
        let target = layout.index_at(if end { f32::MAX } else { f32::MIN }, stop.top + stop.height / 2.0);
        self.move_caret(target, extend);
    }
}

impl App {
    pub(crate) fn text_active(&self) -> bool {
        self.tool == Tool::Text && !self.tabs.is_empty() && self.tab().text.is_some()
    }

    pub(crate) fn sync_keyboard(&mut self, ctx: &Context) {
        let tapped = ctx.input(|input| input.pointer.any_pressed());
        if tapped {
            self.keyboard_dismissed = false;
        }
        let wanted = (ctx.egui_wants_keyboard_input() || self.text_active()) && !self.keyboard_dismissed;
        if wanted != self.keyboard_visible || (wanted && tapped) {
            self.keyboard_visible = wanted;
            crate::device::set_keyboard(wanted);
        }
        if wanted {
            ctx.request_repaint_after(KEYBOARD_POLL);
        }
    }

    fn resolved_text_face(&mut self, options: &TextOptions) -> Result<TextFace, String> {
        let key = (options.family.clone(), options.bold, options.italic);
        if let Some(face) = self.text_faces.get(&key) {
            return Ok(face.clone());
        }
        let face = text_face(options.family.as_deref(), options.bold, options.italic)?;
        self.text_faces.insert(key, face.clone());
        Ok(face)
    }

    fn text_style(&self, face: &TextFace, secondary: bool) -> TextStyle {
        let options = &self.options.text;
        let (primary, other) = if secondary { (self.colors.secondary, self.colors.primary) } else { (self.colors.primary, self.colors.secondary) };
        let (outline, fill) = options.fill_style.colors(primary, other);
        TextStyle {
            size: options.size,
            synthetic_bold: face.synthetic_bold,
            synthetic_italic: face.synthetic_italic,
            underline: options.underline,
            strikethrough: options.strikethrough,
            hinting: options.hinting,
            smoothing: options.smoothing,
            align: options.align,
            outline,
            fill,
            outline_width: options.outline_width,
            outline_align: options.outline_align,
        }
    }

    pub(crate) fn begin_text(&mut self, at: FPoint, secondary: bool) {
        self.commit_text();
        self.tab_mut().text = Some(TextSession::new(at, secondary));
        self.refresh_text_preview();
    }

    pub(crate) fn refresh_text_preview(&mut self) {
        let Some(session) = self.tab().text.as_ref() else {
            return;
        };
        let key = RenderKey {
            content: session.content.clone(),
            options: self.options.text.clone(),
            primary: self.colors.primary,
            secondary: self.colors.secondary,
            origin: session.origin,
        };
        if session.rendered.as_ref() == Some(&key) {
            return;
        }
        let (content, origin, secondary) = (session.content.clone(), session.origin, session.secondary);
        let options = self.options.text.clone();
        let face = match self.resolved_text_face(&options) {
            Ok(face) => face,
            Err(message) => {
                self.say(message);
                return;
            }
        };
        let style = self.text_style(&face, secondary);
        let layout = Layout::build(&face.face, &content, &style);
        let tab = self.tab_mut();
        let overlay = text::render(&face.face, &layout, &style, origin, BlendMode::Normal, tab.doc.overlay_canvas());
        let area = tab.doc.set_preview(overlay);
        tab.mark_dirty(area);
        if let Some(session) = tab.text.as_mut() {
            session.layout = Some(layout);
            session.rendered = Some(key);
        }
    }

    pub(crate) fn center_text_on_canvas(&mut self) {
        let tab = self.tab_mut();
        let (width, height) = (tab.doc.width() as f32, tab.doc.height() as f32);
        let Some(session) = tab.text.as_mut() else {
            return;
        };
        let Some(layout) = &session.layout else {
            return;
        };
        session.origin = FPoint::new(((width - layout.width) / 2.0).round(), ((height - layout.height) / 2.0).round());
    }

    pub(crate) fn cancel_text(&mut self) {
        let tab = self.tab_mut();
        tab.text = None;
        let area = tab.doc.set_preview(None);
        tab.mark_dirty(area);
        tab.mark_all_dirty();
    }

    pub(crate) fn commit_text(&mut self) {
        let options = self.options.text.clone();
        let tab = self.tab_mut();
        let Some(session) = tab.text.take() else {
            return;
        };
        let cursor_before = tab.doc.history().cursor();
        let area = tab.doc.commit_preview(HistoryKind::TextInsert);
        tab.mark_dirty(area);
        let history_cursor = tab.doc.history().cursor();
        if history_cursor > cursor_before {
            tab.committed_text = Some(CommittedText { session, options, history_cursor });
        }
    }

    pub(crate) fn reopen_committed_text(&mut self) -> bool {
        let tab = self.tab_mut();
        let history = tab.doc.history();
        let is_last_entry = tab.committed_text.as_ref().is_some_and(|c| c.history_cursor == history.cursor())
            && history.kinds().nth(history.cursor().wrapping_sub(1)) == Some(HistoryKind::TextInsert);
        if !is_last_entry {
            return false;
        }
        let Some(CommittedText { mut session, options, .. }) = tab.committed_text.take() else {
            return false;
        };
        let area = tab.doc.undo();
        tab.mark_dirty(area);
        session.rendered = None;
        self.options.text = options;
        self.set_tool(Tool::Text);
        self.tab_mut().text = Some(session);
        self.refresh_text_preview();
        true
    }

    fn text_image_rect(&self) -> Option<IRect> {
        let session = self.tab().text.as_ref()?;
        let (width, height) = session.layout.as_ref().map_or((MIN_BOX_WIDTH, self.options.text.size), |layout| (layout.width.max(MIN_BOX_WIDTH), layout.height));
        Some(IRect::new(
            session.origin.x.floor() as i32,
            session.origin.y.floor() as i32,
            (session.origin.x + width).ceil() as i32,
            (session.origin.y + height).ceil() as i32,
        ))
    }

    pub(crate) fn text_screen_rect(&self, canvas: Rect) -> Option<Rect> {
        let frame = self.text_image_rect()?;
        let view = &self.tab().view;
        Some(Rect::from_two_pos(view.image_to_screen(canvas, frame.x0 as f32, frame.y0 as f32), view.image_to_screen(canvas, frame.x1 as f32, frame.y1 as f32)))
    }

    pub(crate) fn text_pressed(&mut self, canvas: Rect, pos: Pos2, image_pos: FPoint, secondary: bool) {
        if let Some(rect) = self.text_screen_rect(canvas) {
            if ScreenBox::from_rect(rect).hits_move_icon(pos) {
                let origin = self.tab().text.as_ref().map_or(image_pos, |session| session.origin);
                self.tab_mut().gesture = crate::canvas::Gesture::TextMove { anchor: image_pos, start: origin };
                return;
            }
            if rect.expand(TEXT_HIT_PADDING).contains(pos) {
                if let Some(session) = self.tab_mut().text.as_mut() {
                    if let Some(layout) = &session.layout {
                        let caret = layout.index_at(image_pos.x - session.origin.x, image_pos.y - session.origin.y);
                        session.move_to(caret);
                        session.anchor = Some(caret);
                        self.tab_mut().gesture = crate::canvas::Gesture::TextSelect;
                    }
                }
                return;
            }
        }
        self.begin_text(image_pos, secondary);
    }

    pub(crate) fn text_selected(&mut self, image_pos: FPoint) {
        let Some(session) = self.tab_mut().text.as_mut() else {
            return;
        };
        if let Some(layout) = &session.layout {
            session.caret = layout.index_at(image_pos.x - session.origin.x, image_pos.y - session.origin.y);
        }
    }

    pub(crate) fn text_moved(&mut self, anchor: FPoint, start: FPoint, image_pos: FPoint, shift: bool) {
        let offset = crate::canvas::move_offset(anchor, image_pos, shift);
        if let Some(session) = self.tab_mut().text.as_mut() {
            session.origin = FPoint::new(start.x + offset.x as f32, start.y + offset.y as f32);
        }
    }

    pub(crate) fn handle_text_input(&mut self, ctx: &Context) {
        let events = ctx.input(|input| input.events.clone());
        let mut commit = false;
        let Some(session) = self.tab_mut().text.as_mut() else {
            return;
        };
        for event in events {
            match event {
                Event::Text(text) => session.insert(&text, true),
                Event::Paste(text) => session.insert(&text.replace("\r\n", "\n"), false),
                Event::Key { key: Key::A, pressed: true, modifiers, .. } if modifiers.command => session.select_all(),
                Event::Key { key, pressed: true, modifiers, .. } => match key {
                    Key::Backspace if modifiers.command => session.delete_word(false),
                    Key::Delete if modifiers.command => session.delete_word(true),
                    Key::Backspace => session.delete_before(),
                    Key::Delete => session.delete_after(),
                    Key::Enter if modifiers.shift => session.insert("\n", false),
                    Key::Enter => commit = true,
                    Key::Escape => commit = true,
                    Key::ArrowLeft if modifiers.command => session.move_caret(session.word_boundary(session.caret, false), modifiers.shift),
                    Key::ArrowRight if modifiers.command => session.move_caret(session.word_boundary(session.caret, true), modifiers.shift),
                    Key::ArrowLeft => session.move_caret(session.caret.saturating_sub(1), modifiers.shift),
                    Key::ArrowRight => session.move_caret((session.caret + 1).min(session.char_count()), modifiers.shift),
                    Key::ArrowUp => session.move_vertically(-1, modifiers.shift),
                    Key::ArrowDown => session.move_vertically(1, modifiers.shift),
                    Key::Home => session.move_to_line_edge(false, modifiers.shift),
                    Key::End => session.move_to_line_edge(true, modifiers.shift),
                    _ => {}
                },
                _ => {}
            }
        }
        if commit {
            self.commit_text();
        }
    }

    pub(crate) fn paint_text_session(&self, ctx: &Context, painter: &egui::Painter, canvas: Rect, hover: Option<Pos2>) {
        let (Some(rect), Some(session)) = (self.text_screen_rect(canvas), self.tab().text.as_ref()) else {
            return;
        };
        let scale = self.tab().view.scale();
        if let Some(layout) = &session.layout {
            if let Some(range) = session.selection() {
                for index in range {
                    let (from, to) = (layout.caret(index), layout.caret(index + 1));
                    let width = if to.top == from.top { to.x - from.x } else { NEWLINE_SELECTION_WIDTH };
                    let min = self.tab().view.image_to_screen(canvas, session.origin.x + from.x, session.origin.y + from.top);
                    painter.rect_filled(Rect::from_min_size(min, vec2(width * scale, from.height * scale)), 0.0, skin::theme().highlight.gamma_multiply(SELECTION_ALPHA));
                }
            }
            let stop = layout.caret(session.caret);
            let top = self.tab().view.image_to_screen(canvas, session.origin.x + stop.x, session.origin.y + stop.top);
            let blink_on = ctx.input(|input| (input.time / CARET_BLINK.as_secs_f64()) as u64 % 2 == 0);
            if blink_on {
                let caret = Rect::from_min_size(top, vec2(CARET_WIDTH, stop.height * scale));
                painter.rect_filled(caret, 0.0, egui::Color32::BLACK);
                painter.rect_stroke(caret.expand(CARET_WIDTH / 2.0), 0.0, Stroke::new(CARET_WIDTH, egui::Color32::WHITE), egui::StrokeKind::Outside);
            }
        }
        let grip = ScreenBox::from_rect(rect);
        let hot = matches!(self.tab().gesture, crate::canvas::Gesture::TextMove { .. }) || hover.is_some_and(|pos| grip.hits_move_icon(pos));
        paint_move_icon(painter, &self.skin, grip.move_icon_center(), hot);
        ctx.request_repaint_after(CARET_BLINK);
    }
}
