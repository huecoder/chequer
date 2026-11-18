
use std::time::Duration;

use eframe::egui::epaint::{Vertex, WHITE_UV};
use eframe::egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, TextureId, vec2};

pub const DASH_PX: f32 = 4.0;
const PERIOD_PX: f32 = DASH_PX * 2.0;
const SPEED_PX_PER_SECOND: f32 = 10.0;
const SHADOW_REACH_PX: f32 = 2.0;
const SHADOW_OUTER_ALPHA: u8 = 22;
const SHADOW_INNER_ALPHA: u8 = 26;
const BLACK_U: f32 = 0.25;
const WHITE_U: f32 = 0.75;
const SOLID_HALO_PX: f32 = 1.5;
pub const REPAINT_INTERVAL: Duration = Duration::from_millis(80);
pub const PATTERN_WIDTH: usize = PERIOD_PX as usize;

pub struct Ants<'a> {
    painter: &'a Painter,
    ppp: f32,
    clip: Rect,
    phase_px: f32,
    shadow: Mesh,
    dashes: Mesh,
    animated: bool,
}

impl<'a> Ants<'a> {
    pub fn new(painter: &'a Painter, pattern: TextureId, time: f64) -> Self {
        let ppp = painter.pixels_per_point();
        let clip = painter.clip_rect();
        Self {
            painter,
            ppp,
            clip: Rect::from_min_max(clip.min * ppp, clip.max * ppp),
            phase_px: (time * SPEED_PX_PER_SECOND as f64).rem_euclid(PERIOD_PX as f64) as f32,
            shadow: Mesh::default(),
            dashes: Mesh::with_texture(pattern),
            animated: false,
        }
    }

    fn quad(mesh: &mut Mesh, ppp: f32, corners: [Pos2; 4], uv: [Pos2; 4], color: Color32) {
        let base = mesh.vertices.len() as u32;
        for (corner, uv) in corners.into_iter().zip(uv) {
            mesh.vertices.push(Vertex { pos: corner / ppp, uv, color });
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }

    fn pattern_u(&self, arc_px: f32) -> f32 {
        (arc_px - self.phase_px) / PERIOD_PX
    }

    fn shadow_color(alpha: u8) -> Color32 {
        Color32::from_black_alpha(alpha)
    }

    pub fn edge(&mut self, from: Pos2, to: Pos2, arc_px: f32) -> f32 {
        self.animated = true;
        let (a, b) = (from * self.ppp, to * self.ppp);
        let horizontal = (b.x - a.x).abs() >= (b.y - a.y).abs();
        let (start, end, cross) = if horizontal { (a.x.round(), b.x.round(), a.y.round()) } else { (a.y.round(), b.y.round(), a.x.round()) };
        let length = (end - start).abs();
        if length == 0.0 {
            return 0.0;
        }
        let direction = (end - start).signum();
        let inside_after = if horizontal { direction > 0.0 } else { direction < 0.0 };
        let (low, high) = if inside_after { (cross, cross + 1.0) } else { (cross - 1.0, cross) };
        let (clip_along, clip_cross) = if horizontal {
            ((self.clip.min.x, self.clip.max.x), (self.clip.min.y, self.clip.max.y))
        } else {
            ((self.clip.min.y, self.clip.max.y), (self.clip.min.x, self.clip.max.x))
        };
        if high + SHADOW_REACH_PX < clip_cross.0 || low - SHADOW_REACH_PX > clip_cross.1 {
            return length;
        }
        let (lo, hi) = (start.min(end).max(clip_along.0), start.max(end).min(clip_along.1));
        if lo >= hi {
            return length;
        }
        let arc_at = |along: f32| arc_px + (along - start) * direction;
        let (u_lo, u_hi) = (self.pattern_u(arc_at(lo)), self.pattern_u(arc_at(hi)));
        let point = |along: f32, across: f32| if horizontal { Pos2::new(along, across) } else { Pos2::new(across, along) };
        let band = |reach: f32| [point(lo, low - reach), point(hi, low - reach), point(hi, high + reach), point(lo, high + reach)];
        let flat = [WHITE_UV; 4];
        Self::quad(&mut self.shadow, self.ppp, band(SHADOW_REACH_PX), flat, Self::shadow_color(SHADOW_OUTER_ALPHA));
        Self::quad(&mut self.shadow, self.ppp, band(SHADOW_REACH_PX / 2.0), flat, Self::shadow_color(SHADOW_INNER_ALPHA));
        let uv = [Pos2::new(u_lo, 0.5), Pos2::new(u_hi, 0.5), Pos2::new(u_hi, 0.5), Pos2::new(u_lo, 0.5)];
        Self::quad(&mut self.dashes, self.ppp, band(0.0), uv, Color32::WHITE);
        length
    }

    pub fn contour(&mut self, points: &[Pos2]) {
        let mut arc = 0.0;
        for pair in points.windows(2) {
            arc += self.edge(pair[0], pair[1], arc);
        }
    }

    pub fn rect(&mut self, rect: Rect) {
        self.contour(&[rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()]);
    }

    pub fn polyline(&mut self, points: &[Pos2]) {
        self.animated = true;
        let mut arc = 0.0;
        for pair in points.windows(2) {
            let (a, b) = (pair[0] * self.ppp, pair[1] * self.ppp);
            let along = b - a;
            let length = along.length();
            if length < f32::EPSILON {
                continue;
            }
            let normal = vec2(-along.y, along.x) / length;
            if Rect::from_two_pos(a, b).expand(SHADOW_REACH_PX + 1.0).intersects(self.clip) {
                let (u0, u1) = (self.pattern_u(arc), self.pattern_u(arc + length));
                let strip = |half: f32| [a - normal * half, b - normal * half, b + normal * half, a + normal * half];
                let flat = [WHITE_UV; 4];
                Self::quad(&mut self.shadow, self.ppp, strip(SHADOW_REACH_PX + 0.5), flat, Self::shadow_color(SHADOW_OUTER_ALPHA));
                Self::quad(&mut self.shadow, self.ppp, strip(SHADOW_REACH_PX / 2.0 + 0.5), flat, Self::shadow_color(SHADOW_INNER_ALPHA));
                let uv = [Pos2::new(u0, 0.5), Pos2::new(u1, 0.5), Pos2::new(u1, 0.5), Pos2::new(u0, 0.5)];
                Self::quad(&mut self.dashes, self.ppp, strip(0.5), uv, Color32::WHITE);
            }
            arc += length;
        }
    }

    pub fn solid(&mut self, points: &[Pos2]) {
        let (black, white) = (Pos2::new(BLACK_U, 0.5), Pos2::new(WHITE_U, 0.5));
        for pair in points.windows(2) {
            let (a, b) = (pair[0] * self.ppp, pair[1] * self.ppp);
            let along = b - a;
            let length = along.length();
            if length < f32::EPSILON || !Rect::from_two_pos(a, b).expand(SOLID_HALO_PX + 1.0).intersects(self.clip) {
                continue;
            }
            let normal = vec2(-along.y, along.x) / length;
            let strip = |half: f32| [a - normal * half, b - normal * half, b + normal * half, a + normal * half];
            Self::quad(&mut self.dashes, self.ppp, strip(SOLID_HALO_PX), [white; 4], Color32::WHITE);
            Self::quad(&mut self.dashes, self.ppp, strip(0.5), [black; 4], Color32::WHITE);
        }
    }

    pub fn finish(self) {
        if self.dashes.is_empty() {
            return;
        }
        self.painter.add(Shape::mesh(self.shadow));
        self.painter.add(Shape::mesh(self.dashes));
        if self.animated {
            self.painter.ctx().request_repaint_after(REPAINT_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Context;

    fn painter(ctx: &Context) -> Painter {
        ctx.layer_painter(eframe::egui::LayerId::background()).with_clip_rect(Rect::from_min_max(Pos2::ZERO, Pos2::new(400.0, 300.0)))
    }

    #[test]
    fn rectangle_is_four_edges_of_three_quads() {
        let ctx = Context::default();
        let painter = painter(&ctx);
        let mut ants = Ants::new(&painter, TextureId::default(), 0.0);
        ants.rect(Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(110.0, 60.0)));
        assert_eq!(ants.dashes.vertices.len(), 4 * 4);
        assert_eq!(ants.shadow.vertices.len(), 4 * 4 * 2);
    }

    #[test]
    fn edges_outside_the_clip_are_dropped() {
        let ctx = Context::default();
        let painter = painter(&ctx);
        let mut ants = Ants::new(&painter, TextureId::default(), 0.0);
        ants.rect(Rect::from_min_max(Pos2::new(500.0, 10.0), Pos2::new(600.0, 60.0)));
        assert!(ants.dashes.is_empty());
    }

    #[test]
    fn line_is_one_physical_pixel_thick() {
        let ctx = Context::default();
        let painter = painter(&ctx);
        let mut ants = Ants::new(&painter, TextureId::default(), 0.0);
        ants.edge(Pos2::new(10.0, 20.0), Pos2::new(60.0, 20.0), 0.0);
        let ys: Vec<f32> = ants.dashes.vertices.iter().map(|v| v.pos.y).collect();
        let (min, max) = (ys.iter().copied().fold(f32::MAX, f32::min), ys.iter().copied().fold(f32::MIN, f32::max));
        assert_eq!(max - min, 1.0);
    }

    #[test]
    fn pattern_runs_forward_with_time_and_continues_between_edges() {
        let ctx = Context::default();
        let painter = painter(&ctx);
        let first = Ants::new(&painter, TextureId::default(), 0.0);
        let later = Ants::new(&painter, TextureId::default(), 0.5);
        assert!(later.pattern_u(0.0) < first.pattern_u(0.0));
        let mut ants = Ants::new(&painter, TextureId::default(), 0.0);
        let length = ants.edge(Pos2::new(10.0, 10.0), Pos2::new(50.0, 10.0), 0.0);
        ants.edge(Pos2::new(50.0, 10.0), Pos2::new(50.0, 40.0), length);
        let end_of_first = ants.dashes.vertices[1].uv.x;
        let start_of_second = ants.dashes.vertices[4].uv.x;
        assert!((end_of_first - start_of_second).abs() < 1e-4, "{end_of_first} {start_of_second}");
    }

    #[test]
    fn line_sits_on_the_inside_of_the_contour() {
        let ctx = Context::default();
        let painter = painter(&ctx);
        let top_of = |from: Pos2, to: Pos2| {
            let mut ants = Ants::new(&painter, TextureId::default(), 0.0);
            ants.edge(from, to, 0.0);
            ants.dashes.vertices.iter().map(|v| v.pos.y).fold(f32::MAX, f32::min)
        };
        assert_eq!(top_of(Pos2::new(10.0, 20.0), Pos2::new(60.0, 20.0)), 20.0);
        assert_eq!(top_of(Pos2::new(60.0, 20.0), Pos2::new(10.0, 20.0)), 19.0);
    }
}
