use std::sync::Arc;

use chequer_core::brush::{Brush, PreviewStyle, stroke_preview};
use eframe::egui::{self, Color32, Context, Rect, TextureHandle, TextureOptions, Ui, pos2};

use crate::skin;
use crate::view::to_color_image;

const MIN_PREVIEW_PIXELS: f32 = 1.0;

pub fn preview_texture(
    ctx: &Context,
    name: &str,
    brush: &Arc<Brush>,
    brush_size: f32,
    pixels: egui::Vec2,
) -> TextureHandle {
    let ink = skin::theme().ink;
    let style = PreviewStyle {
        size: brush_size,
        color: [ink.r(), ink.g(), ink.b(), u8::MAX],
    };
    let scale = ctx.pixels_per_point();
    let pixel_count = |points: f32| (points * scale).round().max(MIN_PREVIEW_PIXELS) as u32;
    let raster = stroke_preview(brush, &style, pixel_count(pixels.x), pixel_count(pixels.y));
    ctx.load_texture(name, to_color_image(&raster), TextureOptions::LINEAR)
}

pub fn paint_texture(ui: &Ui, texture: &TextureHandle, rect: Rect) {
    ui.painter().image(
        texture.id(),
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}
