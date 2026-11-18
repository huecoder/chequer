use super::palette::{Rgb, nearest};
use super::util::{Cancel, map_area};
use crate::raster::Raster;

const ORDERED_SPREAD_CHANNEL: f32 = 255.0;
const KERNEL_PADDING: usize = 2;
const KERNEL_DEPTH: usize = 3;
const HALF: f32 = 0.5;

type Tap = (i32, usize, f32);

#[derive(Clone, Copy, Debug)]
pub enum Dither {
    None,
    Diffusion(&'static [Tap]),
    Ordered(u32),
}

const FLOYD_STEINBERG: [Tap; 4] = [(1, 0, 7.0 / 16.0), (-1, 1, 3.0 / 16.0), (0, 1, 5.0 / 16.0), (1, 1, 1.0 / 16.0)];
const ATKINSON: [Tap; 6] = [(1, 0, 0.125), (2, 0, 0.125), (-1, 1, 0.125), (0, 1, 0.125), (1, 1, 0.125), (0, 2, 0.125)];
const SIERRA_LITE: [Tap; 3] = [(1, 0, 0.5), (-1, 1, 0.25), (0, 1, 0.25)];
const JARVIS: [Tap; 12] = [
    (1, 0, 7.0 / 48.0),
    (2, 0, 5.0 / 48.0),
    (-2, 1, 3.0 / 48.0),
    (-1, 1, 5.0 / 48.0),
    (0, 1, 7.0 / 48.0),
    (1, 1, 5.0 / 48.0),
    (2, 1, 3.0 / 48.0),
    (-2, 2, 1.0 / 48.0),
    (-1, 2, 3.0 / 48.0),
    (0, 2, 5.0 / 48.0),
    (1, 2, 3.0 / 48.0),
    (2, 2, 1.0 / 48.0),
];

impl Dither {
    pub fn from_key(key: &str) -> Self {
        match key {
            "floyd_steinberg" => Dither::Diffusion(&FLOYD_STEINBERG),
            "atkinson" => Dither::Diffusion(&ATKINSON),
            "sierra_lite" => Dither::Diffusion(&SIERRA_LITE),
            "jarvis" => Dither::Diffusion(&JARVIS),
            "bayer_4" => Dither::Ordered(2),
            "bayer_8" => Dither::Ordered(3),
            _ => Dither::None,
        }
    }
}

fn bayer_threshold(x: usize, y: usize, order: u32) -> f32 {
    let value = (0..order).fold(0usize, |sum, level| {
        let (x_bit, y_bit) = ((x >> level) & 1, (y >> level) & 1);
        let digit = (2 * x_bit + 3 * y_bit) % 4;
        sum + digit * 4usize.pow(order - 1 - level)
    });
    let cells = 4usize.pow(order);
    (value as f32 + HALF) / cells as f32 - HALF
}

fn with_alpha(color: Rgb, alpha: u8) -> [u8; 4] {
    [color[0], color[1], color[2], alpha]
}

pub fn apply(work: &Raster, palette: &[Rgb], dither: Dither, amount: f32, cancel: &Cancel) -> Option<Raster> {
    match dither {
        Dither::None => map_area(work.bounds(), cancel, |x, y| {
            let pixel = work.get(x as u32, y as u32);
            with_alpha(nearest(palette, [pixel[0], pixel[1], pixel[2]].map(f32::from)), pixel[3])
        }),
        Dither::Ordered(order) => ordered(work, palette, order, amount, cancel),
        Dither::Diffusion(taps) => diffused(work, palette, taps, amount, cancel),
    }
}

fn ordered(work: &Raster, palette: &[Rgb], order: u32, amount: f32, cancel: &Cancel) -> Option<Raster> {
    let spread = amount * ORDERED_SPREAD_CHANNEL / (palette.len() as f32).cbrt().max(1.0);
    map_area(work.bounds(), cancel, |x, y| {
        let pixel = work.get(x as u32, y as u32);
        let offset = bayer_threshold(x as usize, y as usize, order) * spread;
        let wanted = [pixel[0], pixel[1], pixel[2]].map(|channel| (f32::from(channel) + offset).clamp(0.0, ORDERED_SPREAD_CHANNEL));
        with_alpha(nearest(palette, wanted), pixel[3])
    })
}

fn diffused(work: &Raster, palette: &[Rgb], taps: &[Tap], amount: f32, cancel: &Cancel) -> Option<Raster> {
    let (width, height) = (work.width() as usize, work.height() as usize);
    let mut errors = vec![vec![[0.0f32; 3]; width + 2 * KERNEL_PADDING]; KERNEL_DEPTH];
    let mut out = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        if cancel.is_set() {
            return None;
        }
        for x in 0..width {
            let pixel = work.get(x as u32, y as u32);
            if pixel[3] == 0 {
                out.extend_from_slice(&pixel);
                continue;
            }
            let carried = errors[0][x + KERNEL_PADDING];
            let wanted: [f32; 3] = std::array::from_fn(|c| (f32::from(pixel[c]) + carried[c]).clamp(0.0, ORDERED_SPREAD_CHANNEL));
            let chosen = nearest(palette, wanted);
            out.extend_from_slice(&with_alpha(chosen, pixel[3]));
            let miss: [f32; 3] = std::array::from_fn(|c| (wanted[c] - f32::from(chosen[c])) * amount);
            for &(dx, dy, share) in taps {
                let target = &mut errors[dy][(x as i32 + KERNEL_PADDING as i32 + dx) as usize];
                for c in 0..3 {
                    target[c] += miss[c] * share;
                }
            }
        }
        errors.rotate_left(1);
        errors[KERNEL_DEPTH - 1].fill([0.0; 3]);
    }
    Raster::from_rgba(width as u32, height as u32, out).ok()
}
