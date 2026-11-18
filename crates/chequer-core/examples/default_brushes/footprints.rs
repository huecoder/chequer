use std::path::PathBuf;

use chequer_core::brush::{Brush, BrushSettings, BrushTip, MaskTip, TipVariant};

use super::classic::brush;
use super::texture::smooth_step;

const TRACKS_CATEGORY: &str = "tracks";
const SHOE_ID: &str = "trk.shoe";
const SNEAKER_ID: &str = "trk.sneaker";
const BARE_FOOT_ID: &str = "trk.bare_foot";
const SHOE_SOURCE: &str = "shoe_prints.png";
const SHOE_ATTRIBUTION: (&str, &str) = ("Wikimedia Commons, File:Footprints.png", "CC0");

const MASK_SIDE: usize = 512;
const PRINT_LENGTH_SHARE: f32 = 0.9;
const FOOT_OFFSET_SHARE: f32 = 0.2;
const TOE_UP_DEGREES: f32 = 90.0;
const STEP_SPACING: f32 = 1.5;
const SIZE_JITTER: f32 = 0.04;
const ANGLE_JITTER: f32 = 0.02;
const OPACITY_JITTER: f32 = 0.12;

const GAP_CLOSING_RADIUS: usize = 40;
const SOLE_RIM: usize = 20;
const TREAD_PERIOD: f32 = 52.0;
const TREAD_GROOVE_HALF: f32 = 6.0;
const TREAD_EDGE_SOFTNESS: f32 = 2.0;
const BRIDGE_REACH: usize = 6;
const INK_THRESHOLD: f32 = 0.5;
const TOE_OUT_DEGREES: f32 = 5.0;
const EDGE_TEXELS: f32 = 1.0;

struct Blob {
    at: (f32, f32),
    radii: (f32, f32),
}

const SOLE_OUTLINE: [(f32, f32); 22] = [
    (-0.150, 0.230),
    (-0.152, 0.320),
    (-0.125, 0.400),
    (-0.070, 0.465),
    (-0.028, 0.545),
    (-0.020, 0.640),
    (-0.045, 0.725),
    (-0.082, 0.800),
    (-0.085, 0.890),
    (-0.045, 0.965),
    (0.020, 0.990),
    (0.075, 0.955),
    (0.100, 0.880),
    (0.105, 0.790),
    (0.118, 0.690),
    (0.140, 0.570),
    (0.162, 0.450),
    (0.178, 0.340),
    (0.175, 0.250),
    (0.140, 0.195),
    (0.050, 0.172),
    (-0.070, 0.182),
];
const OUTLINE_STEPS_PER_SPAN: usize = 14;
const TOES: [Blob; 5] = [
    Blob { at: (-0.100, 0.088), radii: (0.050, 0.064) },
    Blob { at: (-0.011, 0.066), radii: (0.030, 0.042) },
    Blob { at: (0.056, 0.078), radii: (0.028, 0.038) },
    Blob { at: (0.118, 0.100), radii: (0.025, 0.034) },
    Blob { at: (0.173, 0.134), radii: (0.021, 0.030) },
];

#[derive(Clone)]
struct Grid {
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl Grid {
    fn empty(width: usize, height: usize) -> Self {
        Self { width, height, values: vec![0.0; width * height] }
    }

    fn from_alpha(path: &PathBuf) -> Self {
        let image = image::open(path).unwrap_or_else(|error| panic!("{}: {error}", path.display())).to_rgba8();
        let values = image.pixels().map(|pixel| f32::from(pixel[3]) / u8::MAX as f32).collect();
        Self { width: image.width() as usize, height: image.height() as usize, values }
    }

    fn at(&self, x: i64, y: i64) -> f32 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 { 0.0 } else { self.values[y as usize * self.width + x as usize] }
    }

    fn sample(&self, x: f32, y: f32) -> f32 {
        let (left, top) = ((x - 0.5).floor(), (y - 0.5).floor());
        let (fraction_x, fraction_y) = (x - 0.5 - left, y - 0.5 - top);
        let (left, top) = (left as i64, top as i64);
        let upper = self.at(left, top) + (self.at(left + 1, top) - self.at(left, top)) * fraction_x;
        let lower = self.at(left, top + 1) + (self.at(left + 1, top + 1) - self.at(left, top + 1)) * fraction_x;
        upper + (lower - upper) * fraction_y
    }

    fn filtered(&self, radius: usize, vertical_only: bool, pick: fn(f32, f32) -> f32, outside: f32) -> Self {
        let mut result = self.clone();
        let reach = radius as i64;
        for y in 0..self.height as i64 {
            for x in 0..self.width as i64 {
                let mut value = self.values[y as usize * self.width + x as usize];
                for step in -reach..=reach {
                    value = pick(value, self.at_or(x, y + step, outside));
                    if !vertical_only {
                        value = pick(value, self.at_or(x + step, y, outside));
                    }
                }
                result.values[y as usize * self.width + x as usize] = value;
            }
        }
        result
    }

    fn at_or(&self, x: i64, y: i64, outside: f32) -> f32 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 { outside } else { self.values[y as usize * self.width + x as usize] }
    }

    fn binary(&self) -> Self {
        Self { values: self.values.iter().map(|&value| if value > INK_THRESHOLD { 1.0 } else { 0.0 }).collect(), ..self.clone() }
    }

    fn closed_vertically(&self, radius: usize) -> Self {
        let mut padded = Self::empty(self.width, self.height + 2 * radius);
        padded.values[radius * self.width..(radius + self.height) * self.width].copy_from_slice(&self.values);
        let closed = padded.binary().filtered(radius, true, f32::max, 0.0).filtered(radius, true, f32::min, 0.0);
        let inside = &closed.values[radius * self.width..(radius + self.height) * self.width];
        Self { values: inside.iter().zip(&self.values).map(|(&closed, &original)| closed.max(original)).collect(), ..self.clone() }
    }

    fn eroded(&self, radius: usize) -> Self {
        self.filtered(radius, false, f32::min, 0.0)
    }

    fn dilated(&self, radius: usize) -> Self {
        self.filtered(radius, false, f32::max, 0.0)
    }

    fn bounds(&self) -> (f32, f32, f32, f32) {
        let mut found = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (index, &value) in self.values.iter().enumerate() {
            if value > INK_THRESHOLD {
                let (x, y) = ((index % self.width) as f32, (index / self.width) as f32);
                found = (found.0.min(x), found.1.min(y), found.2.max(x + 1.0), found.3.max(y + 1.0));
            }
        }
        found
    }

    fn mask(&self) -> MaskTip {
        let pixels = self.values.iter().map(|value| (value.clamp(0.0, 1.0) * u8::MAX as f32).round() as u8).collect();
        MaskTip::new(self.width as u32, self.height as u32, pixels).expect("footprint mask is within tip limits")
    }
}

fn components(ink: &Grid) -> Vec<(Vec<usize>, f32)> {
    let mut seen = vec![false; ink.values.len()];
    let mut found = Vec::new();
    for start in 0..ink.values.len() {
        if seen[start] || ink.values[start] <= 0.0 {
            continue;
        }
        let mut pixels = Vec::new();
        let mut pending = vec![start];
        seen[start] = true;
        while let Some(index) = pending.pop() {
            pixels.push(index);
            let (x, y) = (index % ink.width, index / ink.width);
            for (step_x, step_y) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                let (next_x, next_y) = (x as i64 + step_x, y as i64 + step_y);
                if next_x < 0 || next_y < 0 || next_x >= ink.width as i64 || next_y >= ink.height as i64 {
                    continue;
                }
                let next = next_y as usize * ink.width + next_x as usize;
                if !seen[next] && ink.values[next] > 0.0 {
                    seen[next] = true;
                    pending.push(next);
                }
            }
        }
        let center_x = pixels.iter().map(|&index| (index % ink.width) as f32).sum::<f32>() / pixels.len() as f32;
        found.push((pixels, center_x));
    }
    found
}

fn left_and_right(pair: &Grid) -> (Grid, Grid) {
    let (mut left, mut right) = (Grid::empty(pair.width, pair.height), Grid::empty(pair.width, pair.height));
    for (pixels, center_x) in components(pair) {
        let foot = if center_x < pair.width as f32 / 2.0 { &mut left } else { &mut right };
        for index in pixels {
            foot.values[index] = pair.values[index];
        }
    }
    (left, right)
}

fn placed(foot: &Grid, toward_right: f32) -> Grid {
    let (left, top, right, bottom) = foot.bounds();
    let scale = PRINT_LENGTH_SHARE * MASK_SIDE as f32 / (bottom - top);
    let (source_x, source_y) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let (center_x, center_y) = (MASK_SIDE as f32 * (0.5 + toward_right * FOOT_OFFSET_SHARE), MASK_SIDE as f32 / 2.0);
    let mut mask = Grid::empty(MASK_SIDE, MASK_SIDE);
    for y in 0..MASK_SIDE {
        for x in 0..MASK_SIDE {
            mask.values[y * MASK_SIDE + x] = foot.sample((x as f32 + 0.5 - center_x) / scale + source_x, (y as f32 + 0.5 - center_y) / scale + source_y);
        }
    }
    mask
}

impl Blob {
    fn distance(&self, across: f32, along: f32) -> f32 {
        let thinnest = self.radii.0.min(self.radii.1);
        (((across - self.at.0) / self.radii.0).hypot((along - self.at.1) / self.radii.1) - 1.0) * thinnest
    }
}

fn smooth_outline() -> Vec<(f32, f32)> {
    let count = SOLE_OUTLINE.len();
    let point = |index: usize| SOLE_OUTLINE[index % count];
    let spline = |from: f32, to: f32, before: f32, after: f32, fraction: f32| {
        0.5 * (2.0 * from + (to - before) * fraction + (2.0 * before - 5.0 * from + 4.0 * to - after) * fraction * fraction + (3.0 * from - before - 3.0 * to + after) * fraction * fraction * fraction)
    };
    (0..count)
        .flat_map(|span| {
            let (before, from, to, after) = (point(span + count - 1), point(span), point(span + 1), point(span + 2));
            (0..OUTLINE_STEPS_PER_SPAN).map(move |step| {
                let fraction = step as f32 / OUTLINE_STEPS_PER_SPAN as f32;
                (spline(from.0, to.0, before.0, after.0, fraction), spline(from.1, to.1, before.1, after.1, fraction))
            })
        })
        .collect()
}

fn outline_distance(outline: &[(f32, f32)], across: f32, along: f32) -> f32 {
    let mut nearest = f32::MAX;
    let mut crossings = 0;
    for (index, &(from_x, from_y)) in outline.iter().enumerate() {
        let (to_x, to_y) = outline[(index + 1) % outline.len()];
        let (edge_x, edge_y) = (to_x - from_x, to_y - from_y);
        let reach = (((across - from_x) * edge_x + (along - from_y) * edge_y) / (edge_x * edge_x + edge_y * edge_y)).clamp(0.0, 1.0);
        nearest = nearest.min((across - from_x - edge_x * reach).hypot(along - from_y - edge_y * reach));
        if (from_y > along) != (to_y > along) && across < from_x + (along - from_y) / edge_y * edge_x {
            crossings += 1;
        }
    }
    if crossings % 2 == 1 { -nearest } else { nearest }
}

fn bare_foot(toward_right: f32) -> Grid {
    let length = PRINT_LENGTH_SHARE * MASK_SIDE as f32;
    let (center_x, center_y) = (MASK_SIDE as f32 * (0.5 + toward_right * FOOT_OFFSET_SHARE), MASK_SIDE as f32 / 2.0);
    let (turn_sin, turn_cos) = (toward_right * TOE_OUT_DEGREES.to_radians()).sin_cos();
    let outline = smooth_outline();
    let mut mask = Grid::empty(MASK_SIDE, MASK_SIDE);
    for y in 0..MASK_SIDE {
        for x in 0..MASK_SIDE {
            let (offset_x, offset_y) = (x as f32 + 0.5 - center_x, y as f32 + 0.5 - center_y);
            let across = (offset_x * turn_cos - offset_y * turn_sin) / length * toward_right;
            let along = (offset_x * turn_sin + offset_y * turn_cos) / length + 0.5;
            let sole = outline_distance(&outline, across, along);
            let nearest = TOES.iter().map(|blob| blob.distance(across, along)).fold(sole, f32::min);
            mask.values[y * MASK_SIDE + x] = (0.5 - nearest * length / EDGE_TEXELS).clamp(0.0, 1.0);
        }
    }
    mask
}

fn waffle_sole(shoe: &Grid) -> Grid {
    let closed = shoe.closed_vertically(GAP_CLOSING_RADIUS);
    let bridge = Grid { values: closed.values.iter().zip(&shoe.values).map(|(closed, shoe)| (closed - shoe).max(0.0)).collect(), ..closed.clone() }.dilated(BRIDGE_REACH);
    let inner = closed.eroded(SOLE_RIM);
    let mut sole = closed.clone();
    for y in 0..closed.height {
        for x in 0..closed.width {
            let index = y * closed.width + x;
            let diagonals = [(x + y) as f32, x as f32 - y as f32 + closed.height as f32];
            let nearest_groove = diagonals.iter().map(|line| ((line / std::f32::consts::SQRT_2).rem_euclid(TREAD_PERIOD) - TREAD_PERIOD / 2.0).abs()).fold(f32::MAX, f32::min);
            let groove = 1.0 - smooth_step(TREAD_GROOVE_HALF - TREAD_EDGE_SOFTNESS, TREAD_GROOVE_HALF + TREAD_EDGE_SOFTNESS, nearest_groove);
            let treaded = inner.values[index] * (1.0 - bridge.values[index]);
            sole.values[index] = closed.values[index] * (1.0 - groove * treaded);
        }
    }
    sole
}

fn stepping_brush(id: &str, left: Grid, right: Grid, attribution: Option<(&str, &str)>) -> Brush {
    let settings = BrushSettings {
        angle: TOE_UP_DEGREES,
        follow_direction: true,
        cycle_tips: true,
        spacing: STEP_SPACING,
        size_jitter: SIZE_JITTER,
        angle_jitter: ANGLE_JITTER,
        opacity_jitter: OPACITY_JITTER,
        pressure_size: 0.0,
        hardness: 1.0,
        ..BrushSettings::default()
    };
    let mut footprints = brush(id, TRACKS_CATEGORY, settings, BrushTip::from_variants(vec![TipVariant::new(left.mask()), TipVariant::new(right.mask())]));
    if let Some((author, license)) = attribution {
        footprints.meta.author = author.into();
        footprints.meta.license = license.into();
    }
    footprints
}

fn source(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/sources").join(name)
}

pub fn footprint_brushes() -> Vec<Brush> {
    let (shoe_left, shoe_right) = left_and_right(&Grid::from_alpha(&source(SHOE_SOURCE)));
    let (shoe_left, shoe_right) = (placed(&shoe_left, -1.0), placed(&shoe_right, 1.0));
    let sneaker = (waffle_sole(&shoe_left), waffle_sole(&shoe_right));
    vec![
        stepping_brush(SHOE_ID, shoe_left, shoe_right, Some(SHOE_ATTRIBUTION)),
        stepping_brush(SNEAKER_ID, sneaker.0, sneaker.1, Some(SHOE_ATTRIBUTION)),
        stepping_brush(BARE_FOOT_ID, bare_foot(-1.0), bare_foot(1.0), None),
    ]
}
