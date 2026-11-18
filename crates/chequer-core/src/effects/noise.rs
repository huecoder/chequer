
use super::params::{float, int, seed};
use super::util::{clamp_channel, gaussian_noise, hash_unit, map_area, mix, percentile_filter};
use super::{Category, EffectDef, Job, ids};
use crate::raster::Raster;

const LUMA_SIGMA_PER_INTENSITY: f32 = 0.6;
const COVERAGE_SALT: i32 = 7_919;
const MAX_SATURATION_PERCENT: f32 = 100.0;

pub static DEFS: [EffectDef; 2] = [
    EffectDef {
        id: ids::ADD_NOISE,
        category: Category::Noise,
        params: &[int("intensity", 0, 100, 64), int("saturation", 0, 400, 100), float("coverage", 0.0, 100.0, 100.0, 1.0), seed("seed")],
        render: add_noise,
    },
    EffectDef { id: ids::REDUCE_NOISE, category: Category::Noise, params: &[int("radius", 0, 200, 10), float("strength", 0.0, 1.0, 0.4, 0.05)], render: reduce_noise },
];

fn add_noise(job: &Job) -> Option<Raster> {
    let sigma = job.params.int("intensity") as f32 * LUMA_SIGMA_PER_INTENSITY;
    let chroma_sigma = sigma * job.params.int("saturation") as f32 / MAX_SATURATION_PERCENT;
    let coverage = job.params.float("coverage") / 100.0;
    let seed = job.params.seed("seed");
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        if hash_unit(seed, x ^ COVERAGE_SALT, y) >= coverage {
            return pixel;
        }
        let luma = gaussian_noise(seed, x, y) * sigma;
        let (chroma_i, chroma_q) = (gaussian_noise(seed.wrapping_add(1), x, y) * chroma_sigma, gaussian_noise(seed.wrapping_add(2), x, y) * chroma_sigma);
        let delta = [luma + 0.956 * chroma_i + 0.621 * chroma_q, luma - 0.272 * chroma_i - 0.647 * chroma_q, luma - 1.106 * chroma_i + 1.703 * chroma_q];
        [clamp_channel(f32::from(pixel[0]) + delta[0]), clamp_channel(f32::from(pixel[1]) + delta[1]), clamp_channel(f32::from(pixel[2]) + delta[2]), pixel[3]]
    })
}

fn reduce_noise(job: &Job) -> Option<Raster> {
    let radius = job.params.int("radius");
    let strength = job.params.float("strength");
    if radius == 0 {
        return Some(job.source.crop(job.area));
    }
    let smoothed = percentile_filter(job.source, job.area, radius.min(REDUCE_NOISE_MAX_RADIUS), 50.0, job.cancel)?;
    map_area(job.area, job.cancel, |x, y| {
        let original = job.source.get(x as u32, y as u32);
        mix(original, smoothed.get((x - job.area.x0) as u32, (y - job.area.y0) as u32), strength)
    })
}

const REDUCE_NOISE_MAX_RADIUS: i32 = 30;
