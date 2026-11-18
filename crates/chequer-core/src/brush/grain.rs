use super::texture::Texture;

pub const GRAIN_SCALE_RANGE: (f32, f32) = (1.0, 32.0);
const PAPER_TEXEL_RATIO: f32 = 0.5;
const RESIDUAL_COVERAGE_LOSS: f32 = 0.5;
const MIN_FILL_RANGE: f32 = 1e-3;
const DEPOSIT_CONTRAST: f32 = 1.6;

pub fn hash_unit(x: i32, y: i32, salt: u32) -> f32 {
    let mut value = (x as u32).wrapping_mul(0x85EB_CA6B) ^ (y as u32).wrapping_mul(0xC2B2_AE35) ^ salt.wrapping_mul(0x27D4_EB2F);
    value ^= value >> 15;
    value = value.wrapping_mul(0x2C1B_3C6D);
    value ^= value >> 12;
    (value & 0xFFFF) as f32 / 65535.0
}

pub fn value_noise(x: f32, y: f32, salt: u32) -> f32 {
    let (cell_x, cell_y) = (x.floor(), y.floor());
    let smooth = |fraction: f32| fraction * fraction * (3.0 - 2.0 * fraction);
    let (blend_x, blend_y) = (smooth(x - cell_x), smooth(y - cell_y));
    let corner = |step_x: i32, step_y: i32| hash_unit(cell_x as i32 + step_x, cell_y as i32 + step_y, salt);
    let lerp = |from: f32, to: f32, amount: f32| from + (to - from) * amount;
    lerp(lerp(corner(0, 0), corner(1, 0), blend_x), lerp(corner(0, 1), corner(1, 1), blend_x), blend_y)
}

fn paper_height(paper: &Texture, x: i32, y: i32, scale: f32) -> f32 {
    let texel = scale * PAPER_TEXEL_RATIO;
    paper.tiled((x as f32 + 0.5) / texel - 0.5, (y as f32 + 0.5) / texel - 0.5)
}

pub fn grained(paper: &Texture, coverage: f32, strength: f32, x: i32, y: i32, scale: f32) -> f32 {
    if strength <= 0.0 {
        return coverage;
    }
    let valley = 1.0 - paper_height(paper, x, y, scale);
    let threshold = strength * valley;
    let filled = ((coverage - threshold) / (1.0 - threshold).max(MIN_FILL_RANGE)).clamp(0.0, 1.0);
    filled * (1.0 - RESIDUAL_COVERAGE_LOSS * threshold)
}

pub fn grained_deposit(paper: &Texture, coverage: f32, strength: f32, x: i32, y: i32, scale: f32) -> f32 {
    if strength <= 0.0 {
        return coverage;
    }
    let valley = 1.0 - paper_height(paper, x, y, scale);
    coverage * (1.0 - strength * DEPOSIT_CONTRAST * valley).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampled(coverage: f32, strength: f32) -> Vec<f32> {
        (0..64).flat_map(|x| (0..64).map(move |y| grained(Texture::builtin_paper(), coverage, strength, x, y, 3.0))).collect()
    }

    #[test]
    fn zero_strength_keeps_coverage() {
        assert_eq!(grained(Texture::builtin_paper(), 0.7, 0.0, 5, 9, 3.0), 0.7);
    }

    #[test]
    fn light_touch_skips_the_valleys_and_heavy_pressure_fills_them() {
        let light = sampled(0.3, 1.0);
        assert!(light.iter().any(|&value| value == 0.0) && light.iter().any(|&value| value > 0.25));
        let heavy = sampled(1.0, 0.5);
        assert!(heavy.iter().all(|&value| value >= 0.7));
    }

    #[test]
    fn deposit_keeps_peaks_and_starves_valleys() {
        assert_eq!(grained_deposit(Texture::builtin_paper(), 0.4, 0.0, 3, 3, 3.0), 0.4);
        let deposits: Vec<f32> = (0..64).flat_map(|x| (0..64).map(move |y| grained_deposit(Texture::builtin_paper(), 0.4, 0.8, x, y, 3.0))).collect();
        assert!(deposits.iter().any(|&value| value == 0.0) && deposits.iter().any(|&value| value > 0.35));
    }

    #[test]
    fn paper_has_a_full_tonal_range() {
        let heights: Vec<f32> = (0..64).flat_map(|x| (0..64).map(move |y| paper_height(Texture::builtin_paper(), x, y, 2.0))).collect();
        assert!(heights.iter().any(|&value| value < 0.15) && heights.iter().any(|&value| value > 0.85));
    }
}
