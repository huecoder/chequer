
use super::params::{angle, center, choice, float, int};
use super::util::{Edge, Plane, bicubic, bilinear, center_of, clamp_channel, gaussian_blur, intensity, map_area, mix, read_region, texel};
use super::{Category, EffectDef, Job, ids};
use crate::blend::{BlendMode, blend};
use crate::geom::IRect;
use crate::raster::{Raster, Rgba};

const SAMPLING: &[&str] = &["bicubic", "nearest", "bilinear"];
const MID_LEVEL: f32 = 127.5;
const LEVELS_PER_PERCENT: f32 = 2.55;
const SHARPEN_GAIN: f32 = 1.5;
const SOFTEN_BLEND: f32 = 0.6;
const WARMTH_SHIFT_PER_UNIT: f32 = 0.8;
const CHANNEL_MAX: f32 = 255.0;
const PERCENT: f32 = 100.0;
const WHITE_BALANCE_RANGE: f32 = 0.3;

pub static DEFS: [EffectDef; 10] = [
    EffectDef {
        id: ids::SHADOWS_HIGHLIGHTS,
        category: Category::Photo,
        params: &[int("shadows", 0, 100, 40), int("highlights", 0, 100, 30), int("radius", 1, 100, 20)],
        render: shadows_highlights,
    },
    EffectDef { id: ids::WHITE_BALANCE, category: Category::Photo, params: &[int("temperature", -100, 100, 0), int("tint", -100, 100, 0)], render: white_balance },
    EffectDef { id: ids::VIBRANCE, category: Category::Photo, params: &[int("vibrance", -100, 100, 40)], render: vibrance },
    EffectDef { id: ids::CLARITY, category: Category::Photo, params: &[int("amount", 0, 100, 40), int("radius", 1, 100, 20)], render: clarity },
    EffectDef {
        id: ids::GLOW,
        category: Category::Photo,
        params: &[int("radius", 1, 20, 6), int("brightness", -100, 100, 10), int("contrast", -100, 100, 10)],
        render: glow,
    },
    EffectDef { id: ids::RED_EYE_REMOVAL, category: Category::Photo, params: &[int("tolerance", 0, 100, 70), int("saturation", 0, 100, 90)], render: red_eye },
    EffectDef { id: ids::SHARPEN, category: Category::Photo, params: &[int("amount", 1, 20, 2), int("threshold", 0, 100, 0)], render: sharpen },
    EffectDef {
        id: ids::SOFTEN_PORTRAIT,
        category: Category::Photo,
        params: &[int("softness", 0, 10, 5), int("lighting", -20, 20, 0), int("warmth", 0, 20, 10)],
        render: soften_portrait,
    },
    EffectDef { id: ids::STRAIGHTEN, category: Category::Photo, params: &[angle("angle", -45.0, 45.0, 0.0), choice("sampling", SAMPLING, 0)], render: straighten },
    EffectDef { id: ids::VIGNETTE, category: Category::Photo, params: &[center("center"), float("radius", 0.1, 4.0, 0.5, 0.05), float("strength", 0.0, 1.0, 1.0, 0.05)], render: vignette },
];

fn blurred_region(job: &Job, sigma: f32) -> Option<(Raster, IRect)> {
    let (region, inner) = read_region(job.source, job.area, (sigma * 3.0).ceil() as i32);
    let blurred = gaussian_blur(Plane::from_raster(job.source, region, 1.0), sigma, job.cancel)?;
    Some((blurred.to_raster(inner, 1.0), inner))
}

fn brightness_contrast(pixel: Rgba, brightness: f32, contrast: f32) -> Rgba {
    let factor = (100.0 + contrast) / 100.0;
    let adjust = |channel: u8| clamp_channel((f32::from(channel) - MID_LEVEL) * factor + MID_LEVEL + brightness * LEVELS_PER_PERCENT);
    [adjust(pixel[0]), adjust(pixel[1]), adjust(pixel[2]), pixel[3]]
}

fn glow(job: &Job) -> Option<Raster> {
    let (brightness, contrast) = (job.params.int("brightness") as f32, job.params.int("contrast") as f32);
    let (blurred, _) = blurred_region(job, job.params.int("radius") as f32)?;
    map_area(job.area, job.cancel, |x, y| {
        let glow = brightness_contrast(blurred.get((x - job.area.x0) as u32, (y - job.area.y0) as u32), brightness, contrast);
        blend(BlendMode::Screen, job.source.get(x as u32, y as u32), glow)
    })
}

fn red_eye(job: &Job) -> Option<Raster> {
    let dominance = 1.0 + (100 - job.params.int("tolerance")) as f32 / 50.0;
    let amount = job.params.int("saturation") as f32 / 100.0;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let other = pixel[1].max(pixel[2]);
        if f32::from(pixel[0]) <= f32::from(other) * dominance {
            return pixel;
        }
        [clamp_channel(f32::from(pixel[0]) + (f32::from(other) - f32::from(pixel[0])) * amount), pixel[1], pixel[2], pixel[3]]
    })
}

fn sharpen(job: &Job) -> Option<Raster> {
    let sigma = 0.5 + job.params.int("amount") as f32 * 0.25;
    let threshold = job.params.int("threshold") as f32 * LEVELS_PER_PERCENT;
    let (blurred, _) = blurred_region(job, sigma)?;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let soft = blurred.get((x - job.area.x0) as u32, (y - job.area.y0) as u32);
        let sharp = |c: usize| {
            let difference = f32::from(pixel[c]) - f32::from(soft[c]);
            if difference.abs() < threshold { pixel[c] } else { clamp_channel(f32::from(pixel[c]) + difference * SHARPEN_GAIN) }
        };
        [sharp(0), sharp(1), sharp(2), pixel[3]]
    })
}

fn soften_portrait(job: &Job) -> Option<Raster> {
    let softness = job.params.int("softness");
    let lighting = job.params.int("lighting") as f32;
    let warmth = job.params.int("warmth") as f32 * WARMTH_SHIFT_PER_UNIT;
    let (blurred, _) = blurred_region(job, softness as f32 * 0.8 + 0.1)?;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let soft = brightness_contrast(blurred.get((x - job.area.x0) as u32, (y - job.area.y0) as u32), lighting, 0.0);
        let warm = [clamp_channel(f32::from(soft[0]) + warmth), soft[1], clamp_channel(f32::from(soft[2]) - warmth), soft[3]];
        mix(pixel, warm, SOFTEN_BLEND * (softness as f32 / 10.0).max(0.1))
    })
}

fn straighten(job: &Job) -> Option<Raster> {
    let theta = job.params.float("angle").to_radians();
    let (width, height) = (job.source.width() as f32, job.source.height() as f32);
    let fill = theta.cos().abs() + theta.sin().abs() * (width / height).max(height / width);
    let (sin, cos) = theta.sin_cos();
    let (cx, cy) = (width / 2.0 - 0.5, height / 2.0 - 0.5);
    let sampling = job.params.choice("sampling");
    map_area(job.area, job.cancel, |x, y| {
        let (dx, dy) = ((x as f32 - cx) / fill, (y as f32 - cy) / fill);
        let (sx, sy) = (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos);
        match sampling {
            0 => bicubic(job.source, sx, sy, Edge::Clamp),
            1 => texel(job.source, sx.round() as i32, sy.round() as i32, Edge::Clamp).unwrap_or_default(),
            _ => bilinear(job.source, sx, sy, Edge::Clamp),
        }
    })
}

fn vignette(job: &Job) -> Option<Raster> {
    let (cx, cy) = center_of(job.area, job.params.point("center"));
    let (width, height) = (job.area.width() as f32, job.area.height() as f32);
    let reach = job.params.float("radius") * (width * width + height * height).sqrt();
    let strength = job.params.float("strength");
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let t = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / reach;
        let light = 1.0 / (1.0 + t * t).powi(2);
        let keep = 1.0 - strength * (1.0 - light);
        [clamp_channel(f32::from(pixel[0]) * keep), clamp_channel(f32::from(pixel[1]) * keep), clamp_channel(f32::from(pixel[2]) * keep), pixel[3]]
    })
}

fn local_luma(blurred: &Raster, job: &Job, x: i32, y: i32) -> f32 {
    f32::from(intensity(blurred.get((x - job.area.x0) as u32, (y - job.area.y0) as u32))) / CHANNEL_MAX
}

fn shadows_highlights(job: &Job) -> Option<Raster> {
    let shadows = job.params.int("shadows") as f32 / PERCENT;
    let highlights = job.params.int("highlights") as f32 / PERCENT;
    let (blurred, _) = blurred_region(job, job.params.int("radius") as f32)?;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let local = local_luma(&blurred, job, x, y);
        let lift = shadows * (1.0 - local).powi(2);
        let pull = highlights * local.powi(2);
        let adjust = |channel: u8| {
            let value = f32::from(channel);
            clamp_channel(value + (CHANNEL_MAX - value) * lift - value * pull)
        };
        [adjust(pixel[0]), adjust(pixel[1]), adjust(pixel[2]), pixel[3]]
    })
}

fn white_balance(job: &Job) -> Option<Raster> {
    let temperature = job.params.int("temperature") as f32 / PERCENT * WHITE_BALANCE_RANGE;
    let tint = job.params.int("tint") as f32 / PERCENT * WHITE_BALANCE_RANGE;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let scale = |channel: u8, gain: f32| clamp_channel(f32::from(channel) * gain);
        [scale(pixel[0], 1.0 + temperature), scale(pixel[1], 1.0 - tint), scale(pixel[2], 1.0 - temperature), pixel[3]]
    })
}

fn vibrance(job: &Job) -> Option<Raster> {
    let amount = job.params.int("vibrance") as f32 / PERCENT;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let (brightest, dimmest) = (pixel[..3].iter().max().copied().unwrap_or(0), pixel[..3].iter().min().copied().unwrap_or(0));
        let saturation = if brightest == 0 { 0.0 } else { f32::from(brightest - dimmest) / f32::from(brightest) };
        let gain = amount * (1.0 - saturation);
        let gray = pixel[..3].iter().map(|&channel| f32::from(channel)).sum::<f32>() / 3.0;
        let push = |channel: u8| clamp_channel(f32::from(channel) + (f32::from(channel) - gray) * gain);
        [push(pixel[0]), push(pixel[1]), push(pixel[2]), pixel[3]]
    })
}

fn clarity(job: &Job) -> Option<Raster> {
    let amount = job.params.int("amount") as f32 / PERCENT;
    let (blurred, _) = blurred_region(job, job.params.int("radius") as f32)?;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let luma = f32::from(intensity(pixel)) / CHANNEL_MAX;
        let midtone_weight = 1.0 - (2.0 * luma - 1.0).powi(2);
        let boost = (luma - local_luma(&blurred, job, x, y)) * CHANNEL_MAX * amount * midtone_weight;
        let lift = |channel: u8| clamp_channel(f32::from(channel) + boost);
        [lift(pixel[0]), lift(pixel[1]), lift(pixel[2]), pixel[3]]
    })
}
