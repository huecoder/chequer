use crate::document::Resample;
use crate::geom::{IPoint, IRect};

pub type Rgba = [u8; 4];

pub const TRANSPARENT: Rgba = [0, 0, 0, 0];
pub const BYTES_PER_PIXEL: usize = 4;

pub const CHANNEL_MAX_U8: u8 = u8::MAX;
const CHANNEL_MAX: u32 = CHANNEL_MAX_U8 as u32;

#[derive(Debug)]
pub enum RasterError {
    SizeMismatch { expected: usize, actual: usize },
    ZeroSize,
}

impl std::fmt::Display for RasterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SizeMismatch { expected, actual } => {
                write!(f, "ожидалось {expected} байт пикселей, получено {actual}")
            }
            Self::ZeroSize => write!(f, "нулевой размер изображения"),
        }
    }
}

impl std::error::Error for RasterError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Raster {
    pub fn filled(width: u32, height: u32, color: Rgba) -> Self {
        let pixel_count = width as usize * height as usize;
        let data = color.iter().copied().cycle().take(pixel_count * BYTES_PER_PIXEL).collect();
        Self { width, height, data }
    }

    pub fn transparent(width: u32, height: u32) -> Self {
        Self { width, height, data: vec![0; width as usize * height as usize * BYTES_PER_PIXEL] }
    }

    pub fn from_rgba(width: u32, height: u32, data: Vec<u8>) -> Result<Self, RasterError> {
        if width == 0 || height == 0 {
            return Err(RasterError::ZeroSize);
        }
        let expected = width as usize * height as usize * BYTES_PER_PIXEL;
        if data.len() != expected {
            return Err(RasterError::SizeMismatch { expected, actual: data.len() });
        }
        Ok(Self { width, height, data })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bounds(&self) -> IRect {
        IRect::from_size(0, 0, self.width, self.height)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    pub fn content_bounds(&self) -> IRect {
        let mut bounds = IRect::EMPTY;
        for y in 0..self.height {
            let pixels = || self.row_bytes(y, 0, self.width).chunks_exact(BYTES_PER_PIXEL);
            let visible = |pixel: &[u8]| pixel[BYTES_PER_PIXEL - 1] != 0;
            let Some(first) = pixels().position(visible) else {
                continue;
            };
            let last = pixels().rposition(visible).unwrap_or(first);
            bounds = bounds.union(IRect::new(first as i32, y as i32, last as i32 + 1, y as i32 + 1));
        }
        bounds
    }

    pub fn is_transparent(&self) -> bool {
        self.data.chunks_exact(BYTES_PER_PIXEL).all(|pixel| pixel[BYTES_PER_PIXEL - 1] == 0)
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * BYTES_PER_PIXEL
    }

    pub fn get(&self, x: u32, y: u32) -> Rgba {
        let i = self.offset(x, y);
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }

    pub fn set(&mut self, x: u32, y: u32, color: Rgba) {
        let i = self.offset(x, y);
        self.data[i..i + BYTES_PER_PIXEL].copy_from_slice(&color);
    }

    pub fn get_at(&self, p: IPoint) -> Option<Rgba> {
        self.bounds().contains(p).then(|| self.get(p.x as u32, p.y as u32))
    }

    pub fn row_bytes(&self, y: u32, x0: u32, x1: u32) -> &[u8] {
        let start = self.offset(x0, y);
        let end = self.offset(x0, y) + (x1 - x0) as usize * BYTES_PER_PIXEL;
        &self.data[start..end]
    }

    pub fn row_bytes_mut(&mut self, y: u32, x0: u32, x1: u32) -> &mut [u8] {
        let start = self.offset(x0, y);
        let end = start + (x1 - x0) as usize * BYTES_PER_PIXEL;
        &mut self.data[start..end]
    }

    pub fn crop(&self, rect: IRect) -> Raster {
        let mut out = Raster::transparent(rect.width() as u32, rect.height() as u32);
        out.blit(self, IPoint::new(-rect.x0, -rect.y0));
        out
    }

    pub fn blit(&mut self, src: &Raster, at: IPoint) {
        let area = src.bounds().translate(at.x, at.y).intersect(self.bounds());
        if area.is_empty() {
            return;
        }
        for y in area.y0..area.y1 {
            let from = src.row_bytes((y - at.y) as u32, (area.x0 - at.x) as u32, (area.x1 - at.x) as u32);
            self.row_bytes_mut(y as u32, area.x0 as u32, area.x1 as u32).copy_from_slice(from);
        }
    }

    pub fn flipped_horizontally(&self) -> Raster {
        let mut out = self.clone();
        for y in 0..self.height {
            for x in 0..self.width {
                out.set(self.width - 1 - x, y, self.get(x, y));
            }
        }
        out
    }

    pub fn shifted_wrapping(&self, dx: u32, dy: u32) -> Raster {
        let mut out = self.clone();
        for y in 0..self.height {
            let target_y = (y + dy) % self.height;
            for x in 0..self.width {
                out.set((x + dx) % self.width, target_y, self.get(x, y));
            }
        }
        out
    }

    pub fn flipped_vertically(&self) -> Raster {
        let mut out = self.clone();
        for y in 0..self.height {
            let from = self.row_bytes(y, 0, self.width);
            out.row_bytes_mut(self.height - 1 - y, 0, self.width).copy_from_slice(from);
        }
        out
    }

    pub fn rotated_clockwise(&self) -> Raster {
        let mut out = Raster::transparent(self.height, self.width);
        for y in 0..self.height {
            for x in 0..self.width {
                out.set(self.height - 1 - y, x, self.get(x, y));
            }
        }
        out
    }

    pub fn rotated_counter_clockwise(&self) -> Raster {
        let mut out = Raster::transparent(self.height, self.width);
        for y in 0..self.height {
            for x in 0..self.width {
                out.set(y, self.width - 1 - x, self.get(x, y));
            }
        }
        out
    }

    fn resized_nearest(&self, width: u32, height: u32) -> Raster {
        let nearest = |index: u32, to: u32, from: u32| ((index as u64 * 2 + 1) * from as u64 / (to as u64 * 2)) as usize;
        let columns: Vec<usize> = (0..width).map(|x| nearest(x, width, self.width) * BYTES_PER_PIXEL).collect();
        let row_len = self.width as usize * BYTES_PER_PIXEL;
        let mut data = Vec::with_capacity(width as usize * height as usize * BYTES_PER_PIXEL);
        for y in 0..height {
            let start = nearest(y, height, self.height) * row_len;
            let row = &self.data[start..start + row_len];
            for &column in &columns {
                data.extend_from_slice(&row[column..column + BYTES_PER_PIXEL]);
            }
        }
        Raster { width, height, data }
    }

    pub fn resized(&self, width: u32, height: u32, resample: Resample) -> Raster {
        use image::imageops::{FilterType, resize};
        if resample == Resample::Nearest {
            return self.resized_nearest(width, height);
        }
        let filter = match resample {
            Resample::Nearest => FilterType::Nearest,
            Resample::Bilinear => FilterType::Triangle,
            Resample::Bicubic => FilterType::CatmullRom,
            Resample::Lanczos => FilterType::Lanczos3,
            Resample::Anisotropic => return self.resized_anisotropic(width, height),
        };
        let source = image::RgbaImage::from_raw(self.width, self.height, self.data.clone())
            .expect("длина буфера всегда равна размеру растра");
        let data = resize(&source, width, height, filter).into_raw();
        Raster { width, height, data }
    }

    fn resized_anisotropic(&self, width: u32, height: u32) -> Raster {
        use image::imageops::{FilterType, resize};
        let axis_filter = |to: u32, from: u32| if to < from { FilterType::Triangle } else { FilterType::CatmullRom };
        let source = image::RgbaImage::from_raw(self.width, self.height, self.data.clone()).expect("длина буфера всегда равна размеру растра");
        let across = resize(&source, width, self.height, axis_filter(width, self.width));
        let data = resize(&across, width, height, axis_filter(height, self.height)).into_raw();
        Raster { width, height, data }
    }

    pub fn rotated_half_turn(&self) -> Raster {
        self.flipped_horizontally().flipped_vertically()
    }
}

pub fn mul_div_255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    (t + (t >> 8)) >> 8
}

pub fn over(dst: Rgba, src: Rgba) -> Rgba {
    let src_alpha = src[3] as u32;
    if src_alpha == CHANNEL_MAX {
        return src;
    }
    if src_alpha == 0 {
        return dst;
    }
    let dst_alpha = dst[3] as u32;
    let dst_weight = mul_div_255(dst_alpha, CHANNEL_MAX - src_alpha);
    let out_alpha = src_alpha + dst_weight;
    let mix = |s: u8, d: u8| ((s as u32 * src_alpha + d as u32 * dst_weight + out_alpha / 2) / out_alpha) as u8;
    [mix(src[0], dst[0]), mix(src[1], dst[1]), mix(src[2], dst[2]), out_alpha as u8]
}

pub fn lerp_premultiplied(bottom: Rgba, top: Rgba, coverage: u8) -> Rgba {
    if coverage == 0 {
        return bottom;
    }
    if coverage == u8::MAX {
        return top;
    }
    let cov = coverage as u32;
    let inverse = CHANNEL_MAX - cov;
    let alpha_bottom = bottom[3] as u32;
    let alpha_top = top[3] as u32;
    let out_alpha = (alpha_bottom * inverse + alpha_top * cov + 127) / CHANNEL_MAX;
    if out_alpha == 0 {
        return TRANSPARENT;
    }
    let weighted = |b: u8, t: u8| {
        let premult = b as u32 * alpha_bottom * inverse + t as u32 * alpha_top * cov;
        ((premult + out_alpha * CHANNEL_MAX / 2) / (out_alpha * CHANNEL_MAX)).min(CHANNEL_MAX) as u8
    };
    [weighted(bottom[0], top[0]), weighted(bottom[1], top[1]), weighted(bottom[2], top[2]), out_alpha as u8]
}

pub fn erases(color: Rgba) -> bool {
    color[3] == 0
}

pub fn scale_alpha(color: Rgba, factor: u8) -> Rgba {
    [color[0], color[1], color[2], mul_div_255(color[3] as u32, factor as u32) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_source_replaces_destination() {
        assert_eq!(over([1, 2, 3, 255], [9, 8, 7, 255]), [9, 8, 7, 255]);
    }

    #[test]
    fn transparent_source_keeps_destination() {
        assert_eq!(over([1, 2, 3, 200], [9, 8, 7, 0]), [1, 2, 3, 200]);
    }

    #[test]
    fn half_red_over_opaque_blue_mixes_evenly() {
        let out = over([0, 0, 255, 255], [255, 0, 0, 128]);
        assert_eq!(out[3], 255);
        assert!((out[0] as i32 - 128).abs() <= 1);
        assert!((out[2] as i32 - 127).abs() <= 1);
    }

    #[test]
    fn over_onto_transparent_keeps_source_color() {
        assert_eq!(over(TRANSPARENT, [10, 20, 30, 100]), [10, 20, 30, 100]);
    }

    #[test]
    fn lerp_full_coverage_is_top_and_zero_is_bottom() {
        let (a, b) = ([1, 2, 3, 255], [200, 100, 50, 0]);
        assert_eq!(lerp_premultiplied(a, b, 255), b);
        assert_eq!(lerp_premultiplied(a, b, 0), a);
    }

    #[test]
    fn lerp_half_to_transparent_halves_alpha() {
        let out = lerp_premultiplied([10, 20, 30, 200], TRANSPARENT, 128);
        assert!((out[3] as i32 - 99).abs() <= 1);
        assert_eq!(&out[..3], &[10, 20, 30]);
    }

    #[test]
    fn crop_outside_is_transparent() {
        let r = Raster::filled(2, 2, [1, 1, 1, 255]);
        let c = r.crop(IRect::new(-1, -1, 1, 1));
        assert_eq!(c.get(0, 0), TRANSPARENT);
        assert_eq!(c.get(1, 1), [1, 1, 1, 255]);
    }

    #[test]
    fn wrapping_shift_brings_the_far_edge_around() {
        let mut raster = Raster::transparent(4, 3);
        raster.set(3, 2, [1, 2, 3, 255]);
        let shifted = raster.shifted_wrapping(2, 1);
        assert_eq!(shifted.get(1, 0), [1, 2, 3, 255]);
        assert_eq!(shifted.get(3, 2)[3], 0);
    }

    #[test]
    fn rotate_clockwise_moves_top_left_to_top_right() {
        let mut r = Raster::transparent(3, 2);
        r.set(0, 0, [5, 5, 5, 255]);
        let c = r.rotated_clockwise();
        assert_eq!((c.width(), c.height()), (2, 3));
        assert_eq!(c.get(1, 0), [5, 5, 5, 255]);
        assert_eq!(c.rotated_counter_clockwise(), r);
    }

    #[test]
    fn from_rgba_rejects_wrong_length() {
        assert!(matches!(Raster::from_rgba(2, 2, vec![0; 3]), Err(RasterError::SizeMismatch { .. })));
        assert!(matches!(Raster::from_rgba(0, 2, vec![]), Err(RasterError::ZeroSize)));
    }
}
