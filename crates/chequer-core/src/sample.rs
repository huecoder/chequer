use crate::geom::{IPoint, IRect};
use crate::raster::{Raster, Rgba};

const CHANNEL_MAX: f32 = 255.0;
const SRGB_LINEAR_CUTOFF: f32 = 0.04045;
const SRGB_LINEAR_SLOPE: f32 = 12.92;
const SRGB_OFFSET: f32 = 0.055;
const SRGB_GAMMA: f32 = 2.4;
const LINEAR_SRGB_CUTOFF: f32 = 0.0031308;
const COLOR_CHANNELS: usize = 3;
const ALPHA_CHANNEL: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixSpace {
    Srgb,
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaMix {
    Weighted,
    Plain,
    Opaque,
}

pub fn square_around(center: IPoint, side: u32) -> IRect {
    let half = (side / 2) as i32;
    IRect::from_size(center.x - half, center.y - half, side, side)
}

pub fn average_area(raster: &Raster, area: IRect, space: MixSpace, alpha: AlphaMix) -> Option<Rgba> {
    let visible = area.intersect(raster.bounds());
    let pixels = (visible.y0..visible.y1).flat_map(|y| (visible.x0..visible.x1).map(move |x| (x, y))).map(|(x, y)| raster.get(x as u32, y as u32));
    average(pixels, space, alpha)
}

fn average(pixels: impl Iterator<Item = Rgba>, space: MixSpace, alpha: AlphaMix) -> Option<Rgba> {
    let (mut channel_sums, mut weight_sum, mut alpha_sum, mut count) = ([0.0_f32; COLOR_CHANNELS], 0.0_f32, 0.0_f32, 0_u32);
    for pixel in pixels {
        let opacity = f32::from(pixel[ALPHA_CHANNEL]) / CHANNEL_MAX;
        let weight = if alpha == AlphaMix::Plain { 1.0 } else { opacity };
        for (sum, &value) in channel_sums.iter_mut().zip(&pixel[..COLOR_CHANNELS]) {
            *sum += decode(value, space) * weight;
        }
        weight_sum += weight;
        alpha_sum += opacity;
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let mean_opacity = if alpha == AlphaMix::Opaque { 1.0 } else { alpha_sum / count as f32 };
    if weight_sum <= 0.0 {
        return Some([0, 0, 0, encode_alpha(mean_opacity)]);
    }
    let [r, g, b] = channel_sums.map(|sum| encode(sum / weight_sum, space));
    Some([r, g, b, encode_alpha(mean_opacity)])
}

fn decode(value: u8, space: MixSpace) -> f32 {
    let unit = f32::from(value) / CHANNEL_MAX;
    match space {
        MixSpace::Srgb => unit,
        MixSpace::Linear if unit <= SRGB_LINEAR_CUTOFF => unit / SRGB_LINEAR_SLOPE,
        MixSpace::Linear => ((unit + SRGB_OFFSET) / (1.0 + SRGB_OFFSET)).powf(SRGB_GAMMA),
    }
}

fn encode(unit: f32, space: MixSpace) -> u8 {
    let encoded = match space {
        MixSpace::Srgb => unit,
        MixSpace::Linear if unit <= LINEAR_SRGB_CUTOFF => unit * SRGB_LINEAR_SLOPE,
        MixSpace::Linear => (1.0 + SRGB_OFFSET) * unit.powf(1.0 / SRGB_GAMMA) - SRGB_OFFSET,
    };
    (encoded.clamp(0.0, 1.0) * CHANNEL_MAX).round() as u8
}

fn encode_alpha(opacity: f32) -> u8 {
    (opacity.clamp(0.0, 1.0) * CHANNEL_MAX).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_average_ignores_transparent_pixels_color() {
        let pixels = [[255, 0, 0, 255], [0, 0, 255, 0]].into_iter();
        assert_eq!(average(pixels, MixSpace::Srgb, AlphaMix::Weighted), Some([255, 0, 0, 128]));
    }

    #[test]
    fn plain_average_mixes_every_channel() {
        let pixels = [[200, 0, 0, 255], [0, 0, 0, 0]].into_iter();
        assert_eq!(average(pixels, MixSpace::Srgb, AlphaMix::Plain), Some([100, 0, 0, 128]));
    }

    #[test]
    fn opaque_mode_forces_full_alpha() {
        let pixels = [[10, 20, 30, 40]].into_iter();
        assert_eq!(average(pixels, MixSpace::Srgb, AlphaMix::Opaque), Some([10, 20, 30, 255]));
    }

    #[test]
    fn linear_mix_of_black_and_white_is_brighter_than_srgb_mix() {
        let pixels = || [[0, 0, 0, 255], [255, 255, 255, 255]].into_iter();
        let linear = average(pixels(), MixSpace::Linear, AlphaMix::Weighted).unwrap();
        let plain = average(pixels(), MixSpace::Srgb, AlphaMix::Weighted).unwrap();
        assert!(linear[0] > plain[0]);
    }

    #[test]
    fn empty_input_has_no_color() {
        assert_eq!(average(std::iter::empty(), MixSpace::Srgb, AlphaMix::Weighted), None);
    }
}
