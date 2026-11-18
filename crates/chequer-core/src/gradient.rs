
use crate::blend::BlendMode;
use crate::geom::{FPoint, IPoint, IRect};
use crate::raster::{Raster, Rgba, lerp_premultiplied};
use crate::shapes::Overlay;

const FULL: f32 = u8::MAX as f32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientKind {
    Linear,
    Reflected,
    Diamond,
    Radial,
    Conical,
}

impl GradientKind {
    pub const ALL: [GradientKind; 5] = [GradientKind::Linear, GradientKind::Reflected, GradientKind::Diamond, GradientKind::Radial, GradientKind::Conical];
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientParams {
    pub kind: GradientKind,
    pub start: FPoint,
    pub end: FPoint,
    pub from: Rgba,
    pub to: Rgba,
}

fn position(params: &GradientParams, point: FPoint) -> f32 {
    let (dx, dy) = (params.end.x - params.start.x, params.end.y - params.start.y);
    let length = dx.hypot(dy);
    if length < f32::EPSILON {
        return 1.0;
    }
    let (px, py) = (point.x - params.start.x, point.y - params.start.y);
    let along = (px * dx + py * dy) / length;
    let across = (py * dx - px * dy) / length;
    let t = match params.kind {
        GradientKind::Linear => along / length,
        GradientKind::Reflected => along.abs() / length,
        GradientKind::Diamond => (along.abs() + across.abs()) / length,
        GradientKind::Radial => px.hypot(py) / length,
        GradientKind::Conical => across.atan2(along).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU,
    };
    t.clamp(0.0, 1.0)
}

pub fn render_gradient(params: &GradientParams, area: IRect, blend: BlendMode) -> Option<Overlay> {
    if area.is_empty() {
        return None;
    }
    let mut raster = Raster::transparent(area.width() as u32, area.height() as u32);
    for y in 0..area.height() as u32 {
        for x in 0..area.width() as u32 {
            let point = FPoint::new((area.x0 + x as i32) as f32 + 0.5, (area.y0 + y as i32) as f32 + 0.5);
            let t = position(params, point);
            raster.set(x, y, lerp_premultiplied(params.from, params.to, (t * FULL).round() as u8));
        }
    }
    Some(Overlay { origin: IPoint::new(area.x0, area.y0), raster, blend, erase: false, lcd: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgba = [0, 0, 0, 255];
    const WHITE: Rgba = [255, 255, 255, 255];

    fn params(kind: GradientKind) -> GradientParams {
        GradientParams { kind, start: FPoint::new(10.0, 10.0), end: FPoint::new(30.0, 10.0), from: BLACK, to: WHITE }
    }

    fn pixel(kind: GradientKind, x: i32, y: i32) -> Rgba {
        let area = IRect::new(0, 0, 40, 20);
        let overlay = render_gradient(&params(kind), area, BlendMode::Normal).unwrap();
        overlay.raster.get(x as u32, y as u32)
    }

    #[test]
    fn linear_runs_from_first_to_second_color_and_stays_flat_outside() {
        assert_eq!(pixel(GradientKind::Linear, 2, 10), BLACK);
        assert_eq!(pixel(GradientKind::Linear, 37, 10), WHITE);
        let middle = pixel(GradientKind::Linear, 20, 10)[0] as i32;
        assert!((middle - 128).abs() <= 8, "{middle}");
    }

    #[test]
    fn reflected_mirrors_around_the_start() {
        assert_eq!(pixel(GradientKind::Reflected, 5, 10), pixel(GradientKind::Reflected, 14, 10));
        assert!(pixel(GradientKind::Reflected, 10, 10)[0] < 12);
    }

    #[test]
    fn radial_is_the_same_in_every_direction() {
        let area = IRect::new(0, 0, 40, 40);
        let radial = GradientParams { start: FPoint::new(20.0, 20.0), end: FPoint::new(30.0, 20.0), ..params(GradientKind::Radial) };
        let overlay = render_gradient(&radial, area, BlendMode::Normal).unwrap();
        assert_eq!(overlay.raster.get(25, 20), overlay.raster.get(20, 25));
        assert!(overlay.raster.get(20, 20)[0] < 24);
        assert_eq!(overlay.raster.get(39, 39), WHITE);
    }

    #[test]
    fn diamond_has_its_own_corners() {
        let area = IRect::new(0, 0, 40, 40);
        let diamond = GradientParams { start: FPoint::new(20.0, 20.0), end: FPoint::new(30.0, 20.0), ..params(GradientKind::Diamond) };
        let overlay = render_gradient(&diamond, area, BlendMode::Normal).unwrap();
        assert!(overlay.raster.get(26, 26)[0] > overlay.raster.get(26, 20)[0]);
    }

    #[test]
    fn conical_changes_with_the_angle_only() {
        assert!(pixel(GradientKind::Conical, 12, 10)[0] < 12 && pixel(GradientKind::Conical, 30, 10)[0] < 12);
        let (below, above) = (pixel(GradientKind::Conical, 10, 15)[0] as i32, pixel(GradientKind::Conical, 10, 5)[0] as i32);
        assert!((below - 64).abs() < 6 && (above - 191).abs() < 6, "{below} {above}");
    }

    #[test]
    fn zero_length_gradient_is_a_flat_end_color() {
        let flat = GradientParams { end: FPoint::new(10.0, 10.0), ..params(GradientKind::Linear) };
        let overlay = render_gradient(&flat, IRect::new(0, 0, 4, 4), BlendMode::Normal).unwrap();
        assert_eq!(overlay.raster.get(2, 2), WHITE);
    }
}
