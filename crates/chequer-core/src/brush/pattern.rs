use std::f32::consts::{PI, TAU};

use serde::{Deserialize, Serialize};

use super::grain::hash_unit;
use super::texture::Texture;

pub const TEXTURE_SIDE: u32 = 256;
pub const HAIR_SLOTS: u32 = 192;
pub const PAPER_KINDS: [PatternKind; 5] = [PatternKind::Noise, PatternKind::Fibers, PatternKind::Dots, PatternKind::Cells, PatternKind::Weave];
pub const HAIR_KINDS: [PatternKind; 1] = [PatternKind::Bristles];

const SIDE: usize = TEXTURE_SIDE as usize;
const SIDE_F: f32 = TEXTURE_SIDE as f32;
const BYTE_RANGE: f32 = 255.0;
const MIN_FILL_RANGE: f32 = 1e-3;
const SEED_STRIDE: u32 = 0x9E37;
const FEATURE_RANGE: (f32, f32) = (3.0, 96.0);
const NOISE_SALT: u32 = 0x41;
const NOISE_DETAIL_SHARE: f32 = 0.5;
const FIBERS_RANGE: (f32, f32) = (200.0, 3000.0);
const FIBER_LENGTH_SCALE: (f32, f32) = (0.3, 2.0);
const DOTS_RANGE: (f32, f32) = (60.0, 3000.0);
const DOT_RADIUS_RANGE: (f32, f32) = (0.8, 7.0);
const DOT_SALT: u32 = 0x43;
const DOT_MIN_DEPTH: f32 = 0.4;
const CELL_SALT: u32 = 0x44;
const WEAVE_SALT: u32 = 0x45;
const WEAVE_OVER_SHARE: f32 = 0.5;
const WEAVE_IRREGULARITY: f32 = 0.5;
const HAIR_SALT: u32 = 0x61;
const HAIR_SPAN_RANGE: (f32, f32) = (1.0, 10.0);
const HAIR_LOAD_BIAS: f32 = 0.8;
const CONTRAST_GAIN: f32 = 2.0;
const PAPER_OCTAVES: [(i32, f32); 3] = [(6, 0.5), (14, 0.3), (30, 0.2)];
const PAPER_BASE_SALT: u32 = 0x51;
const PAPER_FIBER_COUNT: i32 = 1800;
const PAPER_FIBER_SALT: u32 = 0x52;
const FIBER_MIN_LENGTH: f32 = 8.0;
const FIBER_LENGTH_SPAN: f32 = 24.0;
const FIBER_STEP: f32 = 0.6;
const FIBER_MAX_BEND: f32 = 0.08;
const FIBER_AMPLITUDE: f32 = 0.22;
const FIBER_RIDGE_SHARE: f32 = 0.65;
const PORE_SALT: u32 = 0x53;
const PORE_AMPLITUDE: f32 = 0.12;
const LOW_PERCENTILE: f32 = 0.02;
const HIGH_PERCENTILE: f32 = 0.98;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    Noise,
    Fibers,
    Dots,
    Cells,
    Weave,
    Bristles,
}

impl PatternKind {
    pub fn uses_density(self) -> bool {
        self != Self::Bristles
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PatternSpec {
    pub kind: PatternKind,
    pub scale: f32,
    pub density: f32,
    pub contrast: f32,
    pub seed: u32,
}

impl Default for PatternSpec {
    fn default() -> Self {
        Self::preset(PatternKind::Noise)
    }
}

impl PatternSpec {
    pub fn preset(kind: PatternKind) -> Self {
        let (scale, density, contrast) = match kind {
            PatternKind::Noise => (0.4, 0.5, 0.5),
            PatternKind::Fibers => (0.4, 0.5, 0.5),
            PatternKind::Dots => (0.3, 0.5, 0.6),
            PatternKind::Cells => (0.4, 0.7, 0.6),
            PatternKind::Weave => (0.35, 0.3, 0.6),
            PatternKind::Bristles => (4.0 / 9.0, 0.5, 0.95),
        };
        Self { kind, scale, density, contrast, seed: 0 }
    }

    pub fn clamped(self) -> Self {
        let unit = |value: f32| value.clamp(0.0, 1.0);
        Self { scale: unit(self.scale), density: unit(self.density), contrast: unit(self.contrast), ..self }
    }

    pub fn reseeded(self) -> Self {
        Self { seed: self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223), ..self }
    }

    fn salt(&self, base: u32) -> u32 {
        base.wrapping_add(self.seed.wrapping_mul(SEED_STRIDE))
    }
}

pub fn generate(spec: PatternSpec) -> Texture {
    let spec = spec.clamped();
    let (width, height, pixels) = match spec.kind {
        PatternKind::Bristles => (HAIR_SLOTS, 1, bristle_loads(&spec)),
        kind => (TEXTURE_SIDE, TEXTURE_SIDE, shaped(&heights(kind, &spec), spec.contrast)),
    };
    Texture::generated(width, height, pixels, Some(spec))
}

pub(super) fn paper_texture() -> Texture {
    Texture::generated(TEXTURE_SIDE, TEXTURE_SIDE, normalized(&paper_heights()), None)
}

fn heights(kind: PatternKind, spec: &PatternSpec) -> Vec<f32> {
    match kind {
        PatternKind::Noise => noise_heights(spec),
        PatternKind::Fibers => fiber_heights(spec),
        PatternKind::Dots => dot_heights(spec),
        PatternKind::Cells => cell_heights(spec),
        PatternKind::Weave => weave_heights(spec),
        PatternKind::Bristles => Vec::new(),
    }
}

fn lerp(range: (f32, f32), amount: f32) -> f32 {
    range.0 + (range.1 - range.0) * amount
}

fn cells_across(scale: f32) -> i32 {
    (SIDE_F / lerp(FEATURE_RANGE, scale)).round().max(1.0) as i32
}

fn each_pixel(mut height_at: impl FnMut(i32, i32) -> f32) -> Vec<f32> {
    (0..SIDE * SIDE).map(|index| height_at((index % SIDE) as i32, (index / SIDE) as i32)).collect()
}

fn periodic_noise(x: f32, y: f32, cells: i32, salt: u32) -> f32 {
    let (cell_x, cell_y) = (x.floor(), y.floor());
    let smooth = |fraction: f32| fraction * fraction * (3.0 - 2.0 * fraction);
    let (blend_x, blend_y) = (smooth(x - cell_x), smooth(y - cell_y));
    let corner = |step_x: i32, step_y: i32| hash_unit((cell_x as i32 + step_x).rem_euclid(cells), (cell_y as i32 + step_y).rem_euclid(cells), salt);
    let mix = |from: f32, to: f32, amount: f32| from + (to - from) * amount;
    mix(mix(corner(0, 0), corner(1, 0), blend_x), mix(corner(0, 1), corner(1, 1), blend_x), blend_y)
}

fn wrapped_index(x: i32, y: i32) -> usize {
    y.rem_euclid(SIDE as i32) as usize * SIDE + x.rem_euclid(SIDE as i32) as usize
}

fn splat(height: &mut [f32], x: f32, y: f32, amount: f32) {
    let (cell_x, cell_y) = (x.floor(), y.floor());
    let (fraction_x, fraction_y) = (x - cell_x, y - cell_y);
    for (step_x, step_y, weight) in [
        (0, 0, (1.0 - fraction_x) * (1.0 - fraction_y)),
        (1, 0, fraction_x * (1.0 - fraction_y)),
        (0, 1, (1.0 - fraction_x) * fraction_y),
        (1, 1, fraction_x * fraction_y),
    ] {
        height[wrapped_index(cell_x as i32 + step_x, cell_y as i32 + step_y)] += amount * weight;
    }
}

fn lay_fiber(height: &mut [f32], fiber: i32, salt: u32, length_scale: f32) {
    let random = |slot: i32| hash_unit(fiber, slot, salt);
    let (mut x, mut y) = (random(1) * SIDE_F, random(2) * SIDE_F);
    let mut angle = random(3) * TAU;
    let length = (FIBER_MIN_LENGTH + random(4) * FIBER_LENGTH_SPAN) * length_scale;
    let amplitude = if random(5) < FIBER_RIDGE_SHARE { FIBER_AMPLITUDE } else { -FIBER_AMPLITUDE };
    for step in 0..(length / FIBER_STEP) as i32 {
        angle += (random(6 + step) - 0.5) * 2.0 * FIBER_MAX_BEND;
        x += angle.cos() * FIBER_STEP;
        y += angle.sin() * FIBER_STEP;
        splat(height, x, y, amplitude * FIBER_STEP);
    }
}

fn percentile(values: &[f32], fraction: f32) -> f32 {
    let mut copy = values.to_vec();
    let index = ((copy.len() - 1) as f32 * fraction) as usize;
    *copy.select_nth_unstable_by(index, f32::total_cmp).1
}

fn normalized(height: &[f32]) -> Vec<u8> {
    let low = percentile(height, LOW_PERCENTILE);
    let range = (percentile(height, HIGH_PERCENTILE) - low).max(MIN_FILL_RANGE);
    height.iter().map(|&value| (((value - low) / range).clamp(0.0, 1.0) * BYTE_RANGE).round() as u8).collect()
}

fn shaped(height: &[f32], contrast: f32) -> Vec<u8> {
    let gain = contrast * CONTRAST_GAIN;
    normalized(height).into_iter().map(|value| (((f32::from(value) / BYTE_RANGE - 0.5) * gain + 0.5).clamp(0.0, 1.0) * BYTE_RANGE).round() as u8).collect()
}

fn paper_heights() -> Vec<f32> {
    let mut height = each_pixel(|x, y| {
        let undulation: f32 = PAPER_OCTAVES
            .iter()
            .map(|&(cells, weight)| {
                let to_cells = cells as f32 / SIDE_F;
                weight * periodic_noise(x as f32 * to_cells, y as f32 * to_cells, cells, PAPER_BASE_SALT + cells as u32)
            })
            .sum();
        undulation + (hash_unit(x, y, PORE_SALT) - 0.5) * 2.0 * PORE_AMPLITUDE
    });
    for fiber in 0..PAPER_FIBER_COUNT {
        lay_fiber(&mut height, fiber, PAPER_FIBER_SALT, 1.0);
    }
    height
}

fn noise_heights(spec: &PatternSpec) -> Vec<f32> {
    let cells = cells_across(spec.scale);
    let (salt, detail) = (spec.salt(NOISE_SALT), spec.density * NOISE_DETAIL_SHARE);
    each_pixel(|x, y| {
        let (u, v) = (x as f32 / SIDE_F * cells as f32, y as f32 / SIDE_F * cells as f32);
        periodic_noise(u, v, cells, salt) * (1.0 - detail) + periodic_noise(u * 2.0, v * 2.0, cells * 2, salt + 1) * detail
    })
}

fn fiber_heights(spec: &PatternSpec) -> Vec<f32> {
    let mut height = vec![0.0; SIDE * SIDE];
    let (salt, length_scale) = (spec.salt(PAPER_FIBER_SALT), lerp(FIBER_LENGTH_SCALE, spec.scale));
    for fiber in 0..lerp(FIBERS_RANGE, spec.density) as i32 {
        lay_fiber(&mut height, fiber, salt, length_scale);
    }
    height
}

fn dot_heights(spec: &PatternSpec) -> Vec<f32> {
    let mut height = vec![0.0; SIDE * SIDE];
    let (salt, radius) = (spec.salt(DOT_SALT), lerp(DOT_RADIUS_RANGE, spec.scale));
    let reach = (radius * 2.0).ceil() as i32;
    for dot in 0..lerp(DOTS_RANGE, spec.density) as i32 {
        let (center_x, center_y) = (hash_unit(dot, 1, salt) * SIDE_F, hash_unit(dot, 2, salt) * SIDE_F);
        let depth = DOT_MIN_DEPTH + (1.0 - DOT_MIN_DEPTH) * hash_unit(dot, 3, salt);
        for offset_y in -reach..=reach {
            for offset_x in -reach..=reach {
                let (pixel_x, pixel_y) = (center_x.floor() as i32 + offset_x, center_y.floor() as i32 + offset_y);
                let (dx, dy) = (pixel_x as f32 + 0.5 - center_x, pixel_y as f32 + 0.5 - center_y);
                height[wrapped_index(pixel_x, pixel_y)] -= depth * (-(dx * dx + dy * dy) / (radius * radius)).exp();
            }
        }
    }
    height
}

fn cell_heights(spec: &PatternSpec) -> Vec<f32> {
    let (cells, salt, jitter) = (cells_across(spec.scale), spec.salt(CELL_SALT), spec.density);
    let point = |cell_x: i32, cell_y: i32| {
        let (wrapped_x, wrapped_y) = (cell_x.rem_euclid(cells), cell_y.rem_euclid(cells));
        (cell_x as f32 + 0.5 + (hash_unit(wrapped_x, wrapped_y, salt) - 0.5) * jitter, cell_y as f32 + 0.5 + (hash_unit(wrapped_x, wrapped_y, salt + 1) - 0.5) * jitter)
    };
    each_pixel(|x, y| {
        let (u, v) = (x as f32 / SIDE_F * cells as f32, y as f32 / SIDE_F * cells as f32);
        let nearest = (-1..=1)
            .flat_map(|step_y| (-1..=1).map(move |step_x| (step_x, step_y)))
            .map(|(step_x, step_y)| {
                let (point_x, point_y) = point(u.floor() as i32 + step_x, v.floor() as i32 + step_y);
                (point_x - u).hypot(point_y - v)
            })
            .fold(f32::MAX, f32::min);
        -nearest
    })
}

fn weave_heights(spec: &PatternSpec) -> Vec<f32> {
    let cells = cells_across(spec.scale);
    let threads = (cells + cells % 2).max(2);
    let (period, salt) = (SIDE_F / threads as f32, spec.salt(WEAVE_SALT));
    each_pixel(|x, y| {
        let (u, v) = (x as f32 / period, y as f32 / period);
        let (thread_x, thread_y) = (u.floor() as i32, v.floor() as i32);
        let horizontal_over = (thread_x + thread_y).rem_euclid(2) == 0;
        let across = if horizontal_over { v - v.floor() } else { u - u.floor() };
        let irregular = (hash_unit(thread_x.rem_euclid(threads), thread_y.rem_euclid(threads), salt) - 0.5) * spec.density * WEAVE_IRREGULARITY;
        let lift = if horizontal_over { WEAVE_OVER_SHARE } else { 0.0 };
        lift + (across * PI).sin() * (1.0 - WEAVE_OVER_SHARE) + irregular
    })
}

fn bristle_loads(spec: &PatternSpec) -> Vec<u8> {
    let (salt, span) = (spec.salt(HAIR_SALT), lerp(HAIR_SPAN_RANGE, spec.scale));
    let mut loads = Vec::with_capacity(HAIR_SLOTS as usize);
    let mut hair = 0;
    while loads.len() < HAIR_SLOTS as usize {
        let width = 1 + (hash_unit(hair, 1, salt) * span) as usize;
        let load = (1.0 - spec.contrast) + spec.contrast * hash_unit(hair, 2, salt).powf(HAIR_LOAD_BIAS);
        loads.extend(std::iter::repeat_n((load * BYTE_RANGE).round() as u8, width));
        hair += 1;
    }
    loads.truncate(HAIR_SLOTS as usize);
    loads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_makes_a_full_tonal_range() {
        for kind in PAPER_KINDS {
            let texture = generate(PatternSpec::preset(kind));
            let pixels = texture.pixels();
            assert!(pixels.iter().any(|&value| value < 64) && pixels.iter().any(|&value| value > 192), "{kind:?} is flat");
        }
    }

    #[test]
    fn same_spec_gives_same_pixels_and_other_seed_differs() {
        let spec = PatternSpec::preset(PatternKind::Dots);
        assert_eq!(generate(spec).pixels(), generate(spec).pixels());
        assert_ne!(generate(spec).pixels(), generate(spec.reseeded()).pixels());
    }

    #[test]
    fn bristles_fill_all_slots_and_zero_contrast_is_uniform() {
        let flat = generate(PatternSpec { contrast: 0.0, ..PatternSpec::preset(PatternKind::Bristles) });
        assert_eq!(flat.width(), HAIR_SLOTS);
        assert!(flat.pixels().iter().all(|&value| value == u8::MAX));
    }
}
