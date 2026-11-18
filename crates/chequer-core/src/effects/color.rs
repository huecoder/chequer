use super::dither::{self, Dither};
use super::palette::{Method, sample_colors};
use super::params::{ColorSource, Shown, choice, color, int};
use super::util::{clamp_channel, intensity, map_area};
use super::{Category, EffectDef, Job, ids};
use crate::raster::{Raster, Rgba};

const DITHER_LEVELS: f32 = 8.0;
const OPAQUE: u8 = u8::MAX;
const WHITE: [u8; 4] = [u8::MAX; 4];
const BLACK: Rgba = [0, 0, 0, OPAQUE];
const CHANNEL_MAX: f32 = 255.0;
const DEFAULT_THRESHOLD: i32 = 128;

const METHOD_OPTIONS: &[&str] = &["median_cut", "kmeans", "uniform_grid"];
const DITHER_OPTIONS: &[&str] = &["floyd_steinberg", "atkinson", "jarvis", "sierra_lite", "bayer_4", "bayer_8", "none"];
const DITHER_ACTIVE: &[Shown] = &[("dither_method", &["floyd_steinberg", "atkinson", "jarvis", "sierra_lite", "bayer_4", "bayer_8"])];
const ALPHA_BELOW_OPTIONS: &[&str] = &["transparent", "white"];

pub static DEFS: [EffectDef; 4] = [EffectDef {
    id: ids::QUANTIZE,
    category: Category::Color,
    params: &[
        choice("method", METHOD_OPTIONS, 0),
        int("colors", 2, 256, 256),
        choice("dither_method", DITHER_OPTIONS, 0),
        int("dithering", 1, 8, 7).only_if(DITHER_ACTIVE),
        int("alpha_threshold", 0, 255, 0),
        choice("alpha_below", ALPHA_BELOW_OPTIONS, 0),
    ],
    render: quantize,
},
EffectDef { id: ids::SOLARIZE, category: Category::Color, params: &[int("threshold", 0, 255, DEFAULT_THRESHOLD)], render: solarize },
EffectDef { id: ids::THRESHOLD, category: Category::Color, params: &[int("threshold", 0, 255, DEFAULT_THRESHOLD)], render: threshold },
EffectDef {
    id: ids::DUOTONE,
    category: Category::Color,
    params: &[color("color_a", BLACK, ColorSource::Primary), color("color_b", WHITE, ColorSource::Secondary)],
    render: duotone,
}];

fn solarize(job: &Job) -> Option<Raster> {
    let threshold = job.params.int("threshold") as u8;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let flip = |channel: u8| if channel >= threshold { u8::MAX - channel } else { channel };
        [flip(pixel[0]), flip(pixel[1]), flip(pixel[2]), pixel[3]]
    })
}

fn threshold(job: &Job) -> Option<Raster> {
    let threshold = job.params.int("threshold") as u8;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let level = if intensity(pixel) >= threshold { u8::MAX } else { 0 };
        [level, level, level, pixel[3]]
    })
}

fn duotone(job: &Job) -> Option<Raster> {
    let (shadow, light) = (job.params.color("color_a"), job.params.color("color_b"));
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let mix = f32::from(intensity(pixel)) / CHANNEL_MAX;
        let tone = |channel: usize| clamp_channel(f32::from(shadow[channel]) + (f32::from(light[channel]) - f32::from(shadow[channel])) * mix);
        [tone(0), tone(1), tone(2), pixel[3]]
    })
}

fn clamp_alpha(work: &mut Raster, threshold: u8, fill_white: bool) {
    if threshold == 0 {
        return;
    }
    for y in 0..work.height() {
        for x in 0..work.width() {
            let pixel = work.get(x, y);
            let clamped = match (pixel[3] >= threshold, fill_white) {
                (true, _) => [pixel[0], pixel[1], pixel[2], OPAQUE],
                (false, true) => WHITE,
                (false, false) => [pixel[0], pixel[1], pixel[2], 0],
            };
            work.set(x, y, clamped);
        }
    }
}

fn quantize(job: &Job) -> Option<Raster> {
    let mut work = job.source.crop(job.area);
    clamp_alpha(&mut work, job.params.int("alpha_threshold") as u8, job.params.choice_key("alpha_below") == "white");
    let method = Method::from_key(job.params.choice_key("method"));
    let palette = method.build(sample_colors(&work), job.params.int("colors") as usize);
    if palette.is_empty() {
        return Some(work);
    }
    let dither = Dither::from_key(job.params.choice_key("dither_method"));
    let amount = match dither {
        Dither::None => 0.0,
        _ => job.params.int("dithering") as f32 / DITHER_LEVELS,
    };
    dither::apply(&work, &palette, dither, amount, job.cancel)
}
