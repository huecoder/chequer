use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use eframe::egui::{self, ColorImage, Context, TextureHandle, TextureOptions};
use resvg::{tiny_skia, usvg};

use crate::skin;

macro_rules! vector_icons {
    ($($variant:ident => $file:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Icon { $($variant,)* Print, Grid, Contrast, Dot, Swap }

        const VECTORS: &[(Icon, &str, &str)] = &[
            $((Icon::$variant, $file, include_str!(concat!("../assets/icons/", $file, ".svg"))),)*
        ];
    };
}

vector_icons! {
    Pencil => "pencil", Brush => "brush", Bucket => "bucket", Zoom => "search", Wand => "magic_wand",
    MovePixels => "move_all", MoveSelection => "selection_move", Rect => "selection_rect", Shapes => "shapes",
    New => "document_new", Open => "open", Save => "save", SaveAll => "save",
    Cut => "cut", Copy => "copy", Paste => "paste", Undo => "undo", Redo => "redo",
    Crop => "crop", Deselect => "selection_delete", ToolsWindow => "hammer",
    HistoryWindow => "history", LayersWindow => "layers", ColorsWindow => "color_wheel", SettingsWindow => "settings",
    Add => "layer_add", Delete => "trash", LayerRemove => "layer_remove", Merge => "layer_merge", Up => "arrow_up", Down => "arrow_down",
    Eye => "eye", Clock => "history", Close => "error", Tick => "ok", Properties => "layer_properties",
    Duplicate => "layers", Picture => "image", Resize => "resize_all", RotateCw => "turn_right",
    RotateCcw => "turn_left", Refresh => "refresh", PasteLayer => "layer_add",
    PasteImage => "image_add", Info => "info", Help => "help", Palette => "palette",
    ZoomIn => "zoom_in", ZoomOut => "zoom_out", Group => "folder", Warning => "warning", ErrorIcon => "error",
    Accept => "ok", Star => "star", StarEmpty => "star_dark",
    Ellipse => "selection_ellipse", Lasso => "selection_lasso", Eraser => "eraser", Picker => "picker",
    Pan => "pan", Move => "move_all", Gradient => "gradient", CloneStamp => "clone_stamp",
    Recolor => "recolor", TextTool => "text", ShapesTool => "shapes", Line => "path", FlipHorizontal => "resize_horizontal", FlipVertical => "resize_vertical", Exit => "power", Lock => "lock",
    SelectReplace => "selection_rect", SelectAdd => "selection_union", SelectSubtract => "selection_subtract",
    SelectIntersect => "selection_intersect", SelectXor => "selection_xor",
    Play => "play", Pause => "pause", Timeline => "video",
    FirstFrame => "arrow_left_double", PrevFrame => "arrow_left", NextFrame => "arrow_right", LastFrame => "arrow_right_double",
    EffectSepia => "effect_sepia", EffectMonochrome => "effect_monochrome", EffectBrightnessContrast => "effect_brightness_contrast",
    EffectPosterize => "effect_posterize", EffectHueSaturation => "effect_hue_saturation", EffectInvert => "effect_invert", EffectCurves => "effect_curves",
    PixelGrid => "pixel_grid", CanvasSize => "canvas_size", ResizeImage => "resize_image", SelectionInvert => "selection_invert", SeamlessTiling => "seamless_tiling", RecolorWheel => "recolor_wheel", BrushAdd => "brush_add", SelectionAreaPaste => "selection_area_paste",
    FilmAdd => "film_add", FilmRemove => "film_remove", OnionSkin => "onion_skin", EmptyFrame => "layer_empty", SelectAll => "select_all",
}

const DRAWN_ICONS: [Icon; 5] = [Icon::Print, Icon::Grid, Icon::Contrast, Icon::Dot, Icon::Swap];

pub const ICON_SIZE: usize = 16;
const PLACEHOLDER_INK_CHAR: char = '#';

const PLACEHOLDER_PALETTE: [(char, [u8; 4]); 16] = [
    ('B', [0x2d, 0x4f, 0x8f, 255]),
    ('l', [0x9c, 0xc0, 0xf0, 255]),
    ('G', [0x3f, 0x8f, 0x52, 255]),
    ('e', [0x8f, 0xd0, 0x9a, 255]),
    ('R', [0x8f, 0x2d, 0x2d, 255]),
    ('o', [0xe0, 0x9a, 0x40, 255]),
    ('d', [0x5a, 0x5a, 0x5a, 255]),
    ('L', [0xd0, 0xd0, 0xd0, 255]),
    ('p', [0xb0, 0x60, 0xc0, 255]),
    ('m', [0x7a, 0x70, 0xd8, 255]),
    ('c', [0x4a, 0xb0, 0xd0, 255]),
    ('w', [0xf0, 0xf0, 0xf0, 255]),
    ('g', [0x9a, 0x9a, 0x9a, 255]),
    ('r', [0xd0, 0x5a, 0x5a, 255]),
    ('b', [0x4a, 0x7a, 0xc8, 255]),
    ('y', [0xe0, 0xc0, 0x50, 255]),
];

fn placeholder(icon: Icon) -> Option<&'static str> {
    Some(match icon {
        Icon::Print => "\
................
....########....
....#wwwwww#....
....#wwwwww#....
..############..
.#gggggggggggg#.
.#ggggggggggGg#.
.#gggggggggggg#.
..############..
....#wwwwww#....
....#wddddw#....
....#wwwwww#....
....#wddddw#....
....########....
................
................",
        Icon::Grid => "\
................
.#############..
.#...#...#...#..
.#...#...#...#..
.#...#...#...#..
.#############..
.#...#...#...#..
.#...#...#...#..
.#...#...#...#..
.#############..
.#...#...#...#..
.#...#...#...#..
.#...#...#...#..
.#############..
................
................",
        Icon::Contrast => "\
................
.....###ww#.....
...#####wwww#...
..######wwwww#..
.#######wwwwww#.
.#######wwwwww#.
.#######wwwwww#.
.#######wwwwww#.
.#######wwwwww#.
.#######wwwwww#.
.#######wwwwww#.
.#######wwwwww#.
..######wwwww#..
...#####wwww#...
.....###ww#.....
................",
        Icon::Dot => "\
................
................
................
................
................
................
......####......
.....#wwww#.....
.....#wwww#.....
......####......
................
................
................
................
................
................",
        Icon::Swap => "\
................
................
................
..........#.....
..........##....
..############..
..........##....
..........#.....
................
.....#..........
....##..........
..############..
....##..........
.....#..........
................
................",
        _ => return None,
    })
}

fn decode_png(bytes: &[u8]) -> Option<ColorImage> {
    let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?.into_rgba8();
    (decoded.width() == decoded.height()).then(|| ColorImage::from_rgba_unmultiplied([decoded.width() as usize, decoded.height() as usize], decoded.as_raw()))
}

fn draw_placeholder(art: &str) -> ColorImage {
    let mut rgba = vec![0u8; ICON_SIZE * ICON_SIZE * 4];
    for (y, line) in art.lines().enumerate().take(ICON_SIZE) {
        for (x, ch) in line.chars().enumerate().take(ICON_SIZE) {
            let color = if ch == PLACEHOLDER_INK_CHAR {
                Some(skin::theme().ink.to_array())
            } else {
                PLACEHOLDER_PALETTE.iter().find(|(c, _)| *c == ch).map(|(_, color)| *color)
            };
            if let Some(color) = color {
                rgba[(y * ICON_SIZE + x) * 4..][..4].copy_from_slice(&color);
            }
        }
    }
    ColorImage::from_rgba_unmultiplied([ICON_SIZE, ICON_SIZE], &rgba)
}

enum Source {
    Vector(&'static str),
    Raster(ColorImage),
}

const ICON_SCALE_PERCENT: f32 = 120.0;
const RENDER_RESOLUTION_PERCENT: f32 = 200.0;
const HIGH_CONTRAST_PERCENT: f32 = 200.0;
const PERCENT: f32 = 100.0;
const CHANNELS: usize = 4;
const ALPHA: usize = 3;
const MAX_CHANNEL: f32 = 255.0;
const WHOLE_SCALE_TOLERANCE: f32 = 0.12;

static HIGH_CONTRAST: AtomicBool = AtomicBool::new(false);

pub fn set_high_contrast(enabled: bool) {
    HIGH_CONTRAST.store(enabled, Ordering::Relaxed);
}

pub fn icon_side() -> f32 {
    ICON_SIZE as f32 * ICON_SCALE_PERCENT / PERCENT
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TextureKey {
    Smooth,
    Sharp,
    Rendered { pixels: u32, high_contrast: bool, nearest: bool },
}

#[derive(Default)]
struct Cache {
    trees: HashMap<Icon, usvg::Tree>,
    textures: HashMap<(Icon, TextureKey), TextureHandle>,
}

pub struct Icons {
    ctx: Context,
    sources: HashMap<Icon, Source>,
    cache: Mutex<Cache>,
}

fn lock(cache: &Mutex<Cache>) -> MutexGuard<'_, Cache> {
    cache.lock().unwrap_or_else(PoisonError::into_inner)
}

fn fit_transform(tree: &usvg::Tree, pixels: u32) -> tiny_skia::Transform {
    let scale = pixels as f32 / tree.size().width();
    let bounds = tree.root().abs_stroke_bounding_box();
    let extent = bounds.width().max(bounds.height()) * scale;
    let fitted = scale * (extent.round().clamp(1.0, pixels as f32) / extent);
    let left = ((pixels as f32 - bounds.width() * fitted) / 2.0).floor() - bounds.x() * fitted;
    let top = ((pixels as f32 - bounds.height() * fitted) / 2.0).floor() - bounds.y() * fitted;
    tiny_skia::Transform::from_scale(fitted, fitted).post_translate(left, top)
}

fn raise_contrast(data: &mut [u8]) {
    let gain = HIGH_CONTRAST_PERCENT / PERCENT;
    let middle = MAX_CHANNEL / 2.0;
    for pixel in data.chunks_exact_mut(CHANNELS) {
        let alpha = pixel[ALPHA] as f32;
        if alpha == 0.0 {
            continue;
        }
        for channel in &mut pixel[..ALPHA] {
            let straight = *channel as f32 * MAX_CHANNEL / alpha;
            let adjusted = ((straight - middle) * gain + middle).clamp(0.0, MAX_CHANNEL);
            *channel = (adjusted * alpha / MAX_CHANNEL) as u8;
        }
    }
}

fn render_tree(tree: &usvg::Tree, pixels: u32, high_contrast: bool) -> ColorImage {
    let mut pixmap = tiny_skia::Pixmap::new(pixels, pixels).expect("размер иконки не нулевой");
    resvg::render(tree, fit_transform(tree, pixels), &mut pixmap.as_mut());
    let mut data = pixmap.take();
    if high_contrast {
        raise_contrast(&mut data);
    }
    ColorImage::from_rgba_premultiplied([pixels as usize; 2], &data)
}

impl Icons {
    pub fn load(ctx: &Context, override_dir: Option<&Path>) -> Self {
        let themed = |name: &str| override_dir.and_then(|dir| std::fs::read(dir.join(format!("{name}.png"))).ok()).and_then(|bytes| decode_png(&bytes));
        let mut sources = HashMap::new();
        for (icon, file, svg) in VECTORS {
            let source = themed(file).map_or(Source::Vector(svg), Source::Raster);
            sources.insert(*icon, source);
        }
        for icon in DRAWN_ICONS {
            let image = themed(&format!("{icon:?}").to_lowercase()).or_else(|| placeholder(icon).map(draw_placeholder));
            if let Some(image) = image {
                sources.insert(icon, Source::Raster(image));
            }
        }
        Self { ctx: ctx.clone(), sources, cache: Mutex::new(Cache::default()) }
    }

    fn is_sharp(&self, side: f32, native: usize) -> bool {
        let scale = side * self.ctx.pixels_per_point() / native as f32;
        (scale - scale.round().max(1.0)).abs() < WHOLE_SCALE_TOLERANCE
    }

    fn rendered_pixels(&self, side: f32) -> u32 {
        ((side * skin::grid_ppp(self.ctx.pixels_per_point()) * RENDER_RESOLUTION_PERCENT / PERCENT).round() as u32).max(1)
    }

    fn key_for(&self, icon: Icon, side: f32) -> TextureKey {
        match &self.sources[&icon] {
            Source::Vector(_) => TextureKey::Rendered { pixels: self.rendered_pixels(side), high_contrast: HIGH_CONTRAST.load(Ordering::Relaxed), nearest: skin::pixel_mode() },
            Source::Raster(image) if self.is_sharp(side, image.width()) => TextureKey::Sharp,
            Source::Raster(_) => TextureKey::Smooth,
        }
    }

    fn build_texture(&self, cache: &mut Cache, icon: Icon, key: TextureKey) -> TextureHandle {
        let name = format!("{icon:?}-{key:?}");
        match (&self.sources[&icon], key) {
            (Source::Vector(svg), TextureKey::Rendered { pixels, high_contrast, nearest }) => {
                let tree = cache.trees.entry(icon).or_insert_with(|| usvg::Tree::from_str(svg, &usvg::Options::default()).expect("встроенная иконка - корректный SVG"));
                let options = if nearest { TextureOptions::NEAREST } else { TextureOptions::LINEAR };
                self.ctx.load_texture(name, render_tree(tree, pixels, high_contrast), options)
            }
            (Source::Raster(image), TextureKey::Smooth) => self.ctx.load_texture(name, image.clone(), TextureOptions::LINEAR),
            (Source::Raster(image), _) => self.ctx.load_texture(name, image.clone(), TextureOptions::NEAREST),
            (Source::Vector(_), _) => unreachable!("векторная иконка всегда рендерится в целевой размер"),
        }
    }

    pub fn texture_id(&self, icon: Icon, side: f32) -> egui::TextureId {
        let key = self.key_for(icon, side);
        let mut cache = lock(&self.cache);
        if let Some(texture) = cache.textures.get(&(icon, key)) {
            return texture.id();
        }
        let texture = self.build_texture(&mut cache, icon, key);
        let id = texture.id();
        cache.textures.insert((icon, key), texture);
        id
    }

    pub fn clear_rendered(&self) {
        lock(&self.cache).textures.retain(|(_, key), _| !matches!(key, TextureKey::Rendered { .. }));
    }

    pub fn image_sized(&self, icon: Icon, side: f32) -> egui::Image<'static> {
        egui::Image::new(egui::load::SizedTexture::new(self.texture_id(icon, side), egui::vec2(side, side)))
    }

    pub fn image(&self, icon: Icon) -> egui::Image<'static> {
        self.image_sized(icon, icon_side())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PIXELS: u32 = 32;

    #[test]
    fn every_embedded_svg_parses_and_renders() {
        for (icon, file, svg) in VECTORS {
            let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap_or_else(|error| panic!("{file}: {error}"));
            let image = render_tree(&tree, TEST_PIXELS, true);
            assert!(image.pixels.iter().any(|pixel| pixel.a() > 0), "{icon:?} ({file}) отрисовалась пустой");
        }
    }
}
