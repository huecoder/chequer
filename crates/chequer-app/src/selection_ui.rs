
use std::sync::Arc;

use chequer_core::document::Resample;
use chequer_core::geom::{FPoint, FRect, IPoint, IRect};
use chequer_core::history::HistoryKind;
use chequer_core::selection::{CombineMode, LassoSmoothing, Selection, SharedSelection, combine};
use chequer_core::transform::{Placement, Quad, RotationMethod};
use eframe::egui::{CursorIcon, Pos2, Rect, vec2};

use crate::app::App;
use crate::canvas::{Gesture, clamp_pixel, clamp_point, marquee_frame, selection_mode};
use crate::handles::{HANDLE_REACH, Handle, ROTATE_ZONE_WIDTH, ScreenBox, RotationDrag, drag_frame, hits_pivot, in_rotate_band};
use crate::tools::{MoveMethod, Tool};
use eframe::egui::{Modifiers, PointerButton};

#[derive(Clone)]
pub enum SourceShape {
    Rect,
    Ellipse,
    Polygon(Vec<FPoint>, LassoSmoothing),
    Mask(Arc<Selection>),
}

#[derive(Clone)]
pub struct SelectionOrigin {
    pub shape: SourceShape,
    pub frame: FRect,
    pub base: Option<SharedSelection>,
    pub mode: CombineMode,
    pub result: SharedSelection,
}

impl SelectionOrigin {
    fn shaped(&self, frame: FRect) -> Option<(Selection, SourceShape)> {
        match &self.shape {
            SourceShape::Rect => rect_selection(frame).map(|s| (s, SourceShape::Rect)),
            SourceShape::Ellipse => Selection::from_ellipse(frame, true).map(|s| (s, SourceShape::Ellipse)),
            SourceShape::Polygon(points, smoothing) => {
                let scaled: Vec<FPoint> = scale_points(points, self.frame, frame).into_iter().map(on_grid).collect();
                Selection::from_polygon(&scaled, *smoothing).map(|s| (s, SourceShape::Polygon(scaled, *smoothing)))
            }
            SourceShape::Mask(mask) => mask.resampled_to(frame.rounded()).map(|s| (s.clone(), SourceShape::Mask(Arc::new(s)))),
        }
    }
}

fn origin_outline(origin: &SelectionOrigin) -> Option<Vec<Pos2>> {
    match &origin.shape {
        SourceShape::Polygon(points, _) => Some(points.iter().chain(points.first()).map(|p| Pos2::new(p.x, p.y)).collect()),
        SourceShape::Ellipse => Some(crate::canvas::ellipse_points(Rect::from_min_max(Pos2::new(origin.frame.x0, origin.frame.y0), Pos2::new(origin.frame.x1, origin.frame.y1)))),
        SourceShape::Rect | SourceShape::Mask(_) => None,
    }
}

pub(crate) fn rect_selection(frame: FRect) -> Option<Selection> {
    let whole = frame.rounded();
    if FRect::from(whole) == frame.normalized() { Selection::from_rect(whole) } else { Selection::from_frame(frame, true) }
}

fn scale_points(points: &[FPoint], from: FRect, to: FRect) -> Vec<FPoint> {
    let (scale_x, scale_y) = (to.width() / from.width().max(1.0), to.height() / from.height().max(1.0));
    points.iter().map(|p| FPoint::new(to.x0 + (p.x - from.x0) * scale_x, to.y0 + (p.y - from.y0) * scale_y)).collect()
}

const LIVE_RESAMPLE_MAX_AREA: f32 = 1024.0 * 1024.0;

fn drag_quality(placement: &Placement, method: Resample, rotation: RotationMethod) -> (Resample, RotationMethod) {
    if placement.width * placement.height <= LIVE_RESAMPLE_MAX_AREA { (method, rotation) } else { (Resample::Nearest, RotationMethod::Plain) }
}

fn drag_resample(quad: &Quad, method: Resample) -> Resample {
    let bounds = quad.outer_bounds();
    if (bounds.width() as f32) * (bounds.height() as f32) <= LIVE_RESAMPLE_MAX_AREA { method } else { Resample::Nearest }
}

fn warp_start(floating: &chequer_core::document::Floating) -> Quad {
    floating.quad().unwrap_or_else(|| Quad::from_placement(&floating.placement()))
}

fn warp_target(corners: &[Pos2; 4], pos: Pos2) -> Option<WarpTarget> {
    let near = |point: Pos2| point.distance(pos) <= HANDLE_REACH;
    let nearest = |points: Vec<(usize, Pos2)>| points.into_iter().filter(|(_, point)| near(*point)).min_by(|a, b| a.1.distance(pos).total_cmp(&b.1.distance(pos))).map(|(index, _)| index);
    let middles = (0..4).map(|index| (index, corners[index].lerp(corners[(index + 1) % 4], 0.5))).collect();
    nearest(corners.iter().copied().enumerate().collect())
        .map(WarpTarget::Corner)
        .or_else(|| nearest(middles).map(WarpTarget::Edge))
}

fn distance_to_segment(from: Pos2, to: Pos2, pos: Pos2) -> f32 {
    let edge = to - from;
    let along = ((pos - from).dot(edge) / edge.length_sq()).clamp(0.0, 1.0);
    pos.distance(from + edge * along)
}

fn distance_outside_polygon(corners: &[Pos2; 4], pos: Pos2) -> f32 {
    let screen = Quad { corners: corners.map(|c| FPoint::new(c.x, c.y)) };
    if screen.contains(FPoint::new(pos.x, pos.y)) {
        return 0.0;
    }
    (0..4).map(|index| distance_to_segment(corners[index], corners[(index + 1) % 4], pos)).fold(f32::MAX, f32::min)
}

fn on_grid(p: FPoint) -> FPoint {
    FPoint::new(p.x.round(), p.y.round())
}

fn shows_handles(tool: Tool) -> bool {
    matches!(tool, Tool::RectSelect | Tool::EllipseSelect | Tool::Lasso | Tool::MovePixels | Tool::MoveSelection)
}

pub(crate) struct Gizmo {
    pub frame: ScreenBox,
    pub pivot: Option<Pos2>,
    pub rotatable: bool,
    pub corners: Option<[Pos2; 4]>,
}

impl Gizmo {
    pub(crate) fn distance_outside(&self, pos: Pos2) -> f32 {
        match &self.corners {
            Some(corners) => distance_outside_polygon(corners, pos),
            None => self.frame.distance_outside(pos),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarpTarget {
    Corner(usize),
    Edge(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GizmoHit {
    MoveIcon,
    Pivot,
    Handle(Handle),
    Warp(WarpTarget),
    Rotate,
    Body,
}

pub(crate) enum GizmoCursor {
    System(CursorIcon),
    Rotate,
}

impl App {
    pub(crate) fn true_outline(&self) -> Option<Vec<Pos2>> {
        let tab = self.tab();
        let origin = tab.sel_origin.as_ref().filter(|o| o.base.is_none());
        let Some(floating) = tab.doc.floating() else {
            let origin = origin.filter(|o| tab.doc.shared_selection().is_some_and(|s| Arc::ptr_eq(&s, &o.result)))?;
            return origin_outline(origin);
        };
        let (home, placement, quad) = (floating.home(), floating.placement(), floating.quad());
        let own = origin.filter(|o| FRect::from(home.outer_bounds()) == o.frame);
        let carried = own.and_then(origin_outline).map(|points| {
            points
                .into_iter()
                .map(|p| {
                    let (u, v) = home.fractions_of(FPoint::new(p.x, p.y));
                    let moved = quad.map_or_else(|| placement.point(u, v), |quad| quad.point(u, v));
                    Pos2::new(moved.x, moved.y)
                })
                .collect()
        });
        carried.or_else(|| {
            let corners = quad.map_or_else(|| placement.corners(), |quad| quad.corners);
            (placement.angle != 0.0 || quad.is_some()).then(|| corners.iter().chain(corners.first()).map(|c| Pos2::new(c.x, c.y)).collect())
        })
    }

    pub(crate) fn begin_marquee(&mut self, image_pos: FPoint, button: PointerButton, mods: Modifiers) {
        self.flush_pending();
        let mode = selection_mode(self.options.select_mode, button, mods);
        let ellipse = self.tool == Tool::EllipseSelect;
        let tab = self.tab_mut();
        let start = clamp_pixel(image_pos, tab);
        tab.gesture = Gesture::Marquee { ellipse, start, current: start, mode, square: mods.shift };
    }

    pub(crate) fn finish_marquee(&mut self, ellipse: bool, start: FPoint, current: FPoint, mode: CombineMode, square: bool) {
        let tab = self.tab_mut();
        let Some(frame) = marquee_frame(tab, start, current, square) else {
            tab.doc.deselect();
            tab.sel_origin = None;
            return;
        };
        let (shape, kind, source) = if ellipse {
            (Selection::from_ellipse(frame, true), HistoryKind::SelectEllipse, SourceShape::Ellipse)
        } else {
            (rect_selection(frame), HistoryKind::SelectRectangle, SourceShape::Rect)
        };
        let origin_tool = if ellipse { Tool::EllipseSelect } else { Tool::RectSelect };
        self.apply_selection(shape, mode, kind, source, frame, origin_tool);
    }

    pub(crate) fn begin_lasso(&mut self, image_pos: FPoint, button: PointerButton, mods: Modifiers) {
        self.flush_pending();
        let mode = selection_mode(self.options.select_mode, button, mods);
        let tab = self.tab_mut();
        let start = clamp_point(image_pos, tab);
        tab.gesture = Gesture::Lasso { points: vec![start], mode };
    }

    pub(crate) fn finish_lasso(&mut self, points: Vec<FPoint>, mode: CombineMode) {
        let tab = self.tab_mut();
        let clamped: Vec<FPoint> = points.into_iter().map(|p| on_grid(clamp_point(p, tab))).collect();
        let smoothing = self.options.lasso_smoothing;
        let shape = Selection::from_polygon(&clamped, smoothing);
        let frame = shape.as_ref().map_or(FRect::default(), |s| s.bounds().into());
        self.apply_selection(shape, mode, HistoryKind::SelectLasso, SourceShape::Polygon(clamped, smoothing), frame, Tool::Lasso);
    }

    fn apply_selection(&mut self, shape: Option<Selection>, mode: CombineMode, kind: HistoryKind, source: SourceShape, frame: FRect, origin_tool: Tool) {
        let tab = self.tab_mut();
        let Some(shape) = shape else {
            tab.doc.deselect();
            tab.sel_origin = None;
            return;
        };
        let base = tab.doc.shared_selection().filter(|_| mode != CombineMode::Replace);
        tab.doc.select(&shape, mode, kind);
        tab.sel_origin = tab.doc.shared_selection().map(|result| SelectionOrigin { shape: source, frame, base, mode, result });
        if mode == CombineMode::Replace {
            self.enter_move_mode(origin_tool);
        }
    }

    pub(crate) fn enter_move_mode(&mut self, origin: Tool) {
        if self.tab().doc.selection().is_some() {
            self.auto_move_origin = Some(origin);
            self.tool = Tool::MovePixels;
        }
    }

    pub(crate) fn sync_auto_tool(&mut self) {
        let Some(origin) = self.auto_move_origin else {
            return;
        };
        if self.tool != Tool::MovePixels {
            self.auto_move_origin = None;
        } else if self.tab().doc.selection().is_none() && matches!(self.tab().gesture, Gesture::None) {
            self.tool = origin;
            self.auto_move_origin = None;
        }
    }

    pub(crate) fn begin_move_selection(&mut self, image_pos: FPoint) {
        let tab = self.tab_mut();
        if let Some(before) = tab.doc.begin_selection_move() {
            tab.gesture = Gesture::MoveSelection { before: Some(before), anchor: image_pos, applied: IPoint::new(0, 0) };
        }
    }

    pub(crate) fn begin_move_pixels(&mut self, image_pos: FPoint, button: PointerButton) {
        if button != PointerButton::Primary {
            return;
        }
        self.lift_pixels();
        let tab = self.tab_mut();
        if tab.doc.floating().is_some() {
            tab.gesture = Gesture::MovePixels { anchor: image_pos, applied: IPoint::new(0, 0) };
        }
    }

    fn lift_pixels(&mut self) {
        let float_mode = self.options.float_mode;
        let tab = self.tab_mut();
        let lifted = tab.doc.begin_move_pixels();
        let mode_area = tab.doc.set_floating_mode(float_mode);
        tab.mark_dirty(lifted.union(mode_area));
    }

    fn outline_frame(&self) -> Option<FRect> {
        let tab = self.tab();
        let current = tab.doc.shared_selection()?;
        let origin = tab.sel_origin.as_ref().filter(|origin| Arc::ptr_eq(&origin.result, &current));
        Some(origin.map_or_else(|| current.bounds().into(), |origin| origin.frame))
    }

    pub(crate) fn selection_gizmo(&self, canvas: Rect) -> Option<Gizmo> {
        let tab = self.tab();
        let selection = tab.doc.selection()?;
        let to_screen = |p: FPoint| tab.view.image_to_screen(canvas, p.x, p.y);
        if self.tool == Tool::MovePixels && self.options.move_method == MoveMethod::Perspective {
            let (quad, pivot) = match tab.doc.floating() {
                Some(floating) => (warp_start(floating), floating.pivot()),
                None => (Quad::from_placement(&Placement::from_bounds(selection.bounds())), chequer_core::document::CENTER_PIVOT),
            };
            let corners = quad.corners.map(|c| to_screen(c));
            let frame = ScreenBox::from_rect(Rect::from_points(&corners));
            return Some(Gizmo { frame, pivot: Some(to_screen(quad.point(pivot.0, pivot.1))), rotatable: true, corners: Some(corners) });
        }
        if self.tool == Tool::MovePixels {
            let (placement, pivot) = match tab.doc.floating() {
                Some(floating) => (floating.placement(), floating.pivot()),
                None => (Placement::from_bounds(selection.bounds()), chequer_core::document::CENTER_PIVOT),
            };
            let half = vec2(placement.width, placement.height) * tab.view.scale() / 2.0;
            let frame = ScreenBox { center: to_screen(placement.center), half, angle: placement.angle };
            return Some(Gizmo { frame, pivot: Some(to_screen(placement.point(pivot.0, pivot.1))), rotatable: true, corners: None });
        }
        let frame = self.outline_frame()?;
        let rect = Rect::from_two_pos(to_screen(FPoint::new(frame.x0, frame.y0)), to_screen(FPoint::new(frame.x1, frame.y1)));
        Some(Gizmo { frame: ScreenBox::from_rect(rect), pivot: None, rotatable: false, corners: None })
    }

    pub(crate) fn handles_visible(&self) -> bool {
        let tab = self.tab();
        shows_handles(self.tool) && tab.live.is_none() && tab.doc.selection().is_some() && !matches!(tab.gesture, Gesture::Marquee { .. } | Gesture::Lasso { .. } | Gesture::Pan { .. })
    }

    pub(crate) fn gizmo_hit(&self, canvas: Rect, pos: Pos2) -> Option<GizmoHit> {
        let gizmo = self.selection_gizmo(canvas)?;
        if gizmo.frame.hits_move_icon(pos) {
            return Some(GizmoHit::MoveIcon);
        }
        if gizmo.pivot.is_some_and(|pivot| hits_pivot(pivot, pos)) {
            return Some(GizmoHit::Pivot);
        }
        let grabbed = match &gizmo.corners {
            Some(corners) => warp_target(corners, pos).map(GizmoHit::Warp),
            None => gizmo.frame.hit_handle(pos).map(GizmoHit::Handle),
        };
        if grabbed.is_some() {
            return grabbed;
        }
        if gizmo.rotatable && in_rotate_band(gizmo.distance_outside(pos)) {
            return Some(GizmoHit::Rotate);
        }
        (gizmo.distance_outside(pos) == 0.0).then_some(GizmoHit::Body)
    }

    pub(crate) fn gizmo_cursor(&self, canvas: Rect, pointer: Option<Pos2>) -> Option<GizmoCursor> {
        let angle = self.selection_gizmo(canvas).map_or(0.0, |gizmo| gizmo.frame.angle);
        match &self.tab().gesture {
            Gesture::ScalePixels { handle, .. } => return Some(GizmoCursor::System(handle.cursor(angle))),
            Gesture::ResizeSelection { handle, .. } => return Some(GizmoCursor::System(handle.cursor(0.0))),
            Gesture::RotatePixels { .. } | Gesture::RotateWarp { .. } => return Some(GizmoCursor::Rotate),
            Gesture::WarpPixels { .. } => return Some(GizmoCursor::System(CursorIcon::Move)),
            Gesture::MovePivot => return Some(GizmoCursor::System(CursorIcon::Move)),
            Gesture::None => {}
            _ => return None,
        }
        if !self.handles_visible() {
            return None;
        }
        match self.gizmo_hit(canvas, pointer?)? {
            GizmoHit::MoveIcon | GizmoHit::Pivot | GizmoHit::Warp(_) => Some(GizmoCursor::System(CursorIcon::Move)),
            GizmoHit::Handle(handle) => Some(GizmoCursor::System(handle.cursor(angle))),
            GizmoHit::Rotate => Some(GizmoCursor::Rotate),
            GizmoHit::Body => None,
        }
    }

    pub(crate) fn try_begin_handle(&mut self, canvas: Rect, pos: Pos2, button: PointerButton) -> bool {
        if button != PointerButton::Primary || !self.handles_visible() {
            return false;
        }
        let image_pos = self.tab().view.screen_to_image(canvas, pos);
        match self.gizmo_hit(canvas, pos) {
            Some(GizmoHit::MoveIcon) if self.tool == Tool::MovePixels => self.begin_move_pixels(image_pos, button),
            Some(GizmoHit::MoveIcon) => self.begin_move_selection(image_pos),
            Some(GizmoHit::Pivot) => self.begin_move_pivot(),
            Some(GizmoHit::Handle(handle)) => return self.begin_handle(handle),
            Some(GizmoHit::Warp(target)) => self.begin_warp(target, image_pos),
            Some(GizmoHit::Rotate) => self.begin_rotate(image_pos),
            Some(GizmoHit::Body) | None => return false,
        }
        !matches!(self.tab().gesture, Gesture::None)
    }

    fn begin_handle(&mut self, handle: Handle) -> bool {
        if self.tool == Tool::MovePixels || self.tab().doc.floating().is_some() {
            self.lift_pixels();
            let tab = self.tab_mut();
            let Some(start) = tab.doc.floating().map(|f| f.placement()) else {
                return false;
            };
            tab.gesture = Gesture::ScalePixels { handle, start };
            return true;
        }
        self.begin_resize_selection(handle)
    }

    fn begin_warp(&mut self, target: WarpTarget, image_pos: FPoint) {
        self.lift_pixels();
        let tab = self.tab_mut();
        let Some(start) = tab.doc.floating().map(warp_start) else {
            return;
        };
        tab.gesture = Gesture::WarpPixels { target, start, anchor: image_pos };
    }

    fn begin_move_pivot(&mut self) {
        self.lift_pixels();
        let tab = self.tab_mut();
        if tab.doc.floating().is_some() {
            tab.gesture = Gesture::MovePivot;
        }
    }

    fn begin_rotate(&mut self, image_pos: FPoint) {
        let perspective = self.options.move_method == MoveMethod::Perspective;
        self.lift_pixels();
        let tab = self.tab_mut();
        let Some(floating) = tab.doc.floating() else {
            return;
        };
        let pivot = floating.pivot_point();
        tab.gesture = if perspective {
            Gesture::RotateWarp { start: warp_start(floating), drag: RotationDrag::new(0.0, pivot, image_pos) }
        } else {
            let start = floating.placement();
            Gesture::RotatePixels { start, drag: RotationDrag::new(start.angle, pivot, image_pos) }
        };
    }

    fn begin_resize_selection(&mut self, handle: Handle) -> bool {
        let Some(current) = self.tab().doc.shared_selection() else {
            return false;
        };
        let tab = self.tab_mut();
        let origin = tab
            .sel_origin
            .clone()
            .filter(|origin| Arc::ptr_eq(&origin.result, &current))
            .unwrap_or_else(|| SelectionOrigin { shape: SourceShape::Mask(current.clone()), frame: current.bounds().into(), base: None, mode: CombineMode::Replace, result: current.clone() });
        let start = origin.frame;
        tab.gesture = Gesture::ResizeSelection { handle, start, origin: origin.clone(), current: origin, before: Some(current) };
        true
    }

    pub(crate) fn handle_dragged(&mut self, image_pos: FPoint, mods: Modifiers) {
        let (method, rotation) = (self.options.move_resample, self.options.rotation);
        let tab = self.tab_mut();
        match &mut tab.gesture {
            Gesture::ScalePixels { handle, start } => {
                let placement = snap_to_pixels(start.dragged(handle.sides(), image_pos, mods.shift));
                let (method, rotation) = drag_quality(&placement, method, rotation);
                let area = tab.doc.place_floating(placement, method, rotation);
                tab.mark_dirty(area);
            }
            Gesture::RotatePixels { start, drag } => {
                let target = drag.target(image_pos, mods);
                let placement = start.rotated_about(drag.pivot, target - start.angle);
                let (method, rotation) = drag_quality(&placement, method, rotation);
                let area = tab.doc.place_floating(placement, method, rotation);
                tab.mark_dirty(area);
            }
            Gesture::WarpPixels { target, start, anchor } => {
                let (dx, dy) = ((image_pos.x - anchor.x).round(), (image_pos.y - anchor.y).round());
                let warped = match *target {
                    WarpTarget::Corner(index) => start.with_corner(index, on_grid(FPoint::new(start.corners[index].x + dx, start.corners[index].y + dy))),
                    WarpTarget::Edge(index) => start.with_edge(index, dx, dy),
                };
                if let Some(quad) = warped {
                    let method = drag_resample(&quad, method);
                    let area = tab.doc.warp_floating(quad, method, rotation);
                    tab.mark_dirty(area);
                }
            }
            Gesture::RotateWarp { start, drag } => {
                let quad = start.rotated_about(drag.pivot, drag.target(image_pos, mods));
                let method = drag_resample(&quad, method);
                let area = tab.doc.warp_floating(quad, method, rotation);
                tab.mark_dirty(area);
            }
            Gesture::MovePivot => {
                if let Some(pivot) = tab.doc.floating().and_then(|f| f.pivot_fractions_of(image_pos)) {
                    tab.doc.set_floating_pivot(pivot);
                }
            }
            Gesture::ResizeSelection { handle, start, origin, current, .. } => {
                let dragged = drag_frame(*start, *handle, image_pos, mods.shift);
                let frame = clip_to(FRect::from(dragged.rounded()), tab.doc.bounds());
                let Some((shape, source)) = origin.shaped(frame) else {
                    return;
                };
                let combined = combine(origin.base.as_deref(), &shape, origin.mode);
                tab.doc.replace_selection_live(combined);
                *current = SelectionOrigin { shape: source, frame, ..origin.clone() };
            }
            _ => {}
        }
    }

    pub(crate) fn finish_scale_pixels(&mut self) {
        let (method, rotation) = (self.options.move_resample, self.options.rotation);
        let tab = self.tab_mut();
        let area = tab.doc.resample_floating(method, rotation);
        tab.mark_dirty(area);
    }

    pub(crate) fn handle_released(&mut self, gesture: Gesture) {
        if let Gesture::ResizeSelection { current, before, .. } = gesture {
            let tab = self.tab_mut();
            tab.doc.record_selection_edit(HistoryKind::ModifySelection, before);
            tab.sel_origin = tab.doc.shared_selection().map(|result| SelectionOrigin { result, ..current });
        }
    }

    pub(crate) fn click_outside_selection(&mut self, canvas: Rect, pos: Pos2, button: PointerButton, mods: Modifiers) -> bool {
        let reach = |gizmo: &crate::selection_ui::Gizmo| if gizmo.rotatable { ROTATE_ZONE_WIDTH } else { HANDLE_REACH };
        let outside = self.selection_gizmo(canvas).is_some_and(|gizmo| gizmo.distance_outside(pos) > reach(&gizmo));
        if !outside || !matches!(self.tool, Tool::MovePixels | Tool::MoveSelection) {
            return false;
        }
        self.flush_pending();
        let tab = self.tab_mut();
        tab.doc.deselect();
        tab.sel_origin = None;
        if let Some(origin) = self.auto_move_origin.take() {
            self.tool = origin;
            let image_pos = self.tab().view.screen_to_image(canvas, pos);
            if origin == Tool::Lasso {
                self.begin_lasso(image_pos, button, mods);
            } else {
                self.begin_marquee(image_pos, button, mods);
            }
        }
        true
    }
}

pub(crate) fn snap_to_pixels(placement: Placement) -> Placement {
    if placement.angle != 0.0 {
        return placement;
    }
    let [top_left, _, bottom_right, _] = placement.corners();
    Placement::from_bounds(FRect::from_corners(top_left, bottom_right).rounded())
}

fn clip_to(frame: FRect, canvas: IRect) -> FRect {
    let canvas = FRect::from(canvas);
    FRect::new(frame.x0.max(canvas.x0), frame.y0.max(canvas.y0), frame.x1.min(canvas.x1), frame.y1.min(canvas.y1)).normalized()
}
