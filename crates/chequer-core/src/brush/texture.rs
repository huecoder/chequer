use std::sync::OnceLock;

use image::DynamicImage;
use image::imageops::FilterType;

use super::pattern::{PatternKind, PatternSpec, generate, paper_texture};
use super::{BrushError, MaskTip};

pub const MAX_TEXTURE_SIDE: u32 = 1024;

const OPAQUE: f32 = u8::MAX as f32;

static BUILTIN_PAPER: OnceLock<Texture> = OnceLock::new();
static BUILTIN_HAIRS: OnceLock<Texture> = OnceLock::new();

#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    spec: Option<PatternSpec>,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Texture {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, BrushError> {
        let in_range = |side: u32| (1..=MAX_TEXTURE_SIDE).contains(&side);
        if !in_range(width) || !in_range(height) || pixels.len() != width as usize * height as usize {
            return Err(BrushError::TipSize { width, height });
        }
        Ok(Self { spec: None, width, height, pixels })
    }

    pub(super) fn generated(width: u32, height: u32, pixels: Vec<u8>, spec: Option<PatternSpec>) -> Self {
        Self { spec, width, height, pixels }
    }

    fn fitted(image: &DynamicImage) -> DynamicImage {
        if image.width() > MAX_TEXTURE_SIDE || image.height() > MAX_TEXTURE_SIDE {
            image.resize(MAX_TEXTURE_SIDE, MAX_TEXTURE_SIDE, FilterType::Triangle)
        } else {
            image.clone()
        }
    }

    pub fn from_image(image: &DynamicImage) -> Result<Self, BrushError> {
        let gray = Self::fitted(image).to_luma8();
        Self::new(gray.width(), gray.height(), gray.into_raw())
    }

    pub fn from_ink_image(image: &DynamicImage) -> Result<Self, BrushError> {
        let ink = MaskTip::from_image(&Self::fitted(image))?;
        Self::new(ink.width(), ink.height(), ink.pixels().to_vec())
    }

    pub fn from_image_row(image: &DynamicImage) -> Result<Self, BrushError> {
        let full = Self::from_image(image)?;
        let rows = full.height as usize;
        let row = (0..full.width as usize)
            .map(|column| (0..rows).map(|line| u32::from(full.pixels[line * full.width as usize + column])).sum::<u32>() / rows as u32)
            .map(|mean| mean as u8)
            .collect();
        Self::new(full.width, 1, row)
    }

    pub fn with_spec(mut self, spec: Option<PatternSpec>) -> Self {
        self.spec = spec;
        self
    }

    pub fn spec(&self) -> Option<PatternSpec> {
        self.spec
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn builtin_paper() -> &'static Texture {
        BUILTIN_PAPER.get_or_init(paper_texture)
    }

    pub fn builtin_hairs() -> &'static Texture {
        BUILTIN_HAIRS.get_or_init(|| generate(PatternSpec::preset(PatternKind::Bristles)).with_spec(None))
    }

    pub fn tiled(&self, u: f32, v: f32) -> f32 {
        let (cell_u, cell_v) = (u.floor(), v.floor());
        let (fraction_u, fraction_v) = (u - cell_u, v - cell_v);
        let at = |step_u: i32, step_v: i32| f32::from(self.wrapped(cell_u as i32 + step_u, cell_v as i32 + step_v)) / OPAQUE;
        let top = at(0, 0) + (at(1, 0) - at(0, 0)) * fraction_u;
        let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * fraction_u;
        top + (bottom - top) * fraction_v
    }

    pub fn across(&self, position: f32) -> f32 {
        let scaled = position.clamp(0.0, 1.0) * (self.width - 1) as f32;
        let (low, fraction) = (scaled.floor() as usize, scaled.fract());
        let high = (low + 1).min(self.width as usize - 1);
        let row = (self.height as usize / 2) * self.width as usize;
        let (from, to) = (f32::from(self.pixels[row + low]), f32::from(self.pixels[row + high]));
        (from + (to - from) * fraction) / OPAQUE
    }

    pub fn profile(&self, across: f32, down: f32) -> f32 {
        let column = across.clamp(0.0, 1.0) * (self.width - 1) as f32;
        let (left, fraction_x) = (column.floor() as usize, column.fract());
        let right = (left + 1).min(self.width as usize - 1);
        let line = down * self.height as f32 - 0.5;
        let (top, fraction_y) = (line.floor() as i32, line.fract().rem_euclid(1.0));
        let at = |x: usize, y: i32| f32::from(self.pixels[y.rem_euclid(self.height as i32) as usize * self.width as usize + x]) / OPAQUE;
        let upper = at(left, top) + (at(right, top) - at(left, top)) * fraction_x;
        let lower = at(left, top + 1) + (at(right, top + 1) - at(left, top + 1)) * fraction_x;
        upper + (lower - upper) * fraction_y
    }

    fn wrapped(&self, x: i32, y: i32) -> u8 {
        self.pixels[y.rem_euclid(self.height as i32) as usize * self.width as usize + x.rem_euclid(self.width as i32) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiled_sampling_wraps_around_the_edges() {
        let texture = Texture::new(2, 1, vec![0, 255]).unwrap();
        assert_eq!(texture.tiled(0.0, 0.0), 0.0);
        assert_eq!(texture.tiled(2.0, 0.0), 0.0);
        assert_eq!(texture.tiled(-1.0, 0.0), 1.0);
    }

    #[test]
    fn profile_clamps_across_and_wraps_down() {
        let texture = Texture::new(2, 2, vec![0, 255, 0, 255]).unwrap();
        assert_eq!(texture.profile(-1.0, 0.25), 0.0);
        assert_eq!(texture.profile(2.0, 0.25), 1.0);
        assert_eq!(texture.profile(0.5, 0.25), texture.profile(0.5, 1.25));
    }

    #[test]
    fn across_interpolates_and_clamps_to_the_row() {
        let texture = Texture::new(3, 1, vec![0, 255, 0]).unwrap();
        assert_eq!(texture.across(-1.0), 0.0);
        assert_eq!(texture.across(0.5), 1.0);
        assert_eq!(texture.across(0.25), 0.5);
    }

    #[test]
    fn oversized_or_mismatched_texture_is_refused() {
        assert!(Texture::new(MAX_TEXTURE_SIDE + 1, 1, vec![0; MAX_TEXTURE_SIDE as usize + 1]).is_err());
        assert!(Texture::new(2, 2, vec![0; 3]).is_err());
    }

    #[test]
    fn ink_image_reads_dark_and_opaque_as_paint() {
        let image = DynamicImage::ImageLuma8(image::GrayImage::from_raw(2, 1, vec![0, 255]).unwrap());
        assert_eq!(Texture::from_ink_image(&image).unwrap().pixels(), &[255, 0]);
    }

    #[test]
    fn image_row_averages_the_columns() {
        let image = DynamicImage::ImageLuma8(image::GrayImage::from_raw(2, 2, vec![0, 100, 200, 100]).unwrap());
        assert_eq!(Texture::from_image_row(&image).unwrap().pixels(), &[100, 100]);
    }
}
