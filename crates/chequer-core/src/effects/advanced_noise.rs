
use std::f32::consts::SQRT_2;

use super::params::{ColorSource, ParamSpec, Shown, blend, choice, color, flag, float, int, seed};
use super::util::{gaussian_kernel, gaussian_noise, hash, hash_unit, map_area, mix};
use super::{EffectParams, Job};
use crate::blend::blend as blend_pixels;
use crate::raster::{Raster, Rgba};

const ALGORITHMS: &[&str] = &["perlin", "simplex", "value", "cellular", "white", "blue", "voronoi"];
const FRACTALS: &[&str] = &["none", "fbm", "billow", "ridged"];
const CELL_MODES: &[&str] = &["f1", "f2", "edges", "cells"];
const VORONOI_MODES: &[&str] = &["tones", "lines", "outlined"];
const METRICS: &[&str] = &["euclidean", "manhattan", "chebyshev"];
const INTERPOLATIONS: &[&str] = &["nearest", "linear", "smooth"];
const DISTRIBUTIONS: &[&str] = &["uniform", "gaussian"];

const LATTICE: Shown = ("algorithm", &["perlin", "simplex", "value", "cellular", "voronoi"]);
const VALUE_ONLY: Shown = ("algorithm", &["value"]);
const CELLULAR_ONLY: Shown = ("algorithm", &["cellular"]);
const VORONOI_ONLY: Shown = ("algorithm", &["voronoi"]);
const JITTERED: Shown = ("algorithm", &["cellular", "voronoi"]);
const WHITE_ONLY: Shown = ("algorithm", &["white"]);
const GRAINY: Shown = ("algorithm", &["white", "blue"]);
const OCTAVES_ON: Shown = ("fractal", &["fbm", "billow", "ridged"]);

const BLACK: Rgba = [0, 0, 0, 255];
const WHITE: Rgba = [255, 255, 255, 255];

pub const PARAMS: &[ParamSpec] = &[
    choice("algorithm", ALGORITHMS, 0),
    int("scale", 2, 1000, 64).only_if(&[LATTICE]),
    choice("fractal", FRACTALS, 1).only_if(&[LATTICE]),
    int("octaves", 1, 10, 4).only_if(&[LATTICE, OCTAVES_ON]),
    float("lacunarity", 1.0, 4.0, 2.0, 0.05).only_if(&[LATTICE, OCTAVES_ON]),
    float("gain", 0.0, 1.0, 0.5, 0.01).only_if(&[LATTICE, OCTAVES_ON]),
    choice("interpolation", INTERPOLATIONS, 2).only_if(&[VALUE_ONLY]),
    choice("cell_mode", CELL_MODES, 0).only_if(&[CELLULAR_ONLY]),
    choice("metric", METRICS, 0).only_if(&[CELLULAR_ONLY]),
    float("jitter", 0.0, 1.0, 1.0, 0.01).only_if(&[JITTERED]),
    choice("voronoi_mode", VORONOI_MODES, 0).only_if(&[VORONOI_ONLY]),
    float("border", 0.01, 0.5, 0.08, 0.01).only_if(&[VORONOI_ONLY]),
    int("grain", 1, 64, 1).only_if(&[GRAINY]),
    choice("distribution", DISTRIBUTIONS, 0).only_if(&[WHITE_ONLY]),
    float("contrast", 0.1, 4.0, 1.0, 0.05),
    flag("invert", false),
    seed("seed"),
    color("color_a", BLACK, ColorSource::Primary),
    color("color_b", WHITE, ColorSource::Secondary),
    blend("blend"),
];

const PERLIN_GRADIENTS: [(f32, f32); 16] = [
    (1.0, 0.0),
    (0.923_879_5, 0.382_683_43),
    (0.707_106_77, 0.707_106_77),
    (0.382_683_43, 0.923_879_5),
    (0.0, 1.0),
    (-0.382_683_43, 0.923_879_5),
    (-0.707_106_77, 0.707_106_77),
    (-0.923_879_5, 0.382_683_43),
    (-1.0, 0.0),
    (-0.923_879_5, -0.382_683_43),
    (-0.707_106_77, -0.707_106_77),
    (-0.382_683_43, -0.923_879_5),
    (0.0, -1.0),
    (0.382_683_43, -0.923_879_5),
    (0.707_106_77, -0.707_106_77),
    (0.923_879_5, -0.382_683_43),
];
const PERLIN_GRADIENT_MASK: u32 = 15;

const SIMPLEX_GRADIENTS: [(f32, f32); 8] = [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)];
const SIMPLEX_GRADIENT_MASK: u32 = 7;
const SIMPLEX_SKEW: f32 = 0.366_025_4;
const SIMPLEX_UNSKEW: f32 = 0.211_324_87;
const SIMPLEX_FALLOFF_RADIUS: f32 = 0.5;
const SIMPLEX_SCALE: f32 = 70.0;

const HALF_HASH_BITS: u32 = 16;
const HALF_HASH_MASK: u32 = 0xFFFF;
const HALF_HASH_MAX: f32 = 65_535.0;
const CELL_CENTER: f32 = 0.5;
const CELL_DISTANCE_RANGE: f32 = 1.5;
const CELL_SHADE_SALT: u32 = 0x9E37_79B9;

const VORONOI_NEIGHBOURS: i32 = 2;
const VORONOI_TONE_SALT: u32 = 0x7F4A_7C15;
const VORONOI_BORDER_TONE: f32 = -1.0;

const OCTAVE_SEED_STEP: u32 = 17;
const OCTAVE_SHIFT: f32 = 37.31;
const HALF_PIXEL: f32 = 0.5;

const CALIBRATION_SIDE: usize = 64;
const CALIBRATION_STEP: f32 = 0.25;
const CALIBRATION_OFFSET: f32 = 0.37;
const LOW_PERCENTILE: f32 = 0.01;
const HIGH_PERCENTILE: f32 = 0.99;
const MIN_SPAN: f32 = 1e-6;

const MID_LEVEL: f32 = 0.5;
const GAUSSIAN_SIGMA: f32 = 0.2;

const BLUE_TILE: usize = 128;
const BLUE_SIGMA: f32 = 1.5;
const BLUE_REACH: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Interpolation {
    Nearest,
    Linear,
    Smooth,
}

impl Interpolation {
    fn from_key(key: &str) -> Self {
        match key {
            "nearest" => Self::Nearest,
            "linear" => Self::Linear,
            "smooth" => Self::Smooth,
            other => panic!("неизвестная интерполяция шума «{other}»"),
        }
    }
}

#[derive(Clone, Copy)]
enum CellMode {
    Nearest,
    Second,
    Edges,
    Shades,
}

impl CellMode {
    fn from_key(key: &str) -> Self {
        match key {
            "f1" => Self::Nearest,
            "f2" => Self::Second,
            "edges" => Self::Edges,
            "cells" => Self::Shades,
            other => panic!("неизвестный вывод ячеистого шума «{other}»"),
        }
    }
}

#[derive(Clone, Copy)]
enum Metric {
    Euclidean,
    Manhattan,
    Chebyshev,
}

impl Metric {
    fn from_key(key: &str) -> Self {
        match key {
            "euclidean" => Self::Euclidean,
            "manhattan" => Self::Manhattan,
            "chebyshev" => Self::Chebyshev,
            other => panic!("неизвестная метрика шума «{other}»"),
        }
    }

    fn distance(self, dx: f32, dy: f32) -> f32 {
        match self {
            Self::Euclidean => dx.hypot(dy),
            Self::Manhattan => dx.abs() + dy.abs(),
            Self::Chebyshev => dx.abs().max(dy.abs()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fractal {
    Plain,
    Fbm,
    Billow,
    Ridged,
}

impl Fractal {
    fn from_key(key: &str) -> Self {
        match key {
            "none" => Self::Plain,
            "fbm" => Self::Fbm,
            "billow" => Self::Billow,
            "ridged" => Self::Ridged,
            other => panic!("неизвестный фрактал шума «{other}»"),
        }
    }

    fn shape(self, signed: f32) -> f32 {
        match self {
            Self::Plain | Self::Fbm => signed,
            Self::Billow => signed.abs() * 2.0 - 1.0,
            Self::Ridged => 1.0 - signed.abs() * 2.0,
        }
    }
}

#[derive(Clone, Copy)]
struct Cellular {
    mode: CellMode,
    metric: Metric,
    jitter: f32,
}

#[derive(Clone, Copy)]
enum VoronoiMode {
    Tones,
    Lines,
    Outlined,
}

impl VoronoiMode {
    fn from_key(key: &str) -> Self {
        match key {
            "tones" => Self::Tones,
            "lines" => Self::Lines,
            "outlined" => Self::Outlined,
            other => panic!("неизвестный вывод шума Вороного «{other}»"),
        }
    }
}

#[derive(Clone, Copy)]
struct Voronoi {
    mode: VoronoiMode,
    jitter: f32,
    border: f32,
}

#[derive(Clone, Copy)]
enum Base {
    Perlin,
    Simplex,
    Value(Interpolation),
    Cellular(Cellular),
    Voronoi(Voronoi),
}

fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn perlin(seed: u32, x: f32, y: f32) -> f32 {
    let (cell_x, cell_y) = (x.floor(), y.floor());
    let (fx, fy) = (x - cell_x, y - cell_y);
    let (ix, iy) = (cell_x as i32, cell_y as i32);
    let corner = |dx: i32, dy: i32| {
        let (gx, gy) = PERLIN_GRADIENTS[(hash(seed, ix + dx, iy + dy) & PERLIN_GRADIENT_MASK) as usize];
        gx * (fx - dx as f32) + gy * (fy - dy as f32)
    };
    let (u, v) = (fade(fx), fade(fy));
    let top = lerp(corner(0, 0), corner(1, 0), u);
    let bottom = lerp(corner(0, 1), corner(1, 1), u);
    lerp(top, bottom, v) * SQRT_2
}

fn simplex(seed: u32, x: f32, y: f32) -> f32 {
    let skew = (x + y) * SIMPLEX_SKEW;
    let (cell_x, cell_y) = ((x + skew).floor(), (y + skew).floor());
    let unskew = (cell_x + cell_y) * SIMPLEX_UNSKEW;
    let (x0, y0) = (x - (cell_x - unskew), y - (cell_y - unskew));
    let (step_x, step_y) = if x0 > y0 { (1, 0) } else { (0, 1) };
    let corners = [
        (0, 0, x0, y0),
        (step_x, step_y, x0 - step_x as f32 + SIMPLEX_UNSKEW, y0 - step_y as f32 + SIMPLEX_UNSKEW),
        (1, 1, x0 - 1.0 + 2.0 * SIMPLEX_UNSKEW, y0 - 1.0 + 2.0 * SIMPLEX_UNSKEW),
    ];
    let sum: f32 = corners
        .iter()
        .map(|&(dx, dy, px, py)| {
            let falloff = SIMPLEX_FALLOFF_RADIUS - px * px - py * py;
            if falloff <= 0.0 {
                return 0.0;
            }
            let (gx, gy) = SIMPLEX_GRADIENTS[(hash(seed, cell_x as i32 + dx, cell_y as i32 + dy) & SIMPLEX_GRADIENT_MASK) as usize];
            let squared = falloff * falloff;
            squared * squared * (gx * px + gy * py)
        })
        .sum();
    sum * SIMPLEX_SCALE
}

fn value(seed: u32, x: f32, y: f32, interpolation: Interpolation) -> f32 {
    let (cell_x, cell_y) = (x.floor(), y.floor());
    let (ix, iy) = (cell_x as i32, cell_y as i32);
    let corner = |dx: i32, dy: i32| hash_unit(seed, ix + dx, iy + dy) * 2.0 - 1.0;
    let (fx, fy) = (x - cell_x, y - cell_y);
    let (u, v) = match interpolation {
        Interpolation::Nearest => return corner(0, 0),
        Interpolation::Linear => (fx, fy),
        Interpolation::Smooth => (fade(fx), fade(fy)),
    };
    lerp(lerp(corner(0, 0), corner(1, 0), u), lerp(corner(0, 1), corner(1, 1), u), v)
}

fn feature_point(seed: u32, cell_x: i32, cell_y: i32, jitter: f32) -> (f32, f32) {
    let bits = hash(seed, cell_x, cell_y);
    let random_x = (bits & HALF_HASH_MASK) as f32 / HALF_HASH_MAX;
    let random_y = (bits >> HALF_HASH_BITS) as f32 / HALF_HASH_MAX;
    (cell_x as f32 + CELL_CENTER + (random_x - CELL_CENTER) * jitter, cell_y as f32 + CELL_CENTER + (random_y - CELL_CENTER) * jitter)
}

fn cellular(seed: u32, x: f32, y: f32, options: Cellular) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (mut nearest, mut second, mut nearest_cell) = (f32::MAX, f32::MAX, (ix, iy));
    for cell_y in iy - 1..=iy + 1 {
        for cell_x in ix - 1..=ix + 1 {
            let (px, py) = feature_point(seed, cell_x, cell_y, options.jitter);
            let distance = options.metric.distance(px - x, py - y);
            if distance < nearest {
                second = nearest;
                nearest = distance;
                nearest_cell = (cell_x, cell_y);
            } else if distance < second {
                second = distance;
            }
        }
    }
    let signed_distance = |distance: f32| (distance / CELL_DISTANCE_RANGE).min(1.0) * 2.0 - 1.0;
    match options.mode {
        CellMode::Nearest => signed_distance(nearest),
        CellMode::Second => signed_distance(second),
        CellMode::Edges => signed_distance(second - nearest),
        CellMode::Shades => hash_unit(seed ^ CELL_SHADE_SALT, nearest_cell.0, nearest_cell.1) * 2.0 - 1.0,
    }
}

fn voronoi(seed: u32, x: f32, y: f32, options: Voronoi) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let neighbourhood = || (iy - VORONOI_NEIGHBOURS..=iy + VORONOI_NEIGHBOURS).flat_map(move |cell_y| (ix - VORONOI_NEIGHBOURS..=ix + VORONOI_NEIGHBOURS).map(move |cell_x| (cell_x, cell_y)));
    let (mut nearest, mut nearest_cell, mut site) = (f32::MAX, (ix, iy), (x, y));
    for (cell_x, cell_y) in neighbourhood() {
        let point = feature_point(seed, cell_x, cell_y, options.jitter);
        let distance = (point.0 - x).hypot(point.1 - y);
        if distance < nearest {
            (nearest, nearest_cell, site) = (distance, (cell_x, cell_y), point);
        }
    }
    let border_distance = neighbourhood()
        .filter(|&cell| cell != nearest_cell)
        .map(|(cell_x, cell_y)| {
            let other = feature_point(seed, cell_x, cell_y, options.jitter);
            let (toward_x, toward_y) = (other.0 - site.0, other.1 - site.1);
            let (middle_x, middle_y) = ((site.0 + other.0) / 2.0 - x, (site.1 + other.1) / 2.0 - y);
            (middle_x * toward_x + middle_y * toward_y) / toward_x.hypot(toward_y).max(f32::EPSILON)
        })
        .fold(f32::MAX, f32::min);
    let edge = (border_distance / options.border).clamp(0.0, 1.0);
    let tone = hash_unit(seed ^ VORONOI_TONE_SALT, nearest_cell.0, nearest_cell.1) * 2.0 - 1.0;
    match options.mode {
        VoronoiMode::Tones => tone,
        VoronoiMode::Lines => edge * 2.0 - 1.0,
        VoronoiMode::Outlined => lerp(VORONOI_BORDER_TONE, tone, edge),
    }
}

struct Lattice {
    base: Base,
    seed: u32,
    fractal: Fractal,
    octaves: u32,
    lacunarity: f32,
    gain: f32,
}

impl Lattice {
    fn from_params(algorithm: &str, params: &EffectParams) -> Self {
        let base = match algorithm {
            "perlin" => Base::Perlin,
            "simplex" => Base::Simplex,
            "value" => Base::Value(Interpolation::from_key(params.choice_key("interpolation"))),
            "cellular" => Base::Cellular(Cellular {
                mode: CellMode::from_key(params.choice_key("cell_mode")),
                metric: Metric::from_key(params.choice_key("metric")),
                jitter: params.float("jitter"),
            }),
            "voronoi" => Base::Voronoi(Voronoi { mode: VoronoiMode::from_key(params.choice_key("voronoi_mode")), jitter: params.float("jitter"), border: params.float("border") }),
            other => panic!("«{other}» - не решётчатый шум"),
        };
        let fractal = Fractal::from_key(params.choice_key("fractal"));
        let octaves = if fractal == Fractal::Plain { 1 } else { params.int("octaves") as u32 };
        Self { base, seed: params.seed("seed"), fractal, octaves, lacunarity: params.float("lacunarity"), gain: params.float("gain") }
    }

    fn layer(&self, octave: u32, x: f32, y: f32) -> f32 {
        let seed = self.seed.wrapping_add(octave.wrapping_mul(OCTAVE_SEED_STEP));
        match self.base {
            Base::Perlin => perlin(seed, x, y),
            Base::Simplex => simplex(seed, x, y),
            Base::Value(interpolation) => value(seed, x, y, interpolation),
            Base::Cellular(options) => cellular(seed, x, y, options),
            Base::Voronoi(options) => voronoi(seed, x, y, options),
        }
    }

    fn raw(&self, x: f32, y: f32) -> f32 {
        let (mut sum, mut total, mut amplitude, mut frequency) = (0.0, 0.0, 1.0, 1.0);
        for octave in 0..self.octaves {
            let shift = octave as f32 * OCTAVE_SHIFT;
            sum += amplitude * self.fractal.shape(self.layer(octave, x * frequency + shift, y * frequency + shift));
            total += amplitude;
            amplitude *= self.gain;
            frequency *= self.lacunarity;
        }
        sum / total
    }

    fn value_range(&self) -> (f32, f32) {
        let mut samples: Vec<f32> = (0..CALIBRATION_SIDE * CALIBRATION_SIDE)
            .map(|index| {
                let (column, row) = ((index % CALIBRATION_SIDE) as f32, (index / CALIBRATION_SIDE) as f32);
                self.raw(CALIBRATION_OFFSET + column * CALIBRATION_STEP, CALIBRATION_OFFSET + row * CALIBRATION_STEP)
            })
            .collect();
        samples.sort_by(f32::total_cmp);
        let at = |fraction: f32| samples[((samples.len() - 1) as f32 * fraction) as usize];
        (at(LOW_PERCENTILE), at(HIGH_PERCENTILE))
    }
}

fn white_level(seed: u32, x: i32, y: i32, grain: i32, gaussian: bool) -> f32 {
    let (cell_x, cell_y) = (x.div_euclid(grain), y.div_euclid(grain));
    if gaussian {
        (MID_LEVEL + gaussian_noise(seed, cell_x, cell_y) * GAUSSIAN_SIGMA).clamp(0.0, 1.0)
    } else {
        hash_unit(seed, cell_x, cell_y)
    }
}

fn blur_wrapped(data: &[f32], kernel: &[f32], along_rows: bool) -> Vec<f32> {
    let side = BLUE_TILE as i32;
    let reach = (kernel.len() / 2) as i32;
    (0..BLUE_TILE * BLUE_TILE)
        .map(|index| {
            let (x, y) = ((index % BLUE_TILE) as i32, (index / BLUE_TILE) as i32);
            kernel
                .iter()
                .zip(-reach..)
                .map(|(weight, offset)| {
                    let (source_x, source_y) = if along_rows { ((x + offset).rem_euclid(side), y) } else { (x, (y + offset).rem_euclid(side)) };
                    weight * data[(source_y * side + source_x) as usize]
                })
                .sum()
        })
        .collect()
}

fn ranks(values: &[f32]) -> Vec<f32> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let mut result = vec![0.0; values.len()];
    for (rank, &index) in order.iter().enumerate() {
        result[index] = rank as f32 / (values.len() - 1) as f32;
    }
    result
}

fn blue_tile(seed: u32) -> Vec<f32> {
    let white: Vec<f32> = (0..BLUE_TILE * BLUE_TILE).map(|index| hash_unit(seed, (index % BLUE_TILE) as i32, (index / BLUE_TILE) as i32)).collect();
    let kernel = gaussian_kernel(BLUE_SIGMA, BLUE_REACH);
    let smooth = blur_wrapped(&blur_wrapped(&white, &kernel, true), &kernel, false);
    let high_pass: Vec<f32> = white.iter().zip(&smooth).map(|(sharp, soft)| sharp - soft).collect();
    ranks(&high_pass)
}

enum Noise {
    Lattice { lattice: Lattice, frequency: f32, low: f32, span: f32 },
    White { seed: u32, grain: i32, gaussian: bool },
    Blue { tile: Vec<f32>, grain: i32 },
}

impl Noise {
    fn from_params(params: &EffectParams) -> Self {
        let (seed, grain) = (params.seed("seed"), params.int("grain"));
        match params.choice_key("algorithm") {
            "white" => Self::White { seed, grain, gaussian: params.choice_key("distribution") == "gaussian" },
            "blue" => Self::Blue { tile: blue_tile(seed), grain },
            algorithm => {
                let lattice = Lattice::from_params(algorithm, params);
                let (low, high) = lattice.value_range();
                Self::Lattice { lattice, frequency: 1.0 / params.int("scale") as f32, low, span: (high - low).max(MIN_SPAN) }
            }
        }
    }

    fn level(&self, x: i32, y: i32) -> f32 {
        match self {
            Self::Lattice { lattice, frequency, low, span } => {
                let raw = lattice.raw((x as f32 + HALF_PIXEL) * frequency, (y as f32 + HALF_PIXEL) * frequency);
                ((raw - low) / span).clamp(0.0, 1.0)
            }
            Self::White { seed, grain, gaussian } => white_level(*seed, x, y, *grain, *gaussian),
            Self::Blue { tile, grain } => {
                let side = BLUE_TILE as i32;
                let (tile_x, tile_y) = (x.div_euclid(*grain).rem_euclid(side), y.div_euclid(*grain).rem_euclid(side));
                tile[(tile_y * side + tile_x) as usize]
            }
        }
    }
}

fn tone(level: f32, contrast: f32, invert: bool) -> f32 {
    let level = ((level - MID_LEVEL) * contrast + MID_LEVEL).clamp(0.0, 1.0);
    if invert { 1.0 - level } else { level }
}

pub fn render(job: &Job) -> Option<Raster> {
    let noise = Noise::from_params(job.params);
    let (contrast, invert) = (job.params.float("contrast"), job.params.bool("invert"));
    let (mode, first, second) = (job.params.blend_mode("blend"), job.params.color("color_a"), job.params.color("color_b"));
    map_area(job.area, job.cancel, |x, y| {
        let level = tone(noise.level(x, y), contrast, invert);
        blend_pixels(mode, job.source.get(x as u32, y as u32), mix(first, second, level))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{Cancel, Palette, ParamValue, ids};
    use crate::geom::IRect;

    const SIDE: u32 = 96;
    const SCALE_INDEX: usize = 1;
    const TEST_SCALE: i32 = 12;
    const DARK_LIMIT: u8 = 40;
    const BRIGHT_LIMIT: u8 = 215;

    #[test]
    fn every_algorithm_fills_the_tone_range() {
        let palette = Palette { primary: BLACK, secondary: WHITE };
        let source = Raster::filled(SIDE, SIDE, BLACK);
        let area = IRect::new(0, 0, SIDE as i32, SIDE as i32);
        for (index, name) in ALGORITHMS.iter().enumerate() {
            let mut params = EffectParams::defaults(ids::ADVANCED_NOISE, palette);
            params.set(0, ParamValue::Choice(index));
            params.set(SCALE_INDEX, ParamValue::Int(TEST_SCALE));
            let result = crate::effects::render(&source, area, &params, &Cancel::default()).expect("шум посчитан");
            let levels: Vec<u8> = (0..SIDE * SIDE).map(|i| result.get(i % SIDE, i / SIDE)[0]).collect();
            let (darkest, brightest) = (levels.iter().copied().min().unwrap_or(0), levels.iter().copied().max().unwrap_or(0));
            assert!(darkest < DARK_LIMIT && brightest > BRIGHT_LIMIT, "{name}: яркость {darkest}..{brightest}");
        }
    }
}
