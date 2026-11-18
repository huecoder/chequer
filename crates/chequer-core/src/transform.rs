
use crate::document::Resample;
use crate::geom::{FPoint, IRect};
use crate::raster::{BYTES_PER_PIXEL, Raster};
use crate::selection::Selection;

const MIN_SIDE: f32 = 1.0;
const GRID_EPSILON: f32 = 1e-3;
const FULL: f32 = 255.0;
const MAX_SIDE: f32 = 16384.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sides {
    pub x: i8,
    pub y: i8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub center: FPoint,
    pub width: f32,
    pub height: f32,
    pub angle: f32,
}

impl Placement {
    pub fn from_bounds(bounds: IRect) -> Self {
        Self {
            center: FPoint::new((bounds.x0 + bounds.x1) as f32 / 2.0, (bounds.y0 + bounds.y1) as f32 / 2.0),
            width: bounds.width() as f32,
            height: bounds.height() as f32,
            angle: 0.0,
        }
    }

    fn axes(&self) -> (FPoint, FPoint) {
        let (sin, cos) = self.angle.sin_cos();
        (FPoint::new(cos, sin), FPoint::new(-sin, cos))
    }

    pub fn point(&self, u: f32, v: f32) -> FPoint {
        let (ex, ey) = self.axes();
        let (local_x, local_y) = ((u - 0.5) * self.width, (v - 0.5) * self.height);
        FPoint::new(self.center.x + ex.x * local_x + ey.x * local_y, self.center.y + ex.y * local_x + ey.y * local_y)
    }

    pub fn fractions_of(&self, p: FPoint) -> (f32, f32) {
        let (local_x, local_y) = self.to_local(p);
        (local_x / self.width + 0.5, local_y / self.height + 0.5)
    }

    fn to_local(&self, p: FPoint) -> (f32, f32) {
        let (ex, ey) = self.axes();
        let (dx, dy) = (p.x - self.center.x, p.y - self.center.y);
        (dx * ex.x + dy * ex.y, dx * ey.x + dy * ey.y)
    }

    pub fn corners(&self) -> [FPoint; 4] {
        [self.point(0.0, 0.0), self.point(1.0, 0.0), self.point(1.0, 1.0), self.point(0.0, 1.0)]
    }

    pub fn outer_bounds(&self) -> IRect {
        let corners = self.corners();
        let fold = |pick: fn(&FPoint) -> f32, merge: fn(f32, f32) -> f32, start: f32| corners.iter().map(pick).fold(start, merge);
        let (x0, x1) = (fold(|p| p.x, f32::min, f32::MAX), fold(|p| p.x, f32::max, f32::MIN));
        let (y0, y1) = (fold(|p| p.y, f32::min, f32::MAX), fold(|p| p.y, f32::max, f32::MIN));
        let (x0, y0) = ((x0 + GRID_EPSILON).floor() as i32, (y0 + GRID_EPSILON).floor() as i32);
        IRect::new(x0, y0, ((x1 - GRID_EPSILON).ceil() as i32).max(x0 + 1), ((y1 - GRID_EPSILON).ceil() as i32).max(y0 + 1))
    }

    pub fn shifted(self, dx: f32, dy: f32) -> Self {
        Self { center: FPoint::new(self.center.x + dx, self.center.y + dy), ..self }
    }

    pub fn rotated_about(self, pivot: FPoint, delta: f32) -> Self {
        let (sin, cos) = delta.sin_cos();
        let (dx, dy) = (self.center.x - pivot.x, self.center.y - pivot.y);
        Self { center: FPoint::new(pivot.x + dx * cos - dy * sin, pivot.y + dx * sin + dy * cos), angle: self.angle + delta, ..self }
    }

    pub fn dragged(self, sides: Sides, pointer: FPoint, proportional: bool) -> Self {
        let anchor_fraction = |side: i8| match side {
            s if s < 0 => 1.0,
            s if s > 0 => 0.0,
            _ => 0.5,
        };
        let anchor = self.point(anchor_fraction(sides.x), anchor_fraction(sides.y));
        let (ex, ey) = self.axes();
        let (dx, dy) = (pointer.x - anchor.x, pointer.y - anchor.y);
        let (reach_x, reach_y) = (dx * ex.x + dy * ex.y, dx * ey.x + dy * ey.y);
        let direction = |reach: f32, side: i8| if reach != 0.0 { reach.signum() } else { f32::from(side.signum()) };
        let (mut width, mut height) = (if sides.x != 0 { reach_x.abs() } else { self.width }, if sides.y != 0 { reach_y.abs() } else { self.height });
        if proportional && self.width > 0.0 && self.height > 0.0 {
            let scale = match (sides.x != 0, sides.y != 0) {
                (true, true) => (width / self.width).max(height / self.height),
                (true, false) => width / self.width,
                (false, true) => height / self.height,
                (false, false) => 1.0,
            };
            (width, height) = (self.width * scale, self.height * scale);
        }
        let (width, height) = (width.max(MIN_SIDE), height.max(MIN_SIDE));
        let shift_x = if sides.x != 0 { direction(reach_x, sides.x) * width / 2.0 } else { 0.0 };
        let shift_y = if sides.y != 0 { direction(reach_y, sides.y) * height / 2.0 } else { 0.0 };
        let center = FPoint::new(anchor.x + ex.x * shift_x + ey.x * shift_y, anchor.y + ex.y * shift_x + ey.y * shift_y);
        Self { center, width, height, angle: self.angle }
    }

    fn is_grid_aligned(&self, width: u32, height: u32) -> bool {
        let on_grid = |value: f32| (value - value.round()).abs() < GRID_EPSILON;
        let turns = self.angle / std::f32::consts::TAU;
        let bounds = self.outer_bounds();
        (turns - turns.round()).abs() * std::f32::consts::TAU < GRID_EPSILON
            && bounds.width() as u32 == width
            && bounds.height() as u32 == height
            && (self.width - width as f32).abs() < GRID_EPSILON
            && (self.height - height as f32).abs() < GRID_EPSILON
            && on_grid(self.center.x - width as f32 / 2.0)
            && on_grid(self.center.y - height as f32 / 2.0)
    }
}

const MIN_QUAD_EDGE: f32 = 1.0;
const QUAD_EDGE_MARGIN: f32 = 2.0;
const DEGENERATE: f32 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub corners: [FPoint; 4],
}

#[derive(Clone, Copy)]
struct Homography([f32; 9]);

impl Homography {
    fn of_unit_square(corners: &[FPoint; 4]) -> Option<Self> {
        let [p0, p1, p2, p3] = *corners;
        let (sx, sy) = (p0.x - p1.x + p2.x - p3.x, p0.y - p1.y + p2.y - p3.y);
        if sx.abs() < DEGENERATE && sy.abs() < DEGENERATE {
            return Some(Self([p1.x - p0.x, p2.x - p1.x, p0.x, p1.y - p0.y, p2.y - p1.y, p0.y, 0.0, 0.0, 1.0]));
        }
        let (dx1, dx2, dy1, dy2) = (p1.x - p2.x, p3.x - p2.x, p1.y - p2.y, p3.y - p2.y);
        let denominator = dx1 * dy2 - dx2 * dy1;
        if denominator.abs() < DEGENERATE {
            return None;
        }
        let (g, h) = ((sx * dy2 - dx2 * sy) / denominator, (dx1 * sy - sx * dy1) / denominator);
        Some(Self([p1.x - p0.x + g * p1.x, p3.x - p0.x + h * p3.x, p0.x, p1.y - p0.y + g * p1.y, p3.y - p0.y + h * p3.y, p0.y, g, h, 1.0]))
    }

    fn inverse(&self) -> Option<Self> {
        let [a, b, c, d, e, f, g, h, i] = self.0;
        let (cofactor_a, cofactor_b, cofactor_c) = (e * i - f * h, f * g - d * i, d * h - e * g);
        let determinant = a * cofactor_a + b * cofactor_b + c * cofactor_c;
        if determinant.abs() < DEGENERATE {
            return None;
        }
        let scale = 1.0 / determinant;
        Some(Self([
            cofactor_a * scale,
            (c * h - b * i) * scale,
            (b * f - c * e) * scale,
            cofactor_b * scale,
            (a * i - c * g) * scale,
            (c * d - a * f) * scale,
            cofactor_c * scale,
            (b * g - a * h) * scale,
            (a * e - b * d) * scale,
        ]))
    }

    fn apply(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let [a, b, c, d, e, f, g, h, i] = self.0;
        let w = g * x + h * y + i;
        (w > DEGENERATE).then(|| ((a * x + b * y + c) / w, (d * x + e * y + f) / w))
    }
}

impl Quad {
    pub fn from_placement(placement: &Placement) -> Self {
        Self { corners: placement.corners() }
    }

    pub fn point(&self, u: f32, v: f32) -> FPoint {
        Homography::of_unit_square(&self.corners)
            .and_then(|homography| homography.apply(u, v))
            .map_or(self.corners[0], |(x, y)| FPoint::new(x, y))
    }

    pub fn fractions_of(&self, p: FPoint) -> Option<(f32, f32)> {
        Homography::of_unit_square(&self.corners)?.inverse()?.apply(p.x, p.y)
    }

    pub fn contains(&self, p: FPoint) -> bool {
        let sign = |a: FPoint, b: FPoint| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        let signs = (0..4).map(|i| sign(self.corners[i], self.corners[(i + 1) % 4]));
        signs.clone().all(|s| s >= 0.0) || signs.into_iter().all(|s| s <= 0.0)
    }

    pub fn outer_bounds(&self) -> IRect {
        let fold = |pick: fn(&FPoint) -> f32, merge: fn(f32, f32) -> f32, start: f32| self.corners.iter().map(pick).fold(start, merge);
        let (x0, x1) = (fold(|p| p.x, f32::min, f32::MAX), fold(|p| p.x, f32::max, f32::MIN));
        let (y0, y1) = (fold(|p| p.y, f32::min, f32::MAX), fold(|p| p.y, f32::max, f32::MIN));
        let (x0, y0) = ((x0 + GRID_EPSILON).floor() as i32, (y0 + GRID_EPSILON).floor() as i32);
        IRect::new(x0, y0, ((x1 - GRID_EPSILON).ceil() as i32).max(x0 + 1), ((y1 - GRID_EPSILON).ceil() as i32).max(y0 + 1))
    }

    pub fn shifted(self, dx: f32, dy: f32) -> Self {
        Self { corners: self.corners.map(|c| FPoint::new(c.x + dx, c.y + dy)) }
    }

    pub fn rotated_about(self, pivot: FPoint, delta: f32) -> Self {
        let (sin, cos) = delta.sin_cos();
        let rotate = |c: FPoint| {
            let (dx, dy) = (c.x - pivot.x, c.y - pivot.y);
            FPoint::new(pivot.x + dx * cos - dy * sin, pivot.y + dx * sin + dy * cos)
        };
        Self { corners: self.corners.map(rotate) }
    }

    pub fn with_corner(self, index: usize, to: FPoint) -> Option<Self> {
        let mut corners = self.corners;
        corners[index % 4] = to;
        Self { corners }.valid()
    }

    pub fn with_edge(self, index: usize, dx: f32, dy: f32) -> Option<Self> {
        let mut corners = self.corners;
        for corner in [index % 4, (index + 1) % 4] {
            corners[corner] = FPoint::new(corners[corner].x + dx, corners[corner].y + dy);
        }
        Self { corners }.valid()
    }

    fn valid(self) -> Option<Self> {
        let cross = |i: usize| {
            let (a, b, c) = (self.corners[i], self.corners[(i + 1) % 4], self.corners[(i + 2) % 4]);
            (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x)
        };
        let edge = |i: usize| {
            let (a, b) = (self.corners[i], self.corners[(i + 1) % 4]);
            (b.x - a.x).hypot(b.y - a.y)
        };
        let all_left = (0..4).all(|i| cross(i) > 0.0);
        let all_right = (0..4).all(|i| cross(i) < 0.0);
        ((all_left || all_right) && (0..4).all(|i| edge(i) >= MIN_QUAD_EDGE)).then_some(self)
    }

    fn mean_size(&self) -> (f32, f32) {
        let length = |a: usize, b: usize| (self.corners[b].x - self.corners[a].x).hypot(self.corners[b].y - self.corners[a].y);
        ((length(0, 1) + length(3, 2)) / 2.0, (length(0, 3) + length(1, 2)) / 2.0)
    }
}

pub struct Placed {
    pub pixels: Raster,
    pub selection: Selection,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RotationMethod {
    #[default]
    Plain,
    FastRotation,
    RotSprite,
}

const ROTSPRITE_MAX_AREA: [u64; 3] = [1024 * 1024, 512 * 512, 256 * 256];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kernel {
    Nearest,
    Linear,
    Cubic,
}

impl Kernel {
    fn of(resample: Resample) -> Self {
        match resample {
            Resample::Nearest => Kernel::Nearest,
            Resample::Bilinear => Kernel::Linear,
            Resample::Bicubic | Resample::Lanczos | Resample::Anisotropic => Kernel::Cubic,
        }
    }
}

struct Layer {
    pixels: Raster,
    coverage: Vec<u8>,
}

impl Layer {
    fn width(&self) -> u32 {
        self.pixels.width()
    }

    fn height(&self) -> u32 {
        self.pixels.height()
    }

    fn scaled(source: &Raster, plane: &Raster, width: u32, height: u32, resample: Resample) -> Self {
        if (width, height) == (source.width(), source.height()) {
            return Self { pixels: source.clone(), coverage: plane_values(plane) };
        }
        Self { pixels: source.resized(width, height, resample), coverage: plane_values(&plane.resized(width, height, resample)) }
    }

    fn enlarged(&self, zoom: u32) -> Self {
        let (mut pixels, mut plane) = (self.pixels.clone(), self.plane());
        for _ in 0..zoom.trailing_zeros() {
            pixels = scale2x(&pixels);
            plane = scale2x(&plane);
        }
        Self { pixels, coverage: plane_values(&plane) }
    }

    fn plane(&self) -> Raster {
        let mut plane = Raster::transparent(self.width(), self.height());
        for (index, &value) in self.coverage.iter().enumerate() {
            plane.set(index as u32 % self.width(), index as u32 / self.width(), [value, value, value, u8::MAX]);
        }
        plane
    }

    fn coverage_at(&self, x: i32, y: i32) -> f32 {
        match self.index(x, y) {
            Some(index) => self.coverage[index] as f32,
            None => 0.0,
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.width() as i32 && y < self.height() as i32).then(|| y as usize * self.width() as usize + x as usize)
    }
}

pub fn coverage_plane(mask: &Selection) -> Raster {
    let bounds = mask.bounds();
    let mut plane = Raster::transparent(bounds.width() as u32, bounds.height() as u32);
    for y in 0..plane.height() {
        for x in 0..plane.width() {
            let value = mask.coverage_at(bounds.x0 + x as i32, bounds.y0 + y as i32);
            plane.set(x, y, [value, value, value, u8::MAX]);
        }
    }
    plane
}

fn plane_values(plane: &Raster) -> Vec<u8> {
    plane.as_bytes().chunks_exact(BYTES_PER_PIXEL).map(|pixel| pixel[0]).collect()
}

pub fn render_placed(source: &Raster, plane: &Raster, placement: &Placement, resample: Resample, rotation: RotationMethod) -> Option<Placed> {
    let side = |value: f32| value.round().clamp(MIN_SIDE, MAX_SIDE) as u32;
    let layer = Layer::scaled(source, plane, side(placement.width), side(placement.height), resample);
    if placement.is_grid_aligned(layer.width(), layer.height()) {
        let origin = placement.outer_bounds();
        let selection = Selection::from_coverage(origin, layer.coverage)?;
        return Some(Placed { pixels: layer.pixels, selection });
    }
    let bounds = placement.outer_bounds();
    let (width, height) = (bounds.width() as u32, bounds.height() as u32);
    if resample == Resample::Nearest && rotation == RotationMethod::FastRotation {
        return fast_rotated(&layer, placement, bounds);
    }
    let zoom = if resample == Resample::Nearest && rotation == RotationMethod::RotSprite { rotsprite_zoom(&layer) } else { 1 };
    let layer = if zoom > 1 { layer.enlarged(zoom) } else { layer };
    let mut pixels = Raster::transparent(width, height);
    let mut coverage = vec![0u8; width as usize * height as usize];
    let (to_base_x, to_base_y) = (layer.width() as f32 / placement.width, layer.height() as f32 / placement.height);
    let kernel = Kernel::of(resample);
    for y in 0..height {
        for x in 0..width {
            let (local_x, local_y) = placement.to_local(FPoint::new((bounds.x0 + x as i32) as f32 + 0.5, (bounds.y0 + y as i32) as f32 + 0.5));
            let (u, v) = (local_x * to_base_x + layer.width() as f32 / 2.0, local_y * to_base_y + layer.height() as f32 / 2.0);
            let sample = sample_at(&layer, u, v, kernel);
            coverage[y as usize * width as usize + x as usize] = sample.coverage;
            pixels.set(x, y, sample.color);
        }
    }
    let selection = Selection::from_coverage(bounds, coverage)?;
    Some(Placed { pixels, selection })
}

pub fn render_quad(source: &Raster, plane: &Raster, quad: &Quad, resample: Resample) -> Option<Placed> {
    let inverse = Homography::of_unit_square(&quad.corners)?.inverse()?;
    let bounds = quad.outer_bounds();
    let (width, height) = (bounds.width() as u32, bounds.height() as u32);
    if width.max(height) as f32 > MAX_SIDE {
        return None;
    }
    let (mean_width, mean_height) = quad.mean_size();
    let side = |full: u32, mean: f32| full.min(mean.round().clamp(MIN_SIDE, MAX_SIDE) as u32);
    let layer = Layer::scaled(source, plane, side(source.width(), mean_width), side(source.height(), mean_height), resample);
    let (layer_width, layer_height) = (layer.width() as f32, layer.height() as f32);
    let kernel = Kernel::of(resample);
    let mut pixels = Raster::transparent(width, height);
    let mut coverage = vec![0u8; width as usize * height as usize];
    for y in 0..height {
        for x in 0..width {
            let center = ((bounds.x0 + x as i32) as f32 + 0.5, (bounds.y0 + y as i32) as f32 + 0.5);
            let Some((u, v)) = inverse.apply(center.0, center.1) else {
                continue;
            };
            let (layer_x, layer_y) = (u * layer_width, v * layer_height);
            if layer_x < -QUAD_EDGE_MARGIN || layer_y < -QUAD_EDGE_MARGIN || layer_x > layer_width + QUAD_EDGE_MARGIN || layer_y > layer_height + QUAD_EDGE_MARGIN {
                continue;
            }
            let sample = sample_at(&layer, layer_x, layer_y, kernel);
            coverage[y as usize * width as usize + x as usize] = sample.coverage;
            pixels.set(x, y, sample.color);
        }
    }
    let selection = Selection::from_coverage(bounds, coverage)?;
    Some(Placed { pixels, selection })
}

fn rotsprite_zoom(layer: &Layer) -> u32 {
    let area = layer.width() as u64 * layer.height() as u64;
    [8, 4, 2].iter().zip(ROTSPRITE_MAX_AREA.iter().rev()).find(|(_, max)| area <= **max).map_or(1, |(zoom, _)| *zoom)
}

fn scale2x(source: &Raster) -> Raster {
    let (width, height) = (source.width(), source.height());
    let mut out = Raster::transparent(width * 2, height * 2);
    let at = |x: i64, y: i64| source.get(x.clamp(0, width as i64 - 1) as u32, y.clamp(0, height as i64 - 1) as u32);
    for y in 0..height as i64 {
        for x in 0..width as i64 {
            let (p, up, right, left, down) = (at(x, y), at(x, y - 1), at(x + 1, y), at(x - 1, y), at(x, y + 1));
            let mut block = [p; 4];
            if left == up && left != down && up != right { block[0] = up; }
            if up == right && up != left && right != down { block[1] = right; }
            if down == left && down != right && left != up { block[2] = left; }
            if right == down && right != up && down != left { block[3] = down; }
            for (index, color) in block.into_iter().enumerate() {
                out.set(x as u32 * 2 + (index as u32 % 2), y as u32 * 2 + (index as u32 / 2), color);
            }
        }
    }
    out
}

type Cell = ([u8; 4], u8);

struct Grid {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}

impl Grid {
    fn from_layer(layer: &Layer) -> Self {
        let (width, height) = (layer.width() as usize, layer.height() as usize);
        let cells = (0..width * height).map(|i| (layer.pixels.get((i % width) as u32, (i / width) as u32), layer.coverage[i])).collect();
        Self { width, height, cells }
    }

    fn half_turn(mut self) -> Self {
        self.cells.reverse();
        self
    }

    fn sheared(&self, factor: f32, along_rows: bool) -> Self {
        let (lines, length) = if along_rows { (self.height, self.width) } else { (self.width, self.height) };
        let shift = |line: usize| (factor * (line as f32 + 0.5 - lines as f32 / 2.0)).round() as i64;
        let (low, high) = (0..lines).fold((i64::MAX, i64::MIN), |(low, high), line| (low.min(shift(line)), high.max(shift(line))));
        let new_length = length + (high - low) as usize;
        let mut out = Grid { width: if along_rows { new_length } else { self.width }, height: if along_rows { self.height } else { new_length }, cells: Vec::new() };
        out.cells = vec![([0; 4], 0); out.width * out.height];
        for line in 0..lines {
            let offset = (shift(line) - low) as usize;
            for along in 0..length {
                let (x, y) = if along_rows { (along, line) } else { (line, along) };
                let (nx, ny) = if along_rows { (along + offset, line) } else { (line, along + offset) };
                out.cells[ny * out.width + nx] = self.cells[y * self.width + x];
            }
        }
        out
    }
}

fn fast_rotated(layer: &Layer, placement: &Placement, bounds: IRect) -> Option<Placed> {
    let mut angle = placement.angle.rem_euclid(std::f32::consts::TAU);
    let mut grid = Grid::from_layer(layer);
    if angle > std::f32::consts::PI {
        angle -= std::f32::consts::TAU;
    }
    if angle.abs() > std::f32::consts::FRAC_PI_2 {
        grid = grid.half_turn();
        angle -= std::f32::consts::PI.copysign(angle);
    }
    let across = -(angle / 2.0).tan();
    let grid = grid.sheared(across, true).sheared(angle.sin(), false).sheared(across, true);
    let origin_x = (placement.center.x - grid.width as f32 / 2.0).round() as i32 - bounds.x0;
    let origin_y = (placement.center.y - grid.height as f32 / 2.0).round() as i32 - bounds.y0;
    let (width, height) = (bounds.width() as u32, bounds.height() as u32);
    let mut pixels = Raster::transparent(width, height);
    let mut coverage = vec![0u8; width as usize * height as usize];
    for (index, (color, cover)) in grid.cells.iter().enumerate() {
        let (x, y) = ((index % grid.width) as i32 + origin_x, (index / grid.width) as i32 + origin_y);
        if x >= 0 && y >= 0 && x < width as i32 && y < height as i32 {
            pixels.set(x as u32, y as u32, *color);
            coverage[y as usize * width as usize + x as usize] = *cover;
        }
    }
    Some(Placed { pixels, selection: Selection::from_coverage(bounds, coverage)? })
}

struct Sample {
    color: [u8; 4],
    coverage: u8,
}

fn taps(position: f32, kernel: Kernel) -> (i32, [f32; 4]) {
    let shifted = position - 0.5;
    let base = shifted.floor();
    let t = shifted - base;
    let weights = match kernel {
        Kernel::Nearest => {
            let nearest = (t >= 0.5) as usize;
            let mut weights = [0.0; 4];
            weights[1 + nearest] = 1.0;
            weights
        }
        Kernel::Linear => [0.0, 1.0 - t, t, 0.0],
        Kernel::Cubic => {
            let (t2, t3) = (t * t, t * t * t);
            [(-t3 + 2.0 * t2 - t) / 2.0, (3.0 * t3 - 5.0 * t2 + 2.0) / 2.0, (-3.0 * t3 + 4.0 * t2 + t) / 2.0, (t3 - t2) / 2.0]
        }
    };
    (base as i32 - 1, weights)
}

fn sample_at(layer: &Layer, u: f32, v: f32, kernel: Kernel) -> Sample {
    let ((first_x, weights_x), (first_y, weights_y)) = (taps(u, kernel), taps(v, kernel));
    let (mut alpha, mut red, mut green, mut blue, mut coverage) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for (row, &weight_y) in weights_y.iter().enumerate() {
        for (column, &weight_x) in weights_x.iter().enumerate() {
            let weight = weight_x * weight_y;
            if weight == 0.0 {
                continue;
            }
            let (x, y) = (first_x + column as i32, first_y + row as i32);
            coverage += weight * layer.coverage_at(x, y);
            if layer.index(x, y).is_some() {
                let pixel = layer.pixels.get(x as u32, y as u32);
                let pixel_alpha = pixel[3] as f32;
                alpha += weight * pixel_alpha;
                red += weight * pixel[0] as f32 * pixel_alpha / FULL;
                green += weight * pixel[1] as f32 * pixel_alpha / FULL;
                blue += weight * pixel[2] as f32 * pixel_alpha / FULL;
            }
        }
    }
    let alpha = alpha.clamp(0.0, FULL);
    let channel = |premultiplied: f32| if alpha > 0.0 { (premultiplied * FULL / alpha).clamp(0.0, FULL).round() as u8 } else { 0 };
    Sample { color: [channel(red), channel(green), channel(blue), alpha.round() as u8], coverage: coverage.clamp(0.0, FULL).round() as u8 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(x0: f32, y0: f32, width: f32, height: f32, angle: f32) -> Placement {
        Placement { center: FPoint::new(x0 + width / 2.0, y0 + height / 2.0), width, height, angle }
    }

    fn trapezoid() -> Quad {
        Quad { corners: [FPoint::new(10.0, 10.0), FPoint::new(50.0, 14.0), FPoint::new(44.0, 40.0), FPoint::new(8.0, 36.0)] }
    }

    #[test]
    fn quad_fractions_invert_the_projection() {
        let quad = trapezoid();
        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.3, 0.8), (1.0, 1.0)] {
            let (back_u, back_v) = quad.fractions_of(quad.point(u, v)).unwrap();
            assert!((back_u - u).abs() < 1e-3 && (back_v - v).abs() < 1e-3);
        }
    }

    #[test]
    fn quad_edit_is_refused_when_it_would_fold() {
        let quad = Quad::from_placement(&placement(0.0, 0.0, 20.0, 20.0, 0.0));
        assert!(quad.with_corner(0, FPoint::new(30.0, 30.0)).is_none());
        assert!(quad.with_corner(0, FPoint::new(5.0, 3.0)).is_some());
        assert!(quad.with_edge(0, 0.0, 20.0).is_none());
    }

    #[test]
    fn rect_quad_renders_the_source_unchanged() {
        let mut source = Raster::transparent(4, 3);
        for y in 0..3 {
            for x in 0..4 {
                source.set(x, y, [(x * 60) as u8, (y * 90) as u8, 7, 255]);
            }
        }
        let plane = coverage_plane(&Selection::from_rect(IRect::new(0, 0, 4, 3)).unwrap());
        let quad = Quad::from_placement(&placement(5.0, 6.0, 4.0, 3.0, 0.0));
        let placed = render_quad(&source, &plane, &quad, Resample::Nearest).unwrap();
        assert_eq!(placed.selection.bounds(), IRect::new(5, 6, 9, 9));
        for y in 0..3 {
            for x in 0..4 {
                assert_eq!(placed.pixels.get(x, y), source.get(x, y));
            }
        }
    }

    #[test]
    fn dragging_a_side_keeps_the_opposite_side_even_when_rotated() {
        let start = placement(10.0, 10.0, 40.0, 20.0, std::f32::consts::FRAC_PI_2);
        let west = start.point(0.0, 0.5);
        let dragged = start.dragged(Sides { x: 1, y: 0 }, start.point(2.0, 0.5), false);
        let kept = dragged.point(0.0, 0.5);
        assert!((kept.x - west.x).abs() < 1e-3 && (kept.y - west.y).abs() < 1e-3);
        assert!((dragged.width - 80.0).abs() < 1e-3 && (dragged.height - 20.0).abs() < 1e-3);
    }

    #[test]
    fn quarter_turn_about_the_center_moves_the_top_left_pixel_to_the_top_right() {
        let mut source = Raster::transparent(2, 2);
        source.set(0, 0, [9, 9, 9, 255]);
        let plane = coverage_plane(&Selection::from_rect(IRect::new(0, 0, 2, 2)).unwrap());
        let turned = placement(0.0, 0.0, 2.0, 2.0, std::f32::consts::FRAC_PI_2);
        let placed = render_placed(&source, &plane, &turned, Resample::Nearest, RotationMethod::Plain).unwrap();
        assert_eq!(placed.selection.bounds(), IRect::new(0, 0, 2, 2));
        assert_eq!(placed.pixels.get(1, 0), [9, 9, 9, 255]);
    }

    #[test]
    fn half_pixel_frame_gets_half_coverage_at_its_edge_when_smooth() {
        let source = Raster::filled(4, 4, [1, 2, 3, 255]);
        let plane = coverage_plane(&Selection::from_rect(IRect::new(0, 0, 4, 4)).unwrap());
        let shifted = placement(0.5, 0.0, 4.0, 4.0, 0.0);
        let placed = render_placed(&source, &plane, &shifted, Resample::Bilinear, RotationMethod::Plain).unwrap();
        let bounds = placed.selection.bounds();
        assert_eq!(placed.selection.coverage_at(bounds.x0, 1), 128);
        assert_eq!(placed.selection.coverage_at(bounds.x0 + 2, 1), 255);
    }

    #[test]
    fn every_ruler_angle_renders_with_every_method() {
        let source = Raster::filled(7, 5, [1, 2, 3, 255]);
        let plane = coverage_plane(&Selection::from_rect(IRect::new(0, 0, 7, 5)).unwrap());
        let methods = [Resample::Nearest, Resample::Bilinear, Resample::Bicubic, Resample::Lanczos, Resample::Anisotropic];
        let rotations = [RotationMethod::Plain, RotationMethod::FastRotation, RotationMethod::RotSprite];
        for step in -8..=8 {
            let turned = placement(3.0, 2.0, 7.0, 5.0, step as f32 * std::f32::consts::FRAC_PI_4);
            for method in methods {
                for rotation in rotations {
                    assert!(render_placed(&source, &plane, &turned, method, rotation).is_some(), "{step} {method:?} {rotation:?}");
                }
            }
        }
    }

    #[test]
    fn a_hair_off_zero_angle_keeps_coverage_matching_bounds() {
        let source = Raster::filled(26, 21, [1, 2, 3, 255]);
        let plane = coverage_plane(&Selection::from_rect(IRect::new(0, 0, 26, 21)).unwrap());
        let turned = placement(50.0, 40.0, 26.0, 21.0, -0.001);
        let placed = render_placed(&source, &plane, &turned, Resample::Nearest, RotationMethod::Plain).unwrap();
        let bounds = placed.selection.bounds();
        assert_eq!((placed.pixels.width(), placed.pixels.height()), (bounds.width() as u32, bounds.height() as u32));
    }
}
