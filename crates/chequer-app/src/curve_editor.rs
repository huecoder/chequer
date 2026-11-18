use chequer_core::{brush, effects};
use eframe::egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextStyle, Ui, Vec2, pos2, vec2};

use crate::skin::{self, Font};

pub const CURVE_EDITOR_SIZE: Vec2 = Vec2::new(150.0, 96.0);
const HANDLE_HALF: f32 = 3.0;
const FRAME_PAD: f32 = HANDLE_HALF + 1.0;
const GRAB_RADIUS: f32 = 9.0;
const SAMPLES: usize = 48;
const GRID_DIVISIONS: usize = 4;
const CURVE_WIDTH: f32 = 1.5;
const GHOST_WIDTH: f32 = 1.0;
const GHOST_OPACITY: f32 = 0.4;
const GRID_OPACITY: f32 = 0.5;
const REFERENCE_OPACITY: f32 = 0.6;
const LABEL_GAP: f32 = 3.0;
const ORIGIN_LABEL: &str = "0";
const CHANNEL_MAX: f32 = 255.0;

pub trait PlotCurve {
    fn nodes(&self) -> Vec<(f32, f32)>;
    fn samples(&self) -> Vec<(f32, f32)>;
    fn insert_node(&mut self, x: f32, y: f32) -> Option<usize>;
    fn remove_node(&mut self, index: usize) -> bool;
    fn move_node(&mut self, index: usize, x: f32, y: f32);
}

impl PlotCurve for brush::Curve {
    fn nodes(&self) -> Vec<(f32, f32)> {
        self.points().to_vec()
    }

    fn samples(&self) -> Vec<(f32, f32)> {
        (0..=SAMPLES).map(|sample| sample as f32 / SAMPLES as f32).map(|x| (x, self.eval(x))).collect()
    }

    fn insert_node(&mut self, x: f32, y: f32) -> Option<usize> {
        self.insert_point(x, y)
    }

    fn remove_node(&mut self, index: usize) -> bool {
        self.remove_point(index)
    }

    fn move_node(&mut self, index: usize, x: f32, y: f32) {
        self.set_point(index, x, y);
    }
}

fn channel(unit: f32) -> u8 {
    (unit * CHANNEL_MAX).round().clamp(0.0, CHANNEL_MAX) as u8
}

impl PlotCurve for effects::Curve {
    fn nodes(&self) -> Vec<(f32, f32)> {
        self.points().iter().map(|&(x, y)| (f32::from(x) / CHANNEL_MAX, f32::from(y) / CHANNEL_MAX)).collect()
    }

    fn samples(&self) -> Vec<(f32, f32)> {
        self.lut().iter().enumerate().map(|(x, &y)| (x as f32 / CHANNEL_MAX, f32::from(y) / CHANNEL_MAX)).collect()
    }

    fn insert_node(&mut self, x: f32, y: f32) -> Option<usize> {
        self.insert(channel(x), channel(y))
    }

    fn remove_node(&mut self, index: usize) -> bool {
        self.remove(index)
    }

    fn move_node(&mut self, index: usize, x: f32, y: f32) {
        self.move_point(index, channel(x), channel(y));
    }
}

pub struct Ghost {
    trace: Vec<(f32, f32)>,
    color: Color32,
}

impl Ghost {
    pub fn of(curve: &impl PlotCurve, color: Color32) -> Self {
        Self { trace: curve.samples(), color }
    }
}

pub struct PlotStyle<'a> {
    size: Vec2,
    max_label: &'a str,
    tint: Option<Color32>,
    ghosts: &'a [Ghost],
}

impl<'a> PlotStyle<'a> {
    pub fn new(size: Vec2, max_label: &'a str) -> Self {
        Self { size, max_label, tint: None, ghosts: &[] }
    }

    pub fn tinted(self, tint: Option<Color32>) -> Self {
        Self { tint, ..self }
    }

    pub fn with_ghosts(self, ghosts: &'a [Ghost]) -> Self {
        Self { ghosts, ..self }
    }
}

pub fn curve_plot(ui: &mut Ui, curve: &mut impl PlotCurve, style: &PlotStyle) -> bool {
    let label_width = skin::text_width(ui.painter(), style.max_label, Font::Body).max(skin::text_width(ui.painter(), ORIGIN_LABEL, Font::Body));
    let label_height = ui.text_style_height(&TextStyle::Body);
    let insets = (label_width + LABEL_GAP + FRAME_PAD, FRAME_PAD, FRAME_PAD, label_height + FRAME_PAD);
    let outer_size = style.size + vec2(insets.0 + insets.2, insets.1 + insets.3);
    let (outer, response) = ui.allocate_exact_size(outer_size, Sense::click_and_drag());
    let plot = Rect::from_min_max(outer.min + vec2(insets.0, insets.1), outer.max - vec2(insets.2, insets.3));
    let to_screen = |(x, y): (f32, f32)| pos2(plot.left() + x * plot.width(), plot.bottom() - y * plot.height());
    let to_unit = |at: Pos2| (((at.x - plot.left()) / plot.width()).clamp(0.0, 1.0), ((plot.bottom() - at.y) / plot.height()).clamp(0.0, 1.0));
    let nearest = |curve: &dyn PlotCurve, at: Pos2| {
        curve.nodes().into_iter().enumerate().map(|(index, node)| (index, to_screen(node).distance(at))).filter(|&(_, distance)| distance <= GRAB_RADIUS).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(index, _)| index)
    };
    let grab_id = response.id.with("grab");
    let mut changed = false;
    if let Some(at) = response.interact_pointer_pos() {
        if response.double_clicked() || response.secondary_clicked() {
            if let Some(index) = nearest(curve, at) {
                changed |= curve.remove_node(index);
            }
        } else if response.clicked() && nearest(curve, at).is_none() {
            let (x, y) = to_unit(at);
            changed |= curve.insert_node(x, y).is_some();
        }
        if response.drag_started() {
            let grabbed = nearest(curve, at).or_else(|| {
                let (x, y) = to_unit(at);
                curve.insert_node(x, y)
            });
            changed |= grabbed.is_some();
            ui.data_mut(|data| data.insert_temp(grab_id, grabbed));
        }
        if response.dragged() {
            if let Some(index) = ui.data(|data| data.get_temp::<Option<usize>>(grab_id)).flatten() {
                let (x, y) = to_unit(at);
                curve.move_node(index, x, y);
                changed = true;
            }
        }
    }
    if response.drag_stopped() {
        ui.data_mut(|data| data.insert_temp::<Option<usize>>(grab_id, None));
    }

    let theme = skin::theme();
    let painter = ui.painter_at(outer);
    painter.add(Shape::rect_filled(plot, 0.0, theme.field));
    let grid = Stroke::new(1.0, theme.line_soft.gamma_multiply(GRID_OPACITY));
    for step in 1..GRID_DIVISIONS {
        let fraction = step as f32 / GRID_DIVISIONS as f32;
        let (across, along) = (to_screen((fraction, 0.0)).x, to_screen((0.0, fraction)).y);
        painter.add(Shape::line_segment([pos2(across, plot.top()), pos2(across, plot.bottom())], grid));
        painter.add(Shape::line_segment([pos2(plot.left(), along), pos2(plot.right(), along)], grid));
    }
    painter.add(Shape::line_segment([to_screen((0.0, 0.0)), to_screen((1.0, 1.0))], Stroke::new(1.0, theme.ink_faint.gamma_multiply(REFERENCE_OPACITY))));
    for ghost in style.ghosts {
        let trace = ghost.trace.iter().map(|&point| to_screen(point)).collect();
        painter.add(Shape::line(trace, Stroke::new(GHOST_WIDTH, ghost.color.gamma_multiply(GHOST_OPACITY))));
    }
    let trace = curve.samples().into_iter().map(to_screen).collect();
    painter.add(Shape::line(trace, Stroke::new(CURVE_WIDTH, style.tint.unwrap_or(theme.ink))));
    for node in curve.nodes() {
        let handle = Rect::from_center_size(to_screen(node), Vec2::splat(HANDLE_HALF * 2.0));
        painter.add(Shape::rect_filled(handle, 0.0, theme.highlight));
        painter.add(Shape::rect_stroke(handle, 0.0, Stroke::new(1.0, theme.ink), StrokeKind::Outside));
    }
    painter.add(Shape::rect_stroke(plot, 0.0, Stroke::new(1.0, theme.line), StrokeKind::Inside));
    let label_ink = theme.ink_faint;
    let left_edge = plot.left() - FRAME_PAD - LABEL_GAP;
    let below = plot.bottom() + FRAME_PAD;
    skin::text_at(&painter, pos2(left_edge, plot.top()), Align2::RIGHT_TOP, style.max_label, Font::Body, label_ink);
    skin::text_at(&painter, pos2(left_edge, plot.bottom()), Align2::RIGHT_BOTTOM, ORIGIN_LABEL, Font::Body, label_ink);
    skin::text_at(&painter, pos2(plot.left(), below), Align2::LEFT_TOP, ORIGIN_LABEL, Font::Body, label_ink);
    skin::text_at(&painter, pos2(plot.right(), below), Align2::RIGHT_TOP, style.max_label, Font::Body, label_ink);
    changed
}
