use std::collections::HashMap;
use std::sync::Arc;

use crate::blend::{BlendMode, blend};
use crate::brush::{Brush, BrushEngine, BrushTip, Dab, DabSampler, Jitter, ROUNDNESS_RANGE, TrackSampler, dry_gate, grained, grained_deposit, tint};
use crate::edit::{CoverageMerge, PixelEdit};
use crate::geom::{FPoint, IPoint, IRect};
use crate::raster::{Raster, Rgba, erases, mul_div_255};
use crate::ribbon::{Nib, SweptNib};
use crate::selection::Selection;
use crate::smoothing::PathSmoother;
use crate::stroke_path::{PathInterpolator, PathSample, SpeedMeter};

const FULL_COVERAGE: u8 = u8::MAX;
const MIN_BRUSH_SPACING: f32 = 0.5;
const MIN_DAB_SIZE: f32 = 0.1;
const PRESSURE_SCATTER_REACH: f32 = 1.0;
const PRESSURE_SPACING_REACH: f32 = 3.0;
const MAX_TILT_DEGREES: f32 = 90.0;
const MIN_SPEED_SCALE: f32 = 0.1;
const DEPLETION_LENGTH_IN_SIZES: f32 = 60.0;
const HUE_JITTER_TURNS: f32 = 0.5;
const STAMP_MARGIN: i32 = 1;
const HEADING_BLEND: f32 = 0.35;
const RIBBON_STEP_RATIO: f32 = 0.05;
const RIBBON_MIN_STEP: f32 = 1.0;
const RIBBON_HEADING_MEMORY_IN_SIZES: f32 = 0.5;
const SOLID_HARDNESS: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tilt {
    pub x: f32,
    pub y: f32,
}

impl Tilt {
    fn azimuth(self) -> f32 {
        self.y.atan2(self.x)
    }

    fn lean(self) -> f32 {
        (self.x.hypot(self.y) / MAX_TILT_DEGREES).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeKind {
    Pencil,
    Brush,
    Eraser,
    CloneFrom(IPoint),
}

#[derive(Clone, Debug)]
pub struct StrokeParams {
    pub kind: StrokeKind,
    pub color: Rgba,
    pub size: f32,
    pub antialias: bool,
    pub blend: BlendMode,
    pub pixel_perfect: bool,
    pub smooth: bool,
    pub wrap: bool,
    pub brush: Arc<Brush>,
    pub seed: u64,
    pub pressure: f32,
    pub tilt: Option<Tilt>,
}

pub struct StrokeTarget<'a> {
    pub raster: &'a mut Raster,
    pub edit: &'a mut PixelEdit,
    pub selection: Option<&'a Selection>,
}

struct PenResponse {
    size: f32,
    angle: f32,
    roundness: f32,
    strength: f32,
    hardness: f32,
    dryness: f32,
}

pub struct Stroke {
    params: StrokeParams,
    last_sample: PathSample,
    until_next_stamp: f32,
    trail: Vec<IPoint>,
    placed: HashMap<IPoint, u32>,
    smoother: Option<PathSmoother>,
    curve: PathInterpolator,
    jitter: Jitter,
    heading: Option<FPoint>,
    pressure: f32,
    time_ms: Option<f64>,
    travelled: f32,
    tilt: Option<Tilt>,
    speed: SpeedMeter,
    ribbon_anchor: Option<Nib>,
    ribbon_tip: Option<Nib>,
    pending_start: Option<PathSample>,
    dabs_placed: usize,
}

impl Stroke {
    pub fn begin(params: StrokeParams, at: FPoint, target: &mut StrokeTarget) -> (Stroke, IRect) {
        let smoother = params.smooth.then(|| PathSmoother::new(at));
        let jitter = Jitter::new(params.seed);
        let kind = params.kind;
        let pressure = params.pressure.clamp(0.0, 1.0);
        let tilt = params.tilt;
        let start = PathSample { point: at, pressure, tilt, time_ms: None };
        let mut stroke = Stroke {
            params,
            last_sample: start,
            until_next_stamp: 0.0,
            trail: Vec::new(),
            placed: HashMap::new(),
            smoother,
            curve: PathInterpolator::new(start),
            jitter,
            heading: None,
            pressure,
            time_ms: None,
            travelled: 0.0,
            tilt,
            speed: SpeedMeter::default(),
            ribbon_anchor: None,
            ribbon_tip: None,
            pending_start: None,
            dabs_placed: 0,
        };
        let dirty = match kind {
            StrokeKind::Pencil => stroke.plot_pencil_point(target, at.floor()),
            StrokeKind::Brush | StrokeKind::Eraser | StrokeKind::CloneFrom(_) if stroke.is_ribbon() => stroke.begin_ribbon(target, at, pressure, tilt),
            StrokeKind::Brush | StrokeKind::Eraser | StrokeKind::CloneFrom(_) if stroke.waits_for_heading() => {
                stroke.pending_start = Some(start);
                IRect::EMPTY
            }
            StrokeKind::Brush | StrokeKind::Eraser | StrokeKind::CloneFrom(_) => stroke.stamp_at(target, at, pressure, tilt, 0.0),
        };
        stroke.until_next_stamp = stroke.spacing(pressure);
        target.edit.mark_dirty(dirty);
        (stroke, dirty)
    }

    pub fn set_pressure(&mut self, pressure: f32) {
        self.pressure = pressure.clamp(0.0, 1.0);
    }

    pub fn set_tilt(&mut self, tilt: Option<Tilt>) {
        self.tilt = tilt;
    }

    pub fn set_time_ms(&mut self, time_ms: f64) {
        self.time_ms = Some(time_ms);
    }

    fn last_point(&self) -> FPoint {
        self.last_sample.point
    }

    fn is_ribbon(&self) -> bool {
        self.params.kind != StrokeKind::Pencil && self.params.brush.settings.engine == BrushEngine::Ribbon
    }

    fn spacing(&self, pressure: f32) -> f32 {
        let settings = &self.params.brush.settings;
        let loosening = 1.0 + (1.0 - pressure) * settings.pressure_spacing * PRESSURE_SPACING_REACH;
        (self.params.size * settings.spacing * loosening).max(MIN_BRUSH_SPACING)
    }

    fn dryness(&self, pressure: f32, distance: f32) -> f32 {
        let settings = &self.params.brush.settings;
        let depleted = (distance / (DEPLETION_LENGTH_IN_SIZES * self.params.size)).clamp(0.0, 1.0);
        let dryness = settings.dryness + self.speed.value() * settings.velocity_dryness + (1.0 - pressure) * settings.pressure_dryness + depleted * settings.depletion;
        dryness.clamp(0.0, 1.0)
    }

    fn follow(&mut self, to: FPoint) {
        let from = self.last_point();
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let length = dx.hypot(dy);
        if length == 0.0 {
            return;
        }
        let direction = FPoint::new(dx / length, dy / length);
        self.heading = Some(if self.is_ribbon() {
            self.ribbon_heading(direction, length)
        } else {
            self.heading.map_or(direction, |heading| heading.lerp(direction, HEADING_BLEND))
        });
    }

    fn ribbon_heading(&self, direction: FPoint, length: f32) -> FPoint {
        let blend = 1.0 - (-length / (RIBBON_HEADING_MEMORY_IN_SIZES * self.params.size)).exp();
        self.heading.unwrap_or_default().lerp(direction, blend)
    }

    fn pen_response(&self, pressure: f32, tilt: Option<Tilt>, distance: f32) -> PenResponse {
        let settings = &self.params.brush.settings;
        let scaled = |shaped: f32, strength: f32| 1.0 + (shaped - 1.0) * strength;
        let pressure_scale = |strength: f32| scaled(pressure, strength);
        let size_pressure = settings.pressure_size_scale(pressure);
        let opacity_pressure = scaled(settings.pressure_opacity_curve.eval(pressure), settings.pressure_opacity);
        let fade = (distance / (settings.size_fade * self.params.size)).clamp(0.0, 1.0);
        let fade_scale = settings.size_start + (settings.size_end - settings.size_start) * fade;
        let speed = self.speed.value();
        let speed_size = (1.0 - speed * settings.velocity_size).max(MIN_SPEED_SCALE);
        let heading = settings.follow_direction.then_some(self.heading).flatten().map_or(0.0, |heading| heading.y.atan2(heading.x));
        let tilt_angle = tilt.map_or(0.0, |tilt| tilt.azimuth() * settings.tilt_angle);
        let tilt_flattening = 1.0 - tilt.map_or(0.0, |tilt| tilt.lean() * settings.tilt_roundness);
        PenResponse {
            size: self.params.size * fade_scale * size_pressure * speed_size,
            angle: settings.angle.to_radians() + heading + tilt_angle,
            roundness: (settings.roundness * pressure_scale(settings.pressure_roundness) * tilt_flattening).clamp(ROUNDNESS_RANGE.0, ROUNDNESS_RANGE.1),
            strength: settings.flow * opacity_pressure * (1.0 - speed * settings.velocity_opacity),
            hardness: (settings.hardness * pressure_scale(settings.pressure_hardness)).clamp(0.0, 1.0),
            dryness: self.dryness(pressure, distance),
        }
    }

    fn next_dab(&mut self, at: FPoint, pressure: f32, tilt: Option<Tilt>, distance: f32) -> Dab {
        let response = self.pen_response(pressure, tilt, distance);
        let settings = &self.params.brush.settings;
        let size = (response.size * (1.0 - settings.size_jitter * self.jitter.unit())).max(MIN_DAB_SIZE);
        let angle = response.angle + self.jitter.signed() * settings.angle_jitter * std::f32::consts::PI;
        let scatter_angle = self.jitter.unit() * std::f32::consts::TAU;
        let scatter = settings.scatter + settings.pressure_scatter * (1.0 - pressure) * PRESSURE_SCATTER_REACH;
        let scatter_reach = self.jitter.unit().sqrt() * scatter * self.params.size;
        let center = FPoint::new(at.x + scatter_angle.cos() * scatter_reach, at.y + scatter_angle.sin() * scatter_reach);
        let strength = response.strength * (1.0 - settings.opacity_jitter * self.jitter.unit());
        let color = self.dab_color();
        let variant = match &self.params.brush.tip {
            BrushTip::Set(_) if self.params.brush.settings.cycle_tips => self.dabs_placed % self.params.brush.tip.variant_count(),
            BrushTip::Set(_) => self.params.brush.tip.pick_variant(self.jitter.unit()),
            _ => 0,
        };
        self.dabs_placed += 1;
        Dab { center, size, angle, roundness: response.roundness, strength, hardness: response.hardness, color, variant, dryness: response.dryness }
    }

    fn dab_color(&mut self) -> Rgba {
        let settings = &self.params.brush.settings;
        if settings.hue_jitter + settings.saturation_jitter + settings.value_jitter == 0.0 {
            return self.params.color;
        }
        let hue = self.jitter.signed() * settings.hue_jitter * HUE_JITTER_TURNS;
        let saturation = self.jitter.signed() * settings.saturation_jitter;
        let value = self.jitter.signed() * settings.value_jitter;
        tint::shifted(self.params.color, hue, saturation, value)
    }

    fn stamp_at(&mut self, target: &mut StrokeTarget, at: FPoint, pressure: f32, tilt: Option<Tilt>, distance: f32) -> IRect {
        let dab = self.next_dab(at, pressure, tilt, distance);
        stamp(target, &self.params, dab)
    }

    pub fn extend(&mut self, to: FPoint, target: &mut StrokeTarget) -> IRect {
        let input = PathSample { point: to, pressure: self.pressure, tilt: self.tilt, time_ms: self.time_ms };
        let samples = self.path_through(input);
        self.advance_through(samples, target)
    }

    pub fn flush(&mut self, end: FPoint, target: &mut StrokeTarget) -> IRect {
        let samples = self.path_end(end);
        let dirty = self.advance_through(samples, target);
        dirty.union(self.finish_ribbon(target)).union(self.place_pending_start(target))
    }

    fn place_pending_start(&mut self, target: &mut StrokeTarget) -> IRect {
        let Some(start) = self.pending_start.take() else {
            return IRect::EMPTY;
        };
        let dirty = self.stamp_at(target, start.point, start.pressure, start.tilt, 0.0);
        target.edit.mark_dirty(dirty);
        dirty
    }


    fn path_through(&mut self, input: PathSample) -> Vec<PathSample> {
        if let Some(smoother) = self.smoother.as_mut() {
            let points = smoother.push(input.point);
            return self.with_attributes(points, input);
        }
        match self.params.kind {
            StrokeKind::Pencil => vec![input],
            StrokeKind::Brush | StrokeKind::Eraser | StrokeKind::CloneFrom(_) => self.curve.push(input),
        }
    }

    fn path_end(&mut self, end: FPoint) -> Vec<PathSample> {
        if let Some(smoother) = self.smoother.as_mut() {
            let points = smoother.finish(end);
            let input = PathSample { point: end, pressure: self.pressure, tilt: self.tilt, time_ms: self.time_ms };
            return self.with_attributes(points, input);
        }
        match self.params.kind {
            StrokeKind::Pencil => Vec::new(),
            StrokeKind::Brush | StrokeKind::Eraser | StrokeKind::CloneFrom(_) => self.curve.finish(),
        }
    }

    fn with_attributes(&self, points: Vec<FPoint>, input: PathSample) -> Vec<PathSample> {
        let count = points.len() as f32;
        points.into_iter().enumerate().map(|(index, point)| self.last_sample.toward(input, (index + 1) as f32 / count, point)).collect()
    }

    fn advance_through(&mut self, samples: Vec<PathSample>, target: &mut StrokeTarget) -> IRect {
        if self.is_ribbon() {
            return self.extend_ribbon(samples, target);
        }
        samples.into_iter().fold(IRect::EMPTY, |dirty, sample| dirty.union(self.advance_to(sample, target)))
    }

    fn advance_to(&mut self, sample: PathSample, target: &mut StrokeTarget) -> IRect {
        let dirty = match self.params.kind {
            StrokeKind::Pencil => self.extend_pencil(sample.point, target),
            StrokeKind::Brush | StrokeKind::Eraser | StrokeKind::CloneFrom(_) => self.extend_stamps(sample, target),
        };
        self.last_sample = sample;
        target.edit.mark_dirty(dirty);
        dirty
    }

    fn ribbon_nib(&self, at: FPoint, pressure: f32, tilt: Option<Tilt>, distance: f32) -> Nib {
        let response = self.pen_response(pressure, tilt, distance);
        let heading = self.heading.map_or(response.angle + std::f32::consts::FRAC_PI_2, |heading| heading.y.atan2(heading.x));
        Nib {
            center: at,
            radius: response.size.max(MIN_DAB_SIZE) / 2.0,
            angle: response.angle,
            roundness: response.roundness,
            strength: response.strength,
            dryness: response.dryness,
            heading,
            bristles: self.params.brush.settings.bristles,
            distance,
        }
    }

    fn begin_ribbon(&mut self, target: &mut StrokeTarget, at: FPoint, pressure: f32, tilt: Option<Tilt>) -> IRect {
        let nib = self.ribbon_nib(at, pressure, tilt, 0.0);
        self.ribbon_anchor = Some(nib);
        self.ribbon_tip = Some(nib);
        if self.waits_for_heading() {
            return IRect::EMPTY;
        }
        self.paint_standing_nib(target, nib)
    }

    fn track(&self) -> TrackSampler<'_> {
        TrackSampler::new(&self.params.brush.textures.layers, self.params.size)
    }

    fn waits_for_heading(&self) -> bool {
        self.params.brush.settings.follow_direction
    }

    fn paint_standing_nib(&self, target: &mut StrokeTarget, nib: Nib) -> IRect {
        paint_swept(target, &self.params, &SweptNib::new(nib, nib, self.params.brush.hairs(), self.track()))
    }

    fn extend_ribbon(&mut self, samples: Vec<PathSample>, target: &mut StrokeTarget) -> IRect {
        let mut dirty = IRect::EMPTY;
        for sample in samples {
            let moved = self.last_point().distance(sample.point);
            self.follow(sample.point);
            self.speed.advance(moved, sample.time_ms);
            self.travelled += moved;
            let nib = self.ribbon_nib(sample.point, sample.pressure, sample.tilt, self.travelled);
            self.ribbon_tip = Some(nib);
            self.last_sample = sample;
            if self.ribbon_anchor.is_none_or(|anchor| anchor.travel_to(&nib) >= ribbon_step(nib)) {
                dirty = dirty.union(self.sweep_ribbon_to(nib, target));
            }
        }
        target.edit.mark_dirty(dirty);
        dirty
    }

    fn sweep_ribbon_to(&mut self, nib: Nib, target: &mut StrokeTarget) -> IRect {
        let anchor = self.ribbon_anchor.replace(nib).unwrap_or(nib);
        paint_swept(target, &self.params, &SweptNib::new(anchor, nib, self.params.brush.hairs(), self.track()))
    }

    fn finish_ribbon(&mut self, target: &mut StrokeTarget) -> IRect {
        let Some(tip) = self.ribbon_tip else {
            return IRect::EMPTY;
        };
        let dirty = if self.travelled == 0.0 && self.waits_for_heading() {
            self.paint_standing_nib(target, tip)
        } else if self.ribbon_anchor.is_some_and(|anchor| anchor != tip) {
            self.sweep_ribbon_to(tip, target)
        } else {
            return IRect::EMPTY;
        };
        target.edit.mark_dirty(dirty);
        dirty
    }

    fn extend_pencil(&mut self, to: FPoint, target: &mut StrokeTarget) -> IRect {
        let mut dirty = IRect::EMPTY;
        let from = self.last_point().floor();
        for point in line_pixels(from, to.floor()).into_iter().skip(1) {
            dirty = dirty.union(self.plot_pencil_point(target, point));
        }
        dirty
    }

    fn plot_pencil_point(&mut self, target: &mut StrokeTarget, point: IPoint) -> IRect {
        let mut dirty = IRect::EMPTY;
        if self.params.pixel_perfect
            && let [first, corner] = self.trail[..]
            && forms_corner(first, corner, point)
        {
            let count = self.placed.entry(corner).or_insert(1);
            *count -= 1;
            if *count == 0 {
                if let Some(at) = locate(target.raster.bounds(), self.params.wrap, corner.x, corner.y) {
                    target.edit.restore_pixel(target.raster, at.x, at.y);
                }
                dirty = dirty_area(IRect::from_size(corner.x, corner.y, 1, 1), target.raster.bounds(), self.params.wrap);
            }
            self.trail.pop();
        }
        dirty = dirty.union(plot_pencil(target, &self.params, point));
        *self.placed.entry(point).or_insert(0) += 1;
        self.trail.push(point);
        if self.trail.len() > 2 {
            self.trail.remove(0);
        }
        dirty
    }

    fn extend_stamps(&mut self, sample: PathSample, target: &mut StrokeTarget) -> IRect {
        let from = self.last_point();
        let total = from.distance(sample.point);
        self.follow(sample.point);
        self.speed.advance(total, sample.time_ms);
        let mut dirty = if self.heading.is_some() { self.place_pending_start(target) } else { IRect::EMPTY };
        let mut position = self.until_next_stamp;
        while position <= total {
            let along = self.last_sample.toward(sample, position / total, from.lerp(sample.point, position / total));
            dirty = dirty.union(self.stamp_at(target, along.point, along.pressure, along.tilt, self.travelled + position));
            position += self.spacing(along.pressure);
        }
        self.until_next_stamp = position - total;
        self.travelled += total;
        dirty
    }
}

fn ribbon_step(nib: Nib) -> f32 {
    (nib.radius * 2.0 * RIBBON_STEP_RATIO).max(RIBBON_MIN_STEP)
}

fn forms_corner(first: IPoint, middle: IPoint, last: IPoint) -> bool {
    let steps = |a: IPoint, b: IPoint| (b.x - a.x).abs() + (b.y - a.y).abs();
    (last.x - first.x).abs() == 1 && (last.y - first.y).abs() == 1 && steps(first, middle) == 1 && steps(middle, last) == 1
}

pub fn line_pixels(from: IPoint, to: IPoint) -> Vec<IPoint> {
    let (mut x, mut y) = (from.x, from.y);
    let dx = (to.x - from.x).abs();
    let dy = -(to.y - from.y).abs();
    let step_x = if from.x < to.x { 1 } else { -1 };
    let step_y = if from.y < to.y { 1 } else { -1 };
    let mut error = dx + dy;
    let mut pixels = Vec::with_capacity((dx.max(-dy) + 1) as usize);
    loop {
        pixels.push(IPoint::new(x, y));
        if x == to.x && y == to.y {
            return pixels;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x += step_x;
        }
        if doubled <= dx {
            error += dx;
            y += step_y;
        }
    }
}

fn plot_pencil(target: &mut StrokeTarget, params: &StrokeParams, at: IPoint) -> IRect {
    paint_pixel(target, params, params.color, at.x, at.y, FULL_COVERAGE);
    dirty_area(IRect::from_size(at.x, at.y, 1, 1), target.raster.bounds(), params.wrap)
}

fn locate(bounds: IRect, wrap: bool, x: i32, y: i32) -> Option<IPoint> {
    if wrap {
        return Some(IPoint::new(x.rem_euclid(bounds.width()), y.rem_euclid(bounds.height())));
    }
    let point = IPoint::new(x, y);
    bounds.contains(point).then_some(point)
}

pub(crate) fn dirty_area(area: IRect, bounds: IRect, wrap: bool) -> IRect {
    if !wrap {
        return area.intersect(bounds);
    }
    if area.is_empty() {
        return IRect::EMPTY;
    }
    let span = |low: i32, high: i32, size: i32| {
        let start = low.rem_euclid(size);
        if high - low >= size || start + (high - low) > size { (0, size) } else { (start, start + high - low) }
    };
    let (x0, x1) = span(area.x0, area.x1, bounds.width());
    let (y0, y1) = span(area.y0, area.y1, bounds.height());
    IRect::new(bounds.x0 + x0, bounds.y0 + y0, bounds.x0 + x1, bounds.y0 + y1)
}

fn stamp(target: &mut StrokeTarget, params: &StrokeParams, dab: Dab) -> IRect {
    let hardness = if params.antialias { dab.hardness } else { SOLID_HARDNESS };
    let sampler = DabSampler::new(&params.brush.tip, dab, hardness, params.antialias);
    let reach = sampler.reach().ceil() as i32 + STAMP_MARGIN;
    let origin = dab.center.floor();
    let bounds = target.raster.bounds();
    let reached = IRect::new(origin.x - reach, origin.y - reach, origin.x + reach + 1, origin.y + reach + 1);
    let area = if params.wrap { reached } else { reached.intersect(bounds) };
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let coverage = pixel_coverage(target, params, dab.dryness, dab.strength, sampler.coverage_at(x as f32 + 0.5, y as f32 + 0.5), x, y);
            if coverage > 0.0 {
                paint_pixel(target, params, dab.color, x, y, (coverage * FULL_COVERAGE as f32).round() as u8);
            }
        }
    }
    dirty_area(area, bounds, params.wrap)
}

fn paint_swept(target: &mut StrokeTarget, params: &StrokeParams, swept: &SweptNib) -> IRect {
    let reach = swept.reach().ceil() as i32 + STAMP_MARGIN;
    let (low, high) = swept.bounds();
    let (low, high) = (low.floor(), high.floor());
    let bounds = target.raster.bounds();
    let reached = IRect::new(low.x - reach, low.y - reach, high.x + reach + 1, high.y + reach + 1);
    let area = if params.wrap { reached } else { reached.intersect(bounds) };
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let coverage = pixel_coverage(target, params, swept.dryness(), swept.strength(), swept.coverage_at(x as f32 + 0.5, y as f32 + 0.5, params.antialias), x, y);
            if coverage > 0.0 {
                paint_pixel(target, params, params.color, x, y, (coverage * FULL_COVERAGE as f32).round() as u8);
            }
        }
    }
    dirty_area(area, bounds, params.wrap)
}

fn pixel_coverage(target: &StrokeTarget, params: &StrokeParams, dryness: f32, strength: f32, shape: f32, x: i32, y: i32) -> f32 {
    if shape <= 0.0 {
        return 0.0;
    }
    let Some(IPoint { x, y }) = locate(target.raster.bounds(), params.wrap, x, y) else {
        return 0.0;
    };
    let settings = &params.brush.settings;
    let (gated, tooth) = match params.brush.tip {
        BrushTip::Round => (shape, 1.0 - (1.0 - settings.grain) * (1.0 - dryness)),
        BrushTip::Mask(_) | BrushTip::Set(_) => (shape * dry_gate(shape, dryness), settings.grain),
    };
    let coverage = gated * strength;
    if settings.accumulate {
        grained_deposit(params.brush.paper(), coverage, tooth, x, y, settings.grain_scale)
    } else {
        grained(params.brush.paper(), coverage, tooth, x, y, settings.grain_scale)
    }
}

fn clone_source_pixel(target: &StrokeTarget, params: &StrokeParams, offset: IPoint, at: IPoint) -> Option<Rgba> {
    let source = locate(target.raster.bounds(), params.wrap, at.x + offset.x, at.y + offset.y)?;
    Some(target.edit.original_pixel(target.raster, source.x, source.y))
}

fn paint_pixel(target: &mut StrokeTarget, params: &StrokeParams, color: Rgba, x: i32, y: i32, coverage: u8) {
    let Some(IPoint { x, y }) = locate(target.raster.bounds(), params.wrap, x, y) else {
        return;
    };
    let color = match params.kind {
        StrokeKind::CloneFrom(offset) => match clone_source_pixel(target, params, offset, IPoint::new(x, y)) {
            Some(sampled) => sampled,
            None => return,
        },
        _ => color,
    };
    let selection_coverage = target.selection.map_or(FULL_COVERAGE, |s| s.coverage_at(x, y));
    let effective = mul_div_255(coverage as u32, selection_coverage as u32) as u8;
    let mode = params.blend;
    let merge = if params.brush.settings.accumulate && params.kind != StrokeKind::Pencil { CoverageMerge::Accumulate } else { CoverageMerge::Max };
    match params.kind {
        StrokeKind::Eraser => target.edit.paint_pixel_merged(target.raster, x, y, effective, merge, erase_pixel),
        StrokeKind::Pencil | StrokeKind::Brush if erases(color) => target.edit.paint_pixel_merged(target.raster, x, y, effective, merge, erase_pixel),
        StrokeKind::Pencil | StrokeKind::Brush | StrokeKind::CloneFrom(_) => {
            target.edit.paint_pixel_merged(target.raster, x, y, effective, merge, |before, cov| {
                blend(mode, before, [color[0], color[1], color[2], mul_div_255(color[3] as u32, cov as u32) as u8])
            });
        }
    }
}

fn erase_pixel(before: Rgba, coverage: u8) -> Rgba {
    let keep = FULL_COVERAGE as u32 - coverage as u32;
    [before[0], before[1], before[2], mul_div_255(before[3] as u32, keep) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::{BrushEngine, BrushSettings};

    #[test]
    fn bresenham_includes_both_ends_and_is_connected() {
        let pixels = line_pixels(IPoint::new(0, 0), IPoint::new(7, 3));
        assert_eq!(pixels.first(), Some(&IPoint::new(0, 0)));
        assert_eq!(pixels.last(), Some(&IPoint::new(7, 3)));
        assert_eq!(pixels.len(), 8);
        for pair in pixels.windows(2) {
            assert!((pair[1].x - pair[0].x).abs() <= 1 && (pair[1].y - pair[0].y).abs() <= 1);
        }
    }

    #[test]
    fn bresenham_single_point() {
        assert_eq!(line_pixels(IPoint::new(2, 2), IPoint::new(2, 2)), vec![IPoint::new(2, 2)]);
    }

    fn params(kind: StrokeKind, size: f32) -> StrokeParams {
        StrokeParams { kind, color: [255, 0, 0, 255], size, antialias: true, blend: BlendMode::Normal, pixel_perfect: false, smooth: false, wrap: false, brush: Arc::new(Brush { settings: BrushSettings { hardness: 1.0, ..BrushSettings::default() }, ..Brush::round() }), seed: 0, pressure: 1.0, tilt: None }
    }

    #[test]
    fn pencil_line_paints_every_pixel_once() {
        let mut raster = Raster::transparent(10, 10);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        let (mut stroke, _) = Stroke::begin(params(StrokeKind::Pencil, 1.0), FPoint::new(1.5, 1.5), &mut target);
        stroke.extend(FPoint::new(6.5, 1.5), &mut target);
        for x in 1..=6 {
            assert_eq!(raster.get(x, 1), [255, 0, 0, 255]);
        }
        assert_eq!(raster.get(7, 1)[3], 0);
    }

    #[test]
    fn brush_respects_selection() {
        let mut raster = Raster::transparent(20, 20);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let selection = Selection::from_rect(IRect::new(0, 0, 10, 20)).unwrap();
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: Some(&selection) };
        let (mut stroke, _) = Stroke::begin(params(StrokeKind::Brush, 6.0), FPoint::new(4.0, 10.0), &mut target);
        stroke.extend(FPoint::new(16.0, 10.0), &mut target);
        assert_eq!(raster.get(4, 10), [255, 0, 0, 255]);
        assert_eq!(raster.get(15, 10)[3], 0);
    }

    #[test]
    fn ribbon_stroke_has_no_gaps_and_reaches_the_end_after_flush() {
        let mut raster = Raster::transparent(40, 20);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        let ribbon = Arc::new(Brush { settings: BrushSettings { engine: BrushEngine::Ribbon, pressure_size: 0.0, ..BrushSettings::default() }, ..Brush::round() });
        let paint = StrokeParams { brush: ribbon, ..params(StrokeKind::Brush, 4.0) };
        let (mut stroke, _) = Stroke::begin(paint, FPoint::new(5.0, 10.0), &mut target);
        stroke.extend(FPoint::new(20.0, 10.0), &mut target);
        stroke.extend(FPoint::new(35.0, 10.0), &mut target);
        stroke.flush(FPoint::new(35.0, 10.0), &mut target);
        assert!((5..=35).all(|x| raster.get(x, 10)[3] == 255));
    }

    #[test]
    fn direction_following_ribbon_leaves_a_dot_on_click_without_movement() {
        let mut raster = Raster::transparent(20, 20);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        let ribbon = Arc::new(Brush { settings: BrushSettings { engine: BrushEngine::Ribbon, follow_direction: true, ..BrushSettings::default() }, ..Brush::round() });
        let paint = StrokeParams { brush: ribbon, ..params(StrokeKind::Brush, 6.0) };
        let (mut stroke, _) = Stroke::begin(paint, FPoint::new(10.0, 10.0), &mut target);
        stroke.flush(FPoint::new(10.0, 10.0), &mut target);
        assert_eq!(raster.get(10, 10), [255, 0, 0, 255]);
    }

    fn direction_following_flat_brush() -> StrokeParams {
        let flat = BrushSettings { follow_direction: true, roundness: 0.2, hardness: 1.0, ..BrushSettings::default() };
        StrokeParams { brush: Arc::new(Brush { settings: flat, ..Brush::round() }), ..params(StrokeKind::Brush, 12.0) }
    }

    #[test]
    fn direction_following_stamp_starts_along_the_stroke_not_at_zero_degrees() {
        let mut raster = Raster::transparent(20, 30);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        let (mut stroke, _) = Stroke::begin(direction_following_flat_brush(), FPoint::new(10.0, 10.0), &mut target);
        stroke.extend(FPoint::new(10.0, 25.0), &mut target);
        stroke.flush(FPoint::new(10.0, 25.0), &mut target);
        assert_eq!(raster.get(10, 6), [255, 0, 0, 255]);
        assert_eq!(raster.get(5, 10), [0, 0, 0, 0]);
    }

    #[test]
    fn direction_following_stamp_leaves_a_dot_on_click_without_movement() {
        let mut raster = Raster::transparent(20, 20);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        let (mut stroke, _) = Stroke::begin(direction_following_flat_brush(), FPoint::new(10.0, 10.0), &mut target);
        stroke.flush(FPoint::new(10.0, 10.0), &mut target);
        assert_eq!(raster.get(10, 10), [255, 0, 0, 255]);
    }

    #[test]
    fn wrapped_brush_continues_on_the_opposite_edge() {
        let mut raster = Raster::transparent(20, 20);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        let paint = StrokeParams { wrap: true, ..params(StrokeKind::Brush, 6.0) };
        let (_, dirty) = Stroke::begin(paint, FPoint::new(1.0, 10.5), &mut target);
        assert_eq!(raster.get(1, 10), [255, 0, 0, 255]);
        assert_eq!(raster.get(18, 10), [255, 0, 0, 255]);
        assert_eq!((dirty.x0, dirty.x1), (0, 20));
    }

    #[test]
    fn wrapped_pencil_outside_the_layer_lands_inside() {
        let mut raster = Raster::transparent(10, 10);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        let paint = StrokeParams { wrap: true, ..params(StrokeKind::Pencil, 1.0) };
        Stroke::begin(paint, FPoint::new(-0.5, 23.5), &mut target);
        assert_eq!(raster.get(9, 3), [255, 0, 0, 255]);
    }

    #[test]
    fn eraser_clears_alpha() {
        let mut raster = Raster::filled(10, 10, [9, 9, 9, 255]);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        Stroke::begin(params(StrokeKind::Eraser, 4.0), FPoint::new(5.0, 5.0), &mut target);
        assert_eq!(raster.get(5, 5)[3], 0);
        assert_eq!(raster.get(0, 0)[3], 255);
    }

    fn clear_params(kind: StrokeKind) -> StrokeParams {
        StrokeParams { color: [255, 0, 0, 0], ..params(kind, 4.0) }
    }

    fn pencil_path(pixel_perfect: bool, points: &[(f32, f32)]) -> Raster {
        let mut raster = Raster::transparent(12, 12);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        let paint = StrokeParams { pixel_perfect, ..params(StrokeKind::Pencil, 1.0) };
        let (mut stroke, _) = Stroke::begin(paint, FPoint::new(points[0].0, points[0].1), &mut target);
        for &(x, y) in &points[1..] {
            stroke.extend(FPoint::new(x, y), &mut target);
        }
        raster
    }

    fn painted(raster: &Raster) -> Vec<(u32, u32)> {
        (0..12).flat_map(|y| (0..12).map(move |x| (x, y))).filter(|&(x, y)| raster.get(x, y)[3] > 0).collect()
    }

    #[test]
    fn pixel_perfect_drops_the_corner_of_a_staircase() {
        let path = [(1.5, 1.5), (2.5, 1.5), (2.5, 2.5), (3.5, 2.5), (3.5, 3.5)];
        let raw = pencil_path(false, &path);
        assert_eq!(painted(&raw).len(), 5);
        let perfect = pencil_path(true, &path);
        assert_eq!(painted(&perfect), vec![(1, 1), (2, 2), (3, 3)]);
    }

    #[test]
    fn pixel_perfect_keeps_straight_lines_and_cuts_square_turns_like_aseprite() {
        let straight = pencil_path(true, &[(1.5, 1.5), (6.5, 1.5)]);
        assert_eq!(painted(&straight).len(), 6);
        let turn = pencil_path(true, &[(1.5, 1.5), (4.5, 1.5), (4.5, 4.5)]);
        assert_eq!(painted(&turn).len(), 6);
        assert!(!painted(&turn).contains(&(4, 1)));
    }

    #[test]
    fn pixel_perfect_leaves_a_pixel_alone_when_the_stroke_crossed_it_earlier() {
        let path = [(2.5, 2.5), (5.5, 2.5), (5.5, 3.5), (2.5, 3.5), (2.5, 2.5), (1.5, 2.5), (1.5, 3.5)];
        let raster = pencil_path(true, &path);
        assert!(painted(&raster).contains(&(2, 2)));
    }

    #[test]
    fn brush_with_zero_alpha_erases_instead_of_doing_nothing() {
        let mut raster = Raster::filled(10, 10, [9, 9, 9, 255]);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        Stroke::begin(clear_params(StrokeKind::Brush), FPoint::new(5.0, 5.0), &mut target);
        assert_eq!(raster.get(5, 5)[3], 0);
        assert_eq!(raster.get(0, 0)[3], 255);
    }

    #[test]
    fn pencil_with_zero_alpha_erases_one_pixel() {
        let mut raster = Raster::filled(10, 10, [9, 9, 9, 255]);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        let (mut stroke, _) = Stroke::begin(clear_params(StrokeKind::Pencil), FPoint::new(2.5, 2.5), &mut target);
        stroke.extend(FPoint::new(5.5, 2.5), &mut target);
        assert!((2..=5).all(|x| raster.get(x, 2)[3] == 0));
        assert_eq!(raster.get(6, 2)[3], 255);
    }

    #[test]
    fn half_transparent_brush_still_paints_over() {
        let mut raster = Raster::filled(10, 10, [9, 9, 9, 255]);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
        let paint = StrokeParams { color: [255, 0, 0, 128], ..params(StrokeKind::Brush, 4.0) };
        Stroke::begin(paint, FPoint::new(5.0, 5.0), &mut target);
        assert_eq!(raster.get(5, 5)[3], 255);
        assert!(raster.get(5, 5)[0] > 100);
    }
}
