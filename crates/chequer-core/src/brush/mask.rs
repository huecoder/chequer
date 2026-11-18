use image::DynamicImage;

use super::{BrushError, TipSpec};

pub const MAX_TIP_SIDE: u32 = 4096;

const OPAQUE: u8 = u8::MAX;
const LUMA_WEIGHTS: [u32; 3] = [299, 587, 114];
const LUMA_DIVISOR: u32 = 1000;

#[derive(Clone, Debug)]
struct Level {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Level {
    fn texel(&self, x: i32, y: i32) -> f32 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return 0.0;
        }
        self.pixels[y as usize * self.width as usize + x as usize] as f32
    }

    fn bilinear(&self, x: f32, y: f32) -> f32 {
        let (corner_x, corner_y) = (x - 0.5, y - 0.5);
        let (floor_x, floor_y) = (corner_x.floor(), corner_y.floor());
        let (along_x, along_y) = (corner_x - floor_x, corner_y - floor_y);
        let (x0, y0) = (floor_x as i32, floor_y as i32);
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let top = lerp(self.texel(x0, y0), self.texel(x0 + 1, y0), along_x);
        let bottom = lerp(self.texel(x0, y0 + 1), self.texel(x0 + 1, y0 + 1), along_x);
        lerp(top, bottom, along_y) / OPAQUE as f32
    }

    fn halved(&self) -> Level {
        let (width, height) = (self.width.div_ceil(2), self.height.div_ceil(2));
        let mut pixels = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let sum = self.texel(2 * x, 2 * y) + self.texel(2 * x + 1, 2 * y) + self.texel(2 * x, 2 * y + 1) + self.texel(2 * x + 1, 2 * y + 1);
                pixels.push((sum / 4.0).round() as u8);
            }
        }
        Level { width, height, pixels }
    }
}

#[derive(Clone, Debug)]
pub struct MaskTip {
    levels: Vec<Level>,
    spec: Option<TipSpec>,
}

impl MaskTip {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, BrushError> {
        let in_range = |side: u32| (1..=MAX_TIP_SIDE).contains(&side);
        if !in_range(width) || !in_range(height) || pixels.len() != width as usize * height as usize {
            return Err(BrushError::TipSize { width, height });
        }
        let mut levels = Vec::new();
        let mut current = Level { width, height, pixels };
        while current.width > 1 || current.height > 1 {
            let next = current.halved();
            levels.push(current);
            current = next;
        }
        levels.push(current);
        Ok(Self { levels, spec: None })
    }

    pub fn with_spec(mut self, spec: Option<TipSpec>) -> Self {
        self.spec = spec;
        self
    }

    pub fn spec(&self) -> Option<&TipSpec> {
        self.spec.as_ref()
    }

    pub fn from_image(image: &DynamicImage) -> Result<Self, BrushError> {
        let rgba = image.to_rgba8();
        let (width, height) = rgba.dimensions();
        let has_alpha = rgba.pixels().any(|pixel| pixel[3] != OPAQUE);
        let ink = |pixel: &image::Rgba<u8>| {
            if has_alpha {
                return pixel[3];
            }
            let weighted: u32 = (0..3).map(|channel| pixel[channel] as u32 * LUMA_WEIGHTS[channel]).sum();
            OPAQUE - (weighted / LUMA_DIVISOR) as u8
        };
        Self::new(width, height, rgba.pixels().map(ink).collect())
    }

    pub fn width(&self) -> u32 {
        self.levels[0].width
    }

    pub fn height(&self) -> u32 {
        self.levels[0].height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.levels[0].pixels
    }

    pub fn sample(&self, x: f32, y: f32, texel_px: f32) -> f32 {
        let level = self.level_for(texel_px);
        let factor = (1u32 << level) as f32;
        self.levels[level].bilinear(x / factor, y / factor)
    }

    pub fn sample_nearest(&self, x: f32, y: f32) -> f32 {
        self.levels[0].texel(x.floor() as i32, y.floor() as i32) / OPAQUE as f32
    }

    fn level_for(&self, texel_px: f32) -> usize {
        if texel_px >= 1.0 {
            return 0;
        }
        ((1.0 / texel_px).log2().floor() as usize).min(self.levels.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pyramid_ends_at_one_texel_with_the_mean_value() {
        let tip = MaskTip::new(4, 4, vec![255; 16]).unwrap();
        assert_eq!(tip.levels.len(), 3);
        assert_eq!(tip.levels[2].pixels, vec![255]);
    }

    #[test]
    fn sampling_inside_a_flat_tip_is_full_and_outside_is_empty() {
        let tip = MaskTip::new(8, 8, vec![255; 64]).unwrap();
        assert_eq!(tip.sample(4.0, 4.0, 1.0), 1.0);
        assert_eq!(tip.sample(-3.0, 4.0, 1.0), 0.0);
    }

    #[test]
    fn dark_opaque_image_becomes_ink_and_alpha_image_uses_alpha() {
        let dark = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255])));
        assert_eq!(MaskTip::from_image(&dark).unwrap().pixels(), &[255; 4]);
        let mut clear = image::RgbaImage::from_pixel(2, 1, image::Rgba([0, 0, 0, 0]));
        clear.put_pixel(0, 0, image::Rgba([200, 200, 200, 90]));
        assert_eq!(MaskTip::from_image(&DynamicImage::ImageRgba8(clear)).unwrap().pixels(), &[90, 0]);
    }

    #[test]
    fn oversized_or_mismatched_tip_is_refused() {
        assert!(MaskTip::new(MAX_TIP_SIDE + 1, 1, vec![0; MAX_TIP_SIDE as usize + 1]).is_err());
        assert!(MaskTip::new(2, 2, vec![0; 3]).is_err());
    }
}
