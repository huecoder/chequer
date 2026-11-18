
use chequer_core::blend::BlendMode;
use chequer_core::geom::{FPoint, IPoint, IRect};
use chequer_core::gradient::{GradientKind, GradientParams, render_gradient};
use chequer_core::history::HistoryKind;
use chequer_core::raster::Rgba;
use chequer_core::selection::{CombineMode, Selection, flood_select};
use eframe::egui::{Modifiers, PointerButton, Pos2, Rect};

use crate::app::App;
use crate::canvas::{Gesture, clamp_point, selection_mode};
use crate::ants::Ants;
use crate::handles::{HANDLE_REACH, TOUCH_HANDLE_REACH, add_handle_ring, near_handle, near_handle_within};
use crate::tools::Sampling;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnobTarget {
    Seed,
    GradientStart,
    GradientEnd,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FillParams {
    seed: IPoint,
    color: Rgba,
    tolerance: f32,
    global: bool,
    blend: BlendMode,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WandParams {
    seed: IPoint,
    tolerance: f32,
    global: bool,
    sampling: Sampling,
}

pub struct FillOp {
    seed: IPoint,
    secondary: bool,
    force_global: bool,
    shown: Option<FillParams>,
}

pub struct WandOp {
    seed: IPoint,
    mode: CombineMode,
    force_global: bool,
    shown: Option<WandParams>,
    pub preview: Option<Selection>,
}

pub struct GradientOp {
    start: FPoint,
    end: FPoint,
    shown: Option<(GradientParams, IRect, BlendMode)>,
}

pub enum LiveOp {
    Fill(FillOp),
    Wand(WandOp),
    Gradient(GradientOp),
}

fn pixel_center(p: IPoint) -> FPoint {
    FPoint::new(p.x as f32 + 0.5, p.y as f32 + 0.5)
}

impl App {

    pub(crate) fn start_fill(&mut self, image_pos: FPoint, button: PointerButton, mods: Modifiers) {
        self.commit_live();
        let op = FillOp { seed: image_pos.floor(), secondary: button == PointerButton::Secondary, force_global: mods.shift, shown: None };
        let tab = self.tab_mut();
        tab.live = Some(LiveOp::Fill(op));
        tab.gesture = Gesture::Knob(KnobTarget::Seed);
        self.refresh_live();
    }

    pub(crate) fn start_wand(&mut self, image_pos: FPoint, button: PointerButton, mods: Modifiers) {
        self.commit_live();
        let mode = selection_mode(self.options.select_mode, button, mods);
        let op = WandOp { seed: image_pos.floor(), mode, force_global: mods.shift, shown: None, preview: None };
        let tab = self.tab_mut();
        tab.live = Some(LiveOp::Wand(op));
        tab.gesture = Gesture::Knob(KnobTarget::Seed);
        self.refresh_live();
    }

    pub(crate) fn start_gradient(&mut self, image_pos: FPoint) {
        self.commit_live();
        let tab = self.tab_mut();
        let start = clamp_point(image_pos, tab);
        tab.live = Some(LiveOp::Gradient(GradientOp { start, end: start, shown: None }));
        tab.gesture = Gesture::Knob(KnobTarget::GradientEnd);
        self.refresh_live();
    }

    pub(crate) fn live_knobs(&self, canvas: Rect) -> Vec<(KnobTarget, Pos2)> {
        let tab = self.tab();
        let at = |p: FPoint| tab.view.image_to_screen(canvas, p.x, p.y);
        match &tab.live {
            Some(LiveOp::Fill(op)) => vec![(KnobTarget::Seed, at(pixel_center(op.seed)))],
            Some(LiveOp::Wand(op)) => vec![(KnobTarget::Seed, at(pixel_center(op.seed)))],
            Some(LiveOp::Gradient(op)) => vec![(KnobTarget::GradientStart, at(op.start)), (KnobTarget::GradientEnd, at(op.end))],
            None => Vec::new(),
        }
    }

    pub(crate) fn knob_at(&self, canvas: Rect, pos: Pos2) -> Option<KnobTarget> {
        let reach = if self.touch.is_active() { TOUCH_HANDLE_REACH } else { HANDLE_REACH };
        self.live_knobs(canvas)
            .into_iter()
            .filter(|(_, at)| near_handle_within(*at, pos, reach))
            .min_by(|a, b| a.1.distance(pos).total_cmp(&b.1.distance(pos)))
            .map(|(target, _)| target)
    }

    pub(crate) fn try_begin_knob(&mut self, canvas: Rect, pos: Pos2, button: PointerButton) -> bool {
        if button != PointerButton::Primary {
            return false;
        }
        match self.knob_at(canvas, pos) {
            Some(target) => {
                self.tab_mut().gesture = Gesture::Knob(target);
                true
            }
            None => false,
        }
    }

    pub(crate) fn knob_dragged(&mut self, target: KnobTarget, image_pos: FPoint, mods: Modifiers) {
        let tab = self.tab_mut();
        let point = clamp_point(image_pos, tab);
        match (&mut tab.live, target) {
            (Some(LiveOp::Fill(op)), KnobTarget::Seed) => op.seed = point_in_canvas(point, tab.doc.bounds()),
            (Some(LiveOp::Wand(op)), KnobTarget::Seed) => op.seed = point_in_canvas(point, tab.doc.bounds()),
            (Some(LiveOp::Gradient(op)), KnobTarget::GradientStart) => op.start = ruled(op.end, point, mods.shift),
            (Some(LiveOp::Gradient(op)), KnobTarget::GradientEnd) => op.end = ruled(op.start, point, mods.shift),
            _ => {}
        }
        self.refresh_live();
    }

    pub(crate) fn refresh_live(&mut self) {
        let options = self.options.clone();
        let (primary, secondary) = (self.colors.primary, self.colors.secondary);
        let tab = self.tab_mut();
        match &mut tab.live {
            Some(LiveOp::Fill(op)) => {
                let color = if op.secondary { secondary } else { primary };
                let params = FillParams { seed: op.seed, color, tolerance: options.tolerance, global: options.flood_global || op.force_global, blend: options.blend };
                if op.shown != Some(params) {
                    op.shown = Some(params);
                    let overlay = tab.doc.fill_overlay(params.seed, params.color, params.tolerance, !params.global, params.blend);
                    let area = tab.doc.set_preview(overlay);
                    tab.mark_dirty(area);
                }
            }
            Some(LiveOp::Wand(op)) => {
                let params = WandParams { seed: op.seed, tolerance: options.tolerance, global: options.flood_global || op.force_global, sampling: options.sampling };
                if op.shown != Some(params) {
                    op.shown = Some(params);
                    op.preview = match params.sampling {
                        Sampling::Layer => flood_select(tab.doc.active_raster(), params.seed, params.tolerance, !params.global, tab.doc.tiled()),
                        Sampling::Image => flood_select(&tab.doc.flattened(tab.doc.bounds()), params.seed, params.tolerance, !params.global, tab.doc.tiled()),
                    };
                }
            }
            Some(LiveOp::Gradient(op)) => {
                let area = tab.doc.selection().map_or(tab.doc.bounds(), Selection::bounds);
                let params = GradientParams { kind: options.gradient_kind, start: op.start, end: op.end, from: primary, to: secondary };
                if op.shown != Some((params, area, options.blend)) {
                    op.shown = Some((params, area, options.blend));
                    let overlay = render_gradient(&params, area, options.blend);
                    let dirty = tab.doc.set_preview(overlay);
                    tab.mark_dirty(dirty);
                }
            }
            None => {}
        }
    }

    pub(crate) fn commit_live(&mut self) {
        let Some(op) = self.tab_mut().live.take() else {
            return;
        };
        let tab = self.tab_mut();
        match op {
            LiveOp::Fill(_) => {
                crate::device::vibrate(crate::device::Haptic::Click);
                let area = tab.doc.commit_preview(HistoryKind::Fill);
                tab.mark_dirty(area);
            }
            LiveOp::Gradient(_) => {
                crate::device::vibrate(crate::device::Haptic::Click);
                let area = tab.doc.commit_preview(HistoryKind::Gradient);
                tab.mark_dirty(area);
            }
            LiveOp::Wand(op) => match op.preview {
                Some(shape) => tab.doc.select(&shape, op.mode, HistoryKind::SelectWand),
                None => tab.doc.deselect(),
            },
        }
    }

    pub(crate) fn wand_preview_contours(&self, canvas: Rect) -> Vec<Vec<Pos2>> {
        let tab = self.tab();
        let Some(LiveOp::Wand(WandOp { preview: Some(shape), .. })) = &tab.live else {
            return Vec::new();
        };
        shape.contours().iter().map(|contour| contour.iter().map(|p| tab.view.image_to_screen(canvas, p.x as f32, p.y as f32)).collect()).collect()
    }

    pub(crate) fn add_live_knobs(&self, ants: &mut Ants, canvas: Rect, hover: Option<Pos2>) {
        let knobs = self.live_knobs(canvas);
        if let [(_, a), (_, b)] = knobs[..] {
            ants.solid(&[a, b]);
        }
        let dragging = match self.tab().gesture {
            Gesture::Knob(target) => Some(target),
            _ => None,
        };
        for (target, at) in knobs {
            let hot = dragging == Some(target) || hover.is_some_and(|pointer| near_handle(at, pointer));
            add_handle_ring(ants, at, hot);
        }
    }

    pub(crate) fn gradient_kinds() -> [GradientKind; 5] {
        GradientKind::ALL
    }
}

fn ruled(anchor: FPoint, point: FPoint, shift: bool) -> FPoint {
    crate::canvas::constrain_corner(crate::canvas::ShapeGeometry::Line, anchor, point, shift, true)
}

fn point_in_canvas(point: FPoint, canvas: IRect) -> IPoint {
    let floor = point.floor();
    IPoint::new(floor.x.clamp(canvas.x0, canvas.x1 - 1), floor.y.clamp(canvas.y0, canvas.y1 - 1))
}
