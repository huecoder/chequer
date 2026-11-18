use eframe::egui::scroll_area::ScrollAreaOutput;
use eframe::egui::{self, Color32, Id, Mesh, Painter, Pos2, Rect, Shape, Ui, Vec2, pos2};

use crate::skin;

const FADE_POINTS: f32 = 14.0;
const FADE_PIXELS: f32 = 8.0;
const FADE_ALPHA: f32 = 0.95;
const FADE_FALLOFF: f32 = 1.6;
const GLOW_POINTS: f32 = 30.0;
const GLOW_PIXELS: f32 = 14.0;
const GLOW_ALPHA: f32 = 0.3;
const GLOW_FALLOFF: f32 = 1.1;
const GLOW_SECONDS: f64 = 0.65;
const GLOW_FLOOR: f32 = 0.55;
const GLOW_GAIN_PER_POINT: f32 = 0.025;
const MAX_DEPTH_SHARE: f32 = 0.4;
const EDGE_TOLERANCE: f32 = 0.5;
const SMOOTH_ROWS: usize = 8;
const PIXEL_LEVELS: f32 = 12.0;
const LIGHT_BACKGROUND_LUMA: f32 = 0.88;
const GLOW_WHITENING: f32 = 0.4;
const RED_LUMA: f32 = 0.2126;
const GREEN_LUMA: f32 = 0.7152;
const BLUE_LUMA: f32 = 0.0722;
const FAST_SCROLL_FACTOR: f32 = 8.0;

pub fn allow_fast_scroll(ctx: &egui::Context) {
    ctx.options_mut(|options| options.input_options.zoom_modifier = egui::Modifiers::NONE);
}

pub fn boost_fast_scroll(ctx: &egui::Context) {
    ctx.input_mut(|input| {
        if input.modifiers.command {
            input.smooth_scroll_delta *= FAST_SCROLL_FACTOR;
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

const EDGES: [Edge; 4] = [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right];

impl Edge {
    fn index(self) -> usize {
        match self {
            Self::Top => 0,
            Self::Bottom => 1,
            Self::Left => 2,
            Self::Right => 3,
        }
    }

    fn axis(self) -> usize {
        match self {
            Self::Top | Self::Bottom => 1,
            Self::Left | Self::Right => 0,
        }
    }

    fn is_leading(self) -> bool {
        matches!(self, Self::Top | Self::Left)
    }

    fn strip(self, area: Rect, depth: f32) -> Rect {
        match self {
            Self::Top => Rect::from_min_max(area.min, pos2(area.max.x, area.min.y + depth)),
            Self::Bottom => Rect::from_min_max(pos2(area.min.x, area.max.y - depth), area.max),
            Self::Left => Rect::from_min_max(area.min, pos2(area.min.x + depth, area.max.y)),
            Self::Right => Rect::from_min_max(pos2(area.max.x - depth, area.min.y), area.max),
        }
    }

    fn point(self, strip: Rect, along: f32, inward: f32) -> Pos2 {
        let blend = |from: f32, to: f32, amount: f32| from + (to - from) * amount;
        match self {
            Self::Top => pos2(blend(strip.min.x, strip.max.x, along), blend(strip.min.y, strip.max.y, inward)),
            Self::Bottom => pos2(blend(strip.min.x, strip.max.x, along), blend(strip.max.y, strip.min.y, inward)),
            Self::Left => pos2(blend(strip.min.x, strip.max.x, inward), blend(strip.min.y, strip.max.y, along)),
            Self::Right => pos2(blend(strip.max.x, strip.min.x, inward), blend(strip.min.y, strip.max.y, along)),
        }
    }

    fn along_length(self, strip: Rect) -> f32 {
        if self.axis() == 1 { strip.width() } else { strip.height() }
    }

    fn depth_length(self, strip: Rect) -> f32 {
        if self.axis() == 1 { strip.height() } else { strip.width() }
    }
}

struct Region {
    viewport: Rect,
    offset: Vec2,
    overflow: Vec2,
}

impl Region {
    fn scrollable(&self, axis: usize) -> bool {
        self.overflow[axis] > EDGE_TOLERANCE
    }

    fn remaining(&self, edge: Edge) -> f32 {
        let axis = edge.axis();
        if !self.scrollable(axis) {
            return 0.0;
        }
        if edge.is_leading() { self.offset[axis] } else { self.overflow[axis] - self.offset[axis] }
    }

    fn at_edge(&self, edge: Edge) -> bool {
        self.scrollable(edge.axis()) && self.remaining(edge) <= EDGE_TOLERANCE
    }

    fn with_bar_tracks(&self, ui: &Ui, area: Rect) -> Rect {
        let scroll = ui.spacing().scroll;
        let mut full = area;
        if scroll.floating {
            return self.viewport;
        }
        if self.scrollable(1) {
            full.max.x += scroll.allocated_width();
        }
        if self.scrollable(0) {
            full.max.y += scroll.allocated_width();
        }
        full
    }

    fn visible_area(&self, ui: &Ui) -> Rect {
        let scroll = ui.spacing().scroll;
        let mut area = self.viewport;
        if scroll.floating {
            if self.scrollable(1) {
                area.max.x -= scroll.bar_width;
            }
            if self.scrollable(0) {
                area.max.y -= scroll.bar_width;
            }
        }
        area
    }
}

#[derive(Clone, Copy, Default)]
struct Glow {
    peak: f32,
    since: f64,
}

impl Glow {
    fn level(self, now: f64) -> f32 {
        let elapsed = ((now - self.since) / GLOW_SECONDS) as f32;
        if elapsed >= 1.0 { 0.0 } else { self.peak * (1.0 - elapsed.max(0.0)).powi(2) }
    }

    fn retrigger(&mut self, now: f64, peak: f32) {
        *self = Self { peak: self.level(now).max(peak), since: now };
    }
}

#[derive(Clone, Copy, Default)]
struct GlowState {
    edges: [Glow; 4],
}

pub trait ScrollFx: Sized {
    fn with_fx(self, ui: &Ui, background: Color32) -> Self;
}

impl<R> ScrollFx for ScrollAreaOutput<R> {
    fn with_fx(self, ui: &Ui, background: Color32) -> Self {
        let region = Region { viewport: self.inner_rect, offset: self.state.clamped_offset(), overflow: (self.content_size - self.inner_rect.size()).max(Vec2::ZERO) };
        let area = region.visible_area(ui);
        let painter = ui.painter().with_clip_rect(area.intersect(ui.clip_rect()));
        paint_fades(&painter, &region, area, background);
        let glow_area = region.with_bar_tracks(ui, area);
        let glow_painter = ui.painter().with_clip_rect(glow_area.intersect(ui.clip_rect()));
        paint_glow(ui, &glow_painter, self.id, &region, glow_area, background);
        self
    }
}

fn paint_fades(painter: &Painter, region: &Region, area: Rect, background: Color32) {
    for edge in EDGES {
        let depth = depth_for(painter, edge, area, FADE_POINTS, FADE_PIXELS);
        let strength = (region.remaining(edge) / depth).clamp(0.0, 1.0);
        if strength > 0.0 {
            let fade = |_: f32, inward: f32| FADE_ALPHA * strength * (1.0 - inward).powf(FADE_FALLOFF);
            paint_field(painter, edge.strip(area, depth), edge, background, 1, fade);
        }
    }
}

fn paint_glow(ui: &Ui, painter: &Painter, id: Id, region: &Region, area: Rect, background: Color32) {
    let now = ui.input(|input| input.time);
    let mut state: GlowState = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let attempts = attempted_edges(ui, region);
    for &(edge, peak) in &attempts {
        state.edges[edge.index()].retrigger(now, peak);
    }
    let color = glow_color(background);
    let mut active = false;
    for edge in EDGES {
        let level = state.edges[edge.index()].level(now);
        if level <= 0.0 {
            continue;
        }
        active = true;
        let depth = depth_for(painter, edge, area, GLOW_POINTS, GLOW_PIXELS);
        let glow = |_: f32, inward: f32| GLOW_ALPHA * level * (1.0 - inward).powf(GLOW_FALLOFF);
        paint_field_smooth(painter, edge.strip(area, depth), edge, color, 1, glow);
    }
    if active || !attempts.is_empty() {
        ui.data_mut(|data| data.insert_temp(id, state));
    }
    if active {
        ui.ctx().request_repaint();
    }
}

fn attempted_edges(ui: &Ui, region: &Region) -> Vec<(Edge, f32)> {
    let nested = ui.stack().iter().any(|node| node.kind() == Some(egui::UiKind::ScrollArea));
    if nested || !ui.is_enabled() || !ui.rect_contains_pointer(region.viewport) {
        return Vec::new();
    }
    let push = ui.input(|input| {
        let dragging = if input.any_touches() && input.pointer.primary_down() { input.pointer.delta() } else { Vec2::ZERO };
        input.smooth_scroll_delta() + dragging
    });
    [(Edge::Top, push.y), (Edge::Bottom, -push.y), (Edge::Left, push.x), (Edge::Right, -push.x)]
        .into_iter()
        .filter(|&(edge, amount)| amount > 0.0 && region.at_edge(edge))
        .map(|(edge, amount)| (edge, (GLOW_FLOOR + amount * GLOW_GAIN_PER_POINT).min(1.0)))
        .collect()
}

fn glow_color(background: Color32) -> Color32 {
    let highlight = skin::theme().highlight;
    if luma(background) >= LIGHT_BACKGROUND_LUMA {
        return highlight;
    }
    let base = if luma(highlight) > luma(background) { highlight } else { Color32::WHITE };
    base.lerp_to_gamma(Color32::WHITE, GLOW_WHITENING)
}

fn luma(color: Color32) -> f32 {
    (RED_LUMA * f32::from(color.r()) + GREEN_LUMA * f32::from(color.g()) + BLUE_LUMA * f32::from(color.b())) / f32::from(u8::MAX)
}

fn skin_pixel(painter: &Painter) -> f32 {
    1.0 / skin::grid_ppp(painter.pixels_per_point())
}

fn snap(painter: &Painter, value: f32) -> f32 {
    let scale = skin::grid_ppp(painter.pixels_per_point());
    (value * scale).round() / scale
}

fn depth_for(painter: &Painter, edge: Edge, area: Rect, points: f32, pixels: f32) -> f32 {
    let wanted = if skin::pixel_mode() { pixels * skin_pixel(painter) } else { points };
    let extent = if edge.axis() == 1 { area.height() } else { area.width() };
    wanted.min(extent * MAX_DEPTH_SHARE).max(f32::EPSILON)
}

fn paint_field(painter: &Painter, strip: Rect, edge: Edge, color: Color32, columns: usize, alpha_at: impl Fn(f32, f32) -> f32) {
    if skin::pixel_mode() {
        paint_field_stepped(painter, strip, edge, color, alpha_at);
    } else {
        paint_field_smooth(painter, strip, edge, color, columns, alpha_at);
    }
}

fn paint_field_smooth(painter: &Painter, strip: Rect, edge: Edge, color: Color32, columns: usize, alpha_at: impl Fn(f32, f32) -> f32) {
    let mut mesh = Mesh::default();
    for row in 0..=SMOOTH_ROWS {
        for column in 0..=columns {
            let (along, inward) = (column as f32 / columns as f32, row as f32 / SMOOTH_ROWS as f32);
            mesh.colored_vertex(edge.point(strip, along, inward), color.gamma_multiply(alpha_at(along, inward)));
        }
    }
    let stride = columns as u32 + 1;
    for row in 0..SMOOTH_ROWS as u32 {
        for column in 0..columns as u32 {
            let corner = row * stride + column;
            mesh.add_triangle(corner, corner + 1, corner + stride);
            mesh.add_triangle(corner + 1, corner + stride + 1, corner + stride);
        }
    }
    painter.add(Shape::mesh(mesh));
}

fn cover_strip_side(cells: &mut Rect, strip: Rect, axis: usize, leading: bool) {
    if leading {
        cells.min[axis] = strip.min[axis];
    } else {
        cells.max[axis] = strip.max[axis];
    }
}

fn paint_field_stepped(painter: &Painter, strip: Rect, edge: Edge, color: Color32, alpha_at: impl Fn(f32, f32) -> f32) {
    let cell = skin_pixel(painter);
    let rows = (edge.depth_length(strip) / cell).floor().max(1.0) as usize;
    let columns = (edge.along_length(strip) / cell).ceil().max(1.0) as usize;
    let level_at = |row: usize, column: usize| {
        let (along, inward) = ((column as f32 + 0.5) / columns as f32, (row as f32 + 0.5) / rows as f32);
        (alpha_at(along, inward).clamp(0.0, 1.0) * PIXEL_LEVELS).round() / PIXEL_LEVELS
    };
    for row in 0..rows {
        let (near, far) = (row as f32 / rows as f32, (row + 1) as f32 / rows as f32);
        let mut run_start = 0;
        for column in 1..=columns {
            let run_level = level_at(row, run_start);
            if column < columns && level_at(row, column) == run_level {
                continue;
            }
            if run_level > 0.0 {
                let from = edge.point(strip, run_start as f32 / columns as f32, near);
                let to = edge.point(strip, column as f32 / columns as f32, far);
                let mut cells = Rect::from_two_pos(pos2(snap(painter, from.x), snap(painter, from.y)), pos2(snap(painter, to.x), snap(painter, to.y)));
                let along_axis = 1 - edge.axis();
                if row == 0 {
                    cover_strip_side(&mut cells, strip, edge.axis(), edge.is_leading());
                }
                if run_start == 0 {
                    cover_strip_side(&mut cells, strip, along_axis, true);
                }
                if column == columns {
                    cover_strip_side(&mut cells, strip, along_axis, false);
                }
                painter.rect_filled(cells, 0.0, color.gamma_multiply(run_level));
            }
            run_start = column;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glow_starts_at_its_peak_and_is_gone_after_the_duration() {
        let mut glow = Glow::default();
        glow.retrigger(10.0, 0.8);
        assert!((glow.level(10.0) - 0.8).abs() < f32::EPSILON);
        assert!(glow.level(10.0 + GLOW_SECONDS / 2.0) < 0.8);
        assert_eq!(glow.level(10.0 + GLOW_SECONDS + 0.01), 0.0);
    }

    #[test]
    fn no_edge_has_content_beyond_it_when_everything_fits() {
        let region = Region { viewport: Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0)), offset: Vec2::ZERO, overflow: Vec2::ZERO };
        assert!(EDGES.iter().all(|&edge| region.remaining(edge) == 0.0 && !region.at_edge(edge)));
    }

    #[test]
    fn scrolled_to_the_top_only_the_bottom_has_content_left() {
        let region = Region { viewport: Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0)), offset: Vec2::ZERO, overflow: Vec2::new(0.0, 60.0) };
        assert!(region.at_edge(Edge::Top));
        assert_eq!(region.remaining(Edge::Bottom), 60.0);
        assert!(!region.at_edge(Edge::Bottom));
    }
}
