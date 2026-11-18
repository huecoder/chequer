
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::geom::IRect;
use crate::raster::{BYTES_PER_PIXEL, Raster, Rgba, TRANSPARENT};

const BAND_ROWS: usize = 8;
const CHANNELS: usize = 4;
const MAX_CHANNEL: f32 = 255.0;
const BOX_SIGMA_THRESHOLD: f32 = 10.0;
const GAUSS_REACH_SIGMAS: f32 = 3.0;
const BOX_PASSES: usize = 3;

#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

fn thread_count() -> usize {
    std::thread::available_parallelism().map_or(1, NonZero::get)
}

fn drain<T: Send>(queue: Mutex<impl Iterator<Item = T> + Send>, cancel: &Cancel, work: impl Fn(T) + Sync) -> bool {
    std::thread::scope(|scope| {
        for _ in 0..thread_count() {
            scope.spawn(|| {
                while !cancel.is_set() {
                    let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                    let Some(item) = next else { break };
                    work(item);
                }
            });
        }
    });
    !cancel.is_set()
}

pub fn par_rows<T: Send>(data: &mut [T], row_len: usize, cancel: &Cancel, f: impl Fn(usize, &mut [T]) + Sync) -> bool {
    if row_len == 0 {
        return !cancel.is_set();
    }
    let bands = Mutex::new(data.chunks_mut(row_len * BAND_ROWS).enumerate());
    drain(bands, cancel, |(band, chunk)| {
        for (offset, row) in chunk.chunks_mut(row_len).enumerate() {
            f(band * BAND_ROWS + offset, row);
        }
    })
}

pub fn par_groups<T: Send>(data: &mut [T], starts: &[usize], cancel: &Cancel, f: impl Fn(usize, &mut [T]) + Sync) -> bool {
    let mut groups = Vec::with_capacity(starts.len().saturating_sub(1));
    let mut rest = data;
    for (index, bounds) in starts.windows(2).enumerate() {
        let (group, tail) = rest.split_at_mut(bounds[1] - bounds[0]);
        groups.push((index, group));
        rest = tail;
    }
    drain(Mutex::new(groups.into_iter()), cancel, |(index, group)| f(index, group))
}

pub fn render_rows(width: u32, height: u32, cancel: &Cancel, f: impl Fn(u32, &mut [u8]) + Sync) -> Option<Raster> {
    let row_len = width as usize * BYTES_PER_PIXEL;
    let mut data = vec![0; row_len * height as usize];
    if !par_rows(&mut data, row_len, cancel, |y, row| f(y as u32, row)) {
        return None;
    }
    Raster::from_rgba(width, height, data).ok()
}

pub fn map_area(area: IRect, cancel: &Cancel, f: impl Fn(i32, i32) -> Rgba + Sync) -> Option<Raster> {
    render_rows(area.width() as u32, area.height() as u32, cancel, |row_index, row| {
        let y = area.y0 + row_index as i32;
        for (column, pixel) in row.chunks_exact_mut(BYTES_PER_PIXEL).enumerate() {
            pixel.copy_from_slice(&f(area.x0 + column as i32, y));
        }
    })
}

pub fn intensity(color: Rgba) -> u8 {
    ((7471 * u32::from(color[2]) + 38470 * u32::from(color[1]) + 19595 * u32::from(color[0])) >> 16) as u8
}

pub fn clamp_channel(value: f32) -> u8 {
    value.round().clamp(0.0, MAX_CHANNEL) as u8
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Clamp,
    Wrap,
    Reflect,
    Transparent,
}

impl Edge {
    pub fn from_key(key: &str) -> Edge {
        match key {
            "clamp" => Edge::Clamp,
            "wrap" => Edge::Wrap,
            "mirror" => Edge::Reflect,
            "transparent" => Edge::Transparent,
            other => panic!("неизвестный вариант края «{other}»"),
        }
    }
}

fn fold(value: i32, len: i32, edge: Edge) -> Option<i32> {
    if (0..len).contains(&value) {
        return Some(value);
    }
    match edge {
        Edge::Clamp => Some(value.clamp(0, len - 1)),
        Edge::Wrap => Some(value.rem_euclid(len)),
        Edge::Reflect => {
            let period = 2 * len;
            let folded = value.rem_euclid(period);
            Some(if folded < len { folded } else { period - 1 - folded })
        }
        Edge::Transparent => None,
    }
}

pub fn texel(src: &Raster, x: i32, y: i32, edge: Edge) -> Option<Rgba> {
    let column = fold(x, src.width() as i32, edge)?;
    let row = fold(y, src.height() as i32, edge)?;
    Some(src.get(column as u32, row as u32))
}

#[derive(Default)]
struct Accumulator {
    rgb: [f32; 3],
    alpha: f32,
}

impl Accumulator {
    fn add(&mut self, pixel: Rgba, weight: f32) {
        let alpha = f32::from(pixel[3]) * weight;
        for (sum, &channel) in self.rgb.iter_mut().zip(&pixel) {
            *sum += f32::from(channel) * alpha;
        }
        self.alpha += alpha;
    }

    fn finish(self) -> Rgba {
        self.finish_averaged(1.0)
    }

    fn finish_averaged(self, weights_total: f32) -> Rgba {
        if self.alpha <= 0.0 {
            return TRANSPARENT;
        }
        let [r, g, b] = self.rgb.map(|sum| clamp_channel(sum / self.alpha));
        [r, g, b, clamp_channel(self.alpha / weights_total)]
    }
}

pub fn bilinear(src: &Raster, x: f32, y: f32, edge: Edge) -> Rgba {
    let (floor_x, floor_y) = (x.floor(), y.floor());
    let (frac_x, frac_y) = (x - floor_x, y - floor_y);
    let (column, row) = (floor_x as i32, floor_y as i32);
    let taps = [
        (0, 0, (1.0 - frac_x) * (1.0 - frac_y)),
        (1, 0, frac_x * (1.0 - frac_y)),
        (0, 1, (1.0 - frac_x) * frac_y),
        (1, 1, frac_x * frac_y),
    ];
    let mut acc = Accumulator::default();
    for (dx, dy, weight) in taps {
        if weight > 0.0 {
            if let Some(pixel) = texel(src, column + dx, row + dy, edge) {
                acc.add(pixel, weight);
            }
        }
    }
    acc.finish()
}

fn cubic_weights(t: f32) -> [f32; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [-0.5 * t3 + t2 - 0.5 * t, 1.5 * t3 - 2.5 * t2 + 1.0, -1.5 * t3 + 2.0 * t2 + 0.5 * t, 0.5 * t3 - 0.5 * t2]
}

pub fn bicubic(src: &Raster, x: f32, y: f32, edge: Edge) -> Rgba {
    let (floor_x, floor_y) = (x.floor(), y.floor());
    let (weights_x, weights_y) = (cubic_weights(x - floor_x), cubic_weights(y - floor_y));
    let (column, row) = (floor_x as i32, floor_y as i32);
    let mut acc = Accumulator::default();
    for (j, weight_y) in weights_y.iter().enumerate() {
        for (i, weight_x) in weights_x.iter().enumerate() {
            if let Some(pixel) = texel(src, column + i as i32 - 1, row + j as i32 - 1, edge) {
                acc.add(pixel, weight_x * weight_y);
            }
        }
    }
    acc.finish()
}

pub fn hash(seed: u32, a: i32, b: i32) -> u32 {
    let mut h = seed.wrapping_mul(0x9E37_79B1) ^ (a as u32).wrapping_mul(0x85EB_CA77) ^ (b as u32).wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

pub fn hash_unit(seed: u32, a: i32, b: i32) -> f32 {
    (hash(seed, a, b) >> 8) as f32 / (1u32 << 24) as f32
}

pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(hash(seed, 1, 2) | 1)
    }

    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    pub fn next_unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
}

pub struct Plane {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Plane {
    pub fn from_raster(src: &Raster, region: IRect, exponent: f32) -> Plane {
        let (width, height) = (region.width() as usize, region.height() as usize);
        let encode = power_table(exponent);
        let mut data = Vec::with_capacity(width * height * CHANNELS);
        for y in region.y0..region.y1 {
            let row = src.row_bytes(y as u32, region.x0 as u32, region.x1 as u32);
            for pixel in row.chunks_exact(BYTES_PER_PIXEL) {
                let alpha = f32::from(pixel[3]) / MAX_CHANNEL;
                data.extend_from_slice(&[
                    encode[pixel[0] as usize] * alpha,
                    encode[pixel[1] as usize] * alpha,
                    encode[pixel[2] as usize] * alpha,
                    alpha,
                ]);
            }
        }
        Plane { width, height, data }
    }

    pub fn to_raster(&self, inner: IRect, exponent: f32) -> Raster {
        let decode = 1.0 / exponent;
        let mut out = Raster::transparent(inner.width() as u32, inner.height() as u32);
        for y in inner.y0..inner.y1 {
            let start = (y as usize * self.width + inner.x0 as usize) * CHANNELS;
            let source = &self.data[start..start + inner.width() as usize * CHANNELS];
            let target = out.row_bytes_mut((y - inner.y0) as u32, 0, inner.width() as u32);
            for (pixel, texel) in target.chunks_exact_mut(BYTES_PER_PIXEL).zip(source.chunks_exact(CHANNELS)) {
                let alpha = texel[3];
                if alpha <= f32::EPSILON {
                    continue;
                }
                for channel in 0..3 {
                    let linear = (texel[channel] / alpha).clamp(0.0, 1.0);
                    let value = if exponent == 1.0 { linear } else { linear.powf(decode) };
                    pixel[channel] = clamp_channel(value * MAX_CHANNEL);
                }
                pixel[3] = clamp_channel(alpha * MAX_CHANNEL);
            }
        }
        out
    }

    fn transposed(&self) -> Plane {
        let mut data = vec![0.0; self.data.len()];
        for y in 0..self.height {
            for x in 0..self.width {
                let from = (y * self.width + x) * CHANNELS;
                let to = (x * self.height + y) * CHANNELS;
                data[to..to + CHANNELS].copy_from_slice(&self.data[from..from + CHANNELS]);
            }
        }
        Plane { width: self.height, height: self.width, data }
    }
}

fn power_table(exponent: f32) -> [f32; 256] {
    std::array::from_fn(|value| {
        let unit = value as f32 / MAX_CHANNEL;
        if exponent == 1.0 { unit } else { unit.powf(exponent) }
    })
}

fn pass_rows(src: &Plane, pad: usize, cancel: &Cancel, op: &(impl Fn(&[f32], &mut [f32]) + Sync)) -> Option<Plane> {
    let row_len = src.width * CHANNELS;
    let mut out = vec![0.0; src.data.len()];
    let done = par_rows(&mut out, row_len, cancel, |y, target| {
        let row = &src.data[y * row_len..(y + 1) * row_len];
        let mut padded = Vec::with_capacity(row_len + 2 * pad * CHANNELS);
        for _ in 0..pad {
            padded.extend_from_slice(&row[..CHANNELS]);
        }
        padded.extend_from_slice(row);
        for _ in 0..pad {
            padded.extend_from_slice(&row[row_len - CHANNELS..]);
        }
        op(&padded, target);
    });
    done.then_some(Plane { width: src.width, height: src.height, data: out })
}

fn blur_both_axes(plane: Plane, pad: usize, cancel: &Cancel, op: &(impl Fn(&[f32], &mut [f32]) + Sync)) -> Option<Plane> {
    let horizontal = pass_rows(&plane, pad, cancel, op)?;
    let vertical = pass_rows(&horizontal.transposed(), pad, cancel, op)?;
    Some(vertical.transposed())
}

pub fn gaussian_kernel(sigma: f32, reach: usize) -> Vec<f32> {
    let weights: Vec<f32> = (0..=2 * reach)
        .map(|i| {
            let distance = i as f32 - reach as f32;
            (-(distance * distance) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let total: f32 = weights.iter().sum();
    weights.into_iter().map(|w| w / total).collect()
}

fn box_radii(sigma: f32) -> [usize; BOX_PASSES] {
    let passes = BOX_PASSES as f32;
    let ideal_width = (12.0 * sigma * sigma / passes + 1.0).sqrt();
    let mut lower = ideal_width.floor() as i32;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let lower_f = lower as f32;
    let lower_passes =
        ((12.0 * sigma * sigma - passes * lower_f * lower_f - 4.0 * passes * lower_f - 3.0 * passes) / (-4.0 * lower_f - 4.0)).round() as i32;
    std::array::from_fn(|i| {
        let width = if (i as i32) < lower_passes { lower } else { lower + 2 };
        ((width - 1) / 2) as usize
    })
}

fn box_row(radius: usize) -> impl Fn(&[f32], &mut [f32]) + Sync {
    move |padded, target| {
        let width = target.len() / CHANNELS;
        let window = 2 * radius + 1;
        let mut sum = [0.0f32; CHANNELS];
        for i in 0..window {
            for c in 0..CHANNELS {
                sum[c] += padded[i * CHANNELS + c];
            }
        }
        let scale = 1.0 / window as f32;
        for x in 0..width {
            for c in 0..CHANNELS {
                target[x * CHANNELS + c] = sum[c] * scale;
            }
            if x + 1 < width {
                for c in 0..CHANNELS {
                    sum[c] += padded[(x + window) * CHANNELS + c] - padded[x * CHANNELS + c];
                }
            }
        }
    }
}

fn kernel_row(kernel: Vec<f32>) -> impl Fn(&[f32], &mut [f32]) + Sync {
    move |padded, target| {
        let width = target.len() / CHANNELS;
        for x in 0..width {
            let mut sum = [0.0f32; CHANNELS];
            for (i, weight) in kernel.iter().enumerate() {
                let at = (x + i) * CHANNELS;
                for c in 0..CHANNELS {
                    sum[c] += weight * padded[at + c];
                }
            }
            target[x * CHANNELS..(x + 1) * CHANNELS].copy_from_slice(&sum);
        }
    }
}

pub fn gaussian_blur(plane: Plane, sigma: f32, cancel: &Cancel) -> Option<Plane> {
    if sigma < 0.3 {
        return Some(plane);
    }
    if sigma > BOX_SIGMA_THRESHOLD {
        let mut current = plane;
        for radius in box_radii(sigma) {
            current = blur_both_axes(current, radius, cancel, &box_row(radius))?;
        }
        return Some(current);
    }
    let reach = (sigma * GAUSS_REACH_SIGMAS).ceil() as usize;
    blur_both_axes(plane, reach, cancel, &kernel_row(gaussian_kernel(sigma, reach)))
}

pub fn box_blur(plane: Plane, radius: usize, cancel: &Cancel) -> Option<Plane> {
    if radius == 0 {
        return Some(plane);
    }
    blur_both_axes(plane, radius, cancel, &box_row(radius))
}

pub fn read_region(src: &Raster, area: IRect, reach: i32) -> (IRect, IRect) {
    let region = area.expand(reach).intersect(src.bounds());
    (region, area.translate(-region.x0, -region.y0))
}

pub fn mean(pixels: impl IntoIterator<Item = Rgba>) -> Rgba {
    let mut acc = Accumulator::default();
    let mut count = 0.0;
    for pixel in pixels {
        acc.add(pixel, 1.0);
        count += 1.0;
    }
    if count == 0.0 { TRANSPARENT } else { acc.finish_averaged(count) }
}

pub fn subsamples(quality: i32) -> Vec<(f32, f32)> {
    let side = quality.max(1);
    let step = 1.0 / side as f32;
    (0..side).flat_map(|j| (0..side).map(move |i| ((i as f32 + 0.5) * step - 0.5, (j as f32 + 0.5) * step - 0.5))).collect()
}

pub fn center_of(area: IRect, offset: (f32, f32)) -> (f32, f32) {
    let half = (area.width() as f32 / 2.0, area.height() as f32 / 2.0);
    (area.x0 as f32 + half.0 * (1.0 + offset.0), area.y0 as f32 + half.1 * (1.0 + offset.1))
}

pub fn map_supersampled(area: IRect, quality: i32, cancel: &Cancel, f: impl Fn(f32, f32) -> Rgba + Sync) -> Option<Raster> {
    let offsets = subsamples(quality);
    map_area(area, cancel, |x, y| {
        if offsets.len() == 1 {
            return f(x as f32, y as f32);
        }
        mean(offsets.iter().map(|&(dx, dy)| f(x as f32 + dx, y as f32 + dy)))
    })
}

pub fn value_noise(seed: u32, x: f32, y: f32) -> f32 {
    let (cell_x, cell_y) = (x.floor(), y.floor());
    let (fx, fy) = (x - cell_x, y - cell_y);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth(fx), smooth(fy));
    let (ix, iy) = (cell_x as i32, cell_y as i32);
    let corner = |dx: i32, dy: i32| hash_unit(seed, ix + dx, iy + dy);
    let top = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * sx;
    let bottom = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * sx;
    top + (bottom - top) * sy
}

pub fn fractal_noise(seed: u32, x: f32, y: f32, octaves: u32, persistence: f32) -> f32 {
    let (mut sum, mut amplitude, mut total, mut frequency) = (0.0, 1.0, 0.0, 1.0);
    for octave in 0..octaves {
        sum += amplitude * value_noise(seed.wrapping_add(octave), x * frequency, y * frequency);
        total += amplitude;
        amplitude *= persistence;
        frequency *= 2.0;
    }
    sum / total
}

pub fn gaussian_noise(seed: u32, a: i32, b: i32) -> f32 {
    let u1 = (hash_unit(seed, a, b) + f32::EPSILON).min(1.0);
    let u2 = hash_unit(seed ^ 0x5bd1_e995, a, b);
    (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

pub fn convolve3x3(src: &Raster, area: IRect, kernel: &[[f32; 3]; 3], bias: f32, cancel: &Cancel) -> Option<Raster> {
    map_area(area, cancel, |x, y| {
        let mut sum = [0.0f32; 3];
        for (j, row) in kernel.iter().enumerate() {
            for (i, &weight) in row.iter().enumerate() {
                let pixel = texel(src, x + i as i32 - 1, y + j as i32 - 1, Edge::Clamp).unwrap_or(TRANSPARENT);
                for channel in 0..3 {
                    sum[channel] += weight * f32::from(pixel[channel]);
                }
            }
        }
        let centre = src.get(x as u32, y as u32);
        [clamp_channel(sum[0] + bias), clamp_channel(sum[1] + bias), clamp_channel(sum[2] + bias), centre[3]]
    })
}

pub fn gray(color: Rgba) -> Rgba {
    let level = intensity(color);
    [level, level, level, color[3]]
}

pub fn mix(a: Rgba, b: Rgba, amount: f32) -> Rgba {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |i: usize| clamp_channel(f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * amount);
    [channel(0), channel(1), channel(2), channel(3)]
}

pub fn window_extreme(src: &Raster, area: IRect, half: (i32, i32), maximum: bool, cancel: &Cancel) -> Option<Raster> {
    let reach = (half.0, half.1);
    let (region, _) = read_region(src, area, reach.0.max(reach.1));
    let pick = |a: u8, b: u8| if maximum { a.max(b) } else { a.min(b) };
    let horizontal = map_area(region, cancel, |x, y| {
        let mut out = src.get(x as u32, y as u32);
        for dx in -reach.0..=reach.0 {
            let other = texel(src, x + dx, y, Edge::Clamp).unwrap_or(out);
            for c in 0..4 {
                out[c] = pick(out[c], other[c]);
            }
        }
        out
    })?;
    map_area(area, cancel, |x, y| {
        let row = y - region.y0;
        let mut out = horizontal.get((x - region.x0) as u32, row as u32);
        for dy in -reach.1..=reach.1 {
            let other = texel(&horizontal, x - region.x0, row + dy, Edge::Clamp).unwrap_or(out);
            for c in 0..4 {
                out[c] = pick(out[c], other[c]);
            }
        }
        out
    })
}

pub fn percentile_filter(src: &Raster, area: IRect, radius: i32, percentile: f32, cancel: &Cancel) -> Option<Raster> {
    const LEVELS: usize = 256;
    let (region, _) = read_region(src, area, radius);
    let width = area.width() as usize;
    let side = (2 * radius + 1) as f32;
    let target = ((side * side) * percentile.clamp(0.0, 100.0) / 100.0).floor() as u32;
    render_rows(area.width() as u32, area.height() as u32, cancel, |row_index, out| {
        let y = area.y0 + row_index as i32;
        let clamp_x = |x: i32| x.clamp(region.x0, region.x1 - 1);
        let clamp_y = |y: i32| y.clamp(region.y0, region.y1 - 1);
        let mut histogram = [[0u32; LEVELS]; CHANNELS];
        let add = |x: i32, sign: i32, histogram: &mut [[u32; LEVELS]; CHANNELS]| {
            for dy in -radius..=radius {
                let pixel = src.get(clamp_x(x) as u32, clamp_y(y + dy) as u32);
                for (c, bins) in histogram.iter_mut().enumerate() {
                    bins[pixel[c] as usize] = (bins[pixel[c] as usize] as i32 + sign) as u32;
                }
            }
        };
        for x in area.x0 - radius..=area.x0 + radius {
            add(x, 1, &mut histogram);
        }
        for column in 0..width {
            let mut pixel = [0u8; CHANNELS];
            for (c, bins) in histogram.iter().enumerate() {
                let mut seen = 0;
                for (level, &count) in bins.iter().enumerate() {
                    seen += count;
                    if seen > target {
                        pixel[c] = level as u8;
                        break;
                    }
                }
            }
            out[column * 4..column * 4 + 4].copy_from_slice(&pixel);
            let x = area.x0 + column as i32;
            add(x - radius, -1, &mut histogram);
            add(x + radius + 1, 1, &mut histogram);
        }
    })
}
