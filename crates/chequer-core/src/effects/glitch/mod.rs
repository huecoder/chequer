mod channel_shift;
mod databend;
mod dct_corrupt;
mod pixel_sort;
mod seam_melt;
mod slice_shift;
mod sound_bend;
mod spectral;
mod vhs;

use super::EffectDef;
use super::util::hash_unit;
use crate::raster::Rgba;

const EDGES: &[&str] = &["clamp", "wrap", "mirror", "transparent"];

pub static DEFS: [EffectDef; 9] = [
    pixel_sort::DEF,
    slice_shift::DEF,
    vhs::DEF,
    channel_shift::DEF,
    databend::DEF,
    sound_bend::DEF,
    seam_melt::DEF,
    dct_corrupt::DEF,
    spectral::DEF,
];

fn signed_unit(seed: u32, a: i32, b: i32) -> f32 {
    hash_unit(seed, a, b) * 2.0 - 1.0
}

fn merge_channels(red: Rgba, green: Rgba, blue: Rgba) -> Rgba {
    [red[0], green[1], blue[2], red[3].max(green[3]).max(blue[3])]
}

#[cfg(test)]
mod tests {
    use crate::effects::{Cancel, EffectId, EffectParams, Palette, ParamValue, ids, render};
    use crate::geom::IRect;
    use crate::raster::Raster;

    const SIDE: u32 = 40;
    const SEED_STEP: u32 = 7;

    fn noisy_sample() -> Raster {
        let mut raster = Raster::filled(SIDE, SIDE, [0, 0, 0, 255]);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let value = (x * SEED_STEP + y * 31 + x * y) as u8;
                raster.set(x, y, [value, value.wrapping_mul(3), value.wrapping_add(90), 255]);
            }
        }
        raster
    }

    fn sorted_pixels(raster: &Raster) -> Vec<[u8; 4]> {
        let mut pixels: Vec<[u8; 4]> = raster.as_bytes().chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
        pixels.sort_unstable();
        pixels
    }

    #[test]
    fn pixel_sort_only_rearranges_pixels() {
        let source = noisy_sample();
        let palette = Palette { primary: [0, 0, 0, 255], secondary: [255, 255, 255, 255] };
        let params = EffectParams::defaults(ids::PIXEL_SORT, palette);
        let sorted = render(&source, IRect::new(0, 0, SIDE as i32, SIDE as i32), &params, &Cancel::default()).unwrap();
        assert_ne!(sorted.as_bytes(), source.as_bytes());
        assert_eq!(sorted_pixels(&sorted), sorted_pixels(&source));
    }

    fn with_values(id: EffectId, values: &[(&str, ParamValue)]) -> EffectParams {
        let palette = Palette { primary: [0, 0, 0, 255], secondary: [255, 255, 255, 255] };
        let mut params = EffectParams::defaults(id, palette);
        for (key, value) in values {
            params.set(id.params().iter().position(|spec| spec.key == *key).unwrap(), *value);
        }
        params
    }

    fn mean_error(a: &Raster, b: &Raster) -> f32 {
        let total: u32 = a.as_bytes().iter().zip(b.as_bytes()).map(|(x, y)| u32::from(x.abs_diff(*y))).sum();
        total as f32 / a.as_bytes().len() as f32
    }

    #[test]
    fn spectral_and_dct_are_transparent_without_corruption() {
        let source = noisy_sample();
        let area = IRect::new(0, 0, SIDE as i32, SIDE as i32);
        let spectral = with_values(ids::SPECTRAL_GLITCH, &[("phase_noise", ParamValue::Int(0))]);
        let restored = render(&source, area, &spectral, &Cancel::default()).unwrap();
        assert!(mean_error(&restored, &source) < 2.0, "спектр без правок обязан вернуть картинку");
        let calm = [("quality", 100), ("subsampling", 0), ("dc_drift", 0), ("color_drift", 0), ("zero_blocks", 0), ("scramble", 0), ("ringing", 0), ("block_swap", 0)];
        let values: Vec<_> = calm.iter().map(|&(key, value)| (key, if key == "subsampling" { ParamValue::Choice(value as usize) } else { ParamValue::Int(value) })).collect();
        let dct = with_values(ids::DCT_CORRUPT, &values);
        let decoded = render(&source, area, &dct, &Cancel::default()).unwrap();
        assert!(mean_error(&decoded, &source) < 4.0, "DCT на максимальном качестве без порчи обязан быть близок к оригиналу");
    }
}
