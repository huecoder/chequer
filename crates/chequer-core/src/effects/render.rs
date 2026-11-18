
use super::params::{ColorSource, angle, blend, choice, color, flag, float, int, seed};
use super::util::{center_of, clamp_channel, fractal_noise, map_area, map_supersampled, mix, value_noise};
use super::{Category, EffectDef, Job, ids};
use crate::blend::blend as blend_pixels;
use crate::raster::{Raster, Rgba};

const NOISE_KINDS: &[&str] = &["turbulence", "fractal_sum"];
const BLACK: Rgba = [0, 0, 0, 255];
const WHITE: Rgba = [255, 255, 255, 255];
const CLOUD_CONTRAST: f32 = 2.2;
const MAX_CLOUD_OCTAVES: u32 = 8;
const MIN_CLOUD_PERSISTENCE: f32 = 0.1;
const FRACTAL_ITERATIONS_BASE: u32 = 64;
const FRACTAL_ITERATIONS_PER_QUALITY: u32 = 32;
const ESCAPE_RADIUS_SQUARED: f32 = 256.0;
const JULIA_RADIUS: f32 = 0.7885;
const JULIA_ANGLE_PER_FACTOR: f32 = 0.6;
const JULIA_WIDTH: f32 = 4.0;
const MANDELBROT_WIDTH: f32 = 3.5;
const MANDELBROT_CENTER_X: f32 = -0.5;
const MANDELBROT_ZOOM_UNITS: f32 = 10.0;
const HUE_DEGREES: f32 = 360.0;
const HUE_PERIOD: f32 = 32.0;
const TURBULENCE_SIZE_UNITS: f32 = 25.0;
const MID_LEVEL: f32 = 128.0;
const NOISE_SEED_STEP: u32 = 17;
const MAX_FRACTAL_QUALITY: i32 = 3;

pub static DEFS: [EffectDef; 5] = [
    EffectDef { id: ids::ADVANCED_NOISE, category: Category::Render, params: super::advanced_noise::PARAMS, render: super::advanced_noise::render },
    EffectDef {
        id: ids::CLOUDS,
        category: Category::Render,
        params: &[
            int("scale", 2, 1000, 250),
            int("roughness", 0, 100, 50),
            blend("blend"),
            seed("seed"),
            color("color_a", BLACK, ColorSource::Primary),
            color("color_b", WHITE, ColorSource::Secondary),
        ],
        render: clouds,
    },
    EffectDef {
        id: ids::JULIA_FRACTAL,
        category: Category::Render,
        params: &[float("factor", 1.0, 10.0, 4.0, 0.1), float("zoom", 0.1, 50.0, 1.0, 0.1), angle("angle", -180.0, 180.0, 0.0), int("quality", 1, 5, 2), blend("blend")],
        render: julia,
    },
    EffectDef {
        id: ids::MANDELBROT_FRACTAL,
        category: Category::Render,
        params: &[
            float("factor", 1.0, 10.0, 1.0, 0.1),
            float("zoom", 0.0, 100.0, 10.0, 0.5),
            angle("angle", -180.0, 180.0, 0.0),
            int("quality", 1, 5, 2),
            flag("invert", false),
            blend("blend"),
        ],
        render: mandelbrot,
    },
    EffectDef {
        id: ids::TURBULENCE,
        category: Category::Render,
        params: &[int("octaves", 1, 8, 4), int("period", 1, 200, 40), int("size", 1, 100, 25), choice("noise", NOISE_KINDS, 0), seed("seed"), blend("blend")],
        render: turbulence,
    },
];

fn clouds(job: &Job) -> Option<Raster> {
    let scale = job.params.int("scale") as f32;
    let persistence = (job.params.int("roughness") as f32 / 100.0).max(MIN_CLOUD_PERSISTENCE);
    let octaves = (scale.log2().ceil() as u32 + 1).clamp(1, MAX_CLOUD_OCTAVES);
    let (mode, noise_seed) = (job.params.blend_mode("blend"), job.params.seed("seed"));
    let (first, second) = (job.params.color("color_a"), job.params.color("color_b"));
    map_area(job.area, job.cancel, |x, y| {
        let noise = fractal_noise(noise_seed, x as f32 / scale, y as f32 / scale, octaves, persistence);
        let level = ((noise - 0.5) * CLOUD_CONTRAST + 0.5).clamp(0.0, 1.0);
        blend_pixels(mode, job.source.get(x as u32, y as u32), mix(first, second, level))
    })
}

fn turbulence(job: &Job) -> Option<Raster> {
    let (octaves, period) = (job.params.int("octaves") as u32, job.params.int("period") as f32);
    let gain = job.params.int("size") as f32 / TURBULENCE_SIZE_UNITS;
    let ridged = job.params.choice_key("noise") == "turbulence";
    let (mode, noise_seed) = (job.params.blend_mode("blend"), job.params.seed("seed"));
    map_area(job.area, job.cancel, |x, y| {
        let (mut sum, mut amplitude, mut frequency) = (0.0, 1.0, 1.0 / period);
        for octave in 0..octaves {
            let signed = value_noise(noise_seed.wrapping_add(octave * NOISE_SEED_STEP), x as f32 * frequency, y as f32 * frequency) * 2.0 - 1.0;
            sum += amplitude * if ridged { signed.abs() } else { signed };
            amplitude /= 2.0;
            frequency *= 2.0;
        }
        let level = clamp_channel(if ridged { sum * MID_LEVEL * 2.0 * gain } else { MID_LEVEL + sum * MID_LEVEL * gain });
        blend_pixels(mode, job.source.get(x as u32, y as u32), [level, level, level, 255])
    })
}

fn rainbow(position: f32) -> Rgba {
    let hue = (position * HUE_DEGREES / HUE_PERIOD).rem_euclid(HUE_DEGREES) / 60.0;
    let sector = hue.floor() as i32;
    let rise = clamp_channel((hue - sector as f32) * 255.0);
    let fall = 255 - rise;
    let [r, g, b] = match sector {
        0 => [255, rise, 0],
        1 => [fall, 255, 0],
        2 => [0, 255, rise],
        3 => [0, fall, 255],
        4 => [rise, 0, 255],
        _ => [255, 0, fall],
    };
    [r, g, b, 255]
}

fn escape_time(mut z: (f32, f32), c: (f32, f32), limit: u32) -> Option<f32> {
    for step in 0..limit {
        let magnitude_squared = z.0 * z.0 + z.1 * z.1;
        if magnitude_squared > ESCAPE_RADIUS_SQUARED {
            return Some(step as f32 + 1.0 - magnitude_squared.ln().log2());
        }
        z = (z.0 * z.0 - z.1 * z.1 + c.0, 2.0 * z.0 * z.1 + c.1);
    }
    None
}

struct FractalView {
    center: (f32, f32),
    pixels_per_unit: f32,
    sin: f32,
    cos: f32,
    origin: (f32, f32),
}

impl FractalView {
    fn new(job: &Job, center: (f32, f32), width_units: f32) -> Self {
        let (sin, cos) = job.params.float("angle").to_radians().sin_cos();
        let (cx, cy) = center_of(job.area, (0.0, 0.0));
        Self { center, pixels_per_unit: job.area.width() as f32 / width_units, sin, cos, origin: (cx, cy) }
    }

    fn plane_point(&self, x: f32, y: f32) -> (f32, f32) {
        let (dx, dy) = ((x - self.origin.0) / self.pixels_per_unit, (y - self.origin.1) / self.pixels_per_unit);
        (self.center.0 + dx * self.cos - dy * self.sin, self.center.1 + dx * self.sin + dy * self.cos)
    }
}

fn paint_fractal(job: &Job, view: &FractalView, constant: Option<(f32, f32)>, invert: bool) -> Option<Raster> {
    let quality = job.params.int("quality");
    let limit = FRACTAL_ITERATIONS_BASE + FRACTAL_ITERATIONS_PER_QUALITY * quality as u32;
    let (mode, factor) = (job.params.blend_mode("blend"), job.params.float("factor"));
    let fractal = map_supersampled(job.area, quality.min(MAX_FRACTAL_QUALITY), job.cancel, |x, y| {
        let point = view.plane_point(x, y);
        let time = match constant {
            Some(c) => escape_time(point, c, limit),
            None => escape_time((0.0, 0.0), point, limit),
        };
        let mut color = time.map_or(BLACK, |t| rainbow(t * factor));
        if invert {
            color = [255 - color[0], 255 - color[1], 255 - color[2], 255];
        }
        color
    })?;
    map_area(job.area, job.cancel, |x, y| {
        blend_pixels(mode, job.source.get(x as u32, y as u32), fractal.get((x - job.area.x0) as u32, (y - job.area.y0) as u32))
    })
}

fn julia(job: &Job) -> Option<Raster> {
    let theta = job.params.float("factor") * JULIA_ANGLE_PER_FACTOR;
    let constant = (JULIA_RADIUS * theta.cos(), JULIA_RADIUS * theta.sin());
    let view = FractalView::new(job, (0.0, 0.0), JULIA_WIDTH / job.params.float("zoom"));
    paint_fractal(job, &view, Some(constant), false)
}

fn mandelbrot(job: &Job) -> Option<Raster> {
    let width = MANDELBROT_WIDTH / (1.0 + job.params.float("zoom") / MANDELBROT_ZOOM_UNITS);
    let view = FractalView::new(job, (MANDELBROT_CENTER_X, 0.0), width);
    paint_fractal(job, &view, None, job.params.bool("invert"))
}
