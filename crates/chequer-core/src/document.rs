use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::blend::{BlendMode, blend};
use crate::draw::{Stroke, StrokeParams, StrokeTarget};
use crate::edit::PixelEdit;
use crate::frames::{Frame, MAX_FRAMES};
use crate::geom::{FPoint, IPoint, IRect};
use crate::history::{Change, History, HistoryKind};
use crate::raster::{BYTES_PER_PIXEL, Raster, Rgba, TRANSPARENT, erases, lerp_premultiplied, mul_div_255, over, scale_alpha};
use crate::selection::{CombineMode, Selection, SharedSelection, combine, flood_select};
use crate::draw::dirty_area;
use crate::shapes::Overlay;
use crate::transform::{Placement, Quad, RotationMethod, coverage_plane, render_placed, render_quad};

fn overlay_pixel(overlay: &Overlay, wrap: Option<(i32, i32)>, pixel: Rgba, x: i32, y: i32, coverage: u8) -> Rgba {
    let (column, row) = (x - overlay.origin.x, y - overlay.origin.y);
    let (width, height) = (overlay.raster.width() as i32, overlay.raster.height() as i32);
    let (period_x, period_y) = wrap.unwrap_or((i32::MAX, i32::MAX));
    let first = |offset: i32, period: i32| if wrap.is_some() { offset.rem_euclid(period) } else { offset };
    let mut result = pixel;
    let mut local_x = first(column, period_x);
    while local_x < width {
        let mut local_y = first(row, period_y);
        while local_y < height {
            if let Some(source) = overlay.raster.get_at(IPoint::new(local_x, local_y)) {
                result = apply_overlay_pixel(overlay, result, source, coverage, overlay.lcd_at_local(local_x, local_y));
            }
            local_y = local_y.saturating_add(period_y);
        }
        local_x = local_x.saturating_add(period_x);
    }
    result
}

fn apply_overlay_pixel(overlay: &Overlay, pixel: Rgba, source: Rgba, coverage: u8, lcd: Option<[u8; 3]>) -> Rgba {
    if let Some(channels) = lcd {
        apply_lcd_pixel(pixel, source, channels, coverage)
    } else if overlay.erase {
        lerp_premultiplied(pixel, TRANSPARENT, scale_alpha(source, coverage)[3])
    } else {
        blend(overlay.blend, pixel, scale_alpha(source, coverage))
    }
}

fn apply_lcd_pixel(pixel: Rgba, color: Rgba, channels: [u8; 3], selection: u8) -> Rgba {
    let weights = channels.map(|weight| mul_div_255(u32::from(weight), u32::from(selection)) as u8);
    if pixel[3] != OPAQUE {
        let average = (weights.iter().map(|&w| u32::from(w)).sum::<u32>() / 3) as u8;
        return over(pixel, scale_alpha(color, average));
    }
    let mixed = |channel: usize| {
        let (backdrop, ink, weight) = (u32::from(pixel[channel]), u32::from(color[channel]), u32::from(weights[channel]));
        mul_div_255(backdrop, u32::from(OPAQUE) - weight) as u8 + mul_div_255(ink, weight) as u8
    };
    [mixed(0), mixed(1), mixed(2), OPAQUE]
}

pub const MAX_SIDE: u32 = 8192;
const FULL_COVERAGE: u8 = u8::MAX;
const OPAQUE: u8 = u8::MAX;
const ERASE_MASK: Rgba = [0, 0, 0, 255];

#[derive(Debug)]
pub enum DocumentError {
    SideOutOfRange { width: u32, height: u32 },
    InconsistentFrames,
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SideOutOfRange { width, height } => {
                write!(f, "размер {width}×{height} вне диапазона 1..={MAX_SIDE}")
            }
            Self::InconsistentFrames => write!(f, "у слоёв разное число кадров или размеры кадров не совпадают"),
        }
    }
}

impl std::error::Error for DocumentError {}

pub fn check_size(width: u32, height: u32) -> Result<(), DocumentError> {
    let valid = |side: u32| (1..=MAX_SIDE).contains(&side);
    if valid(width) && valid(height) { Ok(()) } else { Err(DocumentError::SideOutOfRange { width, height }) }
}

fn mirror_horizontally(area: IRect, x: i32, y: i32) -> IPoint {
    IPoint::new(area.x0 + area.x1 - 1 - x, y)
}

fn mirror_vertically(area: IRect, x: i32, y: i32) -> IPoint {
    IPoint::new(x, area.y0 + area.y1 - 1 - y)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerProps {
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub blend: BlendMode,
    pub masked: bool,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub(crate) cels: Vec<Arc<Raster>>,
    pub(crate) frame: usize,
    pub visible: bool,
    pub opacity: u8,
    pub blend: BlendMode,
    pub masked: bool,
    pub collapsed: bool,
}

impl Layer {
    pub fn new(name: String, raster: Raster) -> Self {
        Self::with_cels(name, vec![Arc::new(raster)])
    }

    pub fn with_cels(name: String, cels: Vec<Arc<Raster>>) -> Self {
        Self { name, cels, frame: 0, visible: true, opacity: OPAQUE, blend: BlendMode::Normal, masked: false, collapsed: false }
    }

    pub fn blank(name: String, width: u32, height: u32, frames: usize) -> Self {
        Self::with_cels(name, vec![Arc::new(Raster::transparent(width, height)); frames])
    }

    pub fn raster(&self) -> &Raster {
        &self.cels[self.frame]
    }

    pub fn raster_mut(&mut self) -> &mut Raster {
        Arc::make_mut(&mut self.cels[self.frame])
    }

    pub fn props(&self) -> LayerProps {
        LayerProps { name: self.name.clone(), visible: self.visible, opacity: self.opacity, blend: self.blend, masked: self.masked }
    }

    pub fn set_props(&mut self, props: &LayerProps) {
        self.name = props.name.clone();
        self.visible = props.visible;
        self.opacity = props.opacity;
        self.blend = props.blend;
        self.masked = props.masked;
    }
}

#[derive(Clone, Copy)]
struct LayerCel<'a> {
    layer: &'a Layer,
    cel: &'a Raster,
}

#[derive(Clone, Debug)]
pub struct CanvasState {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaMode {
    Blend,
    Replace,
}

impl AlphaMode {
    fn apply(self, backdrop: Rgba, pixel: Rgba, coverage: u8) -> Rgba {
        match self {
            AlphaMode::Replace => lerp_premultiplied(backdrop, pixel, coverage),
            AlphaMode::Blend => over(backdrop, scale_alpha(pixel, coverage)),
        }
    }
}

pub type Pivot = (f32, f32);
pub const CENTER_PIVOT: Pivot = (0.5, 0.5);

#[derive(Clone, Copy, PartialEq)]
struct Pose {
    placement: Placement,
    quad: Option<Quad>,
}

pub struct Floating {
    pixels: Raster,
    selection: Selection,
    source: Raster,
    source_plane: Raster,
    placement: Placement,
    quad: Option<Quad>,
    undo: Vec<Pose>,
    redo: Vec<Pose>,
    step_open: bool,
    home: Placement,
    pivot: Pivot,
    selection_before: Option<SharedSelection>,
    edit: PixelEdit,
    layer: usize,
    mode: AlphaMode,
    kind: HistoryKind,
    moved: bool,
}

impl Floating {
    pub fn bounds(&self) -> IRect {
        self.selection.bounds()
    }

    pub fn mode(&self) -> AlphaMode {
        self.mode
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn placement(&self) -> Placement {
        self.placement
    }

    pub fn quad(&self) -> Option<Quad> {
        self.quad
    }

    pub fn home(&self) -> Placement {
        self.home
    }

    pub fn pivot(&self) -> Pivot {
        self.pivot
    }

    pub fn pivot_point(&self) -> crate::geom::FPoint {
        match self.quad {
            Some(quad) => quad.point(self.pivot.0, self.pivot.1),
            None => self.placement.point(self.pivot.0, self.pivot.1),
        }
    }

    pub fn pivot_fractions_of(&self, p: crate::geom::FPoint) -> Option<Pivot> {
        match self.quad {
            Some(quad) => quad.fractions_of(p),
            None => Some(self.placement.fractions_of(p)),
        }
    }

    fn pose(&self) -> Pose {
        Pose { placement: self.placement, quad: self.quad }
    }

    fn set_pose(&mut self, pose: Pose) {
        self.placement = pose.placement;
        self.quad = pose.quad;
    }

    fn record_step(&mut self) {
        if !self.step_open {
            self.undo.push(self.pose());
            self.redo.clear();
            self.step_open = true;
        }
    }

    fn realize(&mut self, resample: Resample, rotation: RotationMethod) -> bool {
        let placed = match self.quad {
            Some(quad) => render_quad(&self.source, &self.source_plane, &quad, resample),
            None => render_placed(&self.source, &self.source_plane, &self.placement, resample, rotation),
        };
        match placed {
            Some(placed) => {
                self.pixels = placed.pixels;
                self.selection = placed.selection;
                true
            }
            None => false,
        }
    }

    fn row(&self, y: i32) -> Option<FloatingRow<'_>> {
        let bounds = self.selection.bounds();
        (bounds.y0..bounds.y1).contains(&y).then(|| FloatingRow {
            x0: bounds.x0,
            coverage: self.selection.coverage_row(y, bounds.x0, bounds.x1),
            pixels: self.pixels.row_bytes((y - bounds.y0) as u32, 0, bounds.width() as u32),
            mode: self.mode,
        })
    }
}

struct FloatingRow<'a> {
    x0: i32,
    coverage: &'a [u8],
    pixels: &'a [u8],
    mode: AlphaMode,
}

impl FloatingRow<'_> {
    fn apply(&self, backdrop: Rgba, x: i32) -> Rgba {
        let Some(index) = usize::try_from(x - self.x0).ok().filter(|&i| i < self.coverage.len()) else {
            return backdrop;
        };
        let coverage = self.coverage[index];
        if coverage == 0 {
            return backdrop;
        }
        let at = index * BYTES_PER_PIXEL;
        self.mode.apply(backdrop, [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2], self.pixels[at + 3]], coverage)
    }
}

pub struct ActiveStroke {
    stroke: Stroke,
    edit: PixelEdit,
    kind: HistoryKind,
}

impl ActiveStroke {
    pub fn set_pressure(&mut self, pressure: f32) {
        self.stroke.set_pressure(pressure);
    }

    pub fn set_tilt(&mut self, tilt: Option<crate::draw::Tilt>) {
        self.stroke.set_tilt(tilt);
    }

    pub fn set_time_ms(&mut self, time_ms: f64) {
        self.stroke.set_time_ms(time_ms);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    fn offset(self, old: (u32, u32), new: (u32, u32)) -> IPoint {
        let (dx, dy) = (new.0 as i32 - old.0 as i32, new.1 as i32 - old.1 as i32);
        let (col, row) = match self {
            Anchor::TopLeft => (0, 0),
            Anchor::Top => (1, 0),
            Anchor::TopRight => (2, 0),
            Anchor::Left => (0, 1),
            Anchor::Center => (1, 1),
            Anchor::Right => (2, 1),
            Anchor::BottomLeft => (0, 2),
            Anchor::Bottom => (1, 2),
            Anchor::BottomRight => (2, 2),
        };
        IPoint::new(dx * col / 2, dy * row / 2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resample {
    Nearest,
    Bilinear,
    Bicubic,
    Lanczos,
    Anisotropic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentPalette {
    pub name: String,
    pub colors: Vec<[u8; 3]>,
}

pub struct Document {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) layers: Vec<Layer>,
    pub(crate) frames: Vec<Frame>,
    pub(crate) frame: usize,
    pub(crate) active: usize,
    pub palette: Option<DocumentPalette>,
    pub(crate) selection: Option<SharedSelection>,
    pub(crate) floating: Option<Floating>,
    preview: Option<Overlay>,
    tiled: bool,
    pub(crate) history: History,
    pub(crate) revision: u64,
}

impl Document {
    pub fn new(width: u32, height: u32, background: Rgba, layer_name: String) -> Result<Self, DocumentError> {
        check_size(width, height)?;
        Ok(Self::from_raster(layer_name, Raster::filled(width, height, background)))
    }

    pub fn from_raster(layer_name: String, raster: Raster) -> Self {
        Self {
            width: raster.width(),
            height: raster.height(),
            layers: vec![Layer::new(layer_name, raster)],
            frames: vec![Frame::DEFAULT],
            frame: 0,
            active: 0,
            palette: None,
            selection: None,
            floating: None,
            preview: None,
            tiled: false,
            history: History::default(),
            revision: 0,
        }
    }

    pub fn from_layers(layers: Vec<Layer>, active: usize) -> Result<Self, DocumentError> {
        let frames = vec![Frame::DEFAULT; layers.first().map_or(1, Layer::frame_count)];
        Self::from_frames(layers, frames, active)
    }

    pub fn from_frames(layers: Vec<Layer>, frames: Vec<Frame>, active: usize) -> Result<Self, DocumentError> {
        let (width, height) = layers.first().map_or((0, 0), |layer| (layer.raster().width(), layer.raster().height()));
        check_size(width, height)?;
        let consistent = !frames.is_empty()
            && frames.len() <= MAX_FRAMES
            && layers.iter().all(|layer| layer.frame_count() == frames.len() && layer.cels().iter().all(|cel| (cel.width(), cel.height()) == (width, height)));
        if !consistent {
            return Err(DocumentError::InconsistentFrames);
        }
        let active = active.min(layers.len().saturating_sub(1));
        Ok(Self { width, height, layers, frames, active, ..Self::from_raster(String::new(), Raster::transparent(1, 1)) })
    }

    pub fn snapshot(&self) -> Document {
        Document {
            width: self.width,
            height: self.height,
            layers: self.layers.clone(),
            frames: self.frames.clone(),
            frame: self.frame,
            active: self.active,
            palette: self.palette.clone(),
            selection: self.selection.clone(),
            floating: None,
            preview: None,
            tiled: self.tiled,
            history: self.history.clone(),
            revision: self.revision,
        }
    }

    pub fn active_index(&self) -> usize {
        self.active
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

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn active_layer(&self) -> usize {
        self.active
    }

    pub fn set_active_layer(&mut self, index: usize) {
        if index < self.layers.len() {
            self.settle();
            self.active = index;
        }
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn selection(&self) -> Option<&Selection> {
        self.floating.as_ref().map(|f| &f.selection).or(self.selection.as_deref())
    }

    pub fn floating(&self) -> Option<&Floating> {
        self.floating.as_ref()
    }

    pub fn active_raster(&self) -> &Raster {
        self.layers[self.active].raster()
    }

    pub fn composite(&self, area: IRect) -> Raster {
        self.composite_frame(self.frame, area, true, None)
    }

    pub fn flattened(&self, area: IRect) -> Raster {
        self.composite_frame(self.frame, area, false, None)
    }

    pub fn flattened_frame(&self, frame: usize, area: IRect) -> Raster {
        self.composite_frame(frame, area, false, None)
    }

    pub fn frame_changes_against(&self, frame: usize, against: usize) -> Raster {
        self.composite_frame(frame, self.bounds(), false, Some(against))
    }

    pub fn composite_pixel(&self, x: i32, y: i32) -> Rgba {
        if !self.bounds().contains(IPoint::new(x, y)) {
            return TRANSPARENT;
        }
        self.layers
            .iter()
            .enumerate()
            .filter(|(index, _)| self.is_shown(*index))
            .fold(TRANSPARENT, |below, (index, layer)| blend(layer.blend, below, self.masked_pixel(self.frame, index, layer, x, y)))
    }

    pub fn mask_parent(&self, index: usize) -> Option<usize> {
        if !self.layers.get(index)?.masked {
            return None;
        }
        (index + 1..self.layers.len()).find(|&above| !self.layers[above].masked)
    }

    pub fn is_mask(&self, index: usize) -> bool {
        index > 0 && !self.layers[index].masked && self.layers[index - 1].masked
    }

    fn is_shown(&self, index: usize) -> bool {
        self.layers[index].visible && !self.is_mask(index) && self.mask_parent(index).is_none_or(|parent| self.layers[parent].visible)
    }

    fn masked_pixel(&self, frame: usize, index: usize, layer: &Layer, x: i32, y: i32) -> Rgba {
        let pixel = self.layer_pixel(frame, index, layer, x, y, true);
        match self.mask_parent(index) {
            Some(parent) => scale_alpha(pixel, self.layer_pixel(frame, parent, &self.layers[parent], x, y, true)[3]),
            None => pixel,
        }
    }

    fn mask_row(&self, frame: usize, parent: usize, area: IRect, y: i32, with_overlays: bool) -> Vec<u8> {
        let layer = &self.layers[parent];
        (area.x0..area.x1).map(|x| self.layer_pixel(frame, parent, layer, x, y, with_overlays)[3]).collect()
    }

    fn composite_frame(&self, frame: usize, area: IRect, with_overlays: bool, against: Option<usize>) -> Raster {
        let area = area.intersect(self.bounds());
        let mut out = Raster::transparent(area.width() as u32, area.height() as u32);
        let differs = |layer: &Layer| against.is_none_or(|other| !Arc::ptr_eq(layer.cel(frame), layer.cel(other)));
        for (index, layer) in self.layers.iter().enumerate().filter(|(index, layer)| self.is_shown(*index) && differs(layer)) {
            let overlaid = with_overlays && index == self.active;
            let mask = self.mask_parent(index);
            for y in area.y0..area.y1 {
                let mask_row = mask.map(|parent| self.mask_row(frame, parent, area, y, with_overlays));
                self.composite_row(&mut out, area, y, LayerCel { layer, cel: layer.cel(frame) }, overlaid, mask_row.as_deref());
            }
        }
        out
    }

    fn composite_row(&self, out: &mut Raster, area: IRect, y: i32, source: LayerCel, overlaid: bool, mask: Option<&[u8]>) {
        let LayerCel { layer, cel } = source;
        let source = cel.row_bytes(y as u32, area.x0 as u32, area.x1 as u32);
        let target = out.row_bytes_mut((y - area.y0) as u32, 0, area.width() as u32);
        let floating = overlaid.then(|| self.floating.as_ref().and_then(|f| f.row(y))).flatten();
        let preview_span = overlaid.then(|| self.preview.as_ref().map(|o| self.wrapped_area(o.bounds()))).flatten().filter(|b| (b.y0..b.y1).contains(&y));
        for (i, (src, dst)) in source.chunks_exact(BYTES_PER_PIXEL).zip(target.chunks_exact_mut(BYTES_PER_PIXEL)).enumerate() {
            let x = area.x0 + i as i32;
            let mut pixel = [src[0], src[1], src[2], src[3]];
            if let Some(row) = &floating {
                pixel = row.apply(pixel, x);
            }
            if preview_span.is_some_and(|b| (b.x0..b.x1).contains(&x)) {
                pixel = self.apply_preview(pixel, x, y);
            }
            if let Some(mask) = mask {
                pixel = scale_alpha(pixel, mask[i]);
            }
            if pixel[3] == 0 {
                continue;
            }
            if layer.opacity != OPAQUE {
                pixel = scale_alpha(pixel, layer.opacity);
            }
            let merged = if pixel[3] == OPAQUE && layer.blend == BlendMode::Normal { pixel } else { blend(layer.blend, [dst[0], dst[1], dst[2], dst[3]], pixel) };
            dst.copy_from_slice(&merged);
        }
    }

    fn layer_pixel(&self, frame: usize, index: usize, layer: &Layer, x: i32, y: i32, with_overlays: bool) -> Rgba {
        let mut pixel = layer.cel(frame).get(x as u32, y as u32);
        if with_overlays && index == self.active {
            pixel = self.apply_overlays(pixel, x, y);
        }
        scale_alpha(pixel, layer.opacity)
    }

    fn apply_overlays(&self, mut pixel: Rgba, x: i32, y: i32) -> Rgba {
        if let Some(row) = self.floating.as_ref().and_then(|f| f.row(y)) {
            pixel = row.apply(pixel, x);
        }
        self.apply_preview(pixel, x, y)
    }

    fn apply_preview(&self, pixel: Rgba, x: i32, y: i32) -> Rgba {
        let Some(o) = &self.preview else {
            return pixel;
        };
        let coverage = self.selection.as_deref().map_or(FULL_COVERAGE, |s| s.coverage_at(x, y));
        overlay_pixel(o, self.wrap_period(), pixel, x, y, coverage)
    }

    pub fn tiled(&self) -> bool {
        self.tiled
    }

    pub fn set_tiled(&mut self, tiled: bool) {
        self.tiled = tiled;
    }

    fn wrap_period(&self) -> Option<(i32, i32)> {
        self.tiled.then_some((self.width as i32, self.height as i32))
    }

    fn wrapped_area(&self, area: IRect) -> IRect {
        dirty_area(area, self.bounds(), self.tiled)
    }

    pub fn overlay_canvas(&self) -> IRect {
        let (width, height) = (self.width as i32, self.height as i32);
        if self.tiled { IRect::new(-width, -height, 2 * width, 2 * height) } else { self.bounds() }
    }

    pub fn begin_stroke(&mut self, params: StrokeParams, at: FPoint, kind: HistoryKind) -> Option<(ActiveStroke, IRect)> {
        self.settle();
        let index = self.active;
        if !self.layers[index].visible {
            return None;
        }
        let layer = &mut self.layers[index];
        let mut edit = PixelEdit::new(index, self.frame, layer.raster());
        let mut target = StrokeTarget { raster: layer.raster_mut(), edit: &mut edit, selection: self.selection.as_deref() };
        let (stroke, dirty) = Stroke::begin(params, at, &mut target);
        Some((ActiveStroke { stroke, edit, kind }, dirty))
    }

    pub fn extend_stroke(&mut self, active: &mut ActiveStroke, to: FPoint) -> IRect {
        let layer = &mut self.layers[active.edit.layer()];
        let mut target =
            StrokeTarget { raster: layer.raster_mut(), edit: &mut active.edit, selection: self.selection.as_deref() };
        active.stroke.extend(to, &mut target)
    }

    pub fn flush_stroke(&mut self, active: &mut ActiveStroke, end: FPoint) -> IRect {
        let layer = &mut self.layers[active.edit.layer()];
        let mut target =
            StrokeTarget { raster: layer.raster_mut(), edit: &mut active.edit, selection: self.selection.as_deref() };
        active.stroke.flush(end, &mut target)
    }

    pub fn finish_stroke(&mut self, active: ActiveStroke) {
        let index = active.edit.layer();
        let change = active.edit.finish(self.layers[index].raster());
        self.push_entry(active.kind, change.into_iter().collect(), index);
    }

    pub fn cancel_stroke(&mut self, active: ActiveStroke) {
        let index = active.edit.layer();
        active.edit.revert(self.layers[index].raster_mut());
        self.revision += 1;
    }

    pub fn set_preview(&mut self, overlay: Option<Overlay>) -> IRect {
        let old = self.preview.take().map_or(IRect::EMPTY, |o| o.bounds());
        let new = overlay.as_ref().map_or(IRect::EMPTY, Overlay::bounds);
        self.preview = overlay;
        self.wrapped_area(old).union(self.wrapped_area(new))
    }

    pub fn commit_preview(&mut self, kind: HistoryKind) -> IRect {
        let Some(overlay) = self.preview.take() else {
            return IRect::EMPTY;
        };
        let area = self.wrapped_area(overlay.bounds());
        let wrap = self.wrap_period();
        self.modify_active_layer(kind, area, |x, y, pixel, coverage| overlay_pixel(&overlay, wrap, pixel, x, y, coverage));
        area
    }

    pub(crate) fn modify_active_layer(&mut self, kind: HistoryKind, area: IRect, f: impl FnMut(i32, i32, Rgba, u8) -> Rgba) -> IRect {
        let (area, changes) = self.modify_pixels(area, f);
        self.push_entry(kind, changes, self.active);
        area
    }

    fn modify_pixels(&mut self, area: IRect, mut f: impl FnMut(i32, i32, Rgba, u8) -> Rgba) -> (IRect, Vec<Change>) {
        let area = area.intersect(self.bounds());
        if area.is_empty() {
            return (IRect::EMPTY, Vec::new());
        }
        let index = self.active;
        let layer = &mut self.layers[index];
        let mut edit = PixelEdit::new(index, self.frame, layer.raster());
        edit.touch(layer.raster(), area);
        let selection = self.selection.as_deref();
        let raster = layer.raster_mut();
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                let coverage = selection.map_or(FULL_COVERAGE, |s| s.coverage_at(x, y));
                if coverage == 0 {
                    continue;
                }
                let pixel = raster.get(x as u32, y as u32);
                raster.set(x as u32, y as u32, f(x, y, pixel, coverage));
            }
        }
        edit.mark_dirty(area);
        (area, edit.finish(layer.raster()).into_iter().collect())
    }

    pub fn flood_fill(&mut self, seed: IPoint, color: Rgba, tolerance: f32, contiguous: bool, mode: BlendMode) -> IRect {
        self.settle();
        let Some(region) = flood_select(self.active_raster(), seed, tolerance, contiguous, self.tiled) else {
            return IRect::EMPTY;
        };
        self.modify_active_layer(HistoryKind::Fill, region.bounds(), |x, y, pixel, coverage| {
            if region.coverage_at(x, y) == 0 {
                pixel
            } else if erases(color) {
                lerp_premultiplied(pixel, TRANSPARENT, coverage)
            } else {
                blend(mode, pixel, scale_alpha(color, coverage))
            }
        })
    }

    pub fn fill_overlay(&self, seed: IPoint, color: Rgba, tolerance: f32, contiguous: bool, mode: BlendMode) -> Option<Overlay> {
        let region = flood_select(self.active_raster(), seed, tolerance, contiguous, self.tiled)?;
        let bounds = region.bounds();
        let paint = if erases(color) { ERASE_MASK } else { color };
        let mut raster = Raster::transparent(bounds.width() as u32, bounds.height() as u32);
        for y in bounds.y0..bounds.y1 {
            for x in bounds.x0..bounds.x1 {
                let coverage = region.coverage_at(x, y);
                if coverage > 0 {
                    raster.set((x - bounds.x0) as u32, (y - bounds.y0) as u32, scale_alpha(paint, coverage));
                }
            }
        }
        Some(Overlay { origin: IPoint::new(bounds.x0, bounds.y0), raster, blend: mode, erase: erases(color), lcd: None })
    }

    pub fn erase_selection(&mut self, kind: HistoryKind) -> IRect {
        self.settle();
        let area = self.selection_or_canvas();
        let (area, mut changes) =
            self.modify_pixels(area, |_, _, pixel, coverage| lerp_premultiplied(pixel, TRANSPARENT, coverage));
        if let Some(before) = self.selection.take() {
            changes.push(Change::Selection { before: Some(before), after: None });
        }
        self.push_entry(kind, changes, self.active);
        area
    }

    pub fn fill_selection(&mut self, color: Rgba) -> IRect {
        self.settle();
        let area = self.selection_or_canvas();
        self.modify_active_layer(HistoryKind::FillSelection, area, |_, _, pixel, coverage| {
            lerp_premultiplied(pixel, color, coverage)
        })
    }

    pub fn adjust_colors(&mut self, kind: HistoryKind, f: impl Fn(Rgba) -> Rgba) -> IRect {
        self.settle();
        let area = self.selection_or_canvas();
        self.modify_active_layer(kind, area, |_, _, pixel, coverage| lerp_premultiplied(pixel, f(pixel), coverage))
    }

    pub(crate) fn selection_or_canvas(&self) -> IRect {
        self.selection.as_deref().map_or(self.bounds(), Selection::bounds)
    }

    pub fn select(&mut self, shape: &Selection, mode: CombineMode, kind: HistoryKind) {
        self.settle();
        let before = self.selection.clone();
        let combined = combine(before.as_deref(), shape, mode).and_then(|s| s.clipped_to(self.bounds())).map(Arc::new);
        self.set_selection_recorded(kind, before, combined);
    }

    pub fn select_all(&mut self) {
        self.settle();
        let before = self.selection.clone();
        let all = Selection::from_rect(self.bounds()).map(Arc::new);
        self.set_selection_recorded(HistoryKind::SelectAll, before, all);
    }

    pub fn deselect(&mut self) {
        self.settle();
        let before = self.selection.take();
        self.set_selection_recorded(HistoryKind::Deselect, before, None);
    }

    pub fn paste_selection(&mut self, shape: SharedSelection) {
        self.settle();
        let before = self.selection.clone();
        let pasted = shape.clipped_to(self.bounds()).map(Arc::new);
        self.set_selection_recorded(HistoryKind::PasteSelection, before, pasted);
    }

    pub fn invert_selection(&mut self) {
        self.settle();
        let before = self.selection.clone();
        let inverted = match &before {
            Some(s) => s.inverted(self.bounds()).map(Arc::new),
            None => None,
        };
        self.set_selection_recorded(HistoryKind::InvertSelection, before, inverted);
    }

    fn set_selection_recorded(&mut self, kind: HistoryKind, before: Option<SharedSelection>, after: Option<SharedSelection>) {
        self.selection = after.clone();
        if before.as_deref() == after.as_deref() {
            return;
        }
        self.push_entry(kind, vec![Change::Selection { before, after }], self.active);
    }

    pub fn begin_selection_move(&mut self) -> Option<SharedSelection> {
        self.settle();
        self.selection.clone()
    }

    pub fn shared_selection(&self) -> Option<SharedSelection> {
        self.selection.clone()
    }

    pub fn replace_selection_live(&mut self, selection: Option<Selection>) {
        self.settle();
        self.selection = selection.and_then(|s| s.clipped_to(self.bounds())).map(Arc::new);
    }

    pub fn record_selection_edit(&mut self, kind: HistoryKind, before: Option<SharedSelection>) {
        let after = self.selection.clone();
        self.set_selection_recorded(kind, before, after);
    }

    pub fn move_selection(&mut self, dx: i32, dy: i32) {
        if let Some(selection) = self.selection.as_mut() {
            Arc::make_mut(selection).translate(dx, dy);
        }
    }

    pub fn finish_selection_move(&mut self, before: Option<SharedSelection>) {
        let after = self.selection.take().and_then(|s| s.clipped_to(self.bounds())).map(Arc::new);
        self.set_selection_recorded(HistoryKind::MoveSelection, before, after);
    }

    pub fn begin_move_pixels(&mut self) -> IRect {
        if self.floating.is_some() || !self.layers[self.active].visible {
            return IRect::EMPTY;
        }
        let index = self.active;
        let selection = match self.selection.as_deref() {
            Some(s) => s.clone(),
            None => match Selection::from_rect(self.bounds()) {
                Some(all) => all,
                None => return IRect::EMPTY,
            },
        };
        let area = selection.bounds();
        let layer = &mut self.layers[index];
        let pixels = layer.raster().crop(area);
        let mut edit = PixelEdit::new(index, self.frame, layer.raster());
        edit.touch(layer.raster(), area);
        let raster = layer.raster_mut();
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                let coverage = selection.coverage_at(x, y);
                if coverage > 0 {
                    let hole = lerp_premultiplied(raster.get(x as u32, y as u32), TRANSPARENT, coverage);
                    raster.set(x as u32, y as u32, hole);
                }
            }
        }
        edit.mark_dirty(area);
        self.floating = Some(Floating {
            source_plane: coverage_plane(&selection),
            placement: Placement::from_bounds(area),
            quad: None,
            undo: Vec::new(),
            redo: Vec::new(),
            step_open: false,
            home: Placement::from_bounds(area),
            pivot: CENTER_PIVOT,
            source: pixels.clone(),
            pixels,
            selection,
            selection_before: self.selection.clone(),
            edit,
            layer: index,
            mode: AlphaMode::Replace,
            kind: HistoryKind::MoveSelectedPixels,
            moved: false,
        });
        area
    }

    pub fn paste(&mut self, pixels: Raster, at: IPoint) -> IRect {
        let settled = self.settle();
        let area = IRect::from_size(at.x, at.y, pixels.width(), pixels.height());
        let Some(selection) = Selection::from_rect(area) else {
            return settled;
        };
        let index = self.active;
        self.floating = Some(Floating {
            edit: PixelEdit::new(index, self.frame, self.layers[index].raster()),
            source_plane: coverage_plane(&selection),
            placement: Placement::from_bounds(area),
            quad: None,
            undo: Vec::new(),
            redo: Vec::new(),
            step_open: false,
            home: Placement::from_bounds(area),
            pivot: CENTER_PIVOT,
            source: pixels.clone(),
            pixels,
            selection,
            selection_before: self.selection.clone(),
            layer: index,
            mode: AlphaMode::Replace,
            kind: HistoryKind::Paste,
            moved: true,
        });
        settled.union(area.intersect(self.bounds()))
    }

    pub fn move_floating(&mut self, dx: i32, dy: i32) -> IRect {
        let canvas = self.bounds();
        let Some(f) = self.floating.as_mut() else {
            return IRect::EMPTY;
        };
        let old = f.selection.bounds();
        f.record_step();
        f.selection.translate(dx, dy);
        f.placement = f.placement.shifted(dx as f32, dy as f32);
        f.quad = f.quad.map(|quad| quad.shifted(dx as f32, dy as f32));
        f.moved |= dx != 0 || dy != 0;
        old.union(f.selection.bounds()).intersect(canvas)
    }

    pub fn place_floating(&mut self, placement: Placement, resample: Resample, rotation: RotationMethod) -> IRect {
        self.reshape(resample, rotation, |f| {
            f.record_step();
            f.placement = placement;
        })
    }

    pub fn warp_floating(&mut self, quad: Quad, resample: Resample, rotation: RotationMethod) -> IRect {
        self.reshape(resample, rotation, |f| {
            f.record_step();
            f.quad = Some(quad);
        })
    }

    pub fn seal_floating_step(&mut self) {
        if let Some(f) = self.floating.as_mut() {
            f.step_open = false;
        }
    }

    pub fn undo_floating_step(&mut self, resample: Resample, rotation: RotationMethod) -> Option<IRect> {
        let f = self.floating.as_mut()?;
        f.step_open = false;
        let current = f.pose();
        while let Some(pose) = f.undo.pop() {
            if pose == current {
                continue;
            }
            if f.undo.is_empty() {
                return None;
            }
            return Some(self.reshape(resample, rotation, |f| {
                f.redo.push(current);
                f.set_pose(pose);
            }));
        }
        None
    }

    pub fn redo_floating_step(&mut self, resample: Resample, rotation: RotationMethod) -> Option<IRect> {
        let f = self.floating.as_mut()?;
        let pose = f.redo.pop()?;
        f.step_open = false;
        let current = f.pose();
        Some(self.reshape(resample, rotation, |f| {
            f.undo.push(current);
            f.set_pose(pose);
        }))
    }

    fn reshape(&mut self, resample: Resample, rotation: RotationMethod, change: impl FnOnce(&mut Floating)) -> IRect {
        let canvas = self.bounds();
        let Some(f) = self.floating.as_mut() else {
            return IRect::EMPTY;
        };
        let old = f.selection.bounds();
        change(f);
        f.moved = true;
        f.realize(resample, rotation);
        old.union(f.selection.bounds()).intersect(canvas)
    }

    pub fn resample_floating(&mut self, resample: Resample, rotation: RotationMethod) -> IRect {
        let canvas = self.bounds();
        let Some(f) = self.floating.as_mut() else {
            return IRect::EMPTY;
        };
        f.realize(resample, rotation);
        f.selection.bounds().intersect(canvas)
    }

    pub fn set_floating_pivot(&mut self, pivot: Pivot) {
        if let Some(f) = self.floating.as_mut() {
            f.pivot = pivot;
        }
    }

    pub fn set_floating_mode(&mut self, mode: AlphaMode) -> IRect {
        let canvas = self.bounds();
        match self.floating.as_mut() {
            Some(f) if f.mode != mode => {
                f.mode = mode;
                f.selection.bounds().intersect(canvas)
            }
            _ => IRect::EMPTY,
        }
    }

    pub fn commit_floating(&mut self) -> IRect {
        let Some(mut f) = self.floating.take() else {
            return IRect::EMPTY;
        };
        let canvas = self.bounds();
        let area = f.selection.bounds().intersect(canvas);
        let layer = &mut self.layers[f.layer];
        f.edit.touch(layer.raster(), area);
        let origin = f.selection.bounds();
        let raster = layer.raster_mut();
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                let coverage = f.selection.coverage_at(x, y);
                if coverage > 0 {
                    let source = f.pixels.get((x - origin.x0) as u32, (y - origin.y0) as u32);
                    let merged = f.mode.apply(raster.get(x as u32, y as u32), source, coverage);
                    raster.set(x as u32, y as u32, merged);
                }
            }
        }
        f.edit.mark_dirty(area);
        let touched = f.edit.dirty();
        let after = f.selection.clipped_to(canvas).map(Arc::new);
        let mut changes: Vec<Change> = f.edit.finish(layer.raster()).into_iter().collect();
        if f.moved && f.selection_before.as_deref() != after.as_deref() {
            changes.push(Change::Selection { before: f.selection_before, after: after.clone() });
        }
        self.selection = after;
        self.push_entry(f.kind, changes, f.layer);
        touched.union(area)
    }

    pub fn cancel_floating(&mut self) -> IRect {
        let Some(f) = self.floating.take() else {
            return IRect::EMPTY;
        };
        let area = f.edit.dirty().union(f.selection.bounds()).intersect(self.bounds());
        f.edit.revert(self.layers[f.layer].raster_mut());
        self.selection = f.selection_before;
        area
    }

    pub fn settle(&mut self) -> IRect {
        self.commit_floating()
    }

    pub fn copy_selection(&mut self, merged: bool) -> Raster {
        self.settle();
        let area = self.selection_or_canvas();
        let mut pixels = if merged { self.flattened(area) } else { self.active_raster().crop(area) };
        if let Some(selection) = self.selection.as_deref() {
            selection.mask_alpha(&mut pixels);
        }
        pixels
    }

    pub fn thumbnail(&self, max_side: u32) -> Raster {
        let longest = self.width.max(self.height) as f32;
        let scale = (max_side as f32 / longest).min(1.0);
        let side = |full: u32| ((full as f32 * scale).round() as u32).max(1);
        let (width, height) = (side(self.width), side(self.height));
        let mut out = Raster::transparent(width, height);
        for y in 0..height {
            for x in 0..width {
                let source_x = (((x as f32 + 0.5) / scale) as i32).min(self.width as i32 - 1);
                let source_y = (((y as f32 + 0.5) / scale) as i32).min(self.height as i32 - 1);
                let pixel = self.composite_frame(self.frame, IRect::from_size(source_x, source_y, 1, 1), false, None).get(0, 0);
                out.set(x, y, pixel);
            }
        }
        out
    }

    pub fn add_layer(&mut self, name: String) {
        self.settle();
        let index = self.active + 1;
        let mut layer = Layer::blank(name, self.width, self.height, self.frames.len());
        layer.masked = self.layers[self.active].masked;
        self.insert_layer_recorded(HistoryKind::AddLayer, index, layer);
    }

    pub fn add_masked_layer_below(&mut self, index: usize, name: String) -> bool {
        if index >= self.layers.len() {
            return false;
        }
        self.settle();
        let mut layer = Layer::blank(name, self.width, self.height, self.frames.len());
        layer.masked = true;
        self.insert_layer_recorded(HistoryKind::AddLayer, index, layer);
        if let Some(parent) = self.mask_parent(index) {
            self.layers[parent].collapsed = false;
        }
        true
    }

    pub fn toggle_layer_collapsed(&mut self, index: usize) {
        if let Some(layer) = self.layers.get_mut(index) {
            layer.collapsed = !layer.collapsed;
        }
    }

    pub fn duplicate_layer(&mut self, name: String) {
        self.settle();
        let mut copy = self.layers[self.active].clone();
        copy.name = name;
        self.insert_layer_recorded(HistoryKind::DuplicateLayer, self.active + 1, copy);
    }

    fn insert_layer_recorded(&mut self, kind: HistoryKind, index: usize, layer: Layer) {
        let active_before = self.active;
        self.layers.insert(index, layer.clone());
        self.sync_frame();
        self.active = index;
        self.push_entry(kind, vec![Change::LayerInserted { index, layer }], active_before);
    }

    pub fn delete_layer(&mut self) -> bool {
        self.settle();
        if self.layers.len() <= 1 {
            return false;
        }
        let active_before = self.active;
        let mut changes = self.detach_children(active_before);
        let layer = self.layers.remove(active_before);
        changes.push(Change::LayerRemoved { index: active_before, layer });
        self.active = active_before.min(self.layers.len() - 1);
        self.push_entry(HistoryKind::DeleteLayer, changes, active_before);
        true
    }

    fn set_masked(&mut self, index: usize, masked: bool) -> Option<Change> {
        let before = self.layers[index].props();
        if before.masked == masked {
            return None;
        }
        self.layers[index].masked = masked;
        Some(Change::LayerProps { index, before, after: self.layers[index].props() })
    }

    fn detach_children(&mut self, parent: usize) -> Vec<Change> {
        if !self.is_mask(parent) {
            return Vec::new();
        }
        let children: Vec<usize> = (0..parent).rev().take_while(|&index| self.layers[index].masked).collect();
        children.into_iter().filter_map(|index| self.set_masked(index, false)).collect()
    }

    pub fn move_layer(&mut self, up: bool) -> bool {
        let from = self.active;
        let to = if up { from + 1 } else { from.wrapping_sub(1) };
        self.move_layer_to(from, to)
    }

    pub fn move_layer_to(&mut self, from: usize, to: usize) -> bool {
        self.settle();
        if from == to || from >= self.layers.len() || to >= self.layers.len() {
            return false;
        }
        let mut changes = self.detach_children(from);
        let joins_group = self.layers[from].masked;
        let layer = self.layers.remove(from);
        let mut slot = to;
        let inside_group = slot > 0 && slot < self.layers.len() && self.layers[slot - 1].masked;
        let stays_masked = joins_group && inside_group;
        let upward = to > from;
        while !stays_masked && slot > 0 && slot < self.layers.len() && self.layers[slot - 1].masked {
            slot = if upward { slot + 1 } else { slot - 1 };
        }
        self.layers.insert(slot, layer);
        if slot != from {
            changes.push(Change::LayerMoved { from, to: slot });
        }
        changes.extend(self.set_masked(slot, stays_masked));
        self.active = slot;
        let kind = if slot > from { HistoryKind::MoveLayerUp } else { HistoryKind::MoveLayerDown };
        self.push_entry(kind, changes, from);
        true
    }

    pub fn nest_layer(&mut self, from: usize, parent: usize) -> bool {
        self.settle();
        let valid = from != parent
            && from < self.layers.len()
            && parent < self.layers.len()
            && !self.layers[parent].masked
            && !self.is_mask(from);
        if !valid {
            return false;
        }
        let layer = self.layers.remove(from);
        let slot = if parent > from { parent - 1 } else { parent };
        self.layers.insert(slot, layer);
        let mut changes = Vec::new();
        if slot != from {
            changes.push(Change::LayerMoved { from, to: slot });
        }
        changes.extend(self.set_masked(slot, true));
        self.active = slot;
        self.push_entry(HistoryKind::MaskLayer, changes, from);
        true
    }

    pub fn unnest_layer(&mut self, index: usize) -> bool {
        self.settle();
        let Some(parent) = self.mask_parent(index) else {
            return false;
        };
        let layer = self.layers.remove(index);
        self.layers.insert(parent, layer);
        let mut changes = vec![Change::LayerMoved { from: index, to: parent }];
        changes.extend(self.set_masked(parent, false));
        self.active = parent;
        self.push_entry(HistoryKind::UnmaskLayer, changes, index);
        true
    }

    pub fn merge_layer_down(&mut self) -> bool {
        self.settle();
        let upper_index = self.active;
        if upper_index == 0 || self.layers[upper_index].masked != self.layers[upper_index - 1].masked {
            return false;
        }
        let lower_index = upper_index - 1;
        let upper = self.layers.remove(upper_index);
        let mut changes = Vec::new();
        for frame in 0..self.frames.len() {
            let lower = &mut self.layers[lower_index];
            let mut edit = PixelEdit::new(lower_index, frame, lower.cel(frame));
            edit.touch(lower.cel(frame), upper.cel(frame).bounds());
            let source_cel = upper.cel(frame);
            let target = lower.cel_mut(frame);
            for y in 0..self.height {
                for x in 0..self.width {
                    let source = scale_alpha(source_cel.get(x, y), upper.opacity);
                    if source[3] > 0 && upper.visible {
                        target.set(x, y, blend(upper.blend, target.get(x, y), source));
                    }
                }
            }
            edit.mark_dirty(target.bounds());
            changes.extend(edit.finish(lower.cel(frame)));
        }
        changes.push(Change::LayerRemoved { index: upper_index, layer: upper });
        self.active = lower_index;
        self.push_entry(HistoryKind::MergeLayerDown, changes, upper_index);
        true
    }

    pub fn set_layer_props(&mut self, index: usize, props: LayerProps) {
        self.settle();
        let Some(layer) = self.layers.get_mut(index) else {
            return;
        };
        let before = layer.props();
        if before == props {
            return;
        }
        layer.set_props(&props);
        if self.history.coalesce_layer_props(index, &before, &props) {
            self.revision += 1;
            return;
        }
        self.push_entry(HistoryKind::LayerProperties, vec![Change::LayerProps { index, before, after: props }], self.active);
    }

    pub fn preview_layer_props(&mut self, index: usize, props: &LayerProps) {
        if let Some(layer) = self.layers.get_mut(index) {
            layer.set_props(props);
        }
    }

    fn replace_canvas(&mut self, kind: HistoryKind, width: u32, height: u32, layers: Vec<Layer>) {
        let active_before = self.active;
        let selection = self.selection.take();
        let before = CanvasState { width: self.width, height: self.height, layers: std::mem::take(&mut self.layers) };
        let after = CanvasState { width, height, layers };
        self.width = width;
        self.height = height;
        self.layers = after.layers.clone();
        self.sync_frame();
        self.active = self.active.min(self.layers.len() - 1);
        let mut changes = vec![Change::Canvas { before, after }];
        if selection.is_some() {
            changes.push(Change::Selection { before: selection, after: None });
        }
        self.push_entry(kind, changes, active_before);
    }

    fn map_layers(&self, f: impl Fn(&Raster) -> Raster) -> Vec<Layer> {
        self.layers.iter().map(|layer| layer.map_cels(&f)).collect()
    }

    pub fn flip_horizontal(&mut self) {
        self.settle();
        if self.selection.is_some() {
            self.flip_selection(HistoryKind::FlipHorizontal, mirror_horizontally);
            return;
        }
        let layers = self.map_layers(Raster::flipped_horizontally);
        self.replace_canvas(HistoryKind::FlipHorizontal, self.width, self.height, layers);
    }

    pub fn flip_vertical(&mut self) {
        self.settle();
        if self.selection.is_some() {
            self.flip_selection(HistoryKind::FlipVertical, mirror_vertically);
            return;
        }
        let layers = self.map_layers(Raster::flipped_vertically);
        self.replace_canvas(HistoryKind::FlipVertical, self.width, self.height, layers);
    }

    fn flip_selection(&mut self, kind: HistoryKind, mirror: fn(IRect, i32, i32) -> IPoint) {
        let area = self.selection_or_canvas().intersect(self.bounds());
        if area.is_empty() {
            return;
        }
        let source = self.active_raster().crop(area);
        self.modify_active_layer(kind, area, |x, y, pixel, coverage| {
            let from = mirror(area, x, y);
            let mirrored = source.get((from.x - area.x0) as u32, (from.y - area.y0) as u32);
            lerp_premultiplied(pixel, mirrored, coverage)
        });
    }

    pub fn rotate_clockwise(&mut self) {
        self.settle();
        let layers = self.map_layers(Raster::rotated_clockwise);
        self.replace_canvas(HistoryKind::RotateClockwise, self.height, self.width, layers);
    }

    pub fn rotate_counter_clockwise(&mut self) {
        self.settle();
        let layers = self.map_layers(Raster::rotated_counter_clockwise);
        self.replace_canvas(HistoryKind::RotateCounterClockwise, self.height, self.width, layers);
    }

    pub fn offset_half(&mut self) {
        self.settle();
        let (dx, dy) = (self.width / 2, self.height / 2);
        let layers = self.map_layers(|raster| raster.shifted_wrapping(dx, dy));
        self.replace_canvas(HistoryKind::OffsetHalf, self.width, self.height, layers);
    }

    pub fn rotate_half_turn(&mut self) {
        self.settle();
        let layers = self.map_layers(Raster::rotated_half_turn);
        self.replace_canvas(HistoryKind::RotateHalfTurn, self.width, self.height, layers);
    }

    pub fn crop_to_selection(&mut self) -> bool {
        self.settle();
        let Some(area) = self.selection.as_deref().map(|s| s.bounds().intersect(self.bounds())) else {
            return false;
        };
        if area.is_empty() {
            return false;
        }
        let layers = self.map_layers(|r| r.crop(area));
        self.replace_canvas(HistoryKind::CropToSelection, area.width() as u32, area.height() as u32, layers);
        true
    }

    pub fn resize_image(&mut self, width: u32, height: u32, resample: Resample) -> Result<(), DocumentError> {
        check_size(width, height)?;
        self.settle();
        let layers = self.map_layers(|r| r.resized(width, height, resample));
        self.replace_canvas(HistoryKind::ResizeImage, width, height, layers);
        Ok(())
    }

    pub fn resize_canvas(&mut self, width: u32, height: u32, anchor: Anchor) -> Result<(), DocumentError> {
        check_size(width, height)?;
        self.settle();
        let offset = anchor.offset((self.width, self.height), (width, height));
        let layers = self.map_layers(|r| {
            let mut out = Raster::transparent(width, height);
            out.blit(r, offset);
            out
        });
        self.replace_canvas(HistoryKind::ResizeCanvas, width, height, layers);
        Ok(())
    }

    pub fn flatten(&mut self, layer_name: String) -> bool {
        self.settle();
        if self.layers.len() <= 1 {
            return false;
        }
        let mut cels: Vec<Arc<Raster>> = Vec::with_capacity(self.frames.len());
        for frame in 0..self.frames.len() {
            let unchanged = frame > 0 && self.layers.iter().all(|layer| Arc::ptr_eq(layer.cel(frame), layer.cel(frame - 1)));
            let cel = if unchanged { Arc::clone(&cels[frame - 1]) } else { Arc::new(self.flattened_frame(frame, self.bounds())) };
            cels.push(cel);
        }
        let merged = Layer::with_cels(layer_name, cels);
        self.active = 0;
        self.replace_canvas(HistoryKind::Flatten, self.width, self.height, vec![merged]);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::StrokeKind;

    const WHITE: Rgba = [255, 255, 255, 255];
    const RED: Rgba = [255, 0, 0, 255];

    fn doc(width: u32, height: u32) -> Document {
        Document::new(width, height, WHITE, "Фон".into()).unwrap()
    }

    fn pencil(color: Rgba) -> StrokeParams {
        StrokeParams { kind: StrokeKind::Pencil, color, size: 1.0, antialias: false, blend: BlendMode::Normal, pixel_perfect: false, smooth: false, wrap: false, brush: Default::default(), seed: 0, pressure: 1.0, tilt: None }
    }

    #[test]
    fn composite_pixel_matches_composite_and_is_transparent_outside() {
        let mut d = doc(6, 6);
        d.add_layer("верх".into());
        let (mut stroke, _) = d.begin_stroke(pencil(RED), FPoint::new(1.5, 1.5), HistoryKind::Pencil).unwrap();
        d.extend_stroke(&mut stroke, FPoint::new(3.5, 1.5));
        d.finish_stroke(stroke);
        let whole = d.composite(d.bounds());
        for y in 0..6 {
            for x in 0..6 {
                assert_eq!(d.composite_pixel(x, y), whole.get(x as u32, y as u32), "({x}, {y})");
            }
        }
        assert_eq!(d.composite_pixel(-1, 0), TRANSPARENT);
        assert_eq!(d.composite_pixel(6, 6), TRANSPARENT);
    }

    #[test]
    fn fill_preview_matches_the_committed_fill() {
        let mut d = doc(6, 1);
        let (s, _) = d.begin_stroke(pencil([0, 0, 0, 255]), FPoint::new(3.5, 0.5), HistoryKind::Pencil).unwrap();
        d.finish_stroke(s);
        let overlay = d.fill_overlay(IPoint::new(0, 0), RED, 0.1, true, BlendMode::Normal).unwrap();
        d.set_preview(Some(overlay));
        let shown = d.composite(d.bounds());
        d.commit_preview(HistoryKind::Fill);
        assert_eq!(d.flattened(d.bounds()), shown);
        assert_eq!(d.active_raster().get(1, 0), RED);
        assert_eq!(d.active_raster().get(4, 0), WHITE);
    }

    #[test]
    fn bucket_with_transparent_color_erases_the_region() {
        let mut d = doc(6, 1);
        d.flood_fill(IPoint::new(0, 0), [255, 0, 0, 0], 0.1, true, BlendMode::Normal);
        assert_eq!(d.active_raster().get(3, 0)[3], 0);
        d.undo();
        assert_eq!(d.active_raster().get(3, 0), WHITE);
    }

    #[test]
    fn transparent_shape_preview_and_commit_erase_the_layer() {
        use crate::shapes::{ShapeKind, ShapeStyle, render_in_frame};
        let mut d = doc(20, 20);
        let clear = ShapeStyle { outline: None, fill: Some([0, 0, 0, 0]), width: 1.0, align: crate::shapes::StrokeAlign::Center, antialias: false };
        let overlay = render_in_frame(ShapeKind::Rectangle, IRect::new(4, 4, 12, 12).into(), 0.0, &clear, BlendMode::Normal, d.bounds());
        d.set_preview(overlay);
        assert_eq!(d.composite(d.bounds()).get(6, 6)[3], 0);
        assert_eq!(d.composite(d.bounds()).get(15, 15), WHITE);
        d.commit_preview(HistoryKind::Rectangle);
        assert_eq!(d.active_raster().get(6, 6)[3], 0);
        assert_eq!(d.active_raster().get(15, 15), WHITE);
    }

    #[test]
    fn rejects_out_of_range_sizes() {
        assert!(Document::new(0, 10, WHITE, "x".into()).is_err());
        assert!(Document::new(MAX_SIDE + 1, 10, WHITE, "x".into()).is_err());
    }

    #[test]
    fn stroke_then_undo_redo_round_trips() {
        let mut d = doc(20, 20);
        let (mut stroke, _) = d.begin_stroke(pencil(RED), FPoint::new(2.5, 2.5), HistoryKind::Pencil).unwrap();
        d.extend_stroke(&mut stroke, FPoint::new(10.5, 2.5));
        d.finish_stroke(stroke);
        assert_eq!(d.active_raster().get(5, 2), RED);
        d.undo();
        assert_eq!(d.active_raster().get(5, 2), WHITE);
        d.redo();
        assert_eq!(d.active_raster().get(5, 2), RED);
    }

    #[test]
    fn new_action_after_undo_drops_redo_branch() {
        let mut d = doc(8, 8);
        d.add_layer("L2".into());
        d.undo();
        d.add_layer("L3".into());
        assert!(!d.history().can_redo());
        assert_eq!(d.layers().len(), 2);
    }

    #[test]
    fn select_erase_clears_only_selected() {
        let mut d = doc(10, 10);
        d.select(&Selection::from_rect(IRect::new(2, 2, 5, 5)).unwrap(), CombineMode::Replace, HistoryKind::SelectRectangle);
        d.erase_selection(HistoryKind::Delete);
        assert_eq!(d.active_raster().get(3, 3)[3], 0);
        assert_eq!(d.active_raster().get(7, 7), WHITE);
        d.undo();
        assert_eq!(d.active_raster().get(3, 3), WHITE);
    }

    #[test]
    fn move_pixels_commit_leaves_hole_and_copy() {
        let mut d = doc(10, 10);
        d.select(&Selection::from_rect(IRect::new(0, 0, 2, 2)).unwrap(), CombineMode::Replace, HistoryKind::SelectRectangle);
        d.fill_selection(RED);
        d.begin_move_pixels();
        d.move_floating(5, 5);
        d.commit_floating();
        assert_eq!(d.active_raster().get(0, 0)[3], 0);
        assert_eq!(d.active_raster().get(5, 5), RED);
        d.undo();
        assert_eq!(d.active_raster().get(0, 0), RED);
        assert_eq!(d.active_raster().get(5, 5), WHITE);
    }

    #[test]
    fn scaled_floating_pixels_commit_into_the_new_frame_and_undo_cleanly() {
        let mut d = doc(20, 20);
        d.select(&Selection::from_rect(IRect::new(0, 0, 2, 2)).unwrap(), CombineMode::Replace, HistoryKind::SelectRectangle);
        d.fill_selection(RED);
        d.begin_move_pixels();
        d.place_floating(Placement::from_bounds(IRect::new(5, 5, 9, 9)), Resample::Bilinear, RotationMethod::Plain);
        assert_eq!(d.selection().unwrap().bounds(), IRect::new(5, 5, 9, 9));
        d.place_floating(Placement::from_bounds(IRect::new(5, 5, 9, 13)), Resample::Bilinear, RotationMethod::Plain);
        assert_eq!(d.selection().unwrap().bounds(), IRect::new(5, 5, 9, 13));
        d.commit_floating();
        assert_eq!(d.active_raster().get(6, 11), RED);
        assert_eq!(d.active_raster().get(0, 0)[3], 0);
        d.undo();
        assert_eq!(d.active_raster().get(0, 0), RED);
        assert_eq!(d.active_raster().get(6, 11), WHITE);
    }

    #[test]
    fn live_selection_edit_records_one_history_step() {
        let mut d = doc(20, 20);
        let before = d.shared_selection();
        d.replace_selection_live(Selection::from_rect(IRect::new(2, 2, 8, 8)));
        d.replace_selection_live(Selection::from_rect(IRect::new(2, 2, 12, 12)));
        assert_eq!(d.history().cursor(), 0);
        d.record_selection_edit(HistoryKind::SelectRectangle, before);
        assert_eq!(d.history().cursor(), 1);
        assert_eq!(d.selection().unwrap().bounds(), IRect::new(2, 2, 12, 12));
        d.undo();
        assert!(d.selection().is_none());
    }

    #[test]
    fn undo_while_floating_cancels_without_history_step() {
        let mut d = doc(10, 10);
        d.select(&Selection::from_rect(IRect::new(0, 0, 2, 2)).unwrap(), CombineMode::Replace, HistoryKind::SelectRectangle);
        let steps = d.history().cursor();
        d.begin_move_pixels();
        d.move_floating(4, 4);
        d.undo();
        assert!(d.floating().is_none());
        assert_eq!(d.active_raster().get(0, 0), WHITE);
        assert_eq!(d.history().cursor(), steps);
    }

    #[test]
    fn paste_overwrites_backdrop_including_transparent_pixels() {
        let mut d = doc(6, 6);
        let mut clip = Raster::transparent(2, 2);
        clip.set(0, 0, RED);
        d.paste(clip, IPoint::new(1, 1));
        d.commit_floating();
        assert_eq!(d.active_raster().get(1, 1), RED);
        assert_eq!(d.active_raster().get(2, 2)[3], 0);
    }

    #[test]
    fn paste_in_blend_mode_keeps_backdrop() {
        let mut d = doc(6, 6);
        let mut clip = Raster::transparent(2, 2);
        clip.set(0, 0, RED);
        d.paste(clip, IPoint::new(1, 1));
        d.set_floating_mode(AlphaMode::Blend);
        d.commit_floating();
        assert_eq!(d.active_raster().get(2, 2), WHITE);
    }

    #[test]
    fn erase_selection_also_deselects_in_one_step() {
        let mut d = doc(6, 6);
        d.select(&Selection::from_rect(IRect::new(1, 1, 3, 3)).unwrap(), CombineMode::Replace, HistoryKind::SelectRectangle);
        let steps = d.history().cursor();
        d.erase_selection(HistoryKind::Cut);
        assert!(d.selection().is_none());
        assert_eq!(d.history().cursor(), steps + 1);
        d.undo();
        assert!(d.selection().is_some());
        assert_eq!(d.active_raster().get(1, 1), WHITE);
    }

    #[test]
    fn composite_respects_layer_visibility_and_opacity() {
        let mut d = doc(2, 2);
        d.add_layer("Верх".into());
        d.fill_selection(RED);
        let full = d.flattened(d.bounds());
        assert_eq!(full.get(0, 0), RED);
        let mut props = d.layers()[1].props();
        props.opacity = 0;
        d.set_layer_props(1, props);
        assert_eq!(d.flattened(d.bounds()).get(0, 0), WHITE);
    }

    #[test]
    fn merge_down_bakes_upper_layer() {
        let mut d = doc(4, 4);
        d.add_layer("Верх".into());
        d.fill_selection(RED);
        assert!(d.merge_layer_down());
        assert_eq!(d.layers().len(), 1);
        assert_eq!(d.active_raster().get(1, 1), RED);
        d.undo();
        assert_eq!(d.layers().len(), 2);
        assert_eq!(d.layers()[0].raster().get(1, 1), WHITE);
    }

    #[test]
    fn delete_last_layer_is_refused() {
        let mut d = doc(4, 4);
        assert!(!d.delete_layer());
    }

    #[test]
    fn crop_to_selection_resizes_and_undoes() {
        let mut d = doc(10, 10);
        d.select(&Selection::from_rect(IRect::new(2, 3, 6, 8)).unwrap(), CombineMode::Replace, HistoryKind::SelectRectangle);
        assert!(d.crop_to_selection());
        assert_eq!((d.width(), d.height()), (4, 5));
        assert!(d.selection().is_none());
        d.undo();
        assert_eq!((d.width(), d.height()), (10, 10));
        assert!(d.selection().is_some());
    }

    #[test]
    fn canvas_resize_anchors_content() {
        let mut d = doc(2, 2);
        d.resize_canvas(4, 4, Anchor::BottomRight).unwrap();
        assert_eq!(d.active_raster().get(0, 0)[3], 0);
        assert_eq!(d.active_raster().get(3, 3), WHITE);
    }

    #[test]
    fn rotate_swaps_dimensions() {
        let mut d = doc(6, 2);
        d.rotate_clockwise();
        assert_eq!((d.width(), d.height()), (2, 6));
        d.undo();
        assert_eq!((d.width(), d.height()), (6, 2));
    }

    #[test]
    fn flood_fill_fills_connected_area_only() {
        let mut d = doc(6, 1);
        d.select_all();
        d.deselect();
        let mut stroke_doc = d;
        let (s, _) = stroke_doc.begin_stroke(pencil([0, 0, 0, 255]), FPoint::new(3.5, 0.5), HistoryKind::Pencil).unwrap();
        stroke_doc.finish_stroke(s);
        stroke_doc.flood_fill(IPoint::new(0, 0), RED, 0.1, true, BlendMode::Normal);
        assert_eq!(stroke_doc.active_raster().get(2, 0), RED);
        assert_eq!(stroke_doc.active_raster().get(4, 0), WHITE);
    }

    #[test]
    fn copy_selection_applies_coverage_to_alpha() {
        let mut d = doc(8, 8);
        d.select(&Selection::from_ellipse(IRect::new(0, 0, 8, 8).into(), true).unwrap(), CombineMode::Replace, HistoryKind::SelectEllipse);
        let copy = d.copy_selection(false);
        assert_eq!(copy.get(4, 4)[3], 255);
        assert!(copy.get(0, 0)[3] < 255);
    }

    #[test]
    fn jump_to_restores_intermediate_state() {
        let mut d = doc(4, 4);
        d.add_layer("A".into());
        d.add_layer("B".into());
        d.jump_to(1);
        assert_eq!(d.layers().len(), 2);
        d.jump_to(2);
        assert_eq!(d.layers().len(), 3);
    }

    #[test]
    fn tiled_shape_wraps_across_the_edge_on_commit() {
        use crate::geom::FRect;
        use crate::shapes::{ShapeKind, ShapeStyle, render_in_frame};
        let mut d = Document::new(10, 10, [255, 255, 255, 255], "bg".into()).unwrap();
        d.set_tiled(true);
        let style = ShapeStyle { outline: None, fill: Some([255, 0, 0, 255]), width: 1.0, align: crate::shapes::StrokeAlign::Center, antialias: false };
        let overlay = render_in_frame(ShapeKind::Rectangle, FRect::new(8.0, 4.0, 12.0, 6.0), 0.0, &style, BlendMode::Normal, d.overlay_canvas()).unwrap();
        d.set_preview(Some(overlay));
        d.commit_preview(HistoryKind::Rectangle);
        assert_eq!(d.active_raster().get(9, 4), [255, 0, 0, 255]);
        assert_eq!(d.active_raster().get(1, 5), [255, 0, 0, 255]);
        assert_eq!(d.active_raster().get(3, 5), [255, 255, 255, 255]);
    }

    #[test]
    fn floating_undo_steps_back_through_each_warp_then_drops_the_lift() {
        let mut d = doc(10, 10);
        d.select_all();
        d.begin_move_pixels();
        let rect = Placement::from_bounds(d.bounds());
        let first = Quad::from_placement(&rect).with_corner(0, FPoint::new(2.0, 1.0)).unwrap();
        let second = first.with_corner(1, FPoint::new(8.0, 2.0)).unwrap();
        d.warp_floating(first, Resample::Nearest, RotationMethod::Plain);
        d.seal_floating_step();
        d.warp_floating(second, Resample::Nearest, RotationMethod::Plain);
        assert!(d.undo_floating_step(Resample::Nearest, RotationMethod::Plain).is_some());
        assert_eq!(d.floating().and_then(|f| f.quad()), Some(first));
        assert!(d.redo_floating_step(Resample::Nearest, RotationMethod::Plain).is_some());
        assert_eq!(d.floating().and_then(|f| f.quad()), Some(second));
        assert!(d.undo_floating_step(Resample::Nearest, RotationMethod::Plain).is_some());
        assert!(d.undo_floating_step(Resample::Nearest, RotationMethod::Plain).is_none());
        assert!(d.floating().is_some());
        d.undo();
        assert!(d.floating().is_none());
    }

    fn mask_document() -> Document {
        let solid = |color: Rgba| {
            let mut raster = Raster::transparent(2, 1);
            raster.set(0, 0, color);
            raster
        };
        let layers = vec![Layer::new("фон".into(), solid(WHITE)), Layer::new("объект".into(), solid(RED)), Layer::new("маска".into(), solid([0, 0, 0, 128]))];
        Document::from_layers(layers, 2).unwrap()
    }

    #[test]
    fn nested_layer_takes_the_alpha_of_the_mask_above_it() {
        let mut d = mask_document();
        assert!(d.nest_layer(1, 2));
        assert_eq!((d.mask_parent(0), d.mask_parent(1), d.is_mask(2)), (None, Some(2), true));
        let pixel = d.composite_pixel(0, 0);
        assert!(pixel[0] > 250 && (120..135).contains(&pixel[1]), "{pixel:?}");
        assert_eq!(d.composite_pixel(1, 0), TRANSPARENT);
        d.undo();
        assert_eq!(d.mask_parent(1), None);
        assert_eq!(d.composite_pixel(0, 0)[1], 0);
    }

    #[test]
    fn plain_moves_never_land_inside_a_mask_group_and_unnest_restores_order() {
        let mut d = mask_document();
        d.nest_layer(1, 2);
        assert!(d.move_layer_to(0, 1));
        assert_eq!(d.layers().iter().map(|l| l.masked).collect::<Vec<_>>(), [true, false, false]);
        assert_eq!(d.layers()[2].name, "фон");
        assert!(d.unnest_layer(0));
        assert!(d.layers().iter().all(|l| !l.masked));
    }
}
