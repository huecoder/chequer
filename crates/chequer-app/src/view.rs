
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use chequer_core::document::Document;
use chequer_core::geom::{IPoint, IRect};
use chequer_core::io::SaveOptions;
use chequer_core::raster::Raster;
use eframe::egui::{self, Color32, ColorImage, Context, Pos2, Rect, TextureHandle, TextureOptions, Vec2};

use crate::canvas::{Gesture, ShapeSession};
use crate::text_tool::{CommittedText, TextSession};
use crate::live::LiveOp;
use crate::selection_ui::SelectionOrigin;

pub const ZOOM_MIN: f32 = 0.01;
pub const ZOOM_MAX: f32 = 64.0;
static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(0);
const ZOOM_STEP_RATIO: f32 = 1.12;
const ZOOM_ANCHOR_SNAP: f32 = 0.02;
const ZOOM_MIN_PERCENT: f32 = 1.0;
const ZOOM_LEVEL_EPSILON: f32 = 1.001;
pub const TAB_THUMB_SIZE: u32 = 52;
pub const LAYER_THUMB_SIZE: u32 = 32;
const FIT_PADDING: f32 = 24.0;
const SCROLL_MARGIN_RATIO: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Exact,
    Initial,
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub zoom: f32,
    pub ppp: f32,
    pub origin: Vec2,
    pub needs_initial_fit: bool,
    pub tiled: bool,
}

impl View {
    pub fn new() -> Self {
        Self { zoom: 1.0, ppp: 1.0, origin: Vec2::ZERO, needs_initial_fit: true, tiled: false }
    }

    pub fn scale(&self) -> f32 {
        self.zoom / self.ppp
    }

    fn image_origin(&self, canvas: Rect) -> Pos2 {
        let at = canvas.min + self.origin;
        Pos2::new((at.x * self.ppp).round() / self.ppp, (at.y * self.ppp).round() / self.ppp)
    }

    pub fn image_rect(&self, canvas: Rect, width: u32, height: u32) -> Rect {
        let size_px = Vec2::new((width as f32 * self.zoom).round(), (height as f32 * self.zoom).round());
        Rect::from_min_size(self.image_origin(canvas), size_px / self.ppp)
    }

    pub fn fit(&mut self, canvas: Rect, width: u32, height: u32, mode: Fit) {
        let available = (canvas.size() - Vec2::splat(FIT_PADDING * 2.0)) * self.ppp;
        if available.x <= 0.0 || available.y <= 0.0 {
            return;
        }
        let exact = (available.x / width as f32).min(available.y / height as f32).max(ZOOM_MIN);
        self.zoom = match mode {
            Fit::Exact => exact,
            Fit::Initial => ladder_floor(exact.min(1.0)),
        }
        .clamp(ZOOM_MIN, ZOOM_MAX);
        self.center(canvas, width, height);
        self.needs_initial_fit = false;
    }

    pub fn constrain(&mut self, canvas: Rect, width: u32, height: u32) {
        let image = Vec2::new(width as f32, height as f32) * self.scale();
        self.origin = Vec2::new(
            constrained_axis(self.origin.x, image.x, canvas.width()),
            constrained_axis(self.origin.y, image.y, canvas.height()),
        );
    }

    pub fn center(&mut self, canvas: Rect, width: u32, height: u32) {
        self.origin = (canvas.size() - Vec2::new(width as f32, height as f32) * self.scale()) / 2.0;
    }

    pub fn zoom_at(&mut self, canvas: Rect, anchor: Pos2, new_zoom: f32) {
        let new_zoom = new_zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        let relative = anchor - canvas.min;
        self.origin = relative - (relative - self.origin) * (new_zoom / self.zoom);
        self.zoom = new_zoom;
    }

    pub fn step_zoom(&mut self, canvas: Rect, anchor: Pos2, zoom_in: bool) {
        self.zoom_at(canvas, anchor, next_level(self.zoom, zoom_in));
    }

    pub fn screen_to_image(&self, canvas: Rect, pos: Pos2) -> chequer_core::geom::FPoint {
        let local = (pos - self.image_origin(canvas)) / self.scale();
        chequer_core::geom::FPoint::new(local.x, local.y)
    }

    pub fn image_to_screen(&self, canvas: Rect, x: f32, y: f32) -> Pos2 {
        self.image_origin(canvas) + Vec2::new(x, y) * self.scale()
    }
}

fn constrained_axis(origin: f32, image: f32, view: f32) -> f32 {
    let margin = image * SCROLL_MARGIN_RATIO;
    let (near, far) = (margin, view - image - margin);
    origin.clamp(near.min(far), near.max(far))
}

fn ladder() -> &'static [f32] {
    static LADDER: OnceLock<Vec<f32>> = OnceLock::new();
    LADDER.get_or_init(|| {
        let max_percent = ZOOM_MAX * 100.0;
        let mut up = vec![100.0_f32];
        while let Some(&last) = up.last().filter(|&&last| last < max_percent) {
            up.push(next_percent(last, true).min(max_percent));
        }
        let mut down = vec![100.0_f32];
        while let Some(&last) = down.last().filter(|&&last| last > ZOOM_MIN_PERCENT) {
            down.push(next_percent(last, false).max(ZOOM_MIN_PERCENT));
        }
        down.iter().rev().chain(up.iter().skip(1)).map(|percent| percent / 100.0).collect()
    })
}

fn next_percent(percent: f32, zoom_in: bool) -> f32 {
    let ratio = if zoom_in { ZOOM_STEP_RATIO } else { 1.0 / ZOOM_STEP_RATIO };
    let stepped = (percent * ratio).round();
    let octave = (percent / 100.0).log2();
    let anchor_octave = if zoom_in { octave.floor() + 1.0 } else { octave.ceil() - 1.0 };
    let anchor = (100.0 * anchor_octave.exp2()).round();
    let reaches_anchor = if zoom_in { stepped >= anchor * (1.0 - ZOOM_ANCHOR_SNAP) } else { stepped <= anchor * (1.0 + ZOOM_ANCHOR_SNAP) };
    let snapped = if reaches_anchor { anchor } else { stepped };
    if zoom_in { snapped.max(percent + 1.0) } else { snapped.min(percent - 1.0) }
}

fn levels() -> impl DoubleEndedIterator<Item = f32> {
    ladder().iter().copied()
}

fn next_level(zoom: f32, zoom_in: bool) -> f32 {
    if zoom_in {
        levels().find(|&level| level > zoom * ZOOM_LEVEL_EPSILON).unwrap_or(ZOOM_MAX)
    } else {
        levels().rev().find(|&level| level < zoom / ZOOM_LEVEL_EPSILON).unwrap_or(ZOOM_MIN)
    }
}

fn ladder_floor(zoom: f32) -> f32 {
    levels().rev().find(|&level| level <= zoom * ZOOM_LEVEL_EPSILON).unwrap_or(zoom)
}

struct CanvasTexture {
    handle: TextureHandle,
    size: [usize; 2],
    options: TextureOptions,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZoomOutFilter {
    Pixel,
    #[default]
    Smooth,
}

impl ZoomOutFilter {
    pub fn to_setting(self) -> &'static str {
        match self {
            ZoomOutFilter::Pixel => "pixel",
            ZoomOutFilter::Smooth => "smooth",
        }
    }

    pub fn from_setting(value: &str) -> Self {
        if value == "pixel" { ZoomOutFilter::Pixel } else { ZoomOutFilter::Smooth }
    }

    fn texture_options(self, zoom: f32) -> TextureOptions {
        if zoom < 1.0 && self == ZoomOutFilter::Smooth { TextureOptions::LINEAR } else { TextureOptions::NEAREST }
    }
}

pub struct DocTab {
    pub doc: Document,
    pub name: String,
    pub path: Option<PathBuf>,
    pub save_options: Option<SaveOptions>,
    pub saved_revision: u64,
    pub id: u64,
    pub autosaved_revision: Option<u64>,
    pub view: View,
    pub gesture: Gesture,
    pub shape: Option<ShapeSession>,
    pub text: Option<TextSession>,
    pub committed_text: Option<CommittedText>,
    pub committed_shape: Option<crate::canvas::CommittedShape>,
    pub live: Option<LiveOp>,
    pub sel_origin: Option<SelectionOrigin>,
    pub clone_origin: Option<IPoint>,
    pub clone_offset: IPoint,
    canvas_texture: Option<CanvasTexture>,
    dirty: IRect,
    full_refresh: bool,
    shown_frame: usize,
    thumb: Option<((u64, usize), TextureHandle)>,
    layer_thumbs: Option<((u64, usize), Vec<TextureHandle>)>,
    timeline_cache: Option<crate::timeline::TimelineCache>,
    onion: Option<crate::timeline::OnionCache>,
}

impl DocTab {
    pub fn new(doc: Document, name: String, path: Option<PathBuf>) -> Self {
        let saved_revision = doc.revision();
        Self {
            doc,
            name,
            path,
            save_options: None,
            saved_revision,
            id: NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed),
            autosaved_revision: None,
            view: View::new(),
            gesture: Gesture::None,
            shape: None,
            text: None,
            committed_text: None,
            committed_shape: None,
            live: None,
            sel_origin: None,
            clone_origin: None,
            clone_offset: IPoint::default(),
            canvas_texture: None,
            dirty: IRect::EMPTY,
            full_refresh: true,
            shown_frame: 0,
            thumb: None,
            layer_thumbs: None,
            timeline_cache: None,
            onion: None,
        }
    }

    pub fn is_modified(&self) -> bool {
        self.doc.revision() != self.saved_revision
    }

    pub fn can_undo(&self) -> bool {
        self.doc.history().can_undo() || self.doc.floating().is_some() || self.shape.is_some() || self.live.is_some()
    }

    pub fn mark_modified(&mut self) {
        self.saved_revision = self.doc.revision().wrapping_add(1);
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = self.doc.revision();
    }

    pub fn mark_dirty(&mut self, area: IRect) {
        self.dirty = self.dirty.union(area);
    }

    pub fn mark_all_dirty(&mut self) {
        self.full_refresh = true;
    }

    pub fn cancel_stroke(&mut self) {
        if let Gesture::Stroke { stroke } = std::mem::replace(&mut self.gesture, Gesture::None) {
            self.doc.cancel_stroke(stroke);
            self.mark_all_dirty();
        }
    }

    pub fn end_gesture(&mut self) {
        if let Gesture::Stroke { stroke } = std::mem::replace(&mut self.gesture, Gesture::None) {
            self.doc.finish_stroke(stroke);
        }
    }

    pub fn sync_texture(&mut self, ctx: &Context, zoom_out: ZoomOutFilter) {
        let size = [self.doc.width() as usize, self.doc.height() as usize];
        let options = zoom_out.texture_options(self.view.zoom);
        let stale = self.canvas_texture.as_ref().is_none_or(|t| t.size != size);
        let refiltered = self.canvas_texture.as_ref().is_some_and(|t| t.options != options);
        let other_frame = self.shown_frame != self.doc.current_frame();
        if stale || refiltered || other_frame || self.full_refresh {
            self.shown_frame = self.doc.current_frame();
            let image = to_color_image(&self.doc.composite(self.doc.bounds()));
            match self.canvas_texture.as_mut().filter(|_| !stale) {
                Some(texture) => {
                    texture.handle.set(image, options);
                    texture.options = options;
                }
                None => {
                    let handle = ctx.load_texture("canvas", image, options);
                    self.canvas_texture = Some(CanvasTexture { handle, size, options });
                }
            }
            self.dirty = IRect::EMPTY;
            self.full_refresh = false;
            return;
        }
        let area = self.dirty.intersect(self.doc.bounds());
        self.dirty = IRect::EMPTY;
        if area.is_empty() {
            return;
        }
        if let Some(texture) = self.canvas_texture.as_mut() {
            let image = to_color_image(&self.doc.composite(area));
            texture.handle.set_partial([area.x0 as usize, area.y0 as usize], image, options);
        }
    }

    pub fn go_to_frame(&mut self, frame: usize) {
        if self.doc.set_current_frame(frame) {
            self.mark_all_dirty();
        }
    }

    pub fn timeline_cache(&mut self, ctx: &Context, with_thumbs: bool) -> &crate::timeline::TimelineCache {
        let stamp = (self.doc.revision(), self.doc.frame_count(), self.doc.layers().len());
        let fresh = self.timeline_cache.take().filter(|cache| cache.stamp() == stamp && (!with_thumbs || cache.has_thumbs()));
        let cache = fresh.unwrap_or_else(|| crate::timeline::TimelineCache::build(&self.doc, stamp, ctx, with_thumbs));
        self.timeline_cache.insert(cache)
    }

    pub fn sync_onion(&mut self, ctx: &Context, enabled: bool, settings: crate::timeline::OnionSettings) {
        let wanted = enabled.then(|| crate::timeline::OnionSignature::of(&self.doc, settings)).filter(|signature| !signature.is_empty());
        let unchanged = self.onion.as_ref().map(crate::timeline::OnionCache::signature) == wanted.as_ref();
        if unchanged {
            return;
        }
        self.onion = wanted.map(|signature| crate::timeline::OnionCache::build(&self.doc, signature, ctx));
    }

    pub fn onion_layers(&self) -> &[crate::timeline::OnionLayer] {
        self.onion.as_ref().map_or(&[], crate::timeline::OnionCache::layers)
    }

    pub fn canvas_texture_id(&self) -> Option<egui::TextureId> {
        self.canvas_texture.as_ref().map(|t| t.handle.id())
    }

    fn content_stamp(&self) -> (u64, usize) {
        (self.doc.revision(), self.doc.current_frame())
    }

    pub fn tab_thumbnail(&mut self, ctx: &Context) -> egui::TextureId {
        let stamp = self.content_stamp();
        if self.thumb.as_ref().is_none_or(|(known, _)| *known != stamp) {
            let image = to_color_image(&self.doc.thumbnail(TAB_THUMB_SIZE));
            self.thumb = Some((stamp, ctx.load_texture("tab-thumb", image, TextureOptions::NEAREST)));
        }
        self.thumb.as_ref().map(|(_, t)| t.id()).expect("миниатюра создана выше")
    }

    pub fn layer_thumbnails(&mut self, ctx: &Context) -> &[TextureHandle] {
        let stamp = self.content_stamp();
        if self.layer_thumbs.as_ref().is_none_or(|(known, _)| *known != stamp) {
            let thumbs = self
                .doc
                .layers()
                .iter()
                .map(|layer| {
                    let image = to_color_image(&sample_down(layer.raster(), LAYER_THUMB_SIZE));
                    ctx.load_texture("layer-thumb", image, TextureOptions::NEAREST)
                })
                .collect();
            self.layer_thumbs = Some((stamp, thumbs));
        }
        self.layer_thumbs.as_ref().map(|(_, t)| t.as_slice()).unwrap_or_default()
    }
}

pub fn to_color_image(raster: &Raster) -> ColorImage {
    let pixels = raster
        .as_bytes()
        .chunks_exact(4)
        .map(|p| match p[3] {
            u8::MAX => Color32::from_rgb(p[0], p[1], p[2]),
            0 => Color32::TRANSPARENT,
            alpha => Color32::from_rgba_unmultiplied(p[0], p[1], p[2], alpha),
        })
        .collect();
    ColorImage::new([raster.width() as usize, raster.height() as usize], pixels)
}

pub(crate) fn sample_down(raster: &Raster, max_side: u32) -> Raster {
    let longest = raster.width().max(raster.height()) as f32;
    let scale = (max_side as f32 / longest).min(1.0);
    let side = |full: u32| ((full as f32 * scale).round() as u32).max(1);
    let (width, height) = (side(raster.width()), side(raster.height()));
    let mut out = Raster::transparent(width, height);
    for y in 0..height {
        for x in 0..width {
            let sx = (((x as f32 + 0.5) / scale) as u32).min(raster.width() - 1);
            let sy = (((y as f32 + 0.5) / scale) as u32).min(raster.height() - 1);
            out.set(x, y, raster.get(sx, sy));
        }
    }
    out
}

pub fn color32(color: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(1000.0, 600.0))
    }

    #[test]
    fn wheel_steps_walk_the_ladder_both_ways() {
        assert_eq!(next_level(1.0, true), 1.12);
        assert_eq!(next_level(1.12, false), 1.0);
        assert_eq!(next_level(0.99999, true), 1.12);
        assert_eq!(next_level(ZOOM_MAX, true), ZOOM_MAX);
        assert_eq!(next_level(ZOOM_MIN, false), ZOOM_MIN);
    }

    #[test]
    fn ladder_matches_paint_net_around_100_percent() {
        let percents: Vec<u32> = levels().map(|level| (level * 100.0).round() as u32).collect();
        let start = percents.iter().position(|&p| p == 100).unwrap();
        assert_eq!(percents[start..start + 7], [100, 112, 125, 140, 157, 176, 200]);
        assert!(percents.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!((percents[0], *percents.last().unwrap()), (1, 6400));
    }

    #[test]
    fn off_ladder_zoom_snaps_to_neighbours() {
        assert_eq!(next_level(0.7, true), 0.71);
        assert_eq!(next_level(0.7, false), 0.63);
    }

    #[test]
    fn initial_fit_lands_on_a_ladder_step_not_above_100_percent() {
        let mut view = View::new();
        view.fit(canvas(), 800, 600, Fit::Initial);
        assert_eq!(view.zoom, 0.89);
        view.fit(canvas(), 100, 100, Fit::Initial);
        assert_eq!(view.zoom, 1.0);
    }

    #[test]
    fn fit_waits_while_the_window_is_too_small_to_hold_the_padding() {
        let mut view = View::new();
        view.fit(Rect::from_min_size(Pos2::ZERO, Vec2::ZERO), 800, 600, Fit::Initial);
        assert!(view.needs_initial_fit);
        assert_eq!(view.zoom, 1.0);
    }

    #[test]
    fn exact_fit_may_upscale() {
        let mut view = View::new();
        view.fit(canvas(), 100, 100, Fit::Exact);
        assert!(view.zoom > 1.0);
    }

    #[test]
    fn hundred_percent_is_one_physical_pixel_per_image_pixel() {
        let mut view = View::new();
        view.ppp = 1.25;
        view.zoom = 1.0;
        let rect = view.image_rect(canvas(), 800, 600);
        assert_eq!(rect.width() * 1.25, 800.0);
        assert_eq!(rect.height() * 1.25, 600.0);
        assert_eq!((rect.min.x * 1.25).fract(), 0.0);
    }

    #[test]
    fn image_can_be_moved_one_image_size_past_each_edge() {
        let mut view = View::new();
        view.origin = Vec2::new(5000.0, -9000.0);
        view.constrain(canvas(), 4000, 3000);
        assert_eq!(view.origin, Vec2::new(4000.0, 600.0 - 6000.0));
    }

    #[test]
    fn small_image_moves_freely_inside_the_window() {
        let mut view = View::new();
        view.origin = Vec2::new(400.0, -300.0);
        view.constrain(canvas(), 100, 100);
        assert_eq!(view.origin, Vec2::new(400.0, 100.0));
    }

    #[test]
    fn screen_and_image_coordinates_round_trip() {
        let mut view = View::new();
        view.ppp = 1.5;
        view.zoom = 4.0;
        view.origin = Vec2::new(13.0, -7.0);
        let screen = view.image_to_screen(canvas(), 12.0, 34.0);
        let back = view.screen_to_image(canvas(), screen);
        assert!((back.x - 12.0).abs() < 1e-3 && (back.y - 34.0).abs() < 1e-3);
    }

    #[test]
    fn zoom_keeps_the_anchor_pixel_in_place() {
        let mut view = View::new();
        view.origin = Vec2::new(40.0, 30.0);
        let anchor = Pos2::new(300.0, 200.0);
        let before = view.screen_to_image(canvas(), anchor);
        view.zoom_at(canvas(), anchor, 3.0);
        let after = view.screen_to_image(canvas(), anchor);
        assert!((before.x - after.x).abs() < 0.01 && (before.y - after.y).abs() < 0.01);
    }
}
