use std::f32::consts::TAU;
use std::ops::Range;

use crate::effects::curve::Curve;
use crate::effects::params::{angle, center, choice, curve, flag, int, seed};
use crate::effects::util::{Rng, center_of, hash, par_groups, value_noise};
use crate::effects::{Category, EffectDef, Job, ids};
use crate::raster::{BYTES_PER_PIXEL, Raster, Rgba};

const SORT_KEYS: &[&str] = &["key_luma", "key_hue", "key_saturation", "key_value", "key_red", "key_green", "key_blue", "key_alpha"];
const PATHS: &[&str] = &["straight", "rings"];
const INTERVALS: &[&str] = &["threshold", "by_edges", "random", "fixed", "waves"];
const ORDERS: &[&str] = &["ascending", "descending"];
const ON_RINGS: &[&str] = &["rings"];
const ON_MASKED: &[&str] = &["threshold", "by_edges"];
const ON_THRESHOLD: &[&str] = &["threshold"];
const ON_EDGES: &[&str] = &["by_edges"];
const ON_SEEDED: &[&str] = &["random", "waves"];
const MAX_CHANNEL: f32 = 255.0;
const MAX_INTERVAL_LENGTH: i32 = 10000;
const WAVE_ROW_PERIOD: f32 = 24.0;
const WAVE_LENGTH_PERIODS: f32 = 2.0;
const INTERVAL_SALT: i32 = 0x7f4a;
const PERCENT: f32 = 100.0;
const DESCENDING: usize = 1;
const RINGS: usize = 1;
const SECTORS: f32 = 6.0;

pub const DEF: EffectDef = EffectDef {
    id: ids::PIXEL_SORT,
    category: Category::Glitch,
    params: &[
        angle("angle", -180.0, 180.0, 0.0),
        choice("path", PATHS, 0),
        center("center").only_if(&[("path", ON_RINGS)]),
        choice("sort_by", SORT_KEYS, 0),
        curve("sort_curve"),
        choice("order", ORDERS, 0),
        choice("intervals", INTERVALS, 0),
        choice("mask_by", SORT_KEYS, 0).only_if(&[("intervals", ON_MASKED)]),
        int("range_low", 0, 255, 60).only_if(&[("intervals", ON_THRESHOLD)]),
        int("range_high", 0, 255, 255).only_if(&[("intervals", ON_THRESHOLD)]),
        flag("invert_mask", false).only_if(&[("intervals", ON_THRESHOLD)]),
        int("edge_threshold", 1, 255, 40).only_if(&[("intervals", ON_EDGES)]),
        int("min_length", 2, MAX_INTERVAL_LENGTH, 2),
        int("max_length", 2, MAX_INTERVAL_LENGTH, 300),
        int("mix", 0, 100, 100),
        seed("seed").only_if(&[("intervals", ON_SEEDED)]),
    ],
    render,
};

#[derive(Clone, Copy)]
enum SortKey {
    Luma,
    Hue,
    Saturation,
    Value,
    Red,
    Green,
    Blue,
    Alpha,
}

const KEY_KINDS: [SortKey; 8] = [SortKey::Luma, SortKey::Hue, SortKey::Saturation, SortKey::Value, SortKey::Red, SortKey::Green, SortKey::Blue, SortKey::Alpha];

impl SortKey {
    fn of(self, color: Rgba) -> f32 {
        let [red, green, blue, alpha] = color.map(|channel| f32::from(channel) / MAX_CHANNEL);
        let (high, low) = (red.max(green).max(blue), red.min(green).min(blue));
        match self {
            SortKey::Luma => 0.299 * red + 0.587 * green + 0.114 * blue,
            SortKey::Hue => hue(red, green, blue, high - low),
            SortKey::Saturation if high > 0.0 => (high - low) / high,
            SortKey::Saturation => 0.0,
            SortKey::Value => high,
            SortKey::Red => red,
            SortKey::Green => green,
            SortKey::Blue => blue,
            SortKey::Alpha => alpha,
        }
    }
}

fn hue(red: f32, green: f32, blue: f32, spread: f32) -> f32 {
    if spread <= 0.0 {
        return 0.0;
    }
    let high = red.max(green).max(blue);
    let sector = if high == red {
        ((green - blue) / spread).rem_euclid(SECTORS)
    } else if high == green {
        (blue - red) / spread + 2.0
    } else {
        (red - green) / spread + 4.0
    };
    sector / SECTORS
}

#[derive(Clone, Copy)]
enum Path {
    Lines { cos: f32, sin: f32 },
    Rings { center: (f32, f32), turn: f32 },
}

impl Path {
    fn from_job(job: &Job) -> Path {
        let radians = job.params.float("angle").to_radians();
        if job.params.choice("path") == RINGS {
            let (x, y) = center_of(job.area, job.params.point("center"));
            return Path::Rings { center: (x - job.area.x0 as f32, y - job.area.y0 as f32), turn: radians };
        }
        Path::Lines { cos: radians.cos(), sin: radians.sin() }
    }

    fn locate(self, index: usize, width: usize) -> (i32, f32) {
        let (x, y) = ((index % width) as f32, (index / width) as f32);
        match self {
            Path::Lines { cos, sin } => ((x * sin + y * cos).round() as i32, x * cos - y * sin),
            Path::Rings { center, turn } => {
                let (dx, dy) = (x - center.0, y - center.1);
                (dx.hypot(dy).round() as i32, (-dy.atan2(dx) - turn).rem_euclid(TAU))
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Intervals {
    Threshold,
    Edges,
    Random,
    Fixed,
    Waves,
}

const INTERVAL_KINDS: [Intervals; 5] = [Intervals::Threshold, Intervals::Edges, Intervals::Random, Intervals::Fixed, Intervals::Waves];

struct Settings {
    sort_key: SortKey,
    lut: Option<[u8; 256]>,
    descending: bool,
    intervals: Intervals,
    mask_key: SortKey,
    low: f32,
    high: f32,
    invert_mask: bool,
    edge_threshold: f32,
    min_length: usize,
    max_length: usize,
    seed: u32,
    mix: f32,
}

impl Settings {
    fn from_job(job: &Job) -> Settings {
        let params = job.params;
        let sort_curve = params.curve("sort_curve");
        let min_length = params.int("min_length") as usize;
        Settings {
            sort_key: KEY_KINDS[params.choice("sort_by")],
            lut: (sort_curve != Curve::identity()).then(|| sort_curve.lut()),
            descending: params.choice("order") == DESCENDING,
            intervals: INTERVAL_KINDS[params.choice("intervals")],
            mask_key: KEY_KINDS[params.choice("mask_by")],
            low: params.int("range_low") as f32,
            high: params.int("range_high") as f32,
            invert_mask: params.bool("invert_mask"),
            edge_threshold: params.int("edge_threshold") as f32,
            min_length,
            max_length: (params.int("max_length") as usize).max(min_length),
            seed: params.seed("seed"),
            mix: params.int("mix") as f32 / PERCENT,
        }
    }

    fn sort_value(&self, color: Rgba) -> f32 {
        let raw = self.sort_key.of(color);
        match &self.lut {
            Some(lut) => f32::from(lut[(raw * MAX_CHANNEL).round() as usize]),
            None => raw,
        }
    }

    fn masked(&self, color: Rgba) -> bool {
        let level = self.mask_key.of(color) * MAX_CHANNEL;
        (level >= self.low && level <= self.high) != self.invert_mask
    }

    fn intervals(&self, line: &[Rgba], group: usize) -> Vec<Range<usize>> {
        let length = line.len();
        let spread = (self.max_length - self.min_length) as f32;
        let pieces = match self.intervals {
            Intervals::Threshold => runs_where(length, |index| self.masked(line[index])),
            Intervals::Edges => edge_runs(line, self.mask_key, self.edge_threshold),
            Intervals::Fixed => cut(length, |_| self.max_length),
            Intervals::Random => {
                let mut rng = Rng::new(hash(self.seed, group as i32, INTERVAL_SALT));
                cut(length, |_| self.min_length + (rng.next_unit() * (spread + 1.0)) as usize)
            }
            Intervals::Waves => cut(length, |cursor| {
                let along = cursor as f32 / (self.max_length as f32 * WAVE_LENGTH_PERIODS);
                self.min_length + (value_noise(self.seed, along, group as f32 / WAVE_ROW_PERIOD) * spread) as usize
            }),
        };
        pieces
            .into_iter()
            .flat_map(|piece| split_long(piece, self.max_length))
            .filter(|piece| piece.len() >= self.min_length)
            .collect()
    }
}

fn runs_where(length: usize, wanted: impl Fn(usize) -> bool) -> Vec<Range<usize>> {
    let mut runs = Vec::new();
    let mut start = None;
    for index in 0..length {
        match (wanted(index), start) {
            (true, None) => start = Some(index),
            (false, Some(begin)) => {
                runs.push(begin..index);
                start = None;
            }
            _ => {}
        }
    }
    runs.extend(start.map(|begin| begin..length));
    runs
}

fn edge_runs(line: &[Rgba], key: SortKey, threshold: f32) -> Vec<Range<usize>> {
    let levels: Vec<f32> = line.iter().map(|&color| key.of(color) * MAX_CHANNEL).collect();
    let mut runs = Vec::new();
    let mut start = 0;
    for index in 1..levels.len() {
        if (levels[index] - levels[index - 1]).abs() > threshold {
            runs.push(start..index);
            start = index;
        }
    }
    runs.push(start..levels.len());
    runs
}

fn cut(length: usize, mut next_length: impl FnMut(usize) -> usize) -> Vec<Range<usize>> {
    let mut pieces = Vec::new();
    let mut cursor = 0;
    while cursor < length {
        let end = (cursor + next_length(cursor).max(1)).min(length);
        pieces.push(cursor..end);
        cursor = end;
    }
    pieces
}

fn split_long(piece: Range<usize>, max_length: usize) -> impl Iterator<Item = Range<usize>> {
    let end = piece.end;
    piece.step_by(max_length).map(move |start| start..(start + max_length).min(end))
}

fn sort_line(line: &mut [Rgba], group: usize, settings: &Settings) {
    for range in settings.intervals(line, group) {
        let mut keyed: Vec<(f32, Rgba)> = line[range.clone()].iter().map(|&color| (settings.sort_value(color), color)).collect();
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
        if settings.descending {
            keyed.reverse();
        }
        for (slot, (_, color)) in line[range].iter_mut().zip(keyed) {
            *slot = color;
        }
    }
}

fn group_by_bucket(buckets: &[i32]) -> (Vec<u32>, Vec<usize>) {
    let lowest = buckets.iter().copied().min().unwrap_or(0);
    let highest = buckets.iter().copied().max().unwrap_or(0);
    let mut starts = vec![0usize; (highest - lowest + 1) as usize + 1];
    for &bucket in buckets {
        starts[(bucket - lowest) as usize + 1] += 1;
    }
    for group in 1..starts.len() {
        starts[group] += starts[group - 1];
    }
    let mut cursors = starts.clone();
    let mut order = vec![0u32; buckets.len()];
    for (index, &bucket) in buckets.iter().enumerate() {
        let cursor = &mut cursors[(bucket - lowest) as usize];
        order[*cursor] = index as u32;
        *cursor += 1;
    }
    (order, starts)
}

fn mix_pixel(original: Rgba, sorted: Rgba, mix: f32) -> Rgba {
    std::array::from_fn(|channel| (f32::from(original[channel]) + (f32::from(sorted[channel]) - f32::from(original[channel])) * mix).round() as u8)
}

fn render(job: &Job) -> Option<Raster> {
    let region = job.source.crop(job.area);
    let width = region.width() as usize;
    let pixels: Vec<Rgba> = region.as_bytes().chunks_exact(BYTES_PER_PIXEL).map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]]).collect();
    let (settings, path) = (Settings::from_job(job), Path::from_job(job));
    let buckets: Vec<i32> = (0..pixels.len()).map(|index| path.locate(index, width).0).collect();
    let (mut order, starts) = group_by_bucket(&buckets);
    let ordered = par_groups(&mut order, &starts, job.cancel, |_, members| {
        let mut keyed: Vec<(f32, u32)> = members.iter().map(|&index| (path.locate(index as usize, width).1, index)).collect();
        keyed.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        for (slot, (_, index)) in members.iter_mut().zip(keyed) {
            *slot = index;
        }
    });
    if !ordered {
        return None;
    }
    let mut lined: Vec<Rgba> = order.iter().map(|&index| pixels[index as usize]).collect();
    if !par_groups(&mut lined, &starts, job.cancel, |group, line| sort_line(line, group, &settings)) {
        return None;
    }
    let mut result = pixels.clone();
    for (&index, sorted) in order.iter().zip(lined) {
        let target = &mut result[index as usize];
        *target = mix_pixel(pixels[index as usize], sorted, settings.mix);
    }
    Raster::from_rgba(region.width(), region.height(), result.into_iter().flatten().collect()).ok()
}
