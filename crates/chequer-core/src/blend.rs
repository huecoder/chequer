use crate::raster::{CHANNEL_MAX_U8, Rgba, mul_div_255, over};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Additive,
    ColorBurn,
    ColorDodge,
    Reflect,
    Glow,
    Overlay,
    Difference,
    Negation,
    Lighten,
    Darken,
    Screen,
    Xor,
    HardLight,
    SoftLight,
    Color,
    Luminosity,
    Hue,
    Saturation,
}

const MAX: u32 = CHANNEL_MAX_U8 as u32;
const HALF: u32 = 128;
const LUMA_WEIGHTS: [f32; 3] = [0.3, 0.59, 0.11];
const UNIT_MAX: f32 = 255.0;

impl BlendMode {
    pub const ALL: [BlendMode; 20] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Additive,
        BlendMode::ColorBurn,
        BlendMode::ColorDodge,
        BlendMode::Reflect,
        BlendMode::Glow,
        BlendMode::Overlay,
        BlendMode::Difference,
        BlendMode::Negation,
        BlendMode::Lighten,
        BlendMode::Darken,
        BlendMode::Screen,
        BlendMode::Xor,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Color,
        BlendMode::Luminosity,
        BlendMode::Hue,
        BlendMode::Saturation,
    ];

    fn is_separable(self) -> bool {
        !matches!(self, BlendMode::Color | BlendMode::Luminosity | BlendMode::Hue | BlendMode::Saturation)
    }

    fn channel(self, backdrop: u8, source: u8) -> u8 {
        let (b, s) = (backdrop as u32, source as u32);
        let value = match self {
            BlendMode::Normal => s,
            BlendMode::Multiply => mul_div_255(b, s),
            BlendMode::Additive => b + s,
            BlendMode::ColorBurn if b == MAX => MAX,
            BlendMode::ColorBurn if s == 0 => 0,
            BlendMode::ColorBurn => MAX - ((MAX - b) * MAX / s).min(MAX),
            BlendMode::ColorDodge if s == MAX => MAX,
            BlendMode::ColorDodge => b * MAX / (MAX - s),
            BlendMode::Reflect if s == MAX => MAX,
            BlendMode::Reflect => b * b / (MAX - s),
            BlendMode::Glow if b == MAX => MAX,
            BlendMode::Glow => s * s / (MAX - b),
            BlendMode::Overlay => hard_light(s, b),
            BlendMode::Difference => b.abs_diff(s),
            BlendMode::Negation => MAX - (MAX as i32 - b as i32 - s as i32).unsigned_abs(),
            BlendMode::Lighten => b.max(s),
            BlendMode::Darken => b.min(s),
            BlendMode::Screen => b + s - mul_div_255(b, s),
            BlendMode::Xor => b ^ s,
            BlendMode::HardLight => hard_light(b, s),
            BlendMode::SoftLight => soft_light(b, s),
            BlendMode::Color | BlendMode::Luminosity | BlendMode::Hue | BlendMode::Saturation => s,
        };
        value.min(MAX) as u8
    }

    fn mix_rgb(self, backdrop: Rgba, source: Rgba) -> [u8; 3] {
        if self.is_separable() {
            return std::array::from_fn(|channel| self.channel(backdrop[channel], source[channel]));
        }
        let unit = |color: Rgba| [0, 1, 2].map(|i| f32::from(color[i]) / UNIT_MAX);
        let (b, s) = (unit(backdrop), unit(source));
        let result = match self {
            BlendMode::Hue => set_luminosity(set_saturation(s, saturation(b)), luminosity(b)),
            BlendMode::Saturation => set_luminosity(set_saturation(b, saturation(s)), luminosity(b)),
            BlendMode::Color => set_luminosity(s, luminosity(b)),
            _ => set_luminosity(b, luminosity(s)),
        };
        result.map(|value| (value * UNIT_MAX).round().clamp(0.0, UNIT_MAX) as u8)
    }
}

fn hard_light(backdrop: u32, source: u32) -> u32 {
    if backdrop < HALF { 2 * mul_div_255(backdrop, source) } else { MAX - 2 * mul_div_255(MAX - backdrop, MAX - source) }
}

fn soft_light(backdrop: u32, source: u32) -> u32 {
    let (b, s) = (backdrop as f32 / UNIT_MAX, source as f32 / UNIT_MAX);
    let curve = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
    let value = if s <= 0.5 { b - (1.0 - 2.0 * s) * b * (1.0 - b) } else { b + (2.0 * s - 1.0) * (curve - b) };
    (value * UNIT_MAX).round().clamp(0.0, UNIT_MAX) as u32
}

fn luminosity(color: [f32; 3]) -> f32 {
    color.iter().zip(LUMA_WEIGHTS).map(|(c, w)| c * w).sum()
}

fn saturation(color: [f32; 3]) -> f32 {
    color.iter().cloned().fold(f32::MIN, f32::max) - color.iter().cloned().fold(f32::MAX, f32::min)
}

fn set_luminosity(color: [f32; 3], target: f32) -> [f32; 3] {
    let shift = target - luminosity(color);
    clip_color(color.map(|c| c + shift))
}

fn clip_color(color: [f32; 3]) -> [f32; 3] {
    let lum = luminosity(color);
    let (low, high) = (color.iter().cloned().fold(f32::MAX, f32::min), color.iter().cloned().fold(f32::MIN, f32::max));
    let mut result = color;
    if low < 0.0 {
        result = result.map(|c| lum + (c - lum) * lum / (lum - low));
    }
    if high > 1.0 {
        result = result.map(|c| lum + (c - lum) * (1.0 - lum) / (high - lum));
    }
    result
}

fn set_saturation(color: [f32; 3], target: f32) -> [f32; 3] {
    let (low, high) = (color.iter().cloned().fold(f32::MAX, f32::min), color.iter().cloned().fold(f32::MIN, f32::max));
    if high <= low {
        return [0.0; 3];
    }
    color.map(|c| (c - low) * target / (high - low))
}

pub fn blend(mode: BlendMode, dst: Rgba, src: Rgba) -> Rgba {
    if mode == BlendMode::Normal || dst[3] == 0 || src[3] == 0 {
        return over(dst, src);
    }
    let backdrop_alpha = dst[3] as u32;
    let blended = mode.mix_rgb(dst, src);
    let mut mixed = src;
    for channel in 0..3 {
        let source = src[channel] as u32;
        mixed[channel] = ((source * (MAX - backdrop_alpha) + blended[channel] as u32 * backdrop_alpha) / MAX) as u8;
    }
    over(dst, mixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiply_with_white_keeps_backdrop() {
        assert_eq!(blend(BlendMode::Multiply, [100, 50, 20, 255], [255, 255, 255, 255]), [100, 50, 20, 255]);
    }

    #[test]
    fn screen_with_black_keeps_backdrop() {
        assert_eq!(blend(BlendMode::Screen, [100, 50, 20, 255], [0, 0, 0, 255]), [100, 50, 20, 255]);
    }

    #[test]
    fn difference_of_equal_colors_is_black() {
        assert_eq!(blend(BlendMode::Difference, [90, 90, 90, 255], [90, 90, 90, 255]), [0, 0, 0, 255]);
    }

    #[test]
    fn luminosity_of_gray_source_on_color_backdrop_keeps_gray_level() {
        let [r, g, b, _] = blend(BlendMode::Luminosity, [200, 40, 40, 255], [128, 128, 128, 255]);
        let level = (f32::from(r) * 0.3 + f32::from(g) * 0.59 + f32::from(b) * 0.11).round();
        assert!((level - 128.0).abs() <= 2.0);
    }

    #[test]
    fn negation_of_black_and_white_is_white_on_white_backdrop() {
        assert_eq!(blend(BlendMode::Negation, [255, 255, 255, 255], [0, 0, 0, 255]), [255, 255, 255, 255]);
    }

    #[test]
    fn any_mode_over_transparent_backdrop_is_plain_source() {
        for mode in BlendMode::ALL {
            assert_eq!(blend(mode, [0, 0, 0, 0], [10, 20, 30, 255]), [10, 20, 30, 255]);
        }
    }
}
