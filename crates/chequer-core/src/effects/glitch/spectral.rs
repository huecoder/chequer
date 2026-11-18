use std::f32::consts::{PI, SQRT_2, TAU};

use super::signed_unit;
use crate::effects::params::{angle, choice, flag, int, seed};
use crate::effects::util::{Cancel, Edge, par_rows, texel};
use crate::effects::{Category, EffectDef, Job, ids};
use crate::geom::{IPoint, IRect};
use crate::raster::{BYTES_PER_PIXEL, Raster};

const REGIONS: &[&str] = &["region_all", "region_ring", "region_wedge"];
const ON_RING: &[&str] = &["region_ring"];
const ON_WEDGE: &[&str] = &["region_wedge"];
const ALL_REGION: usize = 0;
const RING_REGION: usize = 1;
const MAX_SIDE: usize = 4096;
const COLOR_CHANNELS: usize = 3;
const MAX_CHANNEL: f32 = 255.0;
const PERCENT: f32 = 100.0;
const MIN_DEVIATION: f32 = 1.0e-6;
const MIN_MAGNITUDE: f32 = 1.0e-12;
const MAX_SHIFT: i32 = 1024;

pub const DEF: EffectDef = EffectDef {
    id: ids::SPECTRAL_GLITCH,
    category: Category::Glitch,
    params: &[
        choice("region", REGIONS, 0),
        int("band_low", 0, 100, 5).only_if(&[("region", ON_RING)]),
        int("band_high", 0, 100, 60).only_if(&[("region", ON_RING)]),
        angle("wedge_angle", -180.0, 180.0, 0.0).only_if(&[("region", ON_WEDGE)]),
        int("wedge_width", 1, 180, 30).only_if(&[("region", ON_WEDGE)]),
        flag("invert_region", false),
        int("region_gain", -100, 300, 0),
        int("whiten", 0, 100, 0),
        int("phase_noise", 0, 100, 30),
        int("phase_steps", 0, 32, 0),
        int("shift_x", -MAX_SHIFT, MAX_SHIFT, 0),
        int("shift_y", -MAX_SHIFT, MAX_SHIFT, 0),
        flag("normalize", true),
        seed("seed"),
    ],
    render,
};

type Complex = [f32; 2];

struct Spectrum {
    region: usize,
    band: (f32, f32),
    wedge: (f32, f32),
    invert: bool,
    gain: f32,
    whiten: f32,
    phase_noise: f32,
    phase_steps: f32,
    shift: (i32, i32),
    seed: u32,
}

impl Spectrum {
    fn from_job(job: &Job) -> Spectrum {
        let params = job.params;
        Spectrum {
            region: params.choice("region"),
            band: (params.int("band_low") as f32, params.int("band_high") as f32),
            wedge: (params.float("wedge_angle").to_radians(), (params.int("wedge_width") as f32).to_radians()),
            invert: params.bool("invert_region"),
            gain: params.int("region_gain") as f32 / PERCENT,
            whiten: params.int("whiten") as f32 / PERCENT,
            phase_noise: params.int("phase_noise") as f32 / PERCENT,
            phase_steps: params.int("phase_steps") as f32,
            shift: (params.int("shift_x"), params.int("shift_y")),
            seed: params.seed("seed"),
        }
    }

    fn weight(&self, u: i32, v: i32, size: (usize, usize)) -> f32 {
        let inside = match self.region {
            ALL_REGION => true,
            RING_REGION => {
                let radius = (u as f32 / (size.0 as f32 / 2.0)).hypot(v as f32 / (size.1 as f32 / 2.0)) / SQRT_2 * PERCENT;
                radius >= self.band.0 && radius <= self.band.1
            }
            _ => {
                let off_axis = ((v as f32).atan2(u as f32) - self.wedge.0).rem_euclid(PI);
                off_axis.min(PI - off_axis) <= self.wedge.1 / 2.0
            }
        };
        f32::from(inside != self.invert)
    }
}

fn twiddles(size: usize) -> Vec<Complex> {
    (0..size / 2)
        .map(|k| {
            let angle = -std::f64::consts::TAU * k as f64 / size as f64;
            [angle.cos() as f32, angle.sin() as f32]
        })
        .collect()
}

fn fft(data: &mut [Complex], twiddles: &[Complex], inverse: bool) {
    let size = data.len();
    let bits = size.trailing_zeros();
    for index in 0..size {
        let reversed = if bits == 0 { 0 } else { index.reverse_bits() >> (usize::BITS - bits) };
        if reversed > index {
            data.swap(index, reversed);
        }
    }
    let mut length = 2;
    while length <= size {
        let step = size / length;
        for start in (0..size).step_by(length) {
            for k in 0..length / 2 {
                let [wr, wi] = twiddles[k * step];
                let wi = if inverse { -wi } else { wi };
                let [br, bi] = data[start + k + length / 2];
                let (tr, ti) = (br * wr - bi * wi, br * wi + bi * wr);
                let [ar, ai] = data[start + k];
                data[start + k] = [ar + tr, ai + ti];
                data[start + k + length / 2] = [ar - tr, ai - ti];
            }
        }
        length *= 2;
    }
}

fn transposed(data: &[Complex], width: usize, height: usize) -> Vec<Complex> {
    let mut result = vec![[0.0; 2]; data.len()];
    for y in 0..height {
        for x in 0..width {
            result[x * height + y] = data[y * width + x];
        }
    }
    result
}

fn fft_2d(mut data: Vec<Complex>, size: (usize, usize), inverse: bool, cancel: &Cancel) -> Option<Vec<Complex>> {
    let (twiddles_x, twiddles_y) = (twiddles(size.0), twiddles(size.1));
    par_rows(&mut data, size.0, cancel, |_, row| fft(row, &twiddles_x, inverse)).then_some(())?;
    let mut columns = transposed(&data, size.0, size.1);
    par_rows(&mut columns, size.1, cancel, |_, column| fft(column, &twiddles_y, inverse)).then_some(())?;
    Some(transposed(&columns, size.1, size.0))
}

fn frequency(index: usize, size: usize) -> i32 {
    if index <= size / 2 { index as i32 } else { index as i32 - size as i32 }
}

fn shifted(data: &[Complex], size: (usize, usize), shift: (i32, i32)) -> Vec<Complex> {
    let mut result = vec![[0.0; 2]; data.len()];
    for y in 0..size.1 {
        let target_y = (y as i32 + shift.1).rem_euclid(size.1 as i32) as usize;
        for x in 0..size.0 {
            let target_x = (x as i32 + shift.0).rem_euclid(size.0 as i32) as usize;
            result[target_y * size.0 + target_x] = data[y * size.0 + x];
        }
    }
    result
}

fn reshape(data: &mut [Complex], size: (usize, usize), settings: &Spectrum) {
    let mean_magnitude = {
        let total: f32 = data.iter().map(|&[re, im]| re.hypot(im)).sum();
        total / data.len() as f32
    };
    for (index, value) in data.iter_mut().enumerate() {
        let (u, v) = (frequency(index % size.0, size.0), frequency(index / size.0, size.1));
        if (u, v) == (0, 0) || settings.weight(u, v, size) == 0.0 {
            continue;
        }
        let mut magnitude = value[0].hypot(value[1]).max(MIN_MAGNITUDE);
        let mut phase = value[1].atan2(value[0]);
        magnitude *= (1.0 + settings.gain).max(0.0);
        if settings.whiten > 0.0 {
            magnitude = magnitude.powf(1.0 - settings.whiten) * mean_magnitude.powf(settings.whiten);
        }
        phase += signed_unit(settings.seed, u, v) * PI * settings.phase_noise;
        if settings.phase_steps > 0.0 {
            let step = TAU / settings.phase_steps;
            phase = (phase / step).round() * step;
        }
        *value = [magnitude * phase.cos(), magnitude * phase.sin()];
    }
}

fn mean_and_deviation(values: impl Iterator<Item = f32> + Clone) -> (f32, f32) {
    let count = values.clone().count() as f32;
    let mean = values.clone().sum::<f32>() / count;
    let variance = values.map(|value| (value - mean).powi(2)).sum::<f32>() / count;
    (mean, variance.sqrt())
}

fn process_tile(tile: &Raster, settings: &Spectrum, normalize: bool, cancel: &Cancel) -> Option<Raster> {
    let (width, height) = (tile.width() as usize, tile.height() as usize);
    let size = (width.next_power_of_two(), height.next_power_of_two());
    let mut data = tile.as_bytes().to_vec();
    for channel in 0..COLOR_CHANNELS {
        let padded: Vec<Complex> = (0..size.0 * size.1)
            .map(|index| {
                let pixel = texel(tile, (index % size.0) as i32, (index / size.0) as i32, Edge::Reflect).unwrap_or_default();
                [f32::from(pixel[channel]), 0.0]
            })
            .collect();
        let mut spectrum = fft_2d(padded, size, false, cancel)?;
        if settings.shift != (0, 0) {
            spectrum = shifted(&spectrum, size, settings.shift);
        }
        reshape(&mut spectrum, size, settings);
        let restored = fft_2d(spectrum, size, true, cancel)?;
        let scale = 1.0 / (size.0 * size.1) as f32;
        let inside = (0..width * height).map(|index| restored[(index / width) * size.0 + index % width][0] * scale);
        let original = (0..width * height).map(|index| f32::from(tile.as_bytes()[index * BYTES_PER_PIXEL + channel]));
        let (new_mean, new_deviation) = mean_and_deviation(inside.clone());
        let (old_mean, old_deviation) = mean_and_deviation(original);
        let gain = if normalize && new_deviation > MIN_DEVIATION { old_deviation / new_deviation } else { 1.0 };
        let offset = if normalize { old_mean - new_mean * gain } else { 0.0 };
        for (index, value) in inside.enumerate() {
            data[index * BYTES_PER_PIXEL + channel] = (value * gain + offset).round().clamp(0.0, MAX_CHANNEL) as u8;
        }
    }
    Raster::from_rgba(tile.width(), tile.height(), data).ok()
}

fn render(job: &Job) -> Option<Raster> {
    let region = job.source.crop(job.area);
    let settings = Spectrum::from_job(job);
    let normalize = job.params.bool("normalize");
    let (width, height) = (region.width() as usize, region.height() as usize);
    let mut result = region.clone();
    for top in (0..height).step_by(MAX_SIDE) {
        for left in (0..width).step_by(MAX_SIDE) {
            let bounds = IRect::new(left as i32, top as i32, (left + MAX_SIDE).min(width) as i32, (top + MAX_SIDE).min(height) as i32);
            let processed = process_tile(&region.crop(bounds), &settings, normalize, job.cancel)?;
            result.blit(&processed, IPoint::new(bounds.x0, bounds.y0));
        }
    }
    Some(result)
}
