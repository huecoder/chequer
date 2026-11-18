use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

use crate::blend::BlendMode;
use crate::geom::{FPoint, FRect, IRect};
use crate::raster::{Raster, Rgba, erases};
use crate::spline::{Cubic, segments_path};

const CIRCLE_BEZIER_KAPPA: f32 = 0.552_284_75;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeKind {
    Rectangle,
    RoundedRectangle { radius: f32 },
    Ellipse,
    Triangle { apex: f32 },
    Diamond,
    Polygon { sides: u32 },
    Star { points: u32, inner: f32, outer: f32 },
    Arrow { head: f32, shaft: f32 },
    Heart,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeParams {
    pub corner_radius: f32,
    pub triangle_apex: f32,
    pub polygon_sides: u32,
    pub star_points: u32,
    pub star_inner: f32,
    pub star_outer: f32,
    pub arrow_head: f32,
    pub arrow_shaft: f32,
}

impl Default for ShapeParams {
    fn default() -> Self {
        Self {
            corner_radius: DEFAULT_CORNER_RADIUS,
            triangle_apex: DEFAULT_TRIANGLE_APEX,
            polygon_sides: DEFAULT_POLYGON_SIDES,
            star_points: DEFAULT_STAR_POINTS,
            star_inner: STAR_INNER_RATIO,
            star_outer: DEFAULT_STAR_OUTER,
            arrow_head: DEFAULT_ARROW_HEAD,
            arrow_shaft: DEFAULT_ARROW_SHAFT,
        }
    }
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 9] = [
        ShapeKind::Rectangle,
        ShapeKind::RoundedRectangle { radius: DEFAULT_CORNER_RADIUS },
        ShapeKind::Ellipse,
        ShapeKind::Triangle { apex: DEFAULT_TRIANGLE_APEX },
        ShapeKind::Diamond,
        ShapeKind::Polygon { sides: DEFAULT_POLYGON_SIDES },
        ShapeKind::Star { points: DEFAULT_STAR_POINTS, inner: STAR_INNER_RATIO, outer: DEFAULT_STAR_OUTER },
        ShapeKind::Arrow { head: DEFAULT_ARROW_HEAD, shaft: DEFAULT_ARROW_SHAFT },
        ShapeKind::Heart,
    ];

    pub fn with_params(self, params: &ShapeParams) -> ShapeKind {
        match self {
            ShapeKind::RoundedRectangle { .. } => ShapeKind::RoundedRectangle { radius: params.corner_radius },
            ShapeKind::Triangle { .. } => ShapeKind::Triangle { apex: params.triangle_apex },
            ShapeKind::Polygon { .. } => ShapeKind::Polygon { sides: params.polygon_sides },
            ShapeKind::Star { .. } => ShapeKind::Star { points: params.star_points, inner: params.star_inner, outer: params.star_outer },
            ShapeKind::Arrow { .. } => ShapeKind::Arrow { head: params.arrow_head, shaft: params.arrow_shaft },
            other => other,
        }
    }
}

pub const DEFAULT_CORNER_RADIUS: f32 = 20.0;
const ERASE_MARK: Rgba = [0, 0, 0, 255];
const STAR_INNER_RATIO: f32 = 0.382;
const DEFAULT_TRIANGLE_APEX: f32 = 0.5;
const DEFAULT_POLYGON_SIDES: u32 = 6;
const DEFAULT_STAR_POINTS: u32 = 5;
const DEFAULT_STAR_OUTER: f32 = 1.0;
const DEFAULT_ARROW_HEAD: f32 = 0.4;
const DEFAULT_ARROW_SHAFT: f32 = 0.3;
const MIN_POLYGON_SIDES: u32 = 3;
const MIN_ARROW_PART: f32 = 0.05;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StrokeAlign {
    Inside,
    #[default]
    Center,
    Outside,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeStyle {
    pub outline: Option<Rgba>,
    pub fill: Option<Rgba>,
    pub width: f32,
    pub align: StrokeAlign,
    pub antialias: bool,
}

#[derive(Clone, Debug)]
pub struct Overlay {
    pub origin: crate::geom::IPoint,
    pub raster: Raster,
    pub blend: BlendMode,
    pub erase: bool,
    pub lcd: Option<Vec<[u8; 3]>>,
}

impl Overlay {
    pub fn lcd_at(&self, x: i32, y: i32) -> Option<[u8; 3]> {
        self.lcd_at_local(x - self.origin.x, y - self.origin.y)
    }

    pub fn lcd_at_local(&self, column: i32, row: i32) -> Option<[u8; 3]> {
        let (width, height) = (self.raster.width() as i32, self.raster.height() as i32);
        if column < 0 || row < 0 || column >= width || row >= height {
            return None;
        }
        self.lcd.as_ref()?.get((row * width + column) as usize).copied()
    }

    pub fn bounds(&self) -> IRect {
        self.raster.bounds().translate(self.origin.x, self.origin.y)
    }
}

pub fn render_in_frame(kind: ShapeKind, frame: FRect, angle: f32, style: &ShapeStyle, blend: BlendMode, canvas: IRect) -> Option<Overlay> {
    let inner = inset(frame, style);
    let path = match kind {
        ShapeKind::Rectangle => rect_path(inner, 0.0)?,
        ShapeKind::RoundedRectangle { radius } => rect_path(inner, radius)?,
        ShapeKind::Ellipse => PathBuilder::from_oval(inner)?,
        ShapeKind::Heart => heart_path(inner)?,
        polygon => polygon_path(&preset_points(polygon), inner)?,
    };
    let center = frame.normalized();
    let path = if angle == 0.0 { path } else { path.transform(Transform::from_rotate_at(angle.to_degrees(), (center.x0 + center.x1) / 2.0, (center.y0 + center.y1) / 2.0))? };
    render_path(&path, style, blend, canvas)
}

pub fn render_line(segments: &[Cubic], style: &ShapeStyle, blend: BlendMode, canvas: IRect) -> Option<Overlay> {
    render_path(&segments_path(segments)?, style, blend, canvas)
}

pub fn render_polygon(points: &[FPoint], style: &ShapeStyle, blend: BlendMode, canvas: IRect) -> Option<Overlay> {
    let (first, rest) = points.split_first()?;
    let mut builder = PathBuilder::new();
    builder.move_to(first.x, first.y);
    for p in rest {
        builder.line_to(p.x, p.y);
    }
    builder.close();
    render_path(&builder.finish()?, style, blend, canvas)
}

fn inset(corners: FRect, style: &ShapeStyle) -> Rect {
    let corners = corners.normalized();
    let half = match (style.outline.is_some(), style.align) {
        (false, _) | (true, StrokeAlign::Inside) => 0.0,
        (true, StrokeAlign::Center) => style.width / 2.0,
        (true, StrokeAlign::Outside) => style.width,
    };
    let half_x = half.min(corners.width() / 2.0 - 0.01).max(0.0);
    let half_y = half.min(corners.height() / 2.0 - 0.01).max(0.0);
    Rect::from_ltrb(corners.x0 + half_x, corners.y0 + half_y, corners.x1 - half_x, corners.y1 - half_y)
        .unwrap_or_else(|| Rect::from_xywh(corners.x0, corners.y0, 1.0, 1.0).expect("единичный прямоугольник валиден"))
}

fn preset_points(kind: ShapeKind) -> Vec<(f32, f32)> {
    match kind {
        ShapeKind::Triangle { apex } => vec![(apex.clamp(0.0, 1.0), 0.0), (1.0, 1.0), (0.0, 1.0)],
        ShapeKind::Diamond => vec![(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)],
        ShapeKind::Polygon { sides } => regular_polygon(sides.max(MIN_POLYGON_SIDES)),
        ShapeKind::Star { points, inner, outer } => star(points.max(MIN_POLYGON_SIDES), inner, outer),
        ShapeKind::Arrow { head, shaft } => arrow(head, shaft),
        ShapeKind::Rectangle | ShapeKind::RoundedRectangle { .. } | ShapeKind::Ellipse | ShapeKind::Heart => Vec::new(),
    }
}

fn arrow(head: f32, shaft: f32) -> Vec<(f32, f32)> {
    let head_start = 1.0 - head.clamp(MIN_ARROW_PART, 1.0 - MIN_ARROW_PART);
    let half_shaft = shaft.clamp(MIN_ARROW_PART, 1.0) / 2.0;
    let (top, bottom) = (0.5 - half_shaft, 0.5 + half_shaft);
    vec![(0.0, top), (head_start, top), (head_start, 0.0), (1.0, 0.5), (head_start, 1.0), (head_start, bottom), (0.0, bottom)]
}

fn unit_point(angle: f32, radius: f32) -> (f32, f32) {
    (0.5 + radius * 0.5 * angle.cos(), 0.5 + radius * 0.5 * angle.sin())
}

fn regular_polygon(sides: u32) -> Vec<(f32, f32)> {
    (0..sides)
        .map(|i| unit_point(-std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / sides as f32, 1.0))
        .collect()
}

fn star(points: u32, inner: f32, outer: f32) -> Vec<(f32, f32)> {
    let outer = outer.clamp(0.0, 1.0);
    (0..points * 2)
        .map(|i| {
            let radius = if i % 2 == 0 { outer } else { outer * inner.clamp(0.0, 1.0) };
            unit_point(-std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / points as f32, radius)
        })
        .collect()
}

fn polygon_path(points: &[(f32, f32)], frame: Rect) -> Option<Path> {
    let mut builder = PathBuilder::new();
    for (i, (u, v)) in points.iter().enumerate() {
        let (x, y) = (frame.left() + u * frame.width(), frame.top() + v * frame.height());
        if i == 0 { builder.move_to(x, y) } else { builder.line_to(x, y) }
    }
    builder.close();
    builder.finish()
}

fn heart_path(frame: Rect) -> Option<Path> {
    let at = |u: f32, v: f32| (frame.left() + u * frame.width(), frame.top() + v * frame.height());
    let mut b = PathBuilder::new();
    let (x, y) = at(0.5, 0.25);
    b.move_to(x, y);
    for (c1, c2, to) in [
        ((0.5, 0.05), (0.2, 0.0), (0.05, 0.2)),
        ((-0.05, 0.4), (0.1, 0.6), (0.5, 1.0)),
        ((0.9, 0.6), (1.05, 0.4), (0.95, 0.2)),
        ((0.8, 0.0), (0.5, 0.05), (0.5, 0.25)),
    ] {
        let ((x1, y1), (x2, y2), (x3, y3)) = (at(c1.0, c1.1), at(c2.0, c2.1), at(to.0, to.1));
        b.cubic_to(x1, y1, x2, y2, x3, y3);
    }
    b.close();
    b.finish()
}

fn rect_path(rect: Rect, radius: f32) -> Option<Path> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    if r == 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    let (l, t, rt, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let k = r * CIRCLE_BEZIER_KAPPA;
    let mut p = PathBuilder::new();
    p.move_to(l + r, t);
    p.line_to(rt - r, t);
    p.cubic_to(rt - r + k, t, rt, t + r - k, rt, t + r);
    p.line_to(rt, b - r);
    p.cubic_to(rt, b - r + k, rt - r + k, b, rt - r, b);
    p.line_to(l + r, b);
    p.cubic_to(l + r - k, b, l, b - r + k, l, b - r);
    p.line_to(l, t + r);
    p.cubic_to(l, t + r - k, l + r - k, t, l + r, t);
    p.close();
    p.finish()
}

pub(crate) fn render_path(path: &Path, style: &ShapeStyle, blend: BlendMode, canvas: IRect) -> Option<Overlay> {
    let bounds = path.bounds();
    let margin = style.width.ceil() as i32 + 1;
    let area = IRect::new(
        bounds.left().floor() as i32,
        bounds.top().floor() as i32,
        bounds.right().ceil() as i32,
        bounds.bottom().ceil() as i32,
    )
    .expand(margin)
    .intersect(canvas);
    if area.is_empty() {
        return None;
    }
    let mut pixmap = Pixmap::new(area.width() as u32, area.height() as u32)?;
    let shift = Transform::from_translate(-area.x0 as f32, -area.y0 as f32);
    let erase = style.outline.into_iter().chain(style.fill).all(erases);
    let visible = |color: Rgba| if erase { ERASE_MARK } else { color };
    let draw_fill = |pixmap: &mut Pixmap| {
        if let Some(fill) = style.fill.map(visible) {
            pixmap.fill_path(path, &paint(fill, style.antialias), FillRule::Winding, shift, None);
        }
    };
    let draw_outline = |pixmap: &mut Pixmap, mask: Option<&Mask>| {
        if let Some(outline) = style.outline.map(visible) {
            let width = if style.align == StrokeAlign::Center { style.width } else { style.width * 2.0 };
            let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
            pixmap.stroke_path(path, &paint(outline, style.antialias), &stroke, shift, mask);
        }
    };
    draw_fill(&mut pixmap);
    match style.align {
        StrokeAlign::Center => draw_outline(&mut pixmap, None),
        StrokeAlign::Inside | StrokeAlign::Outside => {
            let mut mask = Mask::new(pixmap.width(), pixmap.height())?;
            mask.fill_path(path, FillRule::Winding, style.antialias, shift);
            if style.align == StrokeAlign::Outside {
                mask.invert();
            }
            draw_outline(&mut pixmap, Some(&mask));
        }
    }
    let raster = Raster::from_rgba(pixmap.width(), pixmap.height(), pixmap.take_demultiplied()).ok()?;
    Some(Overlay { origin: crate::geom::IPoint::new(area.x0, area.y0), raster, blend, erase, lcd: None })
}

fn paint(color: Rgba, antialias: bool) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(color[0], color[1], color[2], color[3]));
    paint.anti_alias = antialias;
    paint
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba = [255, 0, 0, 255];
    const CANVAS: IRect = IRect::new(0, 0, 64, 64);
    const NORMAL: BlendMode = BlendMode::Normal;

    fn outline(width: f32) -> ShapeStyle {
        ShapeStyle { outline: Some(RED), fill: None, width, align: StrokeAlign::Center, antialias: false }
    }

    fn filled() -> ShapeStyle {
        ShapeStyle { outline: None, fill: Some(RED), width: 1.0, align: StrokeAlign::Center, antialias: true }
    }

    fn at(overlay: &Overlay, x: i32, y: i32) -> Rgba {
        overlay.raster.get((x - overlay.origin.x) as u32, (y - overlay.origin.y) as u32)
    }

    #[test]
    fn rectangle_outline_leaves_center_empty() {
        let o = render_in_frame(ShapeKind::Rectangle, IRect::new(10, 10, 30, 30).into(), 0.0, &outline(2.0), NORMAL, CANVAS).unwrap();
        assert_eq!(at(&o, 10, 15)[3], 255);
        assert_eq!(at(&o, 19, 19)[3], 0);
    }

    #[test]
    fn filled_ellipse_center_is_filled() {
        let o = render_in_frame(ShapeKind::Ellipse, IRect::new(10, 10, 31, 31).into(), 0.0, &filled(), NORMAL, CANVAS).unwrap();
        assert_eq!(at(&o, 20, 20), RED);
        assert_eq!(at(&o, 10, 10)[3], 0);
    }

    #[test]
    fn horizontal_one_pixel_line_without_aa_is_one_row() {
        let o = render_line(&[Cubic::straight(FPoint::new(5.5, 20.5), FPoint::new(25.5, 20.5))], &outline(1.0), NORMAL, CANVAS).unwrap();
        assert_eq!(at(&o, 15, 20)[3], 255);
        assert_eq!(at(&o, 15, 19)[3], 0);
        assert_eq!(at(&o, 15, 21)[3], 0);
    }

    #[test]
    fn transparent_shape_becomes_an_erasing_mask() {
        let clear = ShapeStyle { outline: None, fill: Some([255, 0, 0, 0]), width: 1.0, align: StrokeAlign::Center, antialias: true };
        let o = render_in_frame(ShapeKind::Rectangle, IRect::new(10, 10, 30, 30).into(), 0.0, &clear, NORMAL, CANVAS).unwrap();
        assert!(o.erase);
        assert_eq!(at(&o, 20, 20)[3], 255);
        let painted = render_in_frame(ShapeKind::Rectangle, IRect::new(10, 10, 30, 30).into(), 0.0, &filled(), NORMAL, CANVAS).unwrap();
        assert!(!painted.erase);
    }

    #[test]
    fn shape_outside_canvas_gives_none() {
        assert!(render_in_frame(ShapeKind::Rectangle, IRect::new(100, 100, 120, 120).into(), 0.0, &outline(1.0), NORMAL, CANVAS).is_none());
    }

    #[test]
    fn rounded_rectangle_corner_is_cut() {
        let kind = ShapeKind::RoundedRectangle { radius: 8.0 };
        let o = render_in_frame(kind, IRect::new(10, 10, 41, 41).into(), 0.0, &filled(), NORMAL, CANVAS).unwrap();
        assert_eq!(at(&o, 10, 10)[3], 0);
        assert_eq!(at(&o, 25, 25), RED);
    }

    #[test]
    fn every_preset_fills_its_center_area() {
        for kind in ShapeKind::ALL {
            let o = render_in_frame(kind, IRect::new(8, 8, 56, 56).into(), 0.0, &filled(), NORMAL, CANVAS).unwrap();
            assert!(at(&o, 32, 36)[3] > 0, "{kind:?} не закрасил центр");
        }
    }

    #[test]
    fn polygon_triangle_fills() {
        let pts = [FPoint::new(5.0, 5.0), FPoint::new(40.0, 5.0), FPoint::new(5.0, 40.0)];
        let o = render_polygon(&pts, &filled(), NORMAL, CANVAS).unwrap();
        assert_eq!(at(&o, 10, 10), RED);
    }
}
