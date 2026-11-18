use std::sync::Arc;
use std::time::{Duration, Instant};

use chequer_core::document::ActiveStroke;
use chequer_core::draw::{StrokeKind, StrokeParams, Tilt};
use chequer_core::geom::{FPoint, FRect, IPoint, IRect};
use chequer_core::history::HistoryKind;
use chequer_core::raster::Rgba;
use chequer_core::selection::{CombineMode, SharedSelection};
use chequer_core::shapes::{ShapeKind, ShapeStyle, StrokeAlign, render_in_frame, render_line};
use chequer_core::spline::{Cubic, LineMode, line_segments, nearest_on};
use chequer_core::transform::{Placement, Quad, Sides};
use eframe::egui::{
    self, Color32, Context, CursorIcon, Event, Modifiers, MouseWheelUnit, Order, PointerButton, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2,
};

use crate::tool_registry::{ToolButton, ToolGesture};
use crate::ants::Ants;
use crate::icons::{ICON_SIZE, Icon};
use crate::handles::{Frame, Handle, ROTATE_ZONE_WIDTH, RotationDrag, ScreenBox, add_box_outline, add_handle_ring, add_pivot, near_handle, paint_move_icon, paint_rotate_cursor};
use crate::live::KnobTarget;
use crate::selection_ui::{GizmoCursor, GizmoHit, SelectionOrigin, WarpTarget};
use crate::app::{App, Command, PanelId};
use crate::device::Haptic;
use crate::touch::TAP_MAX_TRAVEL;
use crate::dialogs::Dialog;
use crate::skin;
use crate::tools::{FillStyle, Sampling, Tool};
use chequer_core::sample::{average_area, square_around};
use crate::touch::TouchAction;
use crate::trackpad::{GestureEvent, SwipeDirection, TrackpadMode};
use crate::view::{DocTab, Fit};

const LINE_HIT_DISTANCE: f32 = 6.0;
const MIN_LINE_POINTS: usize = 2;
const POINT_DOUBLE_PRESS: Duration = Duration::from_millis(400);
const GRID_MIN_ZOOM: f32 = 4.0;
const ACTUAL_SIZE_ZOOM: f32 = 1.0;
const EXPORT_FLASH_SECONDS: f32 = 0.6;
const ZOOM_DRAG_MIN: f32 = 6.0;
const WHEEL_LINE_POINTS: f32 = 72.0;
const WHEEL_PAGE_POINTS: f32 = 480.0;
const WHEEL_PAGE_LINES: f32 = 3.0;
const TOUCHPAD_POINTS_PER_ZOOM_STEP: f32 = 40.0;
const PINCH_LOG_PER_ZOOM_STEP: f32 = 0.18;
const LONG_PRESS_POLL: std::time::Duration = std::time::Duration::from_millis(100);
const LASSO_MIN_STEP: f32 = 0.5;
const ELLIPSE_SEGMENTS: usize = 96;
const MIN_LASSO_POINTS: usize = 3;
const DOCUMENT_SHADOW_OFFSET: f32 = 3.0;
const DOCUMENT_SHADOW_ALPHA: u8 = 60;
const ANGLE_SNAP_DEGREES: f32 = 45.0;
const MIN_SHAPE_DRAG: f32 = 1.0;
const CURSOR_RING_MIN_DIAMETER: f32 = 6.0;
const CURSOR_CROSS_ARM: f32 = 4.0;
const CURSOR_RING_SEGMENT: f32 = 3.0;
const CURSOR_RING_MIN_SEGMENTS: usize = 16;
const MAX_TILE_COPIES: i32 = 1024;
const PICKER_CURSOR_SIDE: f32 = 32.0;
const TOOL_BADGE_SIDE: f32 = 16.0;
const TOOL_BADGE_OFFSET: Vec2 = vec2(10.0, 10.0);
const PICK_AREA_MIN_SIDE: f32 = 8.0;
const CROSSHAIR_ARM: f32 = 9.0;
const CLONE_SOURCE_ARM: f32 = 7.0;
const CLONE_ORIGIN_CENTER: f32 = 0.5;
const PICKER_TIP: (f32, f32) = (1.5, 14.5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    First,
    Second,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nub {
    pub x: Option<Axis>,
    pub y: Option<Axis>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeGeometry {
    Line,
    Boxed(ShapeKind),
}

#[derive(Clone, Debug)]
pub struct ShapeSession {
    pub geometry: ShapeGeometry,
    pub p0: FPoint,
    pub p1: FPoint,
    pub points: Vec<FPoint>,
    pub line_mode: LineMode,
    pub last_point_press: Option<(usize, Instant)>,
    pub style: ShapeStyle,
    pub blend: chequer_core::blend::BlendMode,
    pub angle: f32,
    pub secondary: bool,
}

fn shape_history_kind(geometry: ShapeGeometry) -> HistoryKind {
    match geometry {
        ShapeGeometry::Line => HistoryKind::Line,
        ShapeGeometry::Boxed(ShapeKind::Rectangle) => HistoryKind::Rectangle,
        ShapeGeometry::Boxed(ShapeKind::RoundedRectangle { .. }) => HistoryKind::RoundedRectangle,
        ShapeGeometry::Boxed(ShapeKind::Ellipse) => HistoryKind::Ellipse,
        ShapeGeometry::Boxed(_) => HistoryKind::Shape,
    }
}

#[derive(Clone, Copy)]
struct ShapeOptions {
    fill_style: FillStyle,
    shape_kind: ShapeKind,
    line_mode: LineMode,
    blend: chequer_core::blend::BlendMode,
    size: f32,
    antialias: bool,
    primary: Rgba,
    secondary: Rgba,
}

impl ShapeOptions {
    fn capture(options: &crate::tools::ToolOptions, colors: &crate::app::ColorState) -> Self {
        Self {
            fill_style: options.fill_style,
            shape_kind: options.shape_kind,
            line_mode: options.line_mode,
            blend: options.blend,
            size: options.size,
            antialias: options.antialias,
            primary: colors.primary,
            secondary: colors.secondary,
        }
    }

    fn restore(self, options: &mut crate::tools::ToolOptions, colors: &mut crate::app::ColorState) {
        options.fill_style = self.fill_style;
        options.shape_kind = self.shape_kind;
        options.line_mode = self.line_mode;
        options.blend = self.blend;
        options.size = self.size;
        options.antialias = self.antialias;
        colors.primary = self.primary;
        colors.secondary = self.secondary;
    }
}

pub struct CommittedShape {
    session: ShapeSession,
    options: ShapeOptions,
    history_cursor: usize,
}

impl ShapeSession {
    fn frame(&self) -> FRect {
        if self.style.antialias || self.angle != 0.0 { FRect::from_corners(self.p0, self.p1) } else { IRect::from_corners(self.p0.floor(), self.p1.floor()).into() }
    }

    fn snapped_points(&self) -> Vec<FPoint> {
        if self.style.antialias { self.points.clone() } else { self.points.iter().map(|p| point_center(p.floor())).collect() }
    }

    fn segments(&self) -> Vec<Cubic> {
        line_segments(&self.snapped_points(), self.line_mode)
    }

    fn handle_indices(&self) -> Vec<usize> {
        match self.line_mode {
            LineMode::Straight => vec![0, self.points.len().saturating_sub(1)],
            LineMode::Smooth => (0..self.points.len()).collect(),
        }
    }

    fn neighbour_of(&self, index: usize) -> FPoint {
        let handles = self.handle_indices();
        let position = handles.iter().position(|&handle| handle == index).unwrap_or(0);
        let neighbour = if position > 0 { handles[position - 1] } else { handles[1.min(handles.len() - 1)] };
        self.points[neighbour]
    }

    fn start_corner(&self) -> FPoint {
        self.points.first().copied().unwrap_or(self.p0)
    }

    fn set_end_corner(&mut self, end: FPoint) {
        match self.points.last_mut() {
            Some(last) => *last = end,
            None => self.p1 = end,
        }
    }

    fn is_curve(&self) -> bool {
        self.geometry == ShapeGeometry::Line && self.line_mode == LineMode::Smooth
    }

    fn extend_nearest_end(&mut self, at: FPoint) -> usize {
        let (first, last) = (self.points[0], self.points[self.points.len() - 1]);
        if at.distance(first) < at.distance(last) {
            self.points.insert(0, at);
            0
        } else {
            self.points.push(at);
            self.points.len() - 1
        }
    }

    fn press_point(&mut self, index: usize) -> bool {
        if self.line_mode != LineMode::Smooth || self.points.len() <= MIN_LINE_POINTS {
            return false;
        }
        let double = self.last_point_press.is_some_and(|(last, at)| last == index && at.elapsed() < POINT_DOUBLE_PRESS);
        self.last_point_press = if double { None } else { Some((index, Instant::now())) };
        double
    }

    fn normalize(&mut self) {
        if self.geometry == ShapeGeometry::Line {
            return;
        }
        let frame = FRect::from_corners(self.p0, self.p1);
        self.p0 = FPoint::new(frame.x0, frame.y0);
        self.p1 = FPoint::new(frame.x1, frame.y1);
    }

    fn resize_rotated(&mut self, nub: Nub, pointer: FPoint, proportional: bool) {
        let frame = FRect::from_corners(self.p0, self.p1);
        let start = Placement { center: FPoint::new((frame.x0 + frame.x1) / 2.0, (frame.y0 + frame.y1) / 2.0), width: frame.width(), height: frame.height(), angle: self.angle };
        let side = |axis: Option<Axis>| match axis {
            Some(Axis::First) => -1,
            Some(Axis::Second) => 1,
            None => 0,
        };
        let resized = start.dragged(Sides { x: side(nub.x), y: side(nub.y) }, pointer, proportional);
        let half = FPoint::new(resized.width / 2.0, resized.height / 2.0);
        self.p0 = FPoint::new(resized.center.x - half.x, resized.center.y - half.y);
        self.p1 = FPoint::new(resized.center.x + half.x, resized.center.y + half.y);
    }

    fn screen_box(&self, tab: &DocTab, canvas: Rect) -> ScreenBox {
        let frame = self.frame();
        let center = tab.view.image_to_screen(canvas, (frame.x0 + frame.x1) / 2.0, (frame.y0 + frame.y1) / 2.0);
        ScreenBox { center, half: vec2(frame.width(), frame.height()) * tab.view.scale() / 2.0, angle: self.angle }
    }

    fn apply_nub(&mut self, nub: Nub, point: FPoint) {
        match nub.x {
            Some(Axis::First) => self.p0.x = point.x,
            Some(Axis::Second) => self.p1.x = point.x,
            None => {}
        }
        match nub.y {
            Some(Axis::First) => self.p0.y = point.y,
            Some(Axis::Second) => self.p1.y = point.y,
            None => {}
        }
    }

    fn is_click(&self) -> bool {
        if self.points.len() > MIN_LINE_POINTS {
            return false;
        }
        let (start, end) = (self.start_corner(), self.points.last().copied().unwrap_or(self.p1));
        if self.style.antialias { start.distance(end) < MIN_SHAPE_DRAG } else { start.floor() == end.floor() }
    }
}

pub enum ShapeOp {
    Create,
    Resize(Nub),
    Rotate(RotationDrag),
    Move { anchor: FPoint, p0: FPoint, p1: FPoint },
    MovePoint(usize),
    MovePath { anchor: FPoint, points: Vec<FPoint> },
    AddPoint { after: usize, at: FPoint },
    Extend(FPoint),
}

pub enum Gesture {
    None,
    Pan { last: Pos2 },
    Stroke { stroke: ActiveStroke },
    Marquee { ellipse: bool, start: FPoint, current: FPoint, mode: CombineMode, square: bool },
    Lasso { points: Vec<FPoint>, mode: CombineMode },
    MoveSelection { before: Option<SharedSelection>, anchor: FPoint, applied: IPoint },
    MovePixels { anchor: FPoint, applied: IPoint },
    ZoomDrag { start: Pos2, current: Pos2, zoom_out: bool },
    Shape(ShapeOp),
    TextMove { anchor: FPoint, start: FPoint },
    TextSelect,
    ResizeSelection { handle: Handle, start: Frame, origin: SelectionOrigin, current: SelectionOrigin, before: Option<SharedSelection> },
    ScalePixels { handle: Handle, start: Placement },
    WarpPixels { target: WarpTarget, start: Quad, anchor: FPoint },
    RotatePixels { start: Placement, drag: RotationDrag },
    RotateWarp { start: Quad, drag: RotationDrag },
    MovePivot,
    Knob(KnobTarget),
    Pick { button: PointerButton },
    Plugin { gesture: Box<dyn ToolGesture>, button: ToolButton },
}

pub(crate) fn move_offset(anchor: FPoint, pointer: FPoint, shift: bool) -> IPoint {
    let (dx, dy) = (pointer.x - anchor.x, pointer.y - anchor.y);
    let (dx, dy) = match (shift, dx.abs() >= dy.abs()) {
        (false, _) => (dx, dy),
        (true, true) => (dx, 0.0),
        (true, false) => (0.0, dy),
    };
    IPoint::new(dx.round() as i32, dy.round() as i32)
}

pub(crate) fn clamp_point(p: FPoint, tab: &DocTab) -> FPoint {
    FPoint::new(p.x.clamp(0.0, tab.doc.width() as f32), p.y.clamp(0.0, tab.doc.height() as f32))
}

pub(crate) fn clamp_pixel(p: FPoint, tab: &DocTab) -> FPoint {
    let max_x = (tab.doc.width() - 1) as f32 + 0.5;
    let max_y = (tab.doc.height() - 1) as f32 + 0.5;
    FPoint::new(p.x.clamp(0.0, max_x), p.y.clamp(0.0, max_y))
}

fn add_cross(ants: &mut Ants, pos: Pos2, arm: f32) {
    ants.solid(&[pos - vec2(arm, 0.0), pos + vec2(arm, 0.0)]);
    ants.solid(&[pos - vec2(0.0, arm), pos + vec2(0.0, arm)]);
}

fn is_alt_pick(tool: Tool, button: PointerButton, mods: Modifiers) -> bool {
    mods.alt && !mods.ctrl && matches!(button, PointerButton::Primary | PointerButton::Secondary) && !tool.is_selection_tool()
}

pub(crate) fn selection_mode(base: CombineMode, button: PointerButton, mods: Modifiers) -> CombineMode {
    match (button, mods.ctrl, mods.alt) {
        (PointerButton::Primary, true, false) => CombineMode::Add,
        (PointerButton::Primary, false, true) => CombineMode::Subtract,
        (PointerButton::Secondary, true, false) => CombineMode::Xor,
        (PointerButton::Secondary, false, true) => CombineMode::Intersect,
        _ => base,
    }
}

fn stroke_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos() as u64)
}

#[derive(Clone, Copy)]
pub(crate) struct DeferredPress {
    pos: Pos2,
    button: PointerButton,
    modifiers: Modifiers,
}

impl App {
    pub(crate) fn show_canvas(&mut self, ui: &mut Ui, ctx: &Context, canvas: Rect) {
        self.workspace = canvas;
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, 0.0, skin::theme().workspace);

        self.tab_mut().view.ppp = ctx.pixels_per_point();
        if self.tab().view.needs_initial_fit {
            let tab = self.tab_mut();
            let (w, h) = (tab.doc.width(), tab.doc.height());
            tab.view.fit(canvas, w, h, Fit::Initial);
        }
        self.handle_touch(ctx, canvas);
        self.handle_wheel(ctx, canvas);
        let tab = self.tab_mut();
        let (width, height) = (tab.doc.width(), tab.doc.height());
        tab.view.constrain(canvas, width, height);
        let hovered_canvas = self.pointer_on_canvas(ctx, canvas);
        self.process_pointer_events(ctx, canvas);
        let hover = self.hover_position(ctx).filter(|_| hovered_canvas);
        self.hover_pixel = hover.map(|p| self.wrapped_into_image(self.tab().view.screen_to_image(canvas, p)).floor());

        self.refresh_live();
        self.refresh_shape_style();
        self.refresh_text_preview();
        self.refresh_floating_resample();
        let zoom_out_filter = self.settings.zoom_out_filter;
        let onion = self.timeline.onion && !self.timeline.playing && self.panels.timeline.open;
        let onion_settings = self.settings.animation.onion;
        let tab = self.tab_mut();
        tab.sync_texture(ctx, zoom_out_filter);
        tab.sync_onion(ctx, onion, onion_settings);
        let image_rect = tab.view.image_rect(canvas, tab.doc.width(), tab.doc.height());
        self.image_rect = image_rect;
        if matches!(self.dialog, Some(Dialog::Welcome)) && self.is_blank_placeholder() {
            return;
        }
        self.paint_document(&painter, canvas, image_rect);
        self.paint_export_flash(ctx, &painter, image_rect);
        self.paint_overlays(ctx, &painter, canvas, hover);
        self.set_cursor(ctx, canvas, hovered_canvas, hover);
        let _ = ui.interact(canvas, egui::Id::unique("canvas-area"), Sense::hover());
    }

    fn pointer_on_canvas(&self, ctx: &Context, canvas: Rect) -> bool {
        let Some(pos) = self.hover_position(ctx) else {
            return false;
        };
        canvas.contains(pos) && self.is_background_at(ctx, pos)
    }

    fn hover_position(&self, ctx: &Context) -> Option<Pos2> {
        match crate::pen_feed::hover_point() {
            Some((x, y)) => Some(pos2(x, y) / ctx.pixels_per_point()),
            None => ctx.pointer_hover_pos(),
        }
    }

    fn panel_is_displayed(&self, id: PanelId) -> bool {
        match id {
            PanelId::Colors => !self.settings.compact_colors,
            PanelId::Swatches => !self.settings.palette_below,
            _ => true,
        }
    }

    fn is_background_at(&self, ctx: &Context, pos: Pos2) -> bool {
        let over_panel = PanelId::ALL
            .into_iter()
            .filter(|&id| self.panel_is_displayed(id))
            .filter_map(|id| self.panels.get(id))
            .any(|panel| panel.open && panel.hit_rect().contains(pos));
        let over_effect = self
            .effect
            .as_ref()
            .is_some_and(|effect| effect.panel_rect().contains(pos));
        !over_panel
            && !over_effect
            && ctx
                .layer_id_at(pos)
                .is_none_or(|layer| matches!(layer.order, Order::Background | Order::Middle))
    }

    fn handle_wheel(&mut self, ctx: &Context, canvas: Rect) {
        if !self.pointer_on_canvas(ctx, canvas) {
            self.zoom_notches = 0.0;
            return;
        }
        let pointer = ctx.pointer_hover_pos().unwrap_or(canvas.center());
        let events = ctx.input(|i| i.events.clone());
        let mut scroll = vec2(0.0, 0.0);
        let mut pinch_factors = Vec::new();
        for event in events {
            match event {
                Event::MouseWheel { unit, delta, modifiers, .. } => {
                    if modifiers.ctrl || modifiers.command {
                        self.zoom_notches += match unit {
                            MouseWheelUnit::Line => delta.y,
                            MouseWheelUnit::Page => delta.y * WHEEL_PAGE_LINES,
                            MouseWheelUnit::Point => delta.y / TOUCHPAD_POINTS_PER_ZOOM_STEP,
                        };
                        continue;
                    }
                    let points = match unit {
                        MouseWheelUnit::Line => delta * WHEEL_LINE_POINTS,
                        MouseWheelUnit::Page => delta * WHEEL_PAGE_POINTS,
                        MouseWheelUnit::Point => delta,
                    };
                    scroll += if modifiers.shift { vec2(points.x + points.y, 0.0) } else { points };
                }
                Event::Zoom(factor) if self.settings.trackpad.mode == TrackpadMode::Auto => pinch_factors.push(factor),
                Event::Zoom(factor) => self.zoom_notches += factor.ln() / PINCH_LOG_PER_ZOOM_STEP,
                _ => {}
            }
        }
        let tab = self.tab_mut();
        tab.view.origin += scroll;
        for factor in pinch_factors {
            self.pinch_zoom(canvas, pointer, factor);
        }
        if self.zoom_notches.abs() >= 1.0 {
            let zoom_in = self.zoom_notches > 0.0;
            self.zoom_notches = 0.0;
            self.tab_mut().view.step_zoom(canvas, pointer, zoom_in);
        }
        let tab = self.tab_mut();
        let (width, height) = (tab.doc.width(), tab.doc.height());
        tab.view.constrain(canvas, width, height);
    }

    fn handle_touch(&mut self, ctx: &Context, canvas: Rect) {
        let (events, time) = ctx.input(|i| (i.events.clone(), i.time));
        self.stylus.observe(&events);
        let events = self.stylus.fingers(&events);
        let mut touch = std::mem::take(&mut self.touch);
        touch.set_single_finger_pans(self.stylus.fingers_blocked());
        let actions = touch.update(&events, time, |pos| canvas.contains(pos) && self.is_background_at(ctx, pos));
        self.touch = touch;
        if self.touch.is_holding() {
            ctx.request_repaint_after(LONG_PRESS_POLL);
        }
        if self.dialog.is_some() {
            return;
        }
        for action in actions {
            match action {
                TouchAction::AbortDrawing => self.abort_drawing(canvas),
                TouchAction::Transform { anchor, zoom, pan } => {
                    let tab = self.tab_mut();
                    let before = tab.view.zoom;
                    let target = before * zoom;
                    tab.view.zoom_at(canvas, anchor, target);
                    if zoom_crossed_stop(before, tab.view.zoom, target) {
                        crate::device::vibrate(Haptic::Click);
                    }
                    tab.view.origin += pan;
                    let (width, height) = (tab.doc.width(), tab.doc.height());
                    tab.view.constrain(canvas, width, height);
                }
                TouchAction::LongPress { pos } => {
                    if !self.stylus.fingers_blocked() {
                        self.abort_drawing(canvas);
                    }
                    crate::device::vibrate(Haptic::LongPress);
                    let image_pos = self.tab().view.screen_to_image(canvas, pos);
                    self.pick_color(image_pos, PointerButton::Primary);
                }
                TouchAction::LongPressDrag { pos } => {
                    let image_pos = self.tab().view.screen_to_image(canvas, pos);
                    self.pick_color(image_pos, PointerButton::Primary);
                }
                TouchAction::Tap { fingers: 2 } => {
                    crate::device::vibrate(Haptic::Tick);
                    self.run(ctx, Command::Undo);
                }
                TouchAction::Tap { fingers: 3 } => {
                    crate::device::vibrate(Haptic::Tick);
                    self.run(ctx, Command::Redo);
                }
                TouchAction::Tap { .. } => self.run(ctx, Command::ZoomWindow),
            }
        }
    }

    fn abort_drawing(&mut self, canvas: Rect) {
        self.deferred_press = None;
        match self.tab().gesture {
            Gesture::None => {}
            Gesture::Stroke { .. } => self.tab_mut().cancel_stroke(),
            _ => self.pointer_released(canvas, canvas.center(), Modifiers::NONE),
        }
    }

    fn pinch_zoom(&mut self, canvas: Rect, anchor: Pos2, factor: f32) {
        let factor = factor.max(f32::EPSILON).powf(self.settings.trackpad.pinch_sensitivity);
        let tab = self.tab_mut();
        let zoom = tab.view.zoom * factor;
        tab.view.zoom_at(canvas, anchor, zoom);
    }

    pub(crate) fn apply_gestures(&mut self, ctx: &Context) {
        let events = self.trackpad.drain();
        if events.is_empty() || self.dialog.is_some() || self.tabs.is_empty() {
            return;
        }
        let canvas = self.workspace;
        let pointer = ctx.pointer_hover_pos().filter(|pos| canvas.contains(*pos)).unwrap_or(canvas.center());
        let settings = self.settings.trackpad;
        for event in events {
            match event {
                GestureEvent::Pinch { factor } => self.pinch_zoom(canvas, pointer, factor),
                GestureEvent::Pan { delta } => {
                    let tab = self.tab_mut();
                    tab.view.origin += delta;
                    let (width, height) = (tab.doc.width(), tab.doc.height());
                    tab.view.constrain(canvas, width, height);
                }
                GestureEvent::Rotate { radians } if settings.rotate => self.rotate_floating_by(radians),
                GestureEvent::Swipe { direction, .. } if settings.swipes => {
                    self.run(ctx, match direction {
                        SwipeDirection::Left => Command::Undo,
                        SwipeDirection::Right => Command::Redo,
                        SwipeDirection::Up => Command::ZoomWindow,
                        SwipeDirection::Down => Command::ActualSize,
                    });
                }
                GestureEvent::Rotate { .. } | GestureEvent::Swipe { .. } => {}
            }
        }
    }

    fn rotate_floating_by(&mut self, radians: f32) {
        let (method, rotation) = (self.options.move_resample, self.options.rotation);
        let tab = self.tab_mut();
        if !matches!(tab.gesture, Gesture::None) {
            return;
        }
        let Some(floating) = tab.doc.floating() else {
            return;
        };
        let area = match floating.quad() {
            Some(quad) => tab.doc.warp_floating(quad.rotated_about(floating.pivot_point(), radians), method, rotation),
            None => {
                let mut placement = floating.placement();
                placement.angle += radians;
                tab.doc.place_floating(placement, method, rotation)
            }
        };
        tab.mark_dirty(area);
    }

    fn alt_picks(&self, ctx: &Context) -> bool {
        let mods = ctx.input(|i| i.modifiers);
        mods.alt && !mods.ctrl && !self.tool.is_selection_tool() && !self.space_down
    }

    fn set_cursor(&self, ctx: &Context, canvas: Rect, over_canvas: bool, hover: Option<Pos2>) {
        if !over_canvas {
            return;
        }
        match self.gizmo_cursor(canvas, hover) {
            Some(GizmoCursor::System(icon)) => return ctx.set_cursor_icon(icon),
            Some(GizmoCursor::Rotate) => return ctx.set_cursor_icon(CursorIcon::None),
            None => {}
        }
        if self.shape_rotate_cursor(canvas, hover) {
            return ctx.set_cursor_icon(CursorIcon::None);
        }
        let icon = match (&self.tab().gesture, self.tool) {
            (Gesture::Pick { .. }, _) => CursorIcon::None,
            _ if self.alt_picks(ctx) => CursorIcon::None,
            (Gesture::Pan { .. }, _) => CursorIcon::Grabbing,
            (_, Tool::Pan) => CursorIcon::Grab,
            (_, Tool::MovePixels | Tool::MoveSelection) => CursorIcon::Move,
            (_, Tool::Zoom) => CursorIcon::ZoomIn,
            _ if self.space_down => CursorIcon::Grab,
            (_, Tool::Brush | Tool::Eraser | Tool::Pencil) => CursorIcon::None,
            _ => CursorIcon::None,
        };
        ctx.set_cursor_icon(icon);
    }

    fn process_pointer_events(&mut self, ctx: &Context, canvas: Rect) {
        let fingers_blocked = self.stylus.fingers_blocked();
        if !fingers_blocked && self.touch.blocks_pointer() {
            return;
        }
        let effect_blocks_tools = self.effect.is_some() && self.tool != Tool::Pan;
        let events = self.stylus.pointer_events(ctx.input(|i| i.events.clone()));
        let mods = ctx.input(|i| i.modifiers);
        let mut latest_move: Option<Pos2> = None;
        for event in events {
            match event {
                Event::PointerMoved(_) if self.stroke_fed_by_pen() => {}
                Event::PointerMoved(pos) if self.gesture_follows_path() => self.pointer_moved(canvas, pos, mods),
                Event::PointerMoved(pos) => latest_move = Some(pos),
                Event::PointerButton { pos, button, pressed: true, modifiers } => {
                    let button = self.pen_button(button);
                    self.flush_pointer_move(canvas, &mut latest_move, mods);
                    let allowed = !effect_blocks_tools || self.pans_on_press(button);
                    if allowed && canvas.contains(pos) && self.is_background_at(ctx, pos) && matches!(self.tab().gesture, Gesture::None) {
                        if self.defers_press(canvas, pos) {
                            self.deferred_press = Some(DeferredPress { pos, button, modifiers });
                        } else {
                            self.pointer_pressed(canvas, pos, button, modifiers);
                        }
                    }
                }
                Event::PointerButton { pos, pressed: false, modifiers, .. } => {
                    self.flush_pointer_move(canvas, &mut latest_move, mods);
                    self.pointer_released(canvas, pos, modifiers);
                }
                _ => {}
            }
        }
        self.flush_pointer_move(canvas, &mut latest_move, mods);
        self.apply_pen_samples(ctx, canvas);
        let (any_down, latest) = ctx.input(|i| (if fingers_blocked { self.stylus.pen_is_down() } else { i.pointer.any_down() }, i.pointer.latest_pos()));
        if !any_down && (self.deferred_press.is_some() || !matches!(self.tab().gesture, Gesture::None)) {
            self.pointer_released(canvas, latest.unwrap_or(canvas.center()), mods);
        }
    }

    fn stroke_fed_by_pen(&self) -> bool {
        matches!(self.tab().gesture, Gesture::Stroke { .. }) && crate::pen_feed::is_live()
    }

    fn apply_pen_samples(&mut self, ctx: &Context, canvas: Rect) {
        let stroking = matches!(self.tab().gesture, Gesture::Stroke { .. });
        let pixels_per_point = ctx.pixels_per_point();
        for sample in crate::pen_feed::drain(stroking) {
            let image_pos = self.tab().view.screen_to_image(canvas, pos2(sample.x / pixels_per_point, sample.y / pixels_per_point));
            self.extend_stroke_with(image_pos, self.pen.shape_pressure(sample.pressure), sample.tilt, sample.time_ms);
        }
    }

    fn flush_pointer_move(&mut self, canvas: Rect, latest_move: &mut Option<Pos2>, mods: Modifiers) {
        if let Some(pos) = latest_move.take() {
            self.pointer_moved(canvas, pos, mods);
        }
    }

    fn gesture_follows_path(&self) -> bool {
        matches!(self.tab().gesture, Gesture::Stroke { .. } | Gesture::Lasso { .. })
    }

    fn pen_button(&self, button: PointerButton) -> PointerButton {
        if button == PointerButton::Primary && self.pen.barrel_pressed() { PointerButton::Secondary } else { button }
    }

    fn pans_on_press(&self, button: PointerButton) -> bool {
        button == PointerButton::Middle || (self.space_down && button == PointerButton::Primary)
    }

    fn pointer_pressed(&mut self, canvas: Rect, pos: Pos2, button: PointerButton, mods: Modifiers) {
        if matches!(button, PointerButton::Extra1 | PointerButton::Extra2) {
            return;
        }
        self.tab_mut().gesture = Gesture::None;
        self.tab_mut().doc.seal_floating_step();
        if self.pans_on_press(button) || self.tool == Tool::Pan {
            self.tab_mut().gesture = Gesture::Pan { last: pos };
            return;
        }
        if is_alt_pick(self.tool, button, mods) {
            self.pick_color(self.tab().view.screen_to_image(canvas, pos), button);
            self.tab_mut().gesture = Gesture::Pick { button };
            return;
        }
        if self.tool != Tool::Line && self.tool != Tool::Shapes {
            self.commit_shape();
        }
        if self.try_begin_knob(canvas, pos, button) || self.try_begin_handle(canvas, pos, button) || self.click_outside_selection(canvas, pos, button, mods) {
            return;
        }
        let image_pos = self.tab().view.screen_to_image(canvas, pos);
        let image_pos = if matches!(self.tool, Tool::Bucket | Tool::MagicWand) { self.wrapped_into_image(image_pos) } else { image_pos };
        match self.tool {
            Tool::Pencil | Tool::Brush | Tool::Eraser => self.begin_stroke(image_pos, button),
            Tool::Bucket => self.start_fill(image_pos, button, mods),
            Tool::Gradient => self.start_gradient(image_pos),
            Tool::Picker => self.pick_color(image_pos, button),
            Tool::MagicWand => self.start_wand(image_pos, button, mods),
            Tool::RectSelect | Tool::EllipseSelect => self.begin_marquee(image_pos, button, mods),
            Tool::Lasso => self.begin_lasso(image_pos, button, mods),
            Tool::MoveSelection => self.begin_move_selection(image_pos),
            Tool::MovePixels => self.begin_move_pixels(image_pos, button),
            Tool::Zoom => self.tab_mut().gesture = Gesture::ZoomDrag { start: pos, current: pos, zoom_out: button == PointerButton::Secondary },
            Tool::Line | Tool::Shapes => self.shape_pressed(canvas, pos, image_pos, button, mods),
            Tool::Text => self.text_pressed(canvas, pos, image_pos, button == PointerButton::Secondary),
            Tool::Plugin(id) => self.plugin_tool_pressed(id, image_pos, button, mods),
            Tool::CloneStamp => self.begin_clone(image_pos, button, mods),
            Tool::Recolor | Tool::Pan => {}
        }
    }

    fn defers_press(&self, canvas: Rect, pos: Pos2) -> bool {
        matches!(self.tool, Tool::Bucket | Tool::Gradient) && self.touch.is_active() && self.knob_at(canvas, pos).is_none()
    }

    fn begin_deferred_press(&mut self, canvas: Rect) {
        if let Some(press) = self.deferred_press.take() {
            self.pointer_pressed(canvas, press.pos, press.button, press.modifiers);
        }
    }

    fn pointer_moved(&mut self, canvas: Rect, pos: Pos2, mods: Modifiers) {
        if self.deferred_press.is_some_and(|press| press.pos.distance(pos) > TAP_MAX_TRAVEL) {
            self.begin_deferred_press(canvas);
        }
        let image_pos = self.tab().view.screen_to_image(canvas, pos);
        let tab = self.tab_mut();
        match &mut tab.gesture {
            Gesture::None => {}
            Gesture::Pan { last } => {
                let delta = pos - *last;
                *last = pos;
                tab.view.origin += delta;
                let (width, height) = (tab.doc.width(), tab.doc.height());
                tab.view.constrain(canvas, width, height);
            }
            Gesture::Stroke { .. } => self.extend_stroke(image_pos),
            Gesture::Marquee { current, square, .. } => {
                *current = image_pos;
                *square = mods.shift;
            }
            Gesture::Lasso { points, .. } => {
                let last = points.last().copied();
                if last.is_none_or(|l| l.distance(image_pos) >= LASSO_MIN_STEP) {
                    points.push(image_pos);
                }
            }
            Gesture::MoveSelection { anchor, applied, .. } => {
                let target = move_offset(*anchor, image_pos, mods.shift);
                let (dx, dy) = (target.x - applied.x, target.y - applied.y);
                *applied = target;
                if dx != 0 || dy != 0 {
                    tab.doc.move_selection(dx, dy);
                }
            }
            Gesture::MovePixels { anchor, applied } => {
                let target = move_offset(*anchor, image_pos, mods.shift);
                let (dx, dy) = (target.x - applied.x, target.y - applied.y);
                *applied = target;
                if dx != 0 || dy != 0 {
                    let area = tab.doc.move_floating(dx, dy);
                    tab.mark_dirty(area);
                }
            }
            Gesture::ZoomDrag { current, .. } => *current = pos,
            Gesture::TextMove { anchor, start } => {
                let (anchor, start) = (*anchor, *start);
                self.text_moved(anchor, start, image_pos, mods.shift);
            }
            Gesture::TextSelect => self.text_selected(image_pos),
            Gesture::Shape(_) => self.shape_dragged(image_pos, mods),
            Gesture::ResizeSelection { .. } | Gesture::ScalePixels { .. } | Gesture::RotatePixels { .. } | Gesture::RotateWarp { .. } | Gesture::WarpPixels { .. } | Gesture::MovePivot => self.handle_dragged(image_pos, mods),
            Gesture::Knob(target) => {
                let target = *target;
                self.knob_dragged(target, image_pos, mods);
            }
            Gesture::Pick { button } => {
                let button = *button;
                self.pick_color(image_pos, button);
            }
            Gesture::Plugin { .. } => self.plugin_tool_moved(image_pos, mods),
        }
    }

    fn pointer_released(&mut self, canvas: Rect, pos: Pos2, mods: Modifiers) {
        self.begin_deferred_press(canvas);
        let gesture = std::mem::replace(&mut self.tab_mut().gesture, Gesture::None);
        let image_pos = self.tab().view.screen_to_image(canvas, pos);
        match gesture {
            Gesture::Stroke { mut stroke } => {
                let tab = self.tab_mut();
                let dirty = tab.doc.flush_stroke(&mut stroke, image_pos);
                tab.mark_dirty(dirty);
                tab.doc.finish_stroke(stroke);
            }
            Gesture::Marquee { ellipse, start, current, mode, square } => {
                self.finish_marquee(ellipse, start, current, mode, square);
            }
            Gesture::Lasso { points, mode } => self.finish_lasso(points, mode),
            Gesture::MoveSelection { before, .. } => self.tab_mut().doc.finish_selection_move(before),
            Gesture::ZoomDrag { start, current, zoom_out } => self.finish_zoom(canvas, start, current.max(pos), zoom_out),
            Gesture::Shape(_) => self.shape_released(),
            Gesture::ResizeSelection { .. } => self.handle_released(gesture),
            Gesture::Plugin { gesture, button } => self.plugin_tool_released(gesture, button, image_pos, mods),
            Gesture::ScalePixels { .. } | Gesture::RotatePixels { .. } | Gesture::RotateWarp { .. } | Gesture::WarpPixels { .. } => self.finish_scale_pixels(),
            Gesture::None | Gesture::Pan { .. } | Gesture::MovePixels { .. } | Gesture::MovePivot | Gesture::Knob(_) | Gesture::Pick { .. } | Gesture::TextMove { .. } | Gesture::TextSelect => {}
        }
    }

    fn button_color(&self, button: PointerButton) -> Rgba {
        if button == PointerButton::Secondary { self.colors.secondary } else { self.colors.primary }
    }

    fn begin_clone(&mut self, image_pos: FPoint, button: PointerButton, mods: Modifiers) {
        let source = self.wrapped_into_image(image_pos).floor();
        if mods.ctrl && button == PointerButton::Primary {
            self.tab_mut().clone_origin = Some(source);
            return;
        }
        let Some(origin) = self.tab().clone_origin else {
            return;
        };
        let start = image_pos.floor();
        self.tab_mut().clone_offset = IPoint::new(origin.x - start.x, origin.y - start.y);
        self.begin_stroke(image_pos, button);
    }

    fn clone_source_marker(&self, canvas: Rect, hover: Option<Pos2>) -> Option<Pos2> {
        let tab = self.tab();
        let source = if matches!(tab.gesture, Gesture::Stroke { .. }) {
            let cursor = tab.view.screen_to_image(canvas, hover?);
            FPoint::new(cursor.x + tab.clone_offset.x as f32, cursor.y + tab.clone_offset.y as f32)
        } else {
            let origin = tab.clone_origin?;
            FPoint::new(origin.x as f32 + CLONE_ORIGIN_CENTER, origin.y as f32 + CLONE_ORIGIN_CENTER)
        };
        Some(tab.view.image_to_screen(canvas, source.x, source.y))
    }

    fn begin_stroke(&mut self, image_pos: FPoint, button: PointerButton) {
        let (kind, history, size, antialias) = match self.tool {
            Tool::CloneStamp => (StrokeKind::CloneFrom(self.tab().clone_offset), HistoryKind::CloneStamp, self.options.size, self.options.antialias),
            Tool::Pencil => (StrokeKind::Pencil, HistoryKind::Pencil, 1.0, false),
            Tool::Eraser => (StrokeKind::Eraser, HistoryKind::Eraser, self.options.size, self.options.antialias),
            _ => (StrokeKind::Brush, HistoryKind::Brush, self.options.size, self.options.antialias),
        };
        let params = StrokeParams {
            kind,
            color: self.button_color(button),
            size,
            antialias,
            blend: self.options.blend,
            pixel_perfect: self.options.pixel_perfect,
            smooth: self.options.smooth_path,
            wrap: self.tab().view.tiled,
            brush: Arc::clone(self.options.brush_for(self.tool)),
            seed: stroke_seed(),
            pressure: self.pen.pressure(),
            tilt: self.pen.tilt(),
        };
        let tab = self.tab_mut();
        if let Some((stroke, dirty)) = tab.doc.begin_stroke(params, image_pos, history) {
            tab.mark_dirty(dirty);
            tab.gesture = Gesture::Stroke { stroke };
        }
    }

    fn extend_stroke(&mut self, image_pos: FPoint) {
        self.extend_stroke_with(image_pos, self.pen.pressure(), self.pen.tilt(), crate::input_clock::now_ms());
    }

    fn extend_stroke_with(&mut self, image_pos: FPoint, pressure: f32, tilt: Option<Tilt>, time_ms: f64) {
        let tab = self.tab_mut();
        if let Gesture::Stroke { stroke } = &mut tab.gesture {
            stroke.set_pressure(pressure);
            stroke.set_tilt(tilt);
            stroke.set_time_ms(time_ms);
            let dirty = tab.doc.extend_stroke(stroke, image_pos);
            tab.mark_dirty(dirty);
        }
    }

    fn wrapped_into_image(&self, p: FPoint) -> FPoint {
        let tab = self.tab();
        if !tab.view.tiled {
            return p;
        }
        FPoint::new(p.x.rem_euclid(tab.doc.width() as f32), p.y.rem_euclid(tab.doc.height() as f32))
    }

    fn pick_color(&mut self, image_pos: FPoint, button: PointerButton) {
        let area = square_around(self.wrapped_into_image(image_pos).floor(), self.options.pick_size);
        let (space, alpha) = (self.options.pick_space, self.options.pick_alpha);
        let tab = self.tab();
        let color = match self.options.sampling {
            Sampling::Layer => average_area(tab.doc.active_raster(), area, space, alpha),
            Sampling::Image => {
                let visible = area.intersect(tab.doc.bounds());
                let flat = tab.doc.flattened(visible);
                average_area(&flat, flat.bounds(), space, alpha).filter(|_| !visible.is_empty())
            }
        };
        if let Some(color) = color {
            if button == PointerButton::Secondary {
                self.colors.secondary = color;
            } else {
                self.colors.primary = color;
            }
        }
    }

    fn finish_zoom(&mut self, canvas: Rect, start: Pos2, end: Pos2, zoom_out: bool) {
        let drag = Rect::from_two_pos(start, end);
        let tab = self.tab_mut();
        if drag.width() < ZOOM_DRAG_MIN && drag.height() < ZOOM_DRAG_MIN {
            tab.view.step_zoom(canvas, start, !zoom_out);
            return;
        }
        let top_left = tab.view.screen_to_image(canvas, drag.min);
        let scale = tab.view.scale();
        let fit_scale = (canvas.width() / (drag.width() / scale)).min(canvas.height() / (drag.height() / scale));
        tab.view.zoom = (fit_scale * tab.view.ppp).clamp(crate::view::ZOOM_MIN, crate::view::ZOOM_MAX);
        tab.view.origin = vec2(-top_left.x * tab.view.scale(), -top_left.y * tab.view.scale());
    }

    pub(crate) fn commit_shape(&mut self) {
        let options = ShapeOptions::capture(&self.options, &self.colors);
        let tab = &mut self.tabs[self.active];
        let Some(session) = tab.shape.take() else {
            return;
        };
        let kind = shape_history_kind(session.geometry);
        let cursor_before = tab.doc.history().cursor();
        let area = tab.doc.commit_preview(kind);
        tab.mark_dirty(area);
        let history_cursor = tab.doc.history().cursor();
        if history_cursor > cursor_before {
            tab.committed_shape = Some(CommittedShape { session, options, history_cursor });
        }
    }

    pub(crate) fn reopen_committed_shape(&mut self) -> bool {
        let tab = self.tab_mut();
        let history = tab.doc.history();
        let is_last_entry = tab.committed_shape.as_ref().is_some_and(|committed| {
            committed.history_cursor == history.cursor() && history.kinds().nth(history.cursor().wrapping_sub(1)) == Some(shape_history_kind(committed.session.geometry))
        });
        if !is_last_entry {
            return false;
        }
        let Some(CommittedShape { mut session, options, .. }) = tab.committed_shape.take() else {
            return false;
        };
        let area = tab.doc.undo();
        tab.mark_dirty(area);
        session.last_point_press = None;
        let tool = if session.geometry == ShapeGeometry::Line { Tool::Line } else { Tool::Shapes };
        options.restore(&mut self.options, &mut self.colors);
        self.set_tool(tool);
        self.tab_mut().shape = Some(session);
        self.refresh_shape_preview();
        true
    }

    fn cancel_shape(&mut self) {
        let tab = self.tab_mut();
        tab.shape = None;
        let area = tab.doc.set_preview(None);
        tab.mark_dirty(area);
    }

    fn refresh_shape_preview(&mut self) {
        let tab = self.tab_mut();
        let canvas = tab.doc.overlay_canvas();
        let overlay = tab.shape.as_ref().and_then(|s| match s.geometry {
            ShapeGeometry::Line => render_line(&s.segments(), &s.style, s.blend, canvas),
            ShapeGeometry::Boxed(kind) => render_in_frame(kind, s.frame(), s.angle, &s.style, s.blend, canvas),
        });
        let area = tab.doc.set_preview(overlay);
        tab.mark_dirty(area);
    }

    fn refresh_floating_resample(&mut self) {
        let (method, rotation) = (self.options.move_resample, self.options.rotation);
        if (method, rotation) == self.floating_resample {
            return;
        }
        self.floating_resample = (method, rotation);
        let tab = self.tab_mut();
        if !matches!(tab.gesture, Gesture::None) {
            return;
        }
        let Some(placement) = tab.doc.floating().map(|f| f.placement()) else {
            return;
        };
        if tab.doc.floating().is_some_and(|f| f.quad().is_some()) {
            let area = tab.doc.resample_floating(method, rotation);
            tab.mark_dirty(area);
            return;
        }
        let snapped = crate::selection_ui::snap_to_pixels(placement);
        let area = tab.doc.place_floating(snapped, method, rotation);
        tab.mark_dirty(area);
    }

    pub(crate) fn refresh_shape_style(&mut self) {
        let Some(session) = self.tab().shape.as_ref() else {
            return;
        };
        let style = self.shape_style(session.secondary);
        let geometry = match session.geometry {
            ShapeGeometry::Line => ShapeGeometry::Line,
            ShapeGeometry::Boxed(_) => ShapeGeometry::Boxed(self.options.shape_kind),
        };
        let blend = self.options.blend;
        let line_mode = self.options.line_mode;
        if style == session.style && geometry == session.geometry && blend == session.blend && line_mode == session.line_mode {
            return;
        }
        if let Some(session) = self.tab_mut().shape.as_mut() {
            session.style = style;
            session.geometry = geometry;
            session.blend = blend;
            session.line_mode = line_mode;
        }
        self.refresh_shape_preview();
    }

    fn shape_style(&self, secondary: bool) -> ShapeStyle {
        let (primary, other) = if secondary { (self.colors.secondary, self.colors.primary) } else { (self.colors.primary, self.colors.secondary) };
        let (outline, fill) = self.options.shape_colors(primary, other);
        ShapeStyle { outline, fill, width: self.options.size, align: StrokeAlign::Center, antialias: self.options.antialias }
    }

    fn shape_point(&self, image_pos: FPoint) -> FPoint {
        let tab = self.tab();
        if tab.view.tiled {
            image_pos
        } else if self.options.antialias {
            clamp_point(image_pos, tab)
        } else {
            clamp_pixel(image_pos, tab)
        }
    }

    fn shape_pressed(&mut self, canvas: Rect, pos: Pos2, image_pos: FPoint, button: PointerButton, mods: Modifiers) {
        if let Some(op) = self.hit_shape(canvas, pos, image_pos, mods.ctrl) {
            self.begin_shape_op(op);
            return;
        }
        if self.tab().shape.as_ref().is_some_and(ShapeSession::is_curve) {
            let at = self.shape_point(image_pos);
            self.begin_shape_op(ShapeOp::Extend(at));
            return;
        }
        self.commit_shape();
        let secondary = button == PointerButton::Secondary;
        let style = self.shape_style(secondary);
        let geometry = if self.tool == Tool::Line {
            ShapeGeometry::Line
        } else {
            ShapeGeometry::Boxed(self.options.shape_kind)
        };
        let blend = self.options.blend;
        let start = self.shape_point(image_pos);
        let (points, line_mode) = if geometry == ShapeGeometry::Line { (vec![start, start], self.options.line_mode) } else { (Vec::new(), LineMode::Straight) };
        let tab = self.tab_mut();
        tab.shape = Some(ShapeSession { geometry, p0: start, p1: start, points, line_mode, last_point_press: None, style, blend, angle: 0.0, secondary });
        tab.gesture = Gesture::Shape(ShapeOp::Create);
        self.refresh_shape_preview();
    }

    fn begin_shape_op(&mut self, op: ShapeOp) {
        let tab = self.tab_mut();
        let Some(session) = tab.shape.as_mut() else {
            return;
        };
        session.normalize();
        let (op, edited) = match op {
            ShapeOp::AddPoint { after, at } => {
                session.points.insert(after + 1, at);
                (Some(ShapeOp::MovePoint(after + 1)), true)
            }
            ShapeOp::Extend(at) => {
                let index = session.extend_nearest_end(at);
                (Some(ShapeOp::MovePoint(index)), true)
            }
            ShapeOp::MovePoint(index) if session.press_point(index) => {
                session.points.remove(index);
                (None, true)
            }
            other => (Some(other), false),
        };
        tab.gesture = op.map_or(Gesture::None, Gesture::Shape);
        if edited {
            self.refresh_shape_preview();
        }
    }

    fn shape_rotate_cursor(&self, canvas: Rect, hover: Option<Pos2>) -> bool {
        match &self.tab().gesture {
            Gesture::Shape(ShapeOp::Rotate(_)) => true,
            Gesture::None if matches!(self.tool, Tool::Shapes | Tool::Line) => hover.is_some_and(|pos| matches!(self.hit_shape(canvas, pos, self.tab().view.screen_to_image(canvas, pos), false), Some(ShapeOp::Rotate(_)))),
            _ => false,
        }
    }

    fn hit_shape(&self, canvas: Rect, pos: Pos2, image_pos: FPoint, grab_whole: bool) -> Option<ShapeOp> {
        let tab = self.tab();
        let session = tab.shape.as_ref()?;
        if session.geometry == ShapeGeometry::Line {
            return self.hit_line(canvas, pos, image_pos, grab_whole);
        }
        let mut normalized = session.clone();
        normalized.normalize();
        for (nub, screen) in nub_positions(&normalized, tab, canvas) {
            if near_handle(screen, pos) {
                return Some(ShapeOp::Resize(nub));
            }
        }
        let frame = session.screen_box(tab, canvas);
        let distance = frame.distance_outside(pos);
        if distance > 0.0 && distance <= ROTATE_ZONE_WIDTH {
            let pivot = tab.view.screen_to_image(canvas, frame.center);
            return Some(ShapeOp::Rotate(RotationDrag::new(session.angle, pivot, image_pos)));
        }
        (distance == 0.0).then(|| ShapeOp::Move { anchor: image_pos, p0: session.p0, p1: session.p1 })
    }

    fn hit_line(&self, canvas: Rect, pos: Pos2, image_pos: FPoint, grab_whole: bool) -> Option<ShapeOp> {
        let tab = self.tab();
        let session = tab.shape.as_ref()?;
        if self.line_move_grip(canvas, session).hits_move_icon(pos) {
            return Some(ShapeOp::MovePath { anchor: image_pos, points: session.points.clone() });
        }
        let points = session.snapped_points();
        let grabbed = session.handle_indices().into_iter().find(|&index| near_handle(tab.view.image_to_screen(canvas, points[index].x, points[index].y), pos));
        if let Some(index) = grabbed {
            return Some(ShapeOp::MovePoint(index));
        }
        let nearest = nearest_on(&session.segments(), image_pos).filter(|nearest| nearest.distance * tab.view.scale() <= LINE_HIT_DISTANCE)?;
        if session.line_mode == LineMode::Smooth && !grab_whole {
            Some(ShapeOp::AddPoint { after: nearest.segment, at: nearest.point })
        } else {
            Some(ShapeOp::MovePath { anchor: image_pos, points: session.points.clone() })
        }
    }

    fn shape_dragged(&mut self, image_pos: FPoint, mods: Modifiers) {
        let smooth = self.options.antialias;
        let point = self.shape_point(image_pos);
        let tab = self.tab_mut();
        let Gesture::Shape(op) = &mut tab.gesture else {
            return;
        };
        let Some(session) = tab.shape.as_mut() else {
            return;
        };
        match op {
            ShapeOp::Create => {
                let end = constrain_corner(session.geometry, session.start_corner(), point, mods.shift, smooth);
                session.set_end_corner(end);
            }
            ShapeOp::MovePoint(index) => {
                let neighbour = session.neighbour_of(*index);
                session.points[*index] = constrain_corner(ShapeGeometry::Line, neighbour, point, mods.shift, smooth);
            }
            ShapeOp::MovePath { anchor, points } => {
                let shift = |delta: f32| if smooth { delta } else { delta.round() };
                let (dx, dy) = (shift(image_pos.x - anchor.x), shift(image_pos.y - anchor.y));
                session.points = points.iter().map(|p| FPoint::new(p.x + dx, p.y + dy)).collect();
            }
            ShapeOp::AddPoint { .. } | ShapeOp::Extend(_) => {}
            ShapeOp::Resize(nub) if session.angle != 0.0 => session.resize_rotated(*nub, image_pos, mods.shift),
            ShapeOp::Rotate(drag) => session.angle = drag.target(image_pos, mods),
            ShapeOp::Resize(nub) => {
                let target = if mods.shift { shift_resized(session, *nub, image_pos, smooth) } else { image_pos };
                session.apply_nub(*nub, target);
            }
            ShapeOp::Move { anchor, p0, p1 } => {
                let shift = |delta: f32| if smooth { delta } else { delta.round() };
                let (dx, dy) = (shift(image_pos.x - anchor.x), shift(image_pos.y - anchor.y));
                session.p0 = FPoint::new(p0.x + dx, p0.y + dy);
                session.p1 = FPoint::new(p1.x + dx, p1.y + dy);
            }
        }
        self.refresh_shape_preview();
    }

    fn shape_released(&mut self) {
        let tab = self.tab_mut();
        let Some(session) = tab.shape.as_mut() else {
            return;
        };
        session.normalize();
        if session.is_click() {
            self.cancel_shape();
        }
    }

    fn paint_export_flash(&mut self, ctx: &Context, painter: &egui::Painter, image_rect: Rect) {
        let Some(started) = self.export_flash else {
            return;
        };
        let now = ctx.input(|i| i.time);
        let started = if started.is_finite() { started } else { now };
        self.export_flash = Some(started);
        let progress = ((now - started) as f32 / EXPORT_FLASH_SECONDS).clamp(0.0, 1.0);
        if progress >= 1.0 {
            self.export_flash = None;
            return;
        }
        let strength = (1.0 - progress).powi(2);
        painter.rect_filled(image_rect, 0.0, Color32::from_white_alpha((strength * 255.0) as u8));
        ctx.request_repaint();
    }

    fn paint_document(&self, painter: &egui::Painter, canvas: Rect, image_rect: Rect) {
        let tab = self.tab();
        let visible = image_rect.intersect(canvas);
        if visible.width() <= 0.0 || visible.height() <= 0.0 {
            return;
        }
        let pixel = 1.0 / painter.pixels_per_point();
        let shadow = image_rect.translate(vec2(DOCUMENT_SHADOW_OFFSET, DOCUMENT_SHADOW_OFFSET));
        painter.rect_filled(shadow, 0.0, Color32::from_black_alpha(DOCUMENT_SHADOW_ALPHA));
        painter.rect_filled(image_rect.expand(pixel), 0.0, skin::theme().line);
        self.skin.paint_checker(&painter.with_clip_rect(visible), visible, image_rect.min);
        if let Some(id) = tab.canvas_texture_id() {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            if tab.view.tiled {
                self.paint_tile_copies(painter, canvas, image_rect, id);
            }
            painter.image(id, image_rect, uv, Color32::WHITE);
            for layer in tab.onion_layers() {
                painter.image(layer.texture.id(), image_rect, uv, Color32::WHITE);
            }
            if tab.view.tiled && self.tile_frame {
                painter.rect_stroke(image_rect, 0.0, Stroke::new(1.0, Color32::from_black_alpha(160)), egui::StrokeKind::Outside);
                painter.rect_stroke(image_rect, 0.0, Stroke::new(1.0, Color32::from_white_alpha(160)), egui::StrokeKind::Inside);
            }
        }
        if self.show_grid && tab.view.zoom >= GRID_MIN_ZOOM {
            paint_pixel_grid(painter, canvas, image_rect, tab);
        }
    }

    fn paint_tile_copies(&self, painter: &egui::Painter, canvas: Rect, image_rect: Rect, texture: egui::TextureId) {
        let size = image_rect.size();
        let columns = ((canvas.min.x - image_rect.min.x) / size.x).floor() as i32..((canvas.max.x - image_rect.min.x) / size.x).ceil() as i32;
        let rows = ((canvas.min.y - image_rect.min.y) / size.y).floor() as i32..((canvas.max.y - image_rect.min.y) / size.y).ceil() as i32;
        if (columns.end - columns.start).saturating_mul(rows.end - rows.start) > MAX_TILE_COPIES {
            return;
        }
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let tint = Color32::WHITE.gamma_multiply(self.tile_opacity);
        for row in rows {
            for column in columns.clone().filter(|&column| column != 0 || row != 0) {
                painter.image(texture, image_rect.translate(vec2(column as f32 * size.x, row as f32 * size.y)), uv, tint);
            }
        }
    }

    fn paint_overlays(&self, ctx: &Context, painter: &egui::Painter, canvas: Rect, hover: Option<Pos2>) {
        let tab = self.tab();
        let to_screen = |p: FPoint| tab.view.image_to_screen(canvas, p.x, p.y);
        let corner_to_screen = |x: f32, y: f32| tab.view.image_to_screen(canvas, x, y);
        let painter = painter.with_clip_rect(canvas);
        let marquee = live_marquee(tab);
        let selection = tab.doc.selection().filter(|_| !replaced_by_live_selection(tab));
        let mut ants = Ants::new(&painter, self.skin.ants_texture(), ctx.input(|i| i.time));
        let outline = self.true_outline();
        if let Some(outline) = &outline {
            let screen: Vec<Pos2> = outline.iter().map(|p| corner_to_screen(p.x, p.y)).collect();
            ants.polyline(&screen);
        }
        if let Some(selection) = selection.filter(|_| outline.is_none()) {
            for contour in selection.contours() {
                let screen: Vec<Pos2> = contour.iter().map(|p| corner_to_screen(p.x as f32, p.y as f32)).collect();
                ants.contour(&screen);
            }
        }
        for contour in self.wand_preview_contours(canvas) {
            ants.contour(&contour);
        }
        match &tab.gesture {
            Gesture::Marquee { ellipse, .. } => {
                if let Some(frame) = marquee {
                    let rect = Rect::from_two_pos(corner_to_screen(frame.x0, frame.y0), corner_to_screen(frame.x1, frame.y1));
                    if *ellipse {
                        ants.polyline(&ellipse_points(rect));
                    } else {
                        ants.rect(rect);
                    }
                }
            }
            Gesture::Lasso { points, .. } => {
                let screen: Vec<Pos2> = points.iter().map(|&p| to_screen(p)).collect();
                ants.polyline(&screen);
            }
            Gesture::ZoomDrag { start, current, .. } => {
                let drag = Rect::from_two_pos(*start, *current);
                if drag.width() >= ZOOM_DRAG_MIN || drag.height() >= ZOOM_DRAG_MIN {
                    ants.rect(drag);
                }
            }
            _ => {}
        }
        if let Some(session) = &tab.shape {
            self.add_shape_frame(&mut ants, canvas, session);
        }
        if let Some(rect) = self.text_screen_rect(canvas) {
            ants.rect(rect);
        }
        if self.handles_visible() {
            self.add_gizmo(&mut ants, canvas, hover);
        }
        self.add_live_knobs(&mut ants, canvas, hover);
        if let Some(session) = &tab.shape {
            self.add_shape_nubs(&mut ants, canvas, session, hover);
        }
        if let Some(pos) = hover.filter(|_| self.draws_own_cursor(ctx)) {
            self.add_tool_cursor(&mut ants, pos);
        }
        if let Some(pos) = hover.filter(|_| self.uses_crosshair(ctx, canvas, hover)) {
            add_cross(&mut ants, pos, CROSSHAIR_ARM);
        }
        if self.tool == Tool::CloneStamp {
            if let Some(pos) = self.clone_source_marker(canvas, hover) {
                add_cross(&mut ants, pos, CLONE_SOURCE_ARM);
            }
        }
        if let Some(pos) = hover.filter(|_| self.shows_pick_area(ctx)) {
            self.add_pick_area(&mut ants, pos);
        }
        ants.finish();
        if self.handles_visible() {
            self.paint_move_handle(&painter, canvas, hover);
        }
        self.paint_line_move_handle(&painter, canvas, hover);
        self.paint_text_session(ctx, &painter, canvas, hover);
        if let Some(pos) = hover.filter(|_| self.alt_picks(ctx) || matches!(tab.gesture, Gesture::Pick { .. })) {
            self.paint_picker_cursor(&painter, pos);
        }
        if let Some(pos) = hover.filter(|_| self.uses_crosshair(ctx, canvas, hover)) {
            self.paint_tool_badge(&painter, pos);
        }
        if let Some(pos) = hover.filter(|_| matches!(self.gizmo_cursor(canvas, hover), Some(GizmoCursor::Rotate)) || self.shape_rotate_cursor(canvas, hover)) {
            paint_rotate_cursor(&painter, &self.skin, pos);
        }
    }

    fn cursor_is_plain(&self, ctx: &Context) -> bool {
        !self.space_down && !self.alt_picks(ctx) && !matches!(self.tab().gesture, Gesture::Pan { .. } | Gesture::Pick { .. })
    }

    fn draws_own_cursor(&self, ctx: &Context) -> bool {
        matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::Pencil | Tool::CloneStamp) && self.cursor_is_plain(ctx)
    }

    fn uses_crosshair(&self, ctx: &Context, canvas: Rect, hover: Option<Pos2>) -> bool {
        let has_own_cursor = matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::Pencil | Tool::CloneStamp | Tool::Pan | Tool::MovePixels | Tool::MoveSelection | Tool::Zoom);
        !has_own_cursor && self.cursor_is_plain(ctx) && self.gizmo_cursor(canvas, hover).is_none() && !self.shape_rotate_cursor(canvas, hover)
    }

    fn shows_pick_area(&self, ctx: &Context) -> bool {
        let picking = self.alt_picks(ctx) || matches!(self.tab().gesture, Gesture::Pick { .. });
        picking || (self.tool == Tool::Picker && self.cursor_is_plain(ctx))
    }

    fn add_pick_area(&self, ants: &mut Ants, pos: Pos2) {
        let view = &self.tab().view;
        let area = square_around(view.screen_to_image(self.workspace, pos).floor(), self.options.pick_size);
        let corner = |x: i32, y: i32| view.image_to_screen(self.workspace, x as f32, y as f32);
        let (min, max) = (corner(area.x0, area.y0), corner(area.x1, area.y1));
        let center = min + (max - min) / 2.0;
        let half = ((max - min) / 2.0).max(Vec2::splat(PICK_AREA_MIN_SIDE / 2.0));
        let (min, max) = (center - half, center + half);
        ants.solid(&[min, pos2(max.x, min.y), max, pos2(min.x, max.y), min]);
    }

    fn paint_tool_badge(&self, painter: &egui::Painter, pos: Pos2) {
        let rect = Rect::from_min_size(pos + TOOL_BADGE_OFFSET, vec2(TOOL_BADGE_SIDE, TOOL_BADGE_SIDE));
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(self.skin.icons.texture_id(self.tool.icon(), TOOL_BADGE_SIDE), rect, uv, Color32::WHITE);
    }

    fn pressure_size_scale(&self) -> f32 {
        self.options.brush_for(self.tool).settings.pressure_size_scale(self.pen.pressure())
    }

    fn add_tool_cursor(&self, ants: &mut Ants, pos: Pos2) {
        let scale = self.tab().view.scale();
        let diameter = if self.tool == Tool::Pencil { scale } else { self.options.size * scale * self.pressure_size_scale() };
        if diameter < CURSOR_RING_MIN_DIAMETER {
            add_cross(ants, pos, CURSOR_CROSS_ARM);
        } else if self.tool == Tool::Pencil {
            let pixel = self.tab().view.screen_to_image(self.workspace, pos).floor();
            let corner = |dx: f32, dy: f32| self.tab().view.image_to_screen(self.workspace, pixel.x as f32 + dx, pixel.y as f32 + dy);
            ants.solid(&[corner(0.0, 0.0), corner(1.0, 0.0), corner(1.0, 1.0), corner(0.0, 1.0), corner(0.0, 0.0)]);
        } else {
            let radius = diameter / 2.0;
            let segments = ((std::f32::consts::TAU * radius / CURSOR_RING_SEGMENT) as usize).max(CURSOR_RING_MIN_SEGMENTS);
            let ring: Vec<Pos2> = (0..=segments)
                .map(|i| {
                    let angle = std::f32::consts::TAU * i as f32 / segments as f32;
                    pos + vec2(angle.cos(), angle.sin()) * radius
                })
                .collect();
            ants.solid(&ring);
        }
    }

    fn paint_picker_cursor(&self, painter: &egui::Painter, pos: Pos2) {
        let side = PICKER_CURSOR_SIDE;
        let tip = vec2(PICKER_TIP.0, PICKER_TIP.1) / ICON_SIZE as f32 * side;
        let rect = Rect::from_min_size(pos - tip, vec2(side, side));
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(self.skin.icons.texture_id(Icon::Picker, side), rect, uv, Color32::WHITE);
    }

    fn add_gizmo(&self, ants: &mut Ants, canvas: Rect, hover: Option<Pos2>) {
        let Some(gizmo) = self.selection_gizmo(canvas) else {
            return;
        };
        let (hit, gesture) = (hover.and_then(|pos| self.gizmo_hit(canvas, pos)), &self.tab().gesture);
        let active_handle = match gesture {
            Gesture::ResizeSelection { handle, .. } | Gesture::ScalePixels { handle, .. } => Some(*handle),
            _ => match hit {
                Some(GizmoHit::Handle(handle)) => Some(handle),
                _ => None,
            },
        };
        match gizmo.corners {
            Some(corners) => {
                let active_target = match gesture {
                    Gesture::WarpPixels { target, .. } => Some(*target),
                    _ => match hit {
                        Some(GizmoHit::Warp(target)) => Some(target),
                        _ => None,
                    },
                };
                ants.solid(&corners.iter().chain(corners.first()).copied().collect::<Vec<_>>());
                for index in 0..corners.len() {
                    add_handle_ring(ants, corners[index], active_target == Some(WarpTarget::Corner(index)));
                    add_handle_ring(ants, corners[index].lerp(corners[(index + 1) % corners.len()], 0.5), active_target == Some(WarpTarget::Edge(index)));
                }
            }
            None => {
                if gizmo.frame.angle != 0.0 {
                    add_box_outline(ants, &gizmo.frame);
                }
                for handle in Handle::ALL {
                    add_handle_ring(ants, gizmo.frame.handle_position(handle), active_handle == Some(handle));
                }
            }
        }
        if let Some(pivot) = gizmo.pivot {
            add_pivot(ants, pivot, matches!(gesture, Gesture::MovePivot) || hit == Some(GizmoHit::Pivot));
        }
    }

    fn paint_move_handle(&self, painter: &egui::Painter, canvas: Rect, hover: Option<Pos2>) {
        let Some(gizmo) = self.selection_gizmo(canvas) else {
            return;
        };
        let hot = matches!(self.tab().gesture, Gesture::MovePixels { .. } | Gesture::MoveSelection { .. })
            || hover.and_then(|pos| self.gizmo_hit(canvas, pos)) == Some(GizmoHit::MoveIcon);
        paint_move_icon(painter, &self.skin, gizmo.frame.move_icon_center(), hot);
    }

    fn line_move_grip(&self, canvas: Rect, session: &ShapeSession) -> ScreenBox {
        let view = &self.tab().view;
        let corners: Vec<Pos2> = session.snapped_points().iter().map(|p| view.image_to_screen(canvas, p.x, p.y)).collect();
        ScreenBox::from_rect(Rect::from_points(&corners))
    }

    fn paint_line_move_handle(&self, painter: &egui::Painter, canvas: Rect, hover: Option<Pos2>) {
        let Some(session) = self.tab().shape.as_ref().filter(|session| session.geometry == ShapeGeometry::Line) else {
            return;
        };
        let grip = self.line_move_grip(canvas, session);
        let hot = matches!(self.tab().gesture, Gesture::Shape(ShapeOp::MovePath { .. })) || hover.is_some_and(|pos| grip.hits_move_icon(pos));
        paint_move_icon(painter, &self.skin, grip.move_icon_center(), hot);
    }

    fn add_shape_frame(&self, ants: &mut Ants, canvas: Rect, session: &ShapeSession) {
        if !matches!(session.geometry, ShapeGeometry::Boxed(_)) {
            return;
        }
        let tab = self.tab();
        if session.angle != 0.0 {
            add_box_outline(ants, &session.screen_box(tab, canvas));
            return;
        }
        let frame = session.frame();
        let a = tab.view.image_to_screen(canvas, frame.x0, frame.y0);
        let b = tab.view.image_to_screen(canvas, frame.x1, frame.y1);
        ants.rect(Rect::from_two_pos(a, b));
    }

    fn add_line_handles(&self, ants: &mut Ants, canvas: Rect, session: &ShapeSession, hover: Option<Pos2>) {
        let tab = self.tab();
        let points = session.snapped_points();
        let dragging = match &tab.gesture {
            Gesture::Shape(ShapeOp::MovePoint(index)) => Some(*index),
            _ => None,
        };
        for index in session.handle_indices() {
            let screen = tab.view.image_to_screen(canvas, points[index].x, points[index].y);
            let hot = dragging == Some(index) || hover.is_some_and(|pointer| near_handle(screen, pointer));
            add_handle_ring(ants, screen, hot);
        }
        let Some(pointer) = hover.filter(|_| matches!(tab.gesture, Gesture::None)) else {
            return;
        };
        if let Some(ShapeOp::AddPoint { at, .. }) = self.hit_line(canvas, pointer, tab.view.screen_to_image(canvas, pointer), false) {
            add_handle_ring(ants, tab.view.image_to_screen(canvas, at.x, at.y), true);
        }
    }

    fn add_shape_nubs(&self, ants: &mut Ants, canvas: Rect, session: &ShapeSession, hover: Option<Pos2>) {
        if session.geometry == ShapeGeometry::Line {
            self.add_line_handles(ants, canvas, session, hover);
            return;
        }
        let tab = self.tab();
        let mut normalized = session.clone();
        normalized.normalize();
        let dragging = match &tab.gesture {
            Gesture::Shape(ShapeOp::Resize(nub)) => Some(*nub),
            _ => None,
        };
        for (nub, screen) in nub_positions(&normalized, tab, canvas) {
            let hot = dragging == Some(nub) || hover.is_some_and(|pointer| near_handle(screen, pointer));
            add_handle_ring(ants, screen, hot);
        }
    }
}

fn live_marquee(tab: &DocTab) -> Option<FRect> {
    match &tab.gesture {
        Gesture::Marquee { start, current, square, .. } => marquee_frame(tab, *start, *current, *square),
        _ => None,
    }
}

fn replaced_by_live_selection(tab: &DocTab) -> bool {
    match &tab.gesture {
        Gesture::Marquee { mode: CombineMode::Replace, .. } => live_marquee(tab).is_some(),
        Gesture::Lasso { mode: CombineMode::Replace, points } => points.len() >= MIN_LASSO_POINTS,
        _ => false,
    }
}

pub(crate) fn ellipse_points(rect: Rect) -> Vec<Pos2> {
    let (center, radii) = (rect.center(), rect.size() / 2.0);
    (0..=ELLIPSE_SEGMENTS)
        .map(|i| {
            let angle = i as f32 / ELLIPSE_SEGMENTS as f32 * std::f32::consts::TAU;
            center + vec2(radii.x * angle.cos(), radii.y * angle.sin())
        })
        .collect()
}

fn point_center(p: IPoint) -> FPoint {
    FPoint::new(p.x as f32 + 0.5, p.y as f32 + 0.5)
}

fn shift_resized(session: &ShapeSession, nub: Nub, pointer: FPoint, smooth: bool) -> FPoint {
    let anchor = match (session.geometry, nub.x, nub.y) {
        (ShapeGeometry::Boxed(_), Some(x), Some(y)) => {
            FPoint::new(if x == Axis::First { session.p1.x } else { session.p0.x }, if y == Axis::First { session.p1.y } else { session.p0.y })
        }
        _ => return pointer,
    };
    constrain_corner(session.geometry, anchor, pointer, true, smooth)
}

pub(crate) fn constrain_corner(geometry: ShapeGeometry, start: FPoint, end: FPoint, shift: bool, smooth: bool) -> FPoint {
    if !shift {
        return end;
    }
    let (start, end) = if smooth { (start, end) } else { (FPoint::new(start.x.floor(), start.y.floor()), FPoint::new(end.x.floor(), end.y.floor())) };
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let constrained = match geometry {
        ShapeGeometry::Boxed(_) => {
            let side = dx.abs().max(dy.abs());
            FPoint::new(start.x + side.copysign(if dx == 0.0 { 1.0 } else { dx }), start.y + side.copysign(if dy == 0.0 { 1.0 } else { dy }))
        }
        ShapeGeometry::Line => {
            let length = dx.hypot(dy);
            let step = ANGLE_SNAP_DEGREES.to_radians();
            let angle = (dy.atan2(dx) / step).round() * step;
            FPoint::new(start.x + length * angle.cos(), start.y + length * angle.sin())
        }
    };
    if smooth { constrained } else { FPoint::new(constrained.x.round(), constrained.y.round()) }
}

fn nub_positions(session: &ShapeSession, tab: &DocTab, canvas: Rect) -> Vec<(Nub, Pos2)> {
    let frame = session.screen_box(tab, canvas);
    let nub = |x, y| Nub { x, y };
    use Axis::{First as A, Second as B};
    let at_fraction = |axis: Option<Axis>| match axis {
        Some(A) => 0.0,
        Some(B) => 1.0,
        None => 0.5,
    };
    [
        nub(Some(A), Some(A)),
        nub(None, Some(A)),
        nub(Some(B), Some(A)),
        nub(Some(A), None),
        nub(Some(B), None),
        nub(Some(A), Some(B)),
        nub(None, Some(B)),
        nub(Some(B), Some(B)),
    ]
    .into_iter()
    .map(|n| (n, frame.point(at_fraction(n.x), at_fraction(n.y))))
    .collect()
}

pub(crate) fn marquee_frame(tab: &DocTab, start: FPoint, current: FPoint, square: bool) -> Option<FRect> {
    let end = clamp_pixel(current, tab);
    let (a, b) = (start.floor(), end.floor());
    if a == b && start.distance(end) < 1.0 {
        return None;
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let b = if square {
        let side = dx.abs().max(dy.abs());
        IPoint::new(a.x + side * if dx < 0 { -1 } else { 1 }, a.y + side * if dy < 0 { -1 } else { 1 })
    } else {
        b
    };
    let frame = IRect::from_corners(a, b).intersect(tab.doc.bounds());
    (!frame.is_empty()).then(|| frame.into())
}

fn paint_pixel_grid(painter: &egui::Painter, canvas: Rect, image_rect: Rect, tab: &DocTab) {
    let visible = image_rect.intersect(canvas);
    let zoom = tab.view.scale();
    let stroke = Stroke::new(1.0, Color32::from_black_alpha(70));
    let first_x = ((visible.min.x - image_rect.min.x) / zoom).floor() as i32;
    let last_x = ((visible.max.x - image_rect.min.x) / zoom).ceil() as i32;
    let first_y = ((visible.min.y - image_rect.min.y) / zoom).floor() as i32;
    let last_y = ((visible.max.y - image_rect.min.y) / zoom).ceil() as i32;
    let mut lines = Vec::new();
    for x in first_x..=last_x {
        let sx = (image_rect.min.x + x as f32 * zoom).round();
        lines.push(Shape::line_segment([Pos2::new(sx, visible.min.y), Pos2::new(sx, visible.max.y)], stroke));
    }
    for y in first_y..=last_y {
        let sy = (image_rect.min.y + y as f32 * zoom).round();
        lines.push(Shape::line_segment([Pos2::new(visible.min.x, sy), Pos2::new(visible.max.x, sy)], stroke));
    }
    painter.extend(lines);
}

pub fn tool_fill_style_label(style: FillStyle) -> crate::i18n::Text {
    match style {
        FillStyle::Outline => crate::i18n::Text::FillOutline,
        FillStyle::Filled => crate::i18n::Text::FillFilled,
        FillStyle::Both => crate::i18n::Text::FillBoth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_keeps_only_the_dominant_axis_of_a_move() {
        let anchor = FPoint::new(10.0, 10.0);
        assert_eq!(move_offset(anchor, FPoint::new(30.0, 14.0), true), IPoint::new(20, 0));
        assert_eq!(move_offset(anchor, FPoint::new(13.0, -20.0), true), IPoint::new(0, -30));
        assert_eq!(move_offset(anchor, FPoint::new(30.0, 14.0), false), IPoint::new(20, 4));
    }
}

fn zoom_crossed_stop(before: f32, after: f32, requested: f32) -> bool {
    let crossed_actual_size = (before - ACTUAL_SIZE_ZOOM) * (after - ACTUAL_SIZE_ZOOM) < 0.0;
    let hit_limit = requested != after && before != after;
    crossed_actual_size || hit_limit
}
