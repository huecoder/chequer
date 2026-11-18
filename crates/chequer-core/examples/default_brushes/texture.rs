use chequer_core::brush::{BrushTip, MaskTip, TipVariant, hash_unit, value_noise};

pub(super) const TEXTURE_SIDE: u32 = 160;
pub(super) const VARIANT_SALT_STRIDE: u32 = 17;
pub(super) const EDGE_WIDTH: f32 = 0.03;
pub(super) const OCTAVE_GAIN: f32 = 0.5;
pub(super) const OCTAVE_LACUNARITY: f32 = 2.0;
pub(super) const WARP_OCTAVES: u32 = 3;
pub(super) const WARP_OFFSET: f32 = 5.2;
pub(super) const WARP_SALT_X: u32 = 100;
pub(super) const WARP_SALT_Y: u32 = 200;
pub(super) const WORLEY_SALT_Y: u32 = 0x77;
pub(super) const FINE_OCTAVES: u32 = 2;
pub(super) const COARSE_OCTAVES: u32 = 3;
pub(super) const DETAIL_OCTAVES: u32 = 5;

pub(super) fn smooth_step(from: f32, to: f32, value: f32) -> f32 {
    let fraction = ((value - from) / (to - from)).clamp(0.0, 1.0);
    fraction * fraction * (3.0 - 2.0 * fraction)
}

pub(super) fn edge(inside: f32) -> f32 {
    smooth_step(0.0, EDGE_WIDTH, inside)
}

pub(super) fn fbm(x: f32, y: f32, octaves: u32, salt: u32) -> f32 {
    let (mut sum, mut weight, mut amplitude, mut frequency) = (0.0, 0.0, 1.0, 1.0);
    for octave in 0..octaves {
        sum += amplitude * value_noise(x * frequency, y * frequency, salt + octave);
        weight += amplitude;
        amplitude *= OCTAVE_GAIN;
        frequency *= OCTAVE_LACUNARITY;
    }
    sum / weight
}

pub(super) fn warped_fbm(x: f32, y: f32, octaves: u32, warp: f32, salt: u32) -> f32 {
    let shift_x = fbm(x + WARP_OFFSET, y, WARP_OCTAVES, salt + WARP_SALT_X) - 0.5;
    let shift_y = fbm(x, y + WARP_OFFSET, WARP_OCTAVES, salt + WARP_SALT_Y) - 0.5;
    fbm(x + shift_x * warp, y + shift_y * warp, octaves, salt)
}

pub(super) fn worley(x: f32, y: f32, salt: u32) -> (f32, f32) {
    let (cell_x, cell_y) = (x.floor() as i32, y.floor() as i32);
    let mut nearest = (f32::MAX, f32::MAX);
    for row in cell_y - 1..=cell_y + 1 {
        for column in cell_x - 1..=cell_x + 1 {
            let point_x = column as f32 + hash_unit(column, row, salt);
            let point_y = row as f32 + hash_unit(column, row, salt ^ WORLEY_SALT_Y);
            let distance = (x - point_x).hypot(y - point_y);
            if distance < nearest.0 {
                nearest = (distance, nearest.0);
            } else if distance < nearest.1 {
                nearest.1 = distance;
            }
        }
    }
    nearest
}

pub(super) fn ragged_reach(x: f32, y: f32, reach: f32, raggedness: f32, frequency: f32, salt: u32) -> f32 {
    reach + (fbm(x * frequency, y * frequency, COARSE_OCTAVES, salt) - 0.5) * raggedness - x.hypot(y)
}

pub(super) fn texture(coverage: impl Fn(f32, f32) -> f32) -> MaskTip {
    let half = TEXTURE_SIDE as f32 / 2.0;
    let pixels = (0..TEXTURE_SIDE * TEXTURE_SIDE)
        .map(|index| {
            let (x, y) = ((index % TEXTURE_SIDE) as f32 + 0.5 - half, (index / TEXTURE_SIDE) as f32 + 0.5 - half);
            (coverage(x / half, y / half).clamp(0.0, 1.0) * u8::MAX as f32).round() as u8
        })
        .collect();
    MaskTip::new(TEXTURE_SIDE, TEXTURE_SIDE, pixels).expect("texture side is within tip limits")
}

pub(super) fn variant_set(count: u32, build: impl Fn(u32) -> MaskTip) -> BrushTip {
    BrushTip::from_variants((0..count).map(|variant| TipVariant::new(build(variant))).collect())
}

pub(super) fn salted(base: u32, variant: u32) -> u32 {
    base + variant * VARIANT_SALT_STRIDE
}
