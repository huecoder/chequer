use super::params::{ColorSource, choice, color, curve, float, int};
use super::util::{clamp_channel, intensity, map_area};
use super::{Category, EffectDef, Job, ids};
use crate::raster::{Raster, Rgba};

const CHANNEL_MAX: f32 = 255.0;
const LUT_SIZE: usize = 256;
const PERCENT: f32 = 100.0;
const SEPIA_TONE: Rgba = [255, 189, 110, 255];
const CHANNEL_OPTIONS: &[&str] = &["channel_all", "channel_red", "channel_green", "channel_blue"];
const POSTERIZE_OPTIONS: &[&str] = &["linked", "separate"];
const ONLY_SEPARATE: &[(&str, &[&str])] = &[("channels", &["separate"])];
const ONLY_ALL: &[(&str, &[&str])] = &[("channel", &["channel_all"])];
const ONLY_RED: &[(&str, &[&str])] = &[("channel", &["channel_red"])];
const ONLY_GREEN: &[(&str, &[&str])] = &[("channel", &["channel_green"])];
const ONLY_BLUE: &[(&str, &[&str])] = &[("channel", &["channel_blue"])];
const AUTO_LEVEL_CLIP: f32 = 0.005;
const CONTRAST_SCALE: f32 = 259.0;
const FULL_DEGREES: f32 = 360.0;
const SECTOR_DEGREES: f32 = 60.0;

type Lut = [u8; LUT_SIZE];

pub static DEFS: [EffectDef; 7] = [
    EffectDef { id: ids::AUTO_LEVEL, category: Category::Adjust, params: &[], render: auto_level },
    EffectDef { id: ids::BRIGHTNESS_CONTRAST, category: Category::Adjust, params: &[int("brightness", -100, 100, 0), int("contrast", -100, 100, 0)], render: brightness_contrast },
    EffectDef {
        id: ids::HUE_SATURATION,
        category: Category::Adjust,
        params: &[int("hue", -180, 180, 0), int("saturation", 0, 200, 100), int("lightness", -100, 100, 0)],
        render: hue_saturation,
    },
    EffectDef {
        id: ids::LEVELS,
        category: Category::Adjust,
        params: &[
            choice("channel", CHANNEL_OPTIONS, 0),
            int("input_low", 0, 254, 0),
            int("input_high", 1, 255, 255),
            float("midtones", 0.1, 10.0, 1.0, 0.01),
            int("output_low", 0, 255, 0),
            int("output_high", 0, 255, 255),
        ],
        render: levels,
    },
    EffectDef {
        id: ids::CURVES,
        category: Category::Adjust,
        params: &[
            choice("channel", CHANNEL_OPTIONS, 0),
            curve("curve_all").only_if(ONLY_ALL),
            curve("curve_red").only_if(ONLY_RED),
            curve("curve_green").only_if(ONLY_GREEN),
            curve("curve_blue").only_if(ONLY_BLUE),
        ],
        render: curves,
    },
    EffectDef {
        id: ids::POSTERIZE,
        category: Category::Adjust,
        params: &[
            choice("channels", POSTERIZE_OPTIONS, 0),
            int("red_levels", 2, 64, 4),
            int("green_levels", 2, 64, 4).only_if(ONLY_SEPARATE),
            int("blue_levels", 2, 64, 4).only_if(ONLY_SEPARATE),
        ],
        render: posterize,
    },
    EffectDef { id: ids::SEPIA, category: Category::Adjust, params: &[color("tone", SEPIA_TONE, ColorSource::Fixed), int("strength", 0, 100, 100)], render: sepia },
];

fn apply_luts(job: &Job, luts: &[Lut; 3]) -> Option<Raster> {
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        [luts[0][pixel[0] as usize], luts[1][pixel[1] as usize], luts[2][pixel[2] as usize], pixel[3]]
    })
}

fn identity_lut() -> Lut {
    std::array::from_fn(|value| value as u8)
}

fn selected_channels(channel: usize) -> [bool; 3] {
    std::array::from_fn(|index| channel == 0 || channel == index + 1)
}

fn brightness_contrast(job: &Job) -> Option<Raster> {
    let shift = job.params.int("brightness") as f32 * CHANNEL_MAX / PERCENT;
    let contrast = job.params.int("contrast") as f32 * CHANNEL_MAX / PERCENT;
    let factor = CONTRAST_SCALE * (contrast + CHANNEL_MAX) / (CHANNEL_MAX * (CONTRAST_SCALE - contrast));
    let half = CHANNEL_MAX / 2.0;
    let lut: Lut = std::array::from_fn(|value| clamp_channel(factor * (value as f32 - half) + half + shift));
    apply_luts(job, &[lut; 3])
}

fn levels(job: &Job) -> Option<Raster> {
    let params = job.params;
    let (input_low, input_high) = (params.int("input_low") as f32, params.int("input_high") as f32);
    let (output_low, output_high) = (params.int("output_low") as f32, params.int("output_high") as f32);
    let inverse_gamma = 1.0 / params.float("midtones");
    let stretch: Lut = std::array::from_fn(|value| {
        let unit = ((value as f32 - input_low) / (input_high - input_low).max(1.0)).clamp(0.0, 1.0);
        clamp_channel(output_low + (output_high - output_low) * unit.powf(inverse_gamma))
    });
    let selected = selected_channels(params.choice("channel"));
    apply_luts(job, &std::array::from_fn(|index| if selected[index] { stretch } else { identity_lut() }))
}

fn curves(job: &Job) -> Option<Raster> {
    let params = job.params;
    let master = params.curve("curve_all").lut();
    let luts = ["curve_red", "curve_green", "curve_blue"].map(|key| {
        let channel = params.curve(key).lut();
        std::array::from_fn(|value| channel[master[value] as usize])
    });
    apply_luts(job, &luts)
}

fn posterize(job: &Job) -> Option<Raster> {
    let params = job.params;
    let separate = params.choice("channels") == 1;
    let red = params.int("red_levels");
    let counts = [red, if separate { params.int("green_levels") } else { red }, if separate { params.int("blue_levels") } else { red }];
    let luts = counts.map(|count| {
        let steps = (count - 1) as f32;
        std::array::from_fn(|value| clamp_channel((value as f32 / CHANNEL_MAX * steps).round() / steps * CHANNEL_MAX))
    });
    apply_luts(job, &luts)
}

fn auto_level(job: &Job) -> Option<Raster> {
    let mut histograms = [[0u32; LUT_SIZE]; 3];
    let mut counted = 0u32;
    for y in job.area.y0..job.area.y1 {
        for pixel in job.source.row_bytes(y as u32, job.area.x0 as u32, job.area.x1 as u32).chunks_exact(4).filter(|pixel| pixel[3] > 0) {
            counted += 1;
            for (histogram, &value) in histograms.iter_mut().zip(pixel) {
                histogram[value as usize] += 1;
            }
        }
    }
    let clip = (counted as f32 * AUTO_LEVEL_CLIP) as u32;
    let luts = histograms.map(|histogram| {
        let from_start = cumulative_position(histogram.iter(), clip);
        let from_end = LUT_SIZE - 1 - cumulative_position(histogram.iter().rev(), clip);
        if from_end <= from_start {
            return identity_lut();
        }
        let span = (from_end - from_start) as f32;
        std::array::from_fn(|value| clamp_channel((value as f32 - from_start as f32) / span * CHANNEL_MAX))
    });
    apply_luts(job, &luts)
}

fn cumulative_position<'a>(histogram: impl Iterator<Item = &'a u32>, clip: u32) -> usize {
    let mut total = 0;
    for (position, &count) in histogram.enumerate() {
        total += count;
        if total > clip {
            return position;
        }
    }
    0
}

fn sepia(job: &Job) -> Option<Raster> {
    let tone = job.params.color("tone");
    let strength = job.params.int("strength") as f32 / PERCENT;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let luma = f32::from(intensity(pixel)) / CHANNEL_MAX;
        let toned = |channel: usize| f32::from(pixel[channel]) + (f32::from(tone[channel]) * luma - f32::from(pixel[channel])) * strength;
        [clamp_channel(toned(0)), clamp_channel(toned(1)), clamp_channel(toned(2)), pixel[3]]
    })
}

fn hue_saturation(job: &Job) -> Option<Raster> {
    let hue_shift = job.params.int("hue") as f32;
    let saturation = job.params.int("saturation") as f32 / PERCENT;
    let lightness = job.params.int("lightness") as f32 / PERCENT;
    map_area(job.area, job.cancel, |x, y| {
        let pixel = job.source.get(x as u32, y as u32);
        let (hue, chroma, light) = to_hsl(pixel);
        let light = if lightness >= 0.0 { light + (1.0 - light) * lightness } else { light * (1.0 + lightness) };
        let [r, g, b] = from_hsl((hue + hue_shift).rem_euclid(FULL_DEGREES), (chroma * saturation).clamp(0.0, 1.0), light);
        [r, g, b, pixel[3]]
    })
}

fn to_hsl(pixel: Rgba) -> (f32, f32, f32) {
    let [r, g, b] = [0, 1, 2].map(|i| f32::from(pixel[i]) / CHANNEL_MAX);
    let (high, low) = (r.max(g).max(b), r.min(g).min(b));
    let light = (high + low) / 2.0;
    let delta = high - low;
    if delta == 0.0 {
        return (0.0, 0.0, light);
    }
    let saturation = delta / (1.0 - (2.0 * light - 1.0).abs());
    let sector = if high == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if high == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    (sector * SECTOR_DEGREES, saturation, light)
}

fn from_hsl(hue: f32, saturation: f32, light: f32) -> [u8; 3] {
    let chroma = (1.0 - (2.0 * light - 1.0).abs()) * saturation;
    let sector = hue / SECTOR_DEGREES;
    let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let offset = light - chroma / 2.0;
    [r, g, b].map(|channel| clamp_channel((channel + offset) * CHANNEL_MAX))
}
