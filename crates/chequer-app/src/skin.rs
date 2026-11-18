
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

use eframe::egui::{
    self, Align2, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Mesh, Painter,
    Pos2, Rect, Shape, Stroke, TextStyle, TextureHandle, TextureOptions, Vec2, vec2,
};

use crate::fonts::FontChoice;
use crate::icons::Icons;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Theme {
    pub ink: Color32,
    pub ink_dim: Color32,
    pub ink_faint: Color32,
    pub line: Color32,
    pub line_soft: Color32,
    pub chrome: Color32,
    pub window: Color32,
    pub title: Color32,
    pub button: Color32,
    pub button_hover: Color32,
    pub button_on: Color32,
    pub field: Color32,
    pub list_hover: Color32,
    pub workspace: Color32,
    pub highlight: Color32,
    pub highlight_text: Color32,
    pub close_hover: Color32,
    pub close_hover_text: Color32,
    pub groove_dark: Color32,
    pub groove_light: Color32,
    pub checker_light: Color32,
    pub checker_dark: Color32,
    pub corner_style: CornerStyle,
    pub window_radius: u32,
    pub button_radius: u32,
    pub bevel_rings: usize,
    pub shadow_alpha: u8,
}

impl Theme {
    pub fn colors_mut(&mut self) -> Vec<(&'static str, &'static str, &'static str, &mut Color32)> {
        vec![
            ("ink", "Текст", "Text", &mut self.ink),
            ("ink_dim", "Текст приглушённый", "Text dim", &mut self.ink_dim),
            ("ink_faint", "Текст слабый", "Text faint", &mut self.ink_faint),
            ("line", "Контур", "Outline", &mut self.line),
            ("line_soft", "Контур мягкий", "Outline soft", &mut self.line_soft),
            ("chrome", "Панели и строки", "Bars", &mut self.chrome),
            ("window", "Окна", "Windows", &mut self.window),
            ("title", "Заголовок окна", "Window title", &mut self.title),
            ("button", "Кнопка", "Button", &mut self.button),
            ("button_hover", "Кнопка при наведении", "Button hover", &mut self.button_hover),
            ("button_on", "Кнопка нажатая", "Button pressed", &mut self.button_on),
            ("field", "Поле ввода и списки", "Fields and lists", &mut self.field),
            ("list_hover", "Строка списка при наведении", "List row hover", &mut self.list_hover),
            ("workspace", "Рабочая область", "Workspace", &mut self.workspace),
            ("highlight", "Выделение", "Highlight", &mut self.highlight),
            ("highlight_text", "Текст на выделении", "Highlight text", &mut self.highlight_text),
            ("close_hover", "Закрытие при наведении", "Close hover", &mut self.close_hover),
            ("close_hover_text", "Крестик закрытия", "Close cross", &mut self.close_hover_text),
            ("groove_dark", "Канавка тёмная", "Groove dark", &mut self.groove_dark),
            ("groove_light", "Канавка светлая", "Groove light", &mut self.groove_light),
            ("checker_light", "Шахматка светлая", "Checker light", &mut self.checker_light),
            ("checker_dark", "Шахматка тёмная", "Checker dark", &mut self.checker_dark),
        ]
    }

    pub fn window_corner(&self) -> Corner {
        Corner::new(self.corner_style, self.window_radius)
    }

    pub fn button_corner(&self) -> Corner {
        Corner::new(self.corner_style, self.button_radius)
    }
}

static THEME: LazyLock<RwLock<Theme>> = LazyLock::new(RwLock::default);

pub fn theme() -> Theme {
    *THEME.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn set_theme(theme: Theme) {
    *THEME.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = theme;
}

static UI_PIXEL: AtomicU32 = AtomicU32::new(1);

pub fn ui_pixel() -> u32 {
    UI_PIXEL.load(Ordering::Relaxed)
}

pub fn set_ui_pixel(size: u32) {
    UI_PIXEL.store(size.max(1), Ordering::Relaxed);
}

pub fn pixel_mode() -> bool {
    ui_pixel() > 1
}

static FONT_CHOICE: RwLock<FontChoice> = RwLock::new(FontChoice::Builtin);

pub fn font_choice() -> FontChoice {
    FONT_CHOICE.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

pub fn set_font_choice(choice: FontChoice) {
    *FONT_CHOICE.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = choice;
}

pub fn grid_ppp(ppp: f32) -> f32 {
    ppp / ui_pixel() as f32
}

pub const FIELD_RINGS: usize = 1;
pub const TITLE_HEIGHT: f32 = 18.0;
pub const PANEL_PADDING: f32 = 4.0;
pub const CHECKER_CELL: f32 = 8.0;
const LIGHT_STEPS: [u8; 2] = [30, 14];
const SHADE_STEPS: [u8; 2] = [38, 18];
const SUNKEN_SHADE_STEPS: [u8; 2] = [44, 22];
const SUNKEN_LIGHT_STEPS: [u8; 2] = [26, 12];
const WINDOW_SHADOW_OFFSET: f32 = 3.0;
const UI_ANIMATION_SECONDS: f32 = 0.015;
const SCROLL_FADE_STRENGTH: f32 = 0.9;
const SCROLL_FADE_SIZE: f32 = 14.0;
const TOOLTIP_DELAY_SECONDS: f32 = 0.2;
const TOOLTIP_GRACE_SECONDS: f32 = 0.1;
const POPUP_SHADOW_ALPHA: u8 = 40;
pub const WINDOW_SHADOW_REACH: f32 = 4.0;

const BODY_FONT_SIZE: f32 = 13.0;
const SMALL_FONT_SIZE: f32 = 11.5;
const TITLE_FONT_SIZE: f32 = 12.0;
const HEADING_FONT_SIZE: f32 = 15.0;
const MONO_FONT_SIZE: f32 = 12.5;
const PLEX_SANS: &[u8] = include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf");
const PLEX_SANS_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf");
const PLEX_MONO: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf");
const PIXEL_FONT: &[u8] = include_bytes!("../assets/fonts/Aseprite.ttf");
const PIXEL_EM: u32 = 9;
const LOGO_PNG: &[u8] = include_bytes!("../assets/logo/chequer-image-studio.png");

#[derive(Clone, Copy)]
pub enum Font {
    Body,
    Title,
}

impl Font {
    fn style(self) -> TextStyle {
        match self {
            Font::Body => TextStyle::Body,
            Font::Title => title_style(),
        }
    }
}

fn title_style() -> TextStyle {
    TextStyle::Name("title".into())
}

fn semibold_family() -> FontFamily {
    FontFamily::Name("semibold".into())
}

fn pixel_family() -> FontFamily {
    FontFamily::Name("pixel".into())
}

fn pixel_font_scale(target_px: f32, pixel: u32) -> u32 {
    ((target_px / (PIXEL_EM * pixel) as f32).round() as u32).max(1)
}

const GRAIN_TEXTURE_SIZE: usize = 128;
const GRAIN_ALPHA: u8 = 16;
const GRAIN_COVERAGE_PERCENT: u32 = 12;
pub const GRAIN_DEFAULT_STRENGTH: f32 = 0.3;
const GRAIN_SEED: u32 = 0x9e37_79b9;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bevel {
    Raised,
    Sunken,
}

#[derive(Clone, Copy)]
pub struct Plate {
    pub fill: Color32,
    pub border: Color32,
    pub bevel: Option<Bevel>,
    pub rings: usize,
    pub corner: Corner,
}

impl Plate {
    pub fn button(fill: Color32, bevel: Bevel) -> Self {
        let theme = theme();
        Self { fill, border: theme.line_soft, bevel: Some(bevel), rings: theme.bevel_rings, corner: theme.button_corner() }
    }

    pub fn field(fill: Color32) -> Self {
        let theme = theme();
        Self { fill, border: theme.line, bevel: Some(Bevel::Sunken), rings: theme.bevel_rings.min(FIELD_RINGS), corner: Corner::SQUARE }
    }

    pub fn window() -> Self {
        let theme = theme();
        Self { fill: theme.window, border: theme.line, bevel: Some(Bevel::Raised), rings: theme.bevel_rings, corner: theme.window_corner() }
    }

    pub fn frame_only(bevel: Bevel, rings: usize) -> Self {
        Self { fill: Color32::TRANSPARENT, border: theme().line, bevel: Some(bevel), rings: rings.min(theme().bevel_rings), corner: Corner::SQUARE }
    }
}

pub struct Skin {
    pub icons: Icons,
    grain: TextureHandle,
    grain_strength: AtomicU32,
    checker: TextureHandle,
    ants: TextureHandle,
    logo: TextureHandle,
}

impl Skin {
    pub fn install(ctx: &Context, theme: Theme, icons_dir: Option<&Path>) -> (Self, Option<String>) {
        set_theme(theme);
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        crate::scroll_fx::allow_fast_scroll(ctx);
        let font_error = refresh_style(ctx);
        let skin = Self { icons: Icons::load(ctx, icons_dir), grain: make_grain(ctx), grain_strength: AtomicU32::new(GRAIN_DEFAULT_STRENGTH.to_bits()), checker: make_checker(ctx), ants: make_ants_pattern(ctx), logo: make_logo(ctx) };
        (skin, font_error)
    }

    pub fn with_icons(&self, ctx: &Context, icons_dir: Option<&Path>) -> Self {
        Self { icons: Icons::load(ctx, icons_dir), grain: self.grain.clone(), grain_strength: AtomicU32::new(self.grain_strength.load(Ordering::Relaxed)), checker: self.checker.clone(), ants: self.ants.clone(), logo: self.logo.clone() }
    }

    pub fn logo_size(&self, ppp: f32) -> Vec2 {
        let [width, height] = self.logo.size();
        vec2(width as f32, height as f32) * crate::widgets::icon_scale(ppp) / ppp
    }

    pub fn paint_logo(&self, painter: &Painter, rect: Rect) {
        painter.image(self.logo.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }

    pub fn paint_plate(&self, painter: &Painter, rect: Rect, plate: &Plate) {
        painter.extend(plate_shapes(grid_ppp(painter.pixels_per_point()), rect, plate));
    }

    pub fn plate_shapes(&self, ppp: f32, rect: Rect, plate: &Plate) -> Vec<Shape> {
        plate_shapes(grid_ppp(ppp), rect, plate)
    }

    pub fn window_shapes(&self, ppp: f32, rect: Rect) -> Vec<Shape> {
        let ppp = grid_ppp(ppp);
        let frame = PixelRect::snapped(rect, ppp);
        let shadow = PixelRect { x0: frame.x0 + WINDOW_SHADOW_OFFSET, y0: frame.y0 + WINDOW_SHADOW_OFFSET, x1: frame.x1 + WINDOW_SHADOW_OFFSET, y1: frame.y1 + WINDOW_SHADOW_OFFSET, ppp };
        let theme = theme();
        let mut shapes: Vec<Shape> = if theme.shadow_alpha == 0 {
            Vec::new()
        } else {
            notched_parts(shadow, theme.window_corner().fitted(shadow), Rounded::All).into_iter().map(|part| part.filled(Color32::from_black_alpha(theme.shadow_alpha))).collect()
        };
        shapes.extend(plate_shapes(ppp, rect, &Plate::window()));
        shapes.extend(self.grain_shape(ppp, rect));
        shapes
    }

    pub fn set_grain_strength(&self, strength: f32) {
        self.grain_strength.store(strength.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn grain_shape(&self, ppp: f32, rect: Rect) -> Option<Shape> {
        let strength = f32::from_bits(self.grain_strength.load(Ordering::Relaxed));
        if strength <= 0.0 {
            return None;
        }
        let tint = Color32::from_white_alpha((strength * 255.0).round() as u8);
        Some(Shape::image(self.grain.id(), rect, grain_uv(rect, grid_ppp(ppp)), tint))
    }

    pub fn paint_grain(&self, painter: &Painter, rect: Rect) {
        if let Some(shape) = self.grain_shape(painter.pixels_per_point(), rect) {
            painter.add(shape);
        }
    }

    pub fn ants_texture(&self) -> egui::TextureId {
        self.ants.id()
    }

    pub fn paint_checker(&self, painter: &Painter, rect: Rect, anchor: Pos2) {
        let period = CHECKER_CELL * 2.0;
        let min = Pos2::new((rect.min.x - anchor.x) / period, (rect.min.y - anchor.y) / period);
        let uv = Rect::from_min_size(min, vec2(rect.width() / period, rect.height() / period));
        painter.rect_filled(rect, 0.0, theme().checker_dark);
        painter.image(self.checker.id(), rect, uv, theme().checker_light);
    }
}

const WINDOW_RING_CELLS: u32 = 2;

pub fn title_shapes(ppp: f32, bar: Rect, fill: Color32) -> Vec<Shape> {
    let grid = grid_ppp(ppp);
    let reach = crate::widgets::WINDOW_INSET;
    let window = PixelRect::snapped(Rect::from_min_max(bar.min - vec2(reach, reach), Pos2::new(bar.max.x + reach, bar.max.y)), grid);
    let ring = WINDOW_RING_CELLS as f32;
    let frame = PixelRect { x0: window.x0 + ring, y0: window.y0 + ring, x1: window.x1 - ring, y1: (bar.max.y * grid).round(), ppp: grid };
    let corner = theme().window_corner().shrunk(WINDOW_RING_CELLS).fitted(frame);
    notched_parts(frame, corner, Rounded::Top).into_iter().map(|part| part.filled(fill)).collect()
}

fn grain_uv(rect: Rect, ppp: f32) -> Rect {
    let texels = GRAIN_TEXTURE_SIZE as f32;
    Rect::from_min_max(
        Pos2::new(rect.min.x * ppp / texels, rect.min.y * ppp / texels),
        Pos2::new(rect.max.x * ppp / texels, rect.max.y * ppp / texels),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PixelRect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    ppp: f32,
}

impl PixelRect {
    fn snapped(rect: Rect, ppp: f32) -> Self {
        Self {
            x0: (rect.min.x * ppp).round(),
            y0: (rect.min.y * ppp).round(),
            x1: (rect.max.x * ppp).round(),
            y1: (rect.max.y * ppp).round(),
            ppp,
        }
    }

    fn width(self) -> f32 {
        self.x1 - self.x0
    }

    fn height(self) -> f32 {
        self.y1 - self.y0
    }

    fn inset(self, by: f32) -> Self {
        Self { x0: self.x0 + by, y0: self.y0 + by, x1: self.x1 - by, y1: self.y1 - by, ppp: self.ppp }
    }

    fn to_rect(self) -> Rect {
        Rect::from_min_max(Pos2::new(self.x0 / self.ppp, self.y0 / self.ppp), Pos2::new(self.x1 / self.ppp, self.y1 / self.ppp))
    }

    fn filled(self, color: Color32) -> Shape {
        Shape::rect_filled(self.to_rect(), CornerRadius::ZERO, color)
    }

    fn strip(self, x0: f32, y0: f32, x1: f32, y1: f32, color: Color32, out: &mut Vec<Shape>) {
        if x1 > x0 && y1 > y0 {
            out.push(PixelRect { x0, y0, x1, y1, ppp: self.ppp }.filled(color));
        }
    }

    fn dot(self, x: f32, y: f32, color: Color32, out: &mut Vec<Shape>) {
        self.strip(x, y, x + 1.0, y + 1.0, color, out);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CornerStyle {
    Bevel,
    #[default]
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corner {
    pub style: CornerStyle,
    pub radius: u32,
    depth: u32,
}

impl Corner {
    pub const SQUARE: Corner = Corner { style: CornerStyle::Bevel, radius: 0, depth: 0 };

    pub fn new(style: CornerStyle, radius: u32) -> Self {
        Self { style, radius, depth: 0 }
    }

    pub fn shrunk(self, by: u32) -> Self {
        Self { depth: self.depth + by, ..self }
    }

    fn fitted(self, frame: PixelRect) -> Self {
        let room = (frame.width().min(frame.height()) / 2.0).floor() as u32 + self.depth;
        Self { radius: self.radius.min(room.saturating_sub(1)), ..self }
    }

    fn profile(self) -> Vec<u32> {
        let radius = self.radius;
        let mut insets: Vec<u32> = match self.style {
            CornerStyle::Bevel => (1..=radius).rev().collect(),
            CornerStyle::Round => {
                let r = radius as f32;
                (0..radius)
                    .map(|row| {
                        let dy = row as f32 + 0.5 - r;
                        (0..radius).find(|&col| (col as f32 + 0.5 - r).powi(2) + dy * dy <= r * r).unwrap_or(radius)
                    })
                    .collect()
            }
        };
        for _ in 0..self.depth {
            insets = eroded(&insets);
        }
        while insets.last() == Some(&0) {
            insets.pop();
        }
        insets
    }
}

fn eroded(insets: &[u32]) -> Vec<u32> {
    insets.iter().enumerate().map(|(row, &inset)| if inset > insets.get(row + 1).copied().unwrap_or(0) { inset - 1 } else { inset }).collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rounded {
    All,
    Top,
}

fn notched_parts(frame: PixelRect, corner: Corner, rounded: Rounded) -> Vec<PixelRect> {
    let profile = corner.profile();
    let rows = profile.len();
    let bottom_rows = if rounded == Rounded::All { rows } else { 0 };
    let mut parts = Vec::with_capacity(rows + bottom_rows + 1);
    let row = |inset: u32, y: f32| PixelRect { x0: frame.x0 + inset as f32, y0: y, x1: frame.x1 - inset as f32, y1: y + 1.0, ppp: frame.ppp };
    for (step, &inset) in profile.iter().enumerate() {
        parts.push(row(inset, frame.y0 + step as f32));
        if rounded == Rounded::All {
            parts.push(row(inset, frame.y1 - step as f32 - 1.0));
        }
    }
    parts.push(PixelRect { y0: frame.y0 + rows as f32, y1: frame.y1 - bottom_rows as f32, ..frame });
    parts
}

fn lighten(color: Color32, amount: u8) -> Color32 {
    Color32::from_rgb(color.r().saturating_add(amount), color.g().saturating_add(amount), color.b().saturating_add(amount))
}

fn darken(color: Color32, amount: u8) -> Color32 {
    Color32::from_rgb(color.r().saturating_sub(amount), color.g().saturating_sub(amount), color.b().saturating_sub(amount))
}

fn average(a: Color32, b: Color32) -> Color32 {
    let mid = |p: u8, q: u8| ((p as u16 + q as u16) / 2) as u8;
    Color32::from_rgb(mid(a.r(), b.r()), mid(a.g(), b.g()), mid(a.b(), b.b()))
}

fn plate_shapes(ppp: f32, rect: Rect, plate: &Plate) -> Vec<Shape> {
    let frame = PixelRect::snapped(rect, ppp);
    let corner = plate.corner.fitted(frame);
    let mut shapes = Vec::new();
    let has_body = plate.fill != Color32::TRANSPARENT;
    if has_body {
        shapes.extend(notched_parts(frame, corner, Rounded::All).into_iter().map(|part| part.filled(plate.border)));
        shapes.extend(notched_parts(frame.inset(1.0), corner.shrunk(1), Rounded::All).into_iter().map(|part| part.filled(plate.fill)));
    }
    let Some(bevel) = plate.bevel else {
        return shapes;
    };
    let (light, shade) = match bevel {
        Bevel::Raised => (LIGHT_STEPS, SHADE_STEPS),
        Bevel::Sunken => (SUNKEN_LIGHT_STEPS, SUNKEN_SHADE_STEPS),
    };
    let base = if has_body { plate.fill } else { theme().chrome };
    let (top_left_steps, bottom_right_steps) = if bevel == Bevel::Raised { (light, shade) } else { (shade, light) };
    for ring in 0..plate.rings.min(LIGHT_STEPS.len()) {
        let (top_left, bottom_right) = match bevel {
            Bevel::Raised => (lighten(base, top_left_steps[ring]), darken(base, bottom_right_steps[ring])),
            Bevel::Sunken => (darken(base, top_left_steps[ring]), lighten(base, bottom_right_steps[ring])),
        };
        let inset = 1.0 + ring as f32;
        bevel_ring(frame.inset(inset), corner.shrunk(1 + ring as u32), top_left, bottom_right, &mut shapes);
    }
    shapes
}

fn bevel_ring(frame: PixelRect, corner: Corner, top_left: Color32, bottom_right: Color32, out: &mut Vec<Shape>) {
    let corner = corner.fitted(frame);
    if frame.width() < 2.0 || frame.height() < 2.0 {
        return;
    }
    let profile = corner.profile();
    let rows = profile.len();
    let cut = rows as f32;
    let top_cut = profile.first().copied().unwrap_or(0) as f32;
    let middle = average(top_left, bottom_right);
    frame.strip(frame.x0 + top_cut, frame.y0, frame.x1 - top_cut, frame.y0 + 1.0, top_left, out);
    frame.strip(frame.x0 + top_cut, frame.y1 - 1.0, frame.x1 - top_cut, frame.y1, bottom_right, out);
    frame.strip(frame.x0, frame.y0 + cut + 1.0, frame.x0 + 1.0, frame.y1 - cut - 1.0, top_left, out);
    frame.strip(frame.x1 - 1.0, frame.y0 + cut + 1.0, frame.x1, frame.y1 - cut - 1.0, bottom_right, out);
    for row in 1..=rows {
        let from = profile.get(row).copied().unwrap_or(0);
        let to = profile[row - 1].max(from + 1);
        let dy = row as f32;
        for dx in from..to {
            let dx = dx as f32;
            frame.dot(frame.x0 + dx, frame.y0 + dy, top_left, out);
            frame.dot(frame.x1 - 1.0 - dx, frame.y0 + dy, middle, out);
            frame.dot(frame.x0 + dx, frame.y1 - 1.0 - dy, middle, out);
            frame.dot(frame.x1 - 1.0 - dx, frame.y1 - 1.0 - dy, bottom_right, out);
        }
    }
}

pub fn plate_inset(ppp: f32, rings: usize) -> f32 {
    (1.0 + rings as f32) / grid_ppp(ppp)
}

pub fn gradient_mesh(rect: Rect, stops: &[(f32, Color32)], horizontal: bool) -> Mesh {
    let mut mesh = Mesh::default();
    for &(at, color) in stops {
        let (a, b) = if horizontal {
            let x = rect.min.x + rect.width() * at;
            (Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y))
        } else {
            let y = rect.min.y + rect.height() * at;
            (Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y))
        };
        mesh.colored_vertex(a, color);
        mesh.colored_vertex(b, color);
    }
    for pair in 0..stops.len().saturating_sub(1) as u32 {
        let base = pair * 2;
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base + 1, base + 2, base + 3);
    }
    mesh
}

fn install_fonts(ctx: &Context) -> Option<String> {
    let mut fonts = FontDefinitions::default();
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, bytes) in [("plex-sans", PLEX_SANS), ("plex-sans-semibold", PLEX_SANS_SEMIBOLD), ("plex-mono", PLEX_MONO), ("pixel", PIXEL_FONT)] {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    let with_fallback = |first: &[&str]| first.iter().map(|name| (*name).to_owned()).chain(fallback.iter().cloned()).collect::<Vec<_>>();
    let (mut regular, mut bold, mono) = (with_fallback(&["plex-sans"]), with_fallback(&["plex-sans-semibold"]), with_fallback(&["plex-mono"]));
    let mut error = None;
    match font_choice().load() {
        Ok(Some(loaded)) => {
            let mut data = FontData::from_owned(loaded.bytes);
            data.index = loaded.index;
            fonts.font_data.insert("custom".to_owned(), Arc::new(data));
            regular.insert(0, "custom".to_owned());
            bold.insert(0, "custom".to_owned());
        }
        Ok(None) => {}
        Err(message) => error = Some(message),
    }
    fonts.families.insert(FontFamily::Proportional, regular);
    fonts.families.insert(FontFamily::Monospace, mono);
    fonts.families.insert(semibold_family(), bold);
    fonts.families.insert(pixel_family(), with_fallback(&["pixel"]));
    ctx.set_fonts(fonts);
    error
}

pub fn builtin_font_bytes() -> &'static [u8] {
    if pixel_mode() { PIXEL_FONT } else { PLEX_SANS }
}

pub fn refresh_style(ctx: &Context) -> Option<String> {
    let error = install_fonts(ctx);
    apply_style(ctx);
    error
}

fn pixel_text() -> bool {
    pixel_mode() && font_choice() == FontChoice::Builtin
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FontKind {
    Regular,
    Bold,
    Mono,
}

fn font_id(size: f32, kind: FontKind, ppp: f32) -> FontId {
    if pixel_text() {
        let pixel = ui_pixel();
        let scale = pixel_font_scale(size * ppp, pixel);
        return FontId::new((PIXEL_EM * scale * pixel) as f32 / ppp, pixel_family());
    }
    match kind {
        FontKind::Regular => FontId::proportional(size),
        FontKind::Bold => FontId::new(size, semibold_family()),
        FontKind::Mono => FontId::monospace(size),
    }
}

pub fn apply_style(ctx: &Context) {
    let mut style = (*ctx.global_style()).clone();
    let ppp = ctx.pixels_per_point();
    let pixel = pixel_mode();
    style.text_styles = [
        (TextStyle::Small, font_id(SMALL_FONT_SIZE, FontKind::Regular, ppp)),
        (TextStyle::Body, font_id(BODY_FONT_SIZE, FontKind::Regular, ppp)),
        (TextStyle::Button, font_id(BODY_FONT_SIZE, FontKind::Regular, ppp)),
        (TextStyle::Heading, font_id(HEADING_FONT_SIZE, FontKind::Bold, ppp)),
        (TextStyle::Monospace, font_id(MONO_FONT_SIZE, FontKind::Mono, ppp)),
        (title_style(), font_id(TITLE_FONT_SIZE, FontKind::Bold, ppp)),
    ]
    .into();
    ctx.tessellation_options_mut(|options| options.feathering = !pixel);
    let line = if pixel { ui_pixel() as f32 / ppp } else { 1.0 };
    style.animation_time = UI_ANIMATION_SECONDS;
    style.interaction.tooltip_delay = TOOLTIP_DELAY_SECONDS;
    style.interaction.tooltip_grace_time = TOOLTIP_GRACE_SECONDS;
    let spacing = &mut style.spacing;
    spacing.item_spacing = crate::metrics::ITEM_SPACING;
    spacing.button_padding = vec2(8.0, 3.0);
    spacing.interact_size = vec2(20.0, 22.0);
    spacing.window_margin = egui::Margin::same(6);
    spacing.menu_margin = egui::Margin::same(3);
    spacing.slider_width = 120.0;
    spacing.scroll.kinetic.rubber_band = false;
    spacing.scroll.fade.strength = SCROLL_FADE_STRENGTH;
    spacing.scroll.fade.size = SCROLL_FADE_SIZE;

    let visuals = &mut style.visuals;
    *visuals = egui::Visuals::light();
    visuals.text_options.subpixel_binning = false;
    visuals.override_text_color = None;
    let theme = theme();
    visuals.panel_fill = theme.chrome;
    visuals.window_fill = theme.window;
    visuals.window_stroke = Stroke::new(line, theme.line);
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.popup_shadow = egui::Shadow { offset: [3, 3], blur: 0, spread: 0, color: Color32::from_black_alpha(POPUP_SHADOW_ALPHA) };
    visuals.extreme_bg_color = theme.field;
    visuals.text_edit_bg_color = Some(theme.field);
    visuals.faint_bg_color = theme.window;
    visuals.slider_trailing_fill = false;
    visuals.selection.bg_fill = theme.highlight;
    visuals.selection.stroke = Stroke::new(line, theme.highlight_text);
    visuals.hyperlink_color = theme.ink;

    let widget = |fill: Color32, stroke: Color32, text: Color32| egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(line, stroke),
        corner_radius: CornerRadius::ZERO,
        fg_stroke: Stroke::new(line, text),
        expansion: 0.0,
    };
    visuals.widgets.noninteractive = widget(theme.chrome, theme.groove_dark, theme.ink);
    visuals.widgets.inactive = widget(theme.button, theme.line_soft, theme.ink);
    visuals.widgets.hovered = widget(theme.button_hover, theme.line, theme.ink);
    visuals.widgets.active = widget(theme.button_on, theme.line, theme.ink);
    visuals.widgets.open = widget(theme.button_on, theme.line, theme.ink);
    ctx.set_global_style(style);
}

pub fn menu_style(ui: &mut egui::Ui) {
    let hovered = &mut ui.visuals_mut().widgets.hovered;
    hovered.weak_bg_fill = theme().highlight;
    hovered.bg_fill = theme().highlight;
    hovered.fg_stroke = Stroke::new(1.0, theme().highlight_text);
    hovered.bg_stroke = Stroke::NONE;
    let active = &mut ui.visuals_mut().widgets.active;
    active.weak_bg_fill = theme().highlight;
    active.bg_fill = theme().highlight;
    active.fg_stroke = Stroke::new(1.0, theme().highlight_text);
    active.bg_stroke = Stroke::NONE;
}

fn make_grain(ctx: &Context) -> TextureHandle {
    let mut state = GRAIN_SEED;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let mut pixels = Vec::with_capacity(GRAIN_TEXTURE_SIZE * GRAIN_TEXTURE_SIZE);
    for _ in 0..GRAIN_TEXTURE_SIZE * GRAIN_TEXTURE_SIZE {
        let speck = if next() % 100 < GRAIN_COVERAGE_PERCENT {
            let shade = if next() % 2 == 0 { u8::MAX } else { u8::MIN };
            Color32::from_rgba_unmultiplied(shade, shade, shade, GRAIN_ALPHA)
        } else {
            Color32::TRANSPARENT
        };
        pixels.push(speck);
    }
    ctx.load_texture("grain", egui::ColorImage::new([GRAIN_TEXTURE_SIZE; 2], pixels), TextureOptions::NEAREST_REPEAT)
}

fn make_logo(ctx: &Context) -> TextureHandle {
    let decoded = image::load_from_memory_with_format(LOGO_PNG, image::ImageFormat::Png).expect("встроенный баннер - корректный PNG").into_rgba8();
    let size = [decoded.width() as usize, decoded.height() as usize];
    ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied(size, decoded.as_raw()), TextureOptions::NEAREST)
}

fn make_ants_pattern(ctx: &Context) -> TextureHandle {
    let width = crate::ants::PATTERN_WIDTH;
    let pixels = (0..width).map(|x| if x < width / 2 { Color32::BLACK } else { Color32::WHITE }).collect();
    ctx.load_texture("ants", egui::ColorImage::new([width, 1], pixels), TextureOptions::NEAREST_REPEAT)
}

fn make_checker(ctx: &Context) -> TextureHandle {
    let cell = CHECKER_CELL as usize;
    let size = cell * 2;
    let mut pixels = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            pixels.push(if (x / cell + y / cell) % 2 == 0 { Color32::WHITE } else { Color32::TRANSPARENT });
        }
    }
    ctx.load_texture("checker", egui::ColorImage::new([size, size], pixels), TextureOptions::NEAREST_REPEAT)
}

pub fn text_at(painter: &Painter, pos: Pos2, align: Align2, text: &str, font: Font, color: Color32) {
    let id = painter.ctx().global_style().text_styles[&font.style()].clone();
    painter.text(pos, align, text, id, color);
}

pub fn text_width(painter: &Painter, text: &str, font: Font) -> f32 {
    let id = painter.ctx().global_style().text_styles[&font.style()].clone();
    painter.layout_no_wrap(text.to_owned(), id, theme().ink).size().x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: f32, height: f32) -> PixelRect {
        PixelRect { x0: 0.0, y0: 0.0, x1: width, y1: height, ppp: 1.0 }
    }

    fn area(parts: &[PixelRect]) -> f32 {
        parts.iter().map(|p| p.width() * p.height()).sum()
    }

    #[test]
    fn notch_removes_a_staircase_from_each_corner() {
        let bevel = |radius| Corner::new(CornerStyle::Bevel, radius);
        assert_eq!(area(&notched_parts(frame(10.0, 8.0), bevel(0), Rounded::All)), 80.0);
        assert_eq!(area(&notched_parts(frame(10.0, 8.0), bevel(1), Rounded::All)), 80.0 - 4.0);
        assert_eq!(area(&notched_parts(frame(10.0, 8.0), bevel(2), Rounded::All)), 80.0 - 4.0 * 3.0);
    }

    #[test]
    fn notch_never_eats_a_tiny_plate() {
        let tiny = frame(3.0, 3.0);
        let parts = notched_parts(tiny, Corner::new(CornerStyle::Bevel, 5).fitted(tiny), Rounded::All);
        assert!(area(&parts) > 0.0);
        assert!(parts.iter().all(|p| p.width() > 0.0 && p.height() > 0.0));
    }

    #[test]
    fn plate_edges_snap_to_whole_physical_pixels() {
        let rect = Rect::from_min_max(Pos2::new(10.3, 7.7), Pos2::new(50.1, 30.9));
        for ppp in [1.0, 1.25, 1.5, 2.0] {
            let snapped = PixelRect::snapped(rect, ppp);
            assert_eq!(snapped.x0.fract(), 0.0);
            assert_eq!(snapped.y1.fract(), 0.0);
        }
    }

    #[test]
    fn bevel_ring_stays_inside_its_frame() {
        let mut shapes = Vec::new();
        bevel_ring(frame(12.0, 10.0), Corner::new(CornerStyle::Round, 4), Color32::WHITE, Color32::BLACK, &mut shapes);
        let bounds = Rect::from_min_max(Pos2::ZERO, Pos2::new(12.0, 10.0));
        for shape in shapes {
            assert!(bounds.contains_rect(shape.visual_bounding_rect()));
        }
    }

    #[test]
    fn gradient_has_two_vertices_per_stop() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(10.0, 4.0));
        let mesh = gradient_mesh(rect, &[(0.0, Color32::BLACK), (0.5, Color32::RED), (1.0, Color32::WHITE)], true);
        assert_eq!(mesh.vertices.len(), 6);
        assert_eq!(mesh.indices.len(), 12);
    }
}
