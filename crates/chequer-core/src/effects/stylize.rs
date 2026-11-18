
use super::params::{ColorSource, angle, choice, color, flag, int};
use super::util::{Edge, Plane, clamp_channel, convolve3x3, gaussian_blur, intensity, map_area, read_region, texel, window_extreme};
use super::{Category, EffectDef, Job, ids};
use crate::blend::{BlendMode, blend};
use crate::raster::{Raster, Rgba};

const ALGORITHMS: &[&str] = &["sobel", "prewitt"];
const GRAY_BASE: f32 = 128.0;
const EDGE_STRENGTH_UNITS: f32 = 25.0;
const OUTLINE_INTENSITY_UNITS: f32 = 25.0;
const MAX_LEVEL: f32 = 255.0;

const INK: Rgba = [0, 0, 0, 255];
const PAPER: Rgba = [255, 255, 255, 255];
const PERCENT: f32 = 100.0;
const COVERAGE_EDGE: f32 = 0.5;

pub static DEFS: [EffectDef; 6] = [
    EffectDef {
        id: ids::HALFTONE,
        category: Category::Stylize,
        params: &[int("cell_size", 3, 100, 8), angle("angle", -180.0, 180.0, 45.0), color("color_a", INK, ColorSource::Primary), color("color_b", PAPER, ColorSource::Secondary)],
        render: halftone,
    },
    EffectDef { id: ids::SCANLINES, category: Category::Stylize, params: &[int("period", 2, 50, 4), int("strength", 0, 100, 40)], render: scanlines },
    EffectDef {
        id: ids::EDGE_DETECT,
        category: Category::Stylize,
        params: &[int("strength", 1, 100, 50), int("blurring", 0, 10, 0), choice("algorithm", ALGORITHMS, 0), flag("overlay", false)],
        render: edge_detect,
    },
    EffectDef { id: ids::EMBOSS, category: Category::Stylize, params: &[angle("angle", -180.0, 180.0, 0.0)], render: emboss },
    EffectDef { id: ids::OUTLINE, category: Category::Stylize, params: &[int("thickness", 1, 200, 3), int("intensity", 0, 100, 50)], render: outline },
    EffectDef { id: ids::RELIEF, category: Category::Stylize, params: &[angle("angle", -180.0, 180.0, 45.0)], render: relief },
];

fn directional_kernel(degrees: f32) -> [[f32; 3]; 3] {
    let r = degrees.to_radians();
    let step = std::f32::consts::FRAC_PI_4;
    [
        [(r + step).cos(), (r + 2.0 * step).cos(), (r + 3.0 * step).cos()],
        [r.cos(), 0.0, (r + 4.0 * step).cos()],
        [(r - step).cos(), (r - 2.0 * step).cos(), (r - 3.0 * step).cos()],
    ]
}

fn emboss(job: &Job) -> Option<Raster> {
    let kernel = directional_kernel(job.params.float("angle"));
    map_area(job.area, job.cancel, |x, y| {
        let mut sum = 0.0;
        for (j, row) in kernel.iter().enumerate() {
            for (i, weight) in row.iter().enumerate() {
                let pixel = texel(job.source, x + i as i32 - 1, y + j as i32 - 1, Edge::Clamp).unwrap_or_default();
                sum += weight * f32::from(intensity(pixel));
            }
        }
        let level = clamp_channel(sum + GRAY_BASE);
        [level, level, level, job.source.get(x as u32, y as u32)[3]]
    })
}

fn relief(job: &Job) -> Option<Raster> {
    let mut kernel = directional_kernel(job.params.float("angle"));
    kernel[1][1] = 1.0;
    convolve3x3(job.source, job.area, &kernel, 0.0, job.cancel)
}

fn edge_detect(job: &Job) -> Option<Raster> {
    let strength = job.params.int("strength") as f32 / EDGE_STRENGTH_UNITS;
    let blurring = job.params.int("blurring") as f32;
    let prewitt = job.params.choice_key("algorithm") == "prewitt";
    let overlay = job.params.bool("overlay");
    let middle = if prewitt { 1.0 } else { 2.0 };
    let (region, _) = read_region(job.source, job.area, 1 + (blurring * 3.0).ceil() as i32);
    let smoothed;
    let base: &Raster = if blurring > 0.0 {
        let plane = gaussian_blur(Plane::from_raster(job.source, region, 1.0), blurring, job.cancel)?;
        smoothed = plane.to_raster(plane_bounds(&plane), 1.0);
        &smoothed
    } else {
        job.source
    };
    let origin = if blurring > 0.0 { (region.x0, region.y0) } else { (0, 0) };
    let level = |x: i32, y: i32| f32::from(intensity(texel(base, x - origin.0, y - origin.1, Edge::Clamp).unwrap_or_default()));
    map_area(job.area, job.cancel, |x, y| {
        let gx = (level(x + 1, y - 1) + middle * level(x + 1, y) + level(x + 1, y + 1)) - (level(x - 1, y - 1) + middle * level(x - 1, y) + level(x - 1, y + 1));
        let gy = (level(x - 1, y + 1) + middle * level(x, y + 1) + level(x + 1, y + 1)) - (level(x - 1, y - 1) + middle * level(x, y - 1) + level(x + 1, y - 1));
        let edge = clamp_channel((gx * gx + gy * gy).sqrt() * strength);
        let source = job.source.get(x as u32, y as u32);
        if overlay { blend(BlendMode::Screen, source, [edge, edge, edge, source[3]]) } else { [edge, edge, edge, source[3]] }
    })
}

fn plane_bounds(plane: &Plane) -> crate::geom::IRect {
    crate::geom::IRect::new(0, 0, plane.width as i32, plane.height as i32)
}

fn outline(job: &Job) -> Option<Raster> {
    let half = job.params.int("thickness") / 2 + 1;
    let gain = job.params.int("intensity") as f32 / OUTLINE_INTENSITY_UNITS;
    let highest = window_extreme(job.source, job.area, (half, half), true, job.cancel)?;
    let lowest = window_extreme(job.source, job.area, (half, half), false, job.cancel)?;
    map_area(job.area, job.cancel, |x, y| {
        let (column, row) = ((x - job.area.x0) as u32, (y - job.area.y0) as u32);
        let spread = f32::from(intensity(highest.get(column, row))) - f32::from(intensity(lowest.get(column, row)));
        let keep = 1.0 - (spread / MAX_LEVEL * gain).clamp(0.0, 1.0);
        let pixel = job.source.get(x as u32, y as u32);
        [clamp_channel(f32::from(pixel[0]) * keep), clamp_channel(f32::from(pixel[1]) * keep), clamp_channel(f32::from(pixel[2]) * keep), pixel[3]]
    })
}

fn halftone(job: &Job) -> Option<Raster> {
    let cell = job.params.int("cell_size") as f32;
    let (sin, cos) = job.params.float("angle").to_radians().sin_cos();
    let (ink, paper) = (job.params.color("color_a"), job.params.color("color_b"));
    let max_radius = cell * std::f32::consts::FRAC_1_SQRT_2;
    map_area(job.area, job.cancel, |x, y| {
        let (u, v) = (x as f32 * cos + y as f32 * sin, y as f32 * cos - x as f32 * sin);
        let (center_u, center_v) = ((u / cell).floor() * cell + cell / 2.0, (v / cell).floor() * cell + cell / 2.0);
        let (source_x, source_y) = (center_u * cos - center_v * sin, center_v * cos + center_u * sin);
        let sample = texel(job.source, source_x.round() as i32, source_y.round() as i32, Edge::Clamp).unwrap_or_default();
        let darkness = 1.0 - f32::from(intensity(sample)) / MAX_LEVEL;
        let distance = ((u - center_u).powi(2) + (v - center_v).powi(2)).sqrt();
        let coverage = (max_radius * darkness.sqrt() - distance + COVERAGE_EDGE).clamp(0.0, 1.0);
        let tone = |channel: usize| clamp_channel(f32::from(paper[channel]) + (f32::from(ink[channel]) - f32::from(paper[channel])) * coverage);
        [tone(0), tone(1), tone(2), job.source.get(x as u32, y as u32)[3]]
    })
}

fn scanlines(job: &Job) -> Option<Raster> {
    let period = job.params.int("period");
    let keep = 1.0 - job.params.int("strength") as f32 / PERCENT;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let factor = if y % period == 0 { keep } else { 1.0 };
        [clamp_channel(f32::from(pixel[0]) * factor), clamp_channel(f32::from(pixel[1]) * factor), clamp_channel(f32::from(pixel[2]) * factor), pixel[3]]
    })
}
