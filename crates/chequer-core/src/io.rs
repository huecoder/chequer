use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use image::codecs::bmp::BmpEncoder;
use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::codecs::ico::IcoEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngDecoder, PngEncoder};
use image::codecs::tga::TgaEncoder;
use image::codecs::tiff::TiffEncoder;
use image::codecs::webp::{WebPDecoder, WebPEncoder};
use image::{AnimationDecoder, Delay, ExtendedColorType, Frame, ImageDecoder, ImageEncoder, ImageFormat, RgbaImage};

use crate::document::{Document, DocumentError, Layer, MAX_SIDE};
use crate::frames::{self, MAX_FRAMES};
use crate::geom::IPoint;
use crate::raster::{Raster, RasterError, over};

pub const JPEG_QUALITY_MIN: u8 = 1;
pub const JPEG_QUALITY_MAX: u8 = 100;
pub const JPEG_QUALITY_DEFAULT: u8 = 95;

const OPAQUE: u8 = u8::MAX;
const OPAQUE_WHITE: [u8; 4] = [255, 255, 255, 255];
const GIF_ENCODER_SPEED: i32 = 10;

#[derive(Debug)]
pub enum IoError {
    Image(image::ImageError),
    Raster(RasterError),
    Io(std::io::Error),
    UnsupportedExtension(String),
    TooLarge { width: u32, height: u32 },
    TooManyFrames { count: usize },
    Document(DocumentError),
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image(e) => write!(f, "{e}"),
            Self::Raster(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::UnsupportedExtension(ext) => write!(f, "формат «{ext}» не поддерживается"),
            Self::TooLarge { width, height } => {
                write!(f, "изображение {width}×{height} больше предела {MAX_SIDE} пикселей по стороне")
            }
            Self::TooManyFrames { count } => write!(f, "в анимации {count} кадров, предел {MAX_FRAMES}"),
            Self::Document(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for IoError {}

impl From<image::ImageError> for IoError {
    fn from(e: image::ImageError) -> Self {
        Self::Image(e)
    }
}

impl From<std::io::Error> for IoError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngColor {
    Auto,
    Rgba32,
    Rgb24,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngCompression {
    Fast,
    Default,
    Best,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveOptions {
    Png { color: PngColor, compression: PngCompression },
    Jpeg { quality: u8 },
    Bmp { alpha: bool },
    Tga { alpha: bool },
    Gif,
    WebpLossless,
    Tiff,
    Ico,
    Chq { history: bool },
    Ora,
}

impl SaveOptions {
    pub const EXTENSIONS: [&'static str; 11] = ["png", "jpg", "jpeg", "bmp", "tga", "gif", "webp", "tif", "ico", "chq", "ora"];

    pub fn for_extension(extension: &str) -> Option<SaveOptions> {
        Some(match extension.to_ascii_lowercase().as_str() {
            "png" => SaveOptions::Png { color: PngColor::Auto, compression: PngCompression::Default },
            "jpg" | "jpeg" | "jpe" | "jfif" => SaveOptions::Jpeg { quality: JPEG_QUALITY_DEFAULT },
            "bmp" => SaveOptions::Bmp { alpha: true },
            "tga" => SaveOptions::Tga { alpha: true },
            "gif" => SaveOptions::Gif,
            "webp" => SaveOptions::WebpLossless,
            "tif" | "tiff" => SaveOptions::Tiff,
            "ico" => SaveOptions::Ico,
            crate::chq::FILE_EXTENSION => SaveOptions::Chq { history: false },
            crate::ora::FILE_EXTENSION => SaveOptions::Ora,
            _ => return None,
        })
    }

    pub fn extension(&self) -> &'static str {
        match self {
            SaveOptions::Png { .. } => "png",
            SaveOptions::Jpeg { .. } => "jpg",
            SaveOptions::Bmp { .. } => "bmp",
            SaveOptions::Tga { .. } => "tga",
            SaveOptions::Gif => "gif",
            SaveOptions::WebpLossless => "webp",
            SaveOptions::Tiff => "tif",
            SaveOptions::Ico => "ico",
            SaveOptions::Chq { .. } => crate::chq::FILE_EXTENSION,
            SaveOptions::Ora => crate::ora::FILE_EXTENSION,
        }
    }

    pub fn keeps_layers(&self) -> bool {
        matches!(self, SaveOptions::Chq { .. } | SaveOptions::Ora)
    }

    pub fn keeps_frames(&self) -> bool {
        matches!(self, SaveOptions::Chq { .. } | SaveOptions::Gif)
    }

}

pub fn load_raster(path: &Path) -> Result<Raster, IoError> {
    let image = image::open(path)?.into_rgba8();
    let (width, height) = image.dimensions();
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(IoError::TooLarge { width, height });
    }
    Raster::from_rgba(width, height, image.into_raw()).map_err(IoError::Raster)
}

pub fn decode(bytes: &[u8]) -> Result<Raster, IoError> {
    let image = image::load_from_memory(bytes)?.into_rgba8();
    let (width, height) = image.dimensions();
    Raster::from_rgba(width, height, image.into_raw()).map_err(IoError::Raster)
}

pub fn load_document(path: &Path, layer_name: String) -> Result<Document, IoError> {
    let bytes = std::fs::read(path)?;
    let frames = decode_frames(&bytes)?;
    if frames.len() > MAX_FRAMES {
        return Err(IoError::TooManyFrames { count: frames.len() });
    }
    let durations = frames.iter().map(|(_, duration_ms)| frames::Frame::clamped(*duration_ms)).collect();
    let mut cels: Vec<Arc<Raster>> = Vec::with_capacity(frames.len());
    for (raster, _) in frames {
        let repeat = cels.last().filter(|previous| ***previous == raster).map(Arc::clone);
        cels.push(repeat.unwrap_or_else(|| Arc::new(raster)));
    }
    let layer = Layer::with_cels(layer_name, cels);
    Document::from_frames(vec![layer], durations, 0).map_err(IoError::Document)
}

fn decode_frames(bytes: &[u8]) -> Result<Vec<(Raster, u32)>, IoError> {
    let cursor = Cursor::new(bytes);
    let animated = match image::guess_format(bytes) {
        Ok(ImageFormat::Gif) => {
            let decoder = GifDecoder::new(cursor)?;
            let dimensions = decoder.dimensions();
            Some(collect_frames(decoder, dimensions)?)
        }
        Ok(ImageFormat::WebP) => {
            let decoder = WebPDecoder::new(cursor)?;
            let dimensions = decoder.dimensions();
            if decoder.has_animation() { Some(collect_frames(decoder, dimensions)?) } else { None }
        }
        Ok(ImageFormat::Png) => {
            let decoder = PngDecoder::new(cursor)?;
            let dimensions = decoder.dimensions();
            if decoder.is_apng()? { Some(collect_frames(decoder.apng()?, dimensions)?) } else { None }
        }
        _ => None,
    };
    match animated {
        Some(frames) if !frames.is_empty() => Ok(frames),
        _ => Ok(vec![(decode(bytes)?, frames::DEFAULT_FRAME_DURATION_MS)]),
    }
}

fn collect_frames<'a>(decoder: impl AnimationDecoder<'a>, (width, height): (u32, u32)) -> Result<Vec<(Raster, u32)>, IoError> {
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(IoError::TooLarge { width, height });
    }
    let mut collected = Vec::new();
    for frame in decoder.into_frames() {
        let frame = frame?;
        let (left, top) = (frame.left(), frame.top());
        let (numer, denom) = frame.delay().numer_denom_ms();
        let duration_ms = if numer == 0 { frames::DEFAULT_FRAME_DURATION_MS } else { numer / denom.max(1) };
        let buffer = frame.into_buffer();
        let (frame_width, frame_height) = buffer.dimensions();
        let image = Raster::from_rgba(frame_width, frame_height, buffer.into_raw()).map_err(IoError::Raster)?;
        let canvas = if (frame_width, frame_height) == (width, height) && left == 0 && top == 0 {
            image
        } else {
            let mut canvas = Raster::transparent(width, height);
            canvas.blit(&image, IPoint::new(left as i32, top as i32));
            canvas
        };
        collected.push((canvas, duration_ms));
        if collected.len() > MAX_FRAMES {
            return Err(IoError::TooManyFrames { count: collected.len() });
        }
    }
    Ok(collected)
}

pub fn encode_document(document: &Document, options: &SaveOptions) -> Result<Vec<u8>, IoError> {
    match options {
        SaveOptions::Gif if document.frame_count() > 1 => encode_gif_animation(document),
        _ => encode(&document.flattened(document.bounds()), options),
    }
}

fn encode_gif_animation(document: &Document) -> Result<Vec<u8>, IoError> {
    let mut out = Vec::new();
    let mut encoder = GifEncoder::new_with_speed(&mut out, GIF_ENCODER_SPEED);
    encoder.set_repeat(Repeat::Infinite)?;
    for (index, frame) in document.frames().iter().enumerate() {
        let image = rgba_image(&document.flattened_frame(index, document.bounds()))?;
        encoder.encode_frame(Frame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(frame.duration_ms, 1)))?;
    }
    drop(encoder);
    Ok(out)
}

pub fn save_document(path: &Path, document: &Document, options: &SaveOptions) -> Result<(), IoError> {
    std::fs::write(path, encode_document(document, options)?)?;
    Ok(())
}

pub fn save_raster(path: &Path, raster: &Raster, options: &SaveOptions) -> Result<(), IoError> {
    std::fs::write(path, encode(raster, options)?)?;
    Ok(())
}

pub fn encode(raster: &Raster, options: &SaveOptions) -> Result<Vec<u8>, IoError> {
    let (width, height) = (raster.width(), raster.height());
    let mut out = Vec::new();
    match *options {
        SaveOptions::Png { color, compression } => encode_png(&mut out, raster, color, compression)?,
        SaveOptions::Jpeg { quality } => {
            let rgb = rgb_bytes(raster);
            JpegEncoder::new_with_quality(&mut out, quality.clamp(JPEG_QUALITY_MIN, JPEG_QUALITY_MAX))
                .write_image(&rgb, width, height, ExtendedColorType::Rgb8)?;
        }
        SaveOptions::Bmp { alpha } => {
            let encoder = BmpEncoder::new(&mut out);
            write_color_or_rgb(raster, alpha, |bytes, kind| encoder.write_image(bytes, width, height, kind))?;
        }
        SaveOptions::Tga { alpha } => {
            let encoder = TgaEncoder::new(&mut out);
            write_color_or_rgb(raster, alpha, |bytes, kind| encoder.write_image(bytes, width, height, kind))?;
        }
        SaveOptions::Gif => {
            let image = rgba_image(raster)?;
            let mut encoder = GifEncoder::new_with_speed(&mut out, GIF_ENCODER_SPEED);
            encoder.encode_frame(Frame::new(image))?;
        }
        SaveOptions::WebpLossless => {
            WebPEncoder::new_lossless(&mut out).write_image(raster.as_bytes(), width, height, ExtendedColorType::Rgba8)?;
        }
        SaveOptions::Tiff => {
            let mut cursor = Cursor::new(Vec::new());
            TiffEncoder::new(&mut cursor).write_image(raster.as_bytes(), width, height, ExtendedColorType::Rgba8)?;
            out = cursor.into_inner();
        }
        SaveOptions::Ico => {
            IcoEncoder::new(&mut out).write_image(raster.as_bytes(), width, height, ExtendedColorType::Rgba8)?;
        }
        SaveOptions::Chq { history } => {
            let document = Document::from_raster(String::new(), raster.clone());
            out = crate::chq::encode_document(&document, crate::chq::WriteMode::Small, history).map_err(|error| IoError::Io(std::io::Error::other(error.to_string())))?;
        }
        SaveOptions::Ora => out = crate::ora::encode_document(&Document::from_raster(String::new(), raster.clone()))?,
    }
    Ok(out)
}

fn encode_png(out: &mut Vec<u8>, raster: &Raster, color: PngColor, compression: PngCompression) -> Result<(), IoError> {
    let (width, height) = (raster.width(), raster.height());
    let level = match compression {
        PngCompression::Fast => CompressionType::Fast,
        PngCompression::Default => CompressionType::Default,
        PngCompression::Best => CompressionType::Best,
    };
    let encoder = PngEncoder::new_with_quality(out, level, FilterType::Adaptive);
    let has_alpha = raster.as_bytes().chunks_exact(4).any(|p| p[3] != OPAQUE);
    let is_gray = raster.as_bytes().chunks_exact(4).all(|p| p[0] == p[1] && p[1] == p[2]);
    match (color, has_alpha, is_gray) {
        (PngColor::Auto, false, true) => {
            let gray: Vec<u8> = raster.as_bytes().chunks_exact(4).map(|p| p[0]).collect();
            encoder.write_image(&gray, width, height, ExtendedColorType::L8)?;
        }
        (PngColor::Auto, true, true) => {
            let gray_alpha: Vec<u8> = raster.as_bytes().chunks_exact(4).flat_map(|p| [p[0], p[3]]).collect();
            encoder.write_image(&gray_alpha, width, height, ExtendedColorType::La8)?;
        }
        (PngColor::Auto, false, false) | (PngColor::Rgb24, _, _) => {
            encoder.write_image(&rgb_bytes(raster), width, height, ExtendedColorType::Rgb8)?;
        }
        (PngColor::Auto, true, false) | (PngColor::Rgba32, _, _) => {
            encoder.write_image(raster.as_bytes(), width, height, ExtendedColorType::Rgba8)?;
        }
    }
    Ok(())
}

fn write_color_or_rgb(
    raster: &Raster,
    alpha: bool,
    write: impl FnOnce(&[u8], ExtendedColorType) -> image::ImageResult<()>,
) -> Result<(), IoError> {
    if alpha {
        write(raster.as_bytes(), ExtendedColorType::Rgba8)?;
    } else {
        write(&rgb_bytes(raster), ExtendedColorType::Rgb8)?;
    }
    Ok(())
}

fn rgb_bytes(raster: &Raster) -> Vec<u8> {
    raster
        .as_bytes()
        .chunks_exact(4)
        .flat_map(|p| {
            let flat = over(OPAQUE_WHITE, [p[0], p[1], p[2], p[3]]);
            [flat[0], flat[1], flat[2]]
        })
        .collect()
}

fn rgba_image(raster: &Raster) -> Result<RgbaImage, IoError> {
    RgbaImage::from_raw(raster.width(), raster.height(), raster.as_bytes().to_vec())
        .ok_or(IoError::Raster(RasterError::ZeroSize))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Raster {
        let mut raster = Raster::filled(16, 16, [10, 120, 200, 255]);
        raster.set(3, 3, [250, 20, 20, 128]);
        raster
    }

    fn round_trip(options: SaveOptions) -> Raster {
        let dir = std::env::temp_dir().join("chequer-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("round.{}", options.extension()));
        save_raster(&path, &sample(), &options).unwrap();
        load_raster(&path).unwrap()
    }

    #[test]
    fn png_auto_round_trip_keeps_pixels() {
        let options = SaveOptions::for_extension("png").unwrap();
        assert_eq!(round_trip(options), sample());
    }

    #[test]
    fn png_auto_picks_gray_for_gray_images() {
        let gray = Raster::filled(32, 32, [90, 90, 90, 255]);
        let auto = encode(&gray, &SaveOptions::Png { color: PngColor::Auto, compression: PngCompression::Default }).unwrap();
        let rgba = encode(&gray, &SaveOptions::Png { color: PngColor::Rgba32, compression: PngCompression::Default }).unwrap();
        assert!(auto.len() < rgba.len());
    }

    #[test]
    fn jpeg_quality_changes_size() {
        let mut noisy = Raster::transparent(64, 64);
        for y in 0..64u32 {
            for x in 0..64u32 {
                noisy.set(x, y, [(x * 37 + y * 11) as u8, (x * 5 + y * 91) as u8, (x * y) as u8, 255]);
            }
        }
        let low = encode(&noisy, &SaveOptions::Jpeg { quality: 10 }).unwrap();
        let high = encode(&noisy, &SaveOptions::Jpeg { quality: 100 }).unwrap();
        assert!(low.len() < high.len());
    }

    #[test]
    fn lossless_formats_round_trip() {
        for ext in ["bmp", "tga", "webp", "tif"] {
            let options = SaveOptions::for_extension(ext).unwrap();
            assert_eq!(round_trip(options), sample(), "формат {ext}");
        }
    }

    #[test]
    fn rgb_variants_flatten_alpha_on_white() {
        let transparent = Raster::transparent(2, 2);
        let bytes = encode(&transparent, &SaveOptions::Bmp { alpha: false }).unwrap();
        let dir = std::env::temp_dir().join("chequer-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("white.bmp");
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(load_raster(&path).unwrap().get(0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn animated_gif_round_trips_frame_count_and_durations() {
        let blue = Raster::filled(16, 16, [0, 0, 255, 255]);
        let red = Raster::filled(16, 16, [255, 0, 0, 255]);
        let layer = Layer::with_cels("a".into(), vec![Arc::new(blue.clone()), Arc::new(red), Arc::new(blue)]);
        let frames = vec![frames::Frame { duration_ms: 50 }, frames::Frame { duration_ms: 200 }, frames::Frame { duration_ms: 90 }];
        let document = Document::from_frames(vec![layer], frames, 0).unwrap();
        let path = std::env::temp_dir().join("chequer-io-test").join("animation.gif");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, encode_document(&document, &SaveOptions::Gif).unwrap()).unwrap();
        let loaded = load_document(&path, String::new()).unwrap();
        assert_eq!(loaded.frame_count(), 3);
        assert_eq!(loaded.frames().iter().map(|frame| frame.duration_ms).collect::<Vec<_>>(), [50, 200, 90]);
        assert_ne!(loaded.layers()[0].cel(0), loaded.layers()[0].cel(1));
        assert_eq!(loaded.layers()[0].cel(0), loaded.layers()[0].cel(2));
    }

    #[test]
    fn static_images_open_as_one_frame() {
        let path = std::env::temp_dir().join("chequer-io-test").join("static.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        save_raster(&path, &sample(), &SaveOptions::for_extension("png").unwrap()).unwrap();
        let loaded = load_document(&path, String::new()).unwrap();
        assert_eq!(loaded.frame_count(), 1);
        assert_eq!(loaded.layers()[0].raster(), &sample());
    }

    #[test]
    fn unknown_extension_has_no_options() {
        assert!(SaveOptions::for_extension("xyz").is_none());
    }
}
