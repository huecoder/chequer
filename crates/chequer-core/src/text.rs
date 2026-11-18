
use std::collections::HashMap;
use std::sync::Arc;

use harfrust::{ShaperData, UnicodeBuffer};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

use crate::blend::BlendMode;
use crate::geom::{FPoint, IPoint, IRect};
use crate::raster::{Raster, Rgba};
use crate::shapes::{Overlay, ShapeStyle, StrokeAlign, render_path};

const SYNTHETIC_ITALIC_SLANT: f32 = 0.2126;
const SYNTHETIC_BOLD_STROKE: f32 = 1.0 / 22.0;
const FALLBACK_UNDERLINE_OFFSET: f32 = -0.12;
const FALLBACK_STRIKEOUT_OFFSET: f32 = 0.3;
const FALLBACK_DECORATION_THICKNESS: f32 = 0.07;
const LCD_SUBPIXELS: usize = 3;
const LCD_FILTER: [u32; 5] = [0x08, 0x4D, 0x56, 0x4D, 0x08];
const LCD_FILTER_SUM: u32 = 256;
const LCD_FILTER_REACH: usize = 2;
const RENDER_MARGIN: i32 = 2;

#[derive(Debug)]
pub enum TextError {
    FontParse(String),
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FontParse(reason) => write!(f, "шрифт не читается: {reason}"),
        }
    }
}

impl std::error::Error for TextError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSmoothing {
    None,
    Grayscale,
    LcdRgb,
    LcdBgr,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl TextAlign {
    fn leading_share(self) -> f32 {
        match self {
            Self::Left => 0.0,
            Self::Center => 0.5,
            Self::Right => 1.0,
        }
    }
}

impl TextSmoothing {
    fn is_lcd(self) -> bool {
        matches!(self, Self::LcdRgb | Self::LcdBgr)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub synthetic_bold: bool,
    pub synthetic_italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub hinting: bool,
    pub smoothing: TextSmoothing,
    pub align: TextAlign,
    pub outline: Option<Rgba>,
    pub fill: Option<Rgba>,
    pub outline_width: f32,
    pub outline_align: StrokeAlign,
}

#[derive(Clone)]
pub struct FontFace {
    data: Arc<[u8]>,
    index: u32,
}

impl FontFace {
    pub fn new(data: Arc<[u8]>, index: u32) -> Result<Self, TextError> {
        FontRef::from_index(&data, index).map_err(|e| TextError::FontParse(e.to_string()))?;
        Ok(Self { data, index })
    }

    fn font(&self) -> FontRef<'_> {
        FontRef::from_index(&self.data, self.index).expect("шрифт проверен в FontFace::new")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretStop {
    pub x: f32,
    pub top: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug)]
struct PlacedGlyph {
    id: GlyphId,
    x: f32,
    baseline: f32,
}

#[derive(Clone, Copy, Debug)]
struct LineSpan {
    x_start: f32,
    x_end: f32,
    baseline: f32,
}

#[derive(Clone, Debug)]
pub struct Layout {
    glyphs: Vec<PlacedGlyph>,
    lines: Vec<LineSpan>,
    stops: Vec<CaretStop>,
    line_height: f32,
    ascent: f32,
    underline: (f32, f32),
    strikeout: (f32, f32),
    pub width: f32,
    pub height: f32,
}

struct NullPen;

impl OutlinePen for NullPen {
    fn move_to(&mut self, _: f32, _: f32) {}
    fn line_to(&mut self, _: f32, _: f32) {}
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
    fn close(&mut self) {}
}

struct GlyphPen<'a> {
    builder: &'a mut PathBuilder,
    origin: FPoint,
    slant: f32,
}

impl GlyphPen<'_> {
    fn map(&self, x: f32, y: f32) -> (f32, f32) {
        (self.origin.x + x + self.slant * y, self.origin.y - y)
    }
}

impl OutlinePen for GlyphPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.map(x, y);
        self.builder.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.map(x, y);
        self.builder.line_to(x, y);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let ((cx, cy), (x, y)) = (self.map(cx, cy), self.map(x, y));
        self.builder.quad_to(cx, cy, x, y);
    }

    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let ((c1x, c1y), (c2x, c2y), (x, y)) = (self.map(c1x, c1y), self.map(c2x, c2y), self.map(x, y));
        self.builder.cubic_to(c1x, c1y, c2x, c2y, x, y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

fn hinting_target(smoothing: TextSmoothing) -> Target {
    match smoothing {
        TextSmoothing::None => Target::Mono,
        TextSmoothing::Grayscale => SmoothMode::Normal.into(),
        TextSmoothing::LcdRgb | TextSmoothing::LcdBgr => SmoothMode::Lcd.into(),
    }
}

fn hinting_instance(font: &FontRef, style: &TextStyle) -> Option<HintingInstance> {
    if !style.hinting {
        return None;
    }
    let options = HintingOptions { engine: Engine::AutoFallback, target: hinting_target(style.smoothing) };
    HintingInstance::new(&font.outline_glyphs(), Size::new(style.size), LocationRef::default(), options).ok()
}

fn draw_settings<'a>(style: &TextStyle, hinter: Option<&'a HintingInstance>) -> DrawSettings<'a> {
    match hinter {
        Some(hinter) => DrawSettings::hinted(hinter, false),
        None => DrawSettings::unhinted(Size::new(style.size), LocationRef::default()),
    }
}

impl Layout {
    pub fn build(face: &FontFace, text: &str, style: &TextStyle) -> Self {
        let font = face.font();
        let metrics = font.metrics(Size::new(style.size), LocationRef::default());
        let hinter = hinting_instance(&font, style);
        let snap = |value: f32| if hinter.is_some() { value.round() } else { value };
        let ascent = snap(metrics.ascent);
        let line_height = snap(metrics.ascent - metrics.descent + metrics.leading).max(1.0);
        let decoration = |decoration: Option<skrifa::metrics::Decoration>, fallback_offset: f32| {
            let (offset, thickness) = decoration
                .map(|d| (d.offset, d.thickness))
                .unwrap_or((fallback_offset * style.size, FALLBACK_DECORATION_THICKNESS * style.size));
            (snap(offset), snap(thickness).max(1.0))
        };
        let shaper_data = ShaperData::new(&font);
        let shaper = shaper_data.shaper(&font).build();
        let units_per_em = f32::from(metrics.units_per_em.max(1));
        let scale = style.size / units_per_em;
        let outlines = font.outline_glyphs();
        let linear_advances = font.glyph_metrics(Size::new(style.size), LocationRef::default());
        let mut hinted_advances: HashMap<GlyphId, f32> = HashMap::new();
        let mut glyphs = Vec::new();
        let mut lines = Vec::new();
        let mut stops = Vec::new();
        let mut width = 0.0f32;
        let mut line_ranges: Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> = Vec::new();
        let line_count = text.split('\n').count();
        for (line_index, line) in text.split('\n').enumerate() {
            let (glyph_start, stop_start) = (glyphs.len(), stops.len());
            let top = line_index as f32 * line_height;
            let baseline = top + ascent;
            let mut buffer = UnicodeBuffer::new();
            buffer.push_str(line);
            buffer.guess_segment_properties();
            let shaped = shaper.shape(buffer, harfrust::ShapeOptions::new());
            let mut pen_x = 0.0f32;
            let mut cluster_starts: Vec<(usize, f32)> = Vec::new();
            for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
                let id = GlyphId::new(info.glyph_id);
                cluster_starts.push((info.cluster as usize, pen_x));
                glyphs.push(PlacedGlyph { id, x: pen_x + position.x_offset as f32 * scale, baseline: baseline - position.y_offset as f32 * scale });
                let shaped_advance = position.x_advance as f32 * scale;
                pen_x += match &hinter {
                    Some(hinter) => {
                        let hinted = *hinted_advances.entry(id).or_insert_with(|| {
                            outlines
                                .get(id)
                                .and_then(|glyph| glyph.draw(DrawSettings::hinted(hinter, false), &mut NullPen).ok())
                                .and_then(|adjusted| adjusted.advance_width)
                                .unwrap_or_else(|| linear_advances.advance_width(id).unwrap_or(0.0).round())
                        });
                        let nominal = linear_advances.advance_width(id).unwrap_or(shaped_advance);
                        hinted + (shaped_advance - nominal).round()
                    }
                    None => shaped_advance,
                };
            }
            for (byte_offset, _) in line.char_indices() {
                let x = cluster_starts.iter().rev().find(|(cluster, _)| *cluster <= byte_offset).map_or(0.0, |&(_, x)| x);
                stops.push(CaretStop { x, top, height: line_height });
            }
            stops.push(CaretStop { x: pen_x, top, height: line_height });
            lines.push(LineSpan { x_start: 0.0, x_end: pen_x, baseline });
            line_ranges.push((glyph_start..glyphs.len(), stop_start..stops.len()));
            width = width.max(pen_x);
        }
        for (line, (glyph_range, stop_range)) in lines.iter_mut().zip(line_ranges) {
            let shift = snap((width - line.x_end) * style.align.leading_share());
            glyphs[glyph_range].iter_mut().for_each(|glyph| glyph.x += shift);
            stops[stop_range].iter_mut().for_each(|stop| stop.x += shift);
            line.x_start = shift;
            line.x_end += shift;
        }
        Self {
            glyphs,
            lines,
            stops,
            line_height,
            ascent,
            underline: decoration(metrics.underline, FALLBACK_UNDERLINE_OFFSET),
            strikeout: decoration(metrics.strikeout, FALLBACK_STRIKEOUT_OFFSET),
            width,
            height: line_height * line_count as f32,
        }
    }

    pub fn caret(&self, index: usize) -> CaretStop {
        self.stops[index.min(self.stops.len() - 1)]
    }

    pub fn index_at(&self, x: f32, y: f32) -> usize {
        let line = ((y / self.line_height).floor().max(0.0) as usize).min(self.lines.len() - 1);
        let top = line as f32 * self.line_height;
        self.stops
            .iter()
            .enumerate()
            .filter(|(_, stop)| stop.top == top)
            .min_by(|(_, a), (_, b)| (a.x - x).abs().total_cmp(&(b.x - x).abs()))
            .map_or(0, |(index, _)| index)
    }

    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    pub fn line_height(&self) -> f32 {
        self.line_height
    }

    pub fn bounds(&self, origin: FPoint) -> IRect {
        IRect::new(origin.x.floor() as i32, origin.y.floor() as i32, (origin.x + self.width).ceil() as i32, (origin.y + self.height).ceil() as i32)
    }
}

pub fn render(face: &FontFace, layout: &Layout, style: &TextStyle, origin: FPoint, blend: BlendMode, canvas: IRect) -> Option<Overlay> {
    let font = face.font();
    let hinter = hinting_instance(&font, style);
    let origin = if hinter.is_some() { FPoint::new(origin.x.round(), origin.y.round()) } else { origin };
    let outlines = font.outline_glyphs();
    let slant = if style.synthetic_italic { SYNTHETIC_ITALIC_SLANT } else { 0.0 };
    let mut builder = PathBuilder::new();
    for glyph in &layout.glyphs {
        let Some(outline) = outlines.get(glyph.id) else {
            continue;
        };
        let position = FPoint::new(origin.x + glyph.x, origin.y + glyph.baseline);
        let mut pen = GlyphPen { builder: &mut builder, origin: position, slant };
        if outline.draw(draw_settings(style, hinter.as_ref()), &mut pen).is_err() {
            let mut pen = GlyphPen { builder: &mut builder, origin: position, slant };
            let _ = outline.draw(DrawSettings::unhinted(Size::new(style.size), LocationRef::default()), &mut pen);
        }
    }
    for (decorated, (offset, thickness)) in [(style.underline, layout.underline), (style.strikethrough, layout.strikeout)] {
        if !decorated {
            continue;
        }
        for line in &layout.lines {
            if line.x_end <= line.x_start {
                continue;
            }
            let rect = Rect::from_xywh(origin.x + line.x_start, origin.y + line.baseline - offset - thickness, line.x_end - line.x_start, thickness)?;
            builder.push_rect(rect);
        }
    }
    let path = builder.finish()?;
    let paint_style = shape_style(style);
    if style.smoothing.is_lcd() && blend == BlendMode::Normal && style.outline.is_none() && !style.synthetic_bold {
        return render_lcd(&path, style, canvas);
    }
    render_path(&path, &paint_style, blend, canvas)
}

fn shape_style(style: &TextStyle) -> ShapeStyle {
    let bold_stroke = if style.synthetic_bold { style.size * SYNTHETIC_BOLD_STROKE } else { 0.0 };
    let (outline, width, align) = match (style.outline, style.fill) {
        (None, Some(fill)) if style.synthetic_bold => (Some(fill), bold_stroke, StrokeAlign::Center),
        (outline, _) => (outline, style.outline_width + bold_stroke, style.outline_align),
    };
    ShapeStyle { outline, fill: style.fill, width, align, antialias: style.smoothing != TextSmoothing::None }
}

fn render_lcd(path: &tiny_skia::Path, style: &TextStyle, canvas: IRect) -> Option<Overlay> {
    let fill = style.fill?;
    let bounds = path.bounds();
    let area = IRect::new(bounds.left().floor() as i32, bounds.top().floor() as i32, bounds.right().ceil() as i32, bounds.bottom().ceil() as i32)
        .expand(RENDER_MARGIN)
        .intersect(canvas);
    if area.is_empty() {
        return None;
    }
    let (width, height) = (area.width() as usize, area.height() as usize);
    let subpixel_width = width * LCD_SUBPIXELS;
    let mut pixmap = Pixmap::new(subpixel_width as u32, height as u32)?;
    let transform = Transform::from_row(LCD_SUBPIXELS as f32, 0.0, 0.0, 1.0, -(area.x0 * LCD_SUBPIXELS as i32) as f32, -area.y0 as f32);
    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 0, 0, u8::MAX);
    paint.anti_alias = true;
    pixmap.fill_path(path, &paint, FillRule::Winding, transform, None);
    let coverage: Vec<u8> = pixmap.data().chunks_exact(4).map(|pixel| pixel[3]).collect();
    let sample = |row: usize, subpixel: isize| -> u32 {
        if subpixel < 0 || subpixel as usize >= subpixel_width { 0 } else { u32::from(coverage[row * subpixel_width + subpixel as usize]) }
    };
    let channel_order: [usize; 3] = if style.smoothing == TextSmoothing::LcdBgr { [2, 1, 0] } else { [0, 1, 2] };
    let mut bytes = Vec::with_capacity(width * height * 4);
    let mut lcd = Vec::with_capacity(width * height);
    for row in 0..height {
        for column in 0..width {
            let mut channels = [0u8; 3];
            for (slot, &channel) in channel_order.iter().enumerate() {
                let center = (column * LCD_SUBPIXELS + slot) as isize;
                let filtered: u32 = LCD_FILTER.iter().enumerate().map(|(k, weight)| weight * sample(row, center + k as isize - LCD_FILTER_REACH as isize)).sum();
                let covered = (filtered / LCD_FILTER_SUM).min(u32::from(u8::MAX)) as u8;
                channels[channel] = ((u32::from(covered) * u32::from(fill[3])) / u32::from(u8::MAX)) as u8;
            }
            let average = (channels.iter().map(|&c| u32::from(c)).sum::<u32>() / 3) as u8;
            bytes.extend_from_slice(&[fill[0], fill[1], fill[2], average]);
            lcd.push(channels);
        }
    }
    let raster = Raster::from_rgba(width as u32, height as u32, bytes).ok()?;
    Some(Overlay { origin: IPoint::new(area.x0, area.y0), raster, blend: BlendMode::Normal, erase: false, lcd: Some(lcd) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system_font() -> Option<FontFace> {
        let candidates = [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
        ];
        candidates.iter().find_map(|path| std::fs::read(path).ok()).and_then(|bytes| FontFace::new(bytes.into(), 0).ok())
    }

    fn style(hinting: bool, smoothing: TextSmoothing) -> TextStyle {
        TextStyle {
            size: 12.0,
            synthetic_bold: false,
            synthetic_italic: false,
            underline: false,
            strikethrough: false,
            hinting,
            smoothing,
            align: TextAlign::Left,
            outline: None,
            fill: Some([0, 0, 0, 255]),
            outline_width: 1.0,
            outline_align: StrokeAlign::Outside,
        }
    }

    const CANVAS: IRect = IRect { x0: 0, y0: 0, x1: 200, y1: 60 };

    #[test]
    fn garbage_bytes_are_a_font_error_not_a_crash() {
        assert!(FontFace::new(vec![0u8; 64].into(), 0).is_err());
    }

    #[test]
    fn hinted_layout_puts_glyphs_on_whole_pixels() {
        let Some(face) = system_font() else { return };
        let layout = Layout::build(&face, "Hamburgefonstiv", &style(true, TextSmoothing::Grayscale));
        assert!(layout.glyphs.iter().all(|glyph| glyph.x.fract() == 0.0));
    }

    #[test]
    fn unhinted_layout_keeps_fractional_advances() {
        let Some(face) = system_font() else { return };
        let layout = Layout::build(&face, "Hamburgefonstiv", &style(false, TextSmoothing::Grayscale));
        assert!(layout.glyphs.iter().any(|glyph| glyph.x.fract() != 0.0));
    }

    #[test]
    fn caret_moves_right_with_each_character_and_down_with_each_line() {
        let Some(face) = system_font() else { return };
        let layout = Layout::build(&face, "ab\nc", &style(true, TextSmoothing::Grayscale));
        assert!(layout.caret(1).x > layout.caret(0).x);
        assert!(layout.caret(2).x > layout.caret(1).x);
        assert_eq!(layout.caret(3), CaretStop { x: 0.0, top: layout.line_height(), height: layout.line_height() });
        assert_eq!(layout.caret(99), layout.caret(4));
    }

    #[test]
    fn index_at_picks_the_nearest_caret_on_the_clicked_line() {
        let Some(face) = system_font() else { return };
        let layout = Layout::build(&face, "ab\ncd", &style(true, TextSmoothing::Grayscale));
        assert_eq!(layout.index_at(-5.0, 1.0), 0);
        assert_eq!(layout.index_at(1000.0, 1.0), 2);
        assert_eq!(layout.index_at(1000.0, layout.line_height() + 1.0), 5);
    }

    #[test]
    fn aliased_text_has_only_fully_covered_pixels() {
        let Some(face) = system_font() else { return };
        let style = style(true, TextSmoothing::None);
        let layout = Layout::build(&face, "Text", &style);
        let overlay = render(&face, &layout, &style, FPoint::new(4.0, 4.0), BlendMode::Normal, CANVAS).expect("есть что рисовать");
        assert!(overlay.raster.as_bytes().chunks_exact(4).all(|pixel| pixel[3] == 0 || pixel[3] == 255));
    }

    #[test]
    fn antialiased_text_has_partial_coverage_with_and_without_hinting() {
        let Some(face) = system_font() else { return };
        for hinting in [false, true] {
            let style = style(hinting, TextSmoothing::Grayscale);
            let layout = Layout::build(&face, "Text", &style);
            let overlay = render(&face, &layout, &style, FPoint::new(4.0, 4.0), BlendMode::Normal, CANVAS).expect("есть что рисовать");
            assert!(overlay.raster.as_bytes().chunks_exact(4).any(|pixel| pixel[3] != 0 && pixel[3] != 255), "hinting={hinting}");
        }
    }

    #[test]
    fn hinting_changes_the_rendered_pixels() {
        let Some(face) = system_font() else { return };
        let draw = |hinting| {
            let style = style(hinting, TextSmoothing::Grayscale);
            let layout = Layout::build(&face, "Hamburgefonstiv", &style);
            render(&face, &layout, &style, FPoint::new(4.0, 4.0), BlendMode::Normal, CANVAS).expect("есть что рисовать").raster.as_bytes().to_vec()
        };
        assert_ne!(draw(true), draw(false));
    }

    #[test]
    fn outline_only_and_fill_only_differ() {
        let Some(face) = system_font() else { return };
        let mut fill_only = style(true, TextSmoothing::Grayscale);
        fill_only.size = 40.0;
        let mut outline_only = fill_only;
        (outline_only.fill, outline_only.outline) = (None, Some([0, 0, 0, 255]));
        let layout = Layout::build(&face, "O", &fill_only);
        let alpha_sum = |style: &TextStyle| {
            let overlay = render(&face, &layout, style, FPoint::new(4.0, 4.0), BlendMode::Normal, CANVAS).expect("есть что рисовать");
            overlay.raster.as_bytes().chunks_exact(4).map(|pixel| u32::from(pixel[3])).sum::<u32>()
        };
        assert!(alpha_sum(&outline_only) < alpha_sum(&fill_only));
    }

    #[test]
    fn lcd_text_gives_different_coverage_per_channel_at_edges() {
        let Some(face) = system_font() else { return };
        let style = style(false, TextSmoothing::LcdRgb);
        let layout = Layout::build(&face, "Text", &style);
        let overlay = render(&face, &layout, &style, FPoint::new(4.0, 4.0), BlendMode::Normal, CANVAS).expect("есть что рисовать");
        let lcd = overlay.lcd.expect("LCD-покрытие есть");
        assert!(lcd.iter().any(|c| c[0] != c[1] || c[1] != c[2]));
    }
}
