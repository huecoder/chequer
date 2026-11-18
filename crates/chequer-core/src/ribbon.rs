use crate::brush::{Texture, TrackSampler, dry_gate};
use crate::geom::FPoint;
use std::f32::consts::{PI, TAU};

const MIN_DISTANCE: f32 = 1e-6;
const EDGE_HALF_WIDTH: f32 = 0.5;
const STREAK_SLOPE_START: f32 = 0.1;
const STREAK_SLOPE_FULL: f32 = 0.4;
const TRACK_TAPS: [(f32, f32); 4] = [(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nib {
    pub center: FPoint,
    pub radius: f32,
    pub angle: f32,
    pub roundness: f32,
    pub strength: f32,
    pub dryness: f32,
    pub heading: f32,
    pub bristles: f32,
    pub distance: f32,
}

impl Nib {
    pub fn travel_to(&self, other: &Nib) -> f32 {
        let turn = (other.angle - self.angle + PI).rem_euclid(TAU) - PI;
        self.center.distance(other.center).max(turn.abs() * self.radius.max(other.radius))
    }
}

pub struct SweptNib<'a> {
    hairs: &'a Texture,
    track: TrackSampler<'a>,
    from: Nib,
    to: Nib,
    cos: f32,
    sin: f32,
    start: FPoint,
    span: FPoint,
    span_length_sq: f32,
    motion: FPoint,
    streak_weight: f32,
}

impl<'a> SweptNib<'a> {
    pub fn new(from: Nib, to: Nib, hairs: &'a Texture, track: TrackSampler<'a>) -> Self {
        let (cos, sin) = (to.angle.cos(), to.angle.sin());
        let (heading_x, heading_y) = (to.heading.cos(), to.heading.sin());
        let motion = FPoint::new(heading_x * cos + heading_y * sin, (heading_y * cos - heading_x * sin) / to.roundness);
        let streak_slope = motion.y.abs() / motion.x.hypot(motion.y);
        let mut swept = Self {
            hairs,
            track,
            from,
            to,
            cos,
            sin,
            start: FPoint::default(),
            span: FPoint::default(),
            span_length_sq: 0.0,
            motion,
            streak_weight: smooth_step(STREAK_SLOPE_START, STREAK_SLOPE_FULL, streak_slope),
        };
        swept.start = swept.in_nib_space(from.center.x, from.center.y);
        swept.span = FPoint::new(-swept.start.x, -swept.start.y);
        swept.span_length_sq = swept.span.x * swept.span.x + swept.span.y * swept.span.y;
        swept
    }

    pub fn strength(&self) -> f32 {
        self.to.strength
    }

    pub fn dryness(&self) -> f32 {
        self.to.dryness
    }

    pub fn reach(&self) -> f32 {
        self.from.radius.max(self.to.radius)
    }

    pub fn bounds(&self) -> (FPoint, FPoint) {
        let (from, to) = (self.from.center, self.to.center);
        (FPoint::new(from.x.min(to.x), from.y.min(to.y)), FPoint::new(from.x.max(to.x), from.y.max(to.y)))
    }

    pub fn coverage_at(&self, x: f32, y: f32, antialias: bool) -> f32 {
        let point = self.in_nib_space(x, y);
        let along_span = if self.span_length_sq > MIN_DISTANCE {
            (((point.x - self.start.x) * self.span.x + (point.y - self.start.y) * self.span.y) / self.span_length_sq).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let nearest = FPoint::new(self.start.x + self.span.x * along_span, self.start.y + self.span.y * along_span);
        let (offset_x, offset_y) = (point.x - nearest.x, point.y - nearest.y);
        let distance = offset_x.hypot(offset_y);
        let radius = self.from.radius + (self.to.radius - self.from.radius) * along_span;
        let slope = if distance > MIN_DISTANCE { (offset_x / distance).hypot(offset_y / distance / self.to.roundness) } else { 1.0 };
        let depth = (radius - distance) / slope;
        let edge = if antialias { (depth + EDGE_HALF_WIDTH).clamp(0.0, 1.0) } else if depth >= 0.0 { 1.0 } else { 0.0 };
        let streaked = if self.to.bristles > 0.0 && self.streak_weight > 0.0 && edge > 0.0 { edge * self.hair_factor(point) } else { edge };
        if edge > 0.0 { streaked * self.track_factor(FPoint::new(x, y), along_span, radius) } else { streaked }
    }

    fn track_factor(&self, at: FPoint, along_span: f32, radius: f32) -> f32 {
        if self.track.is_empty() {
            return 1.0;
        }
        let spine = self.from.center.lerp(self.to.center, along_span);
        let travelled = self.from.distance + (self.to.distance - self.from.distance) * along_span;
        let (heading_x, heading_y) = (self.to.heading.cos(), self.to.heading.sin());
        let sum: f32 = TRACK_TAPS
            .iter()
            .map(|&(tap_x, tap_y)| {
                let (offset_x, offset_y) = (at.x + tap_x - spine.x, at.y + tap_y - spine.y);
                let across = (offset_y * heading_x - offset_x * heading_y) / (2.0 * radius) + 0.5;
                self.track.value(across, travelled + offset_x * heading_x + offset_y * heading_y)
            })
            .sum();
        sum / TRACK_TAPS.len() as f32
    }

    fn hair_factor(&self, point: FPoint) -> f32 {
        let hit = point.x - point.y * self.motion.x / self.motion.y;
        let position = (hit / self.to.radius * 0.5 + 0.5).clamp(0.0, 1.0);
        let load = 1.0 - self.to.bristles * (1.0 - self.hairs.across(position));
        let streaked = load * dry_gate(load, self.to.dryness);
        1.0 + (streaked - 1.0) * self.streak_weight
    }

    fn in_nib_space(&self, x: f32, y: f32) -> FPoint {
        let (dx, dy) = (x - self.to.center.x, y - self.to.center.y);
        FPoint::new(dx * self.cos + dy * self.sin, (dy * self.cos - dx * self.sin) / self.to.roundness)
    }
}

fn smooth_step(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_nib(x: f32) -> Nib {
        Nib { center: FPoint::new(x, 0.0), radius: 10.0, angle: 0.0, roundness: 0.2, strength: 1.0, dryness: 0.0, heading: 0.0, bristles: 0.0, distance: x }
    }

    #[test]
    fn flat_nib_swept_along_its_long_side_stays_thin_and_reaches_both_ends() {
        let swept = SweptNib::new(flat_nib(-5.0), flat_nib(5.0), Texture::builtin_hairs(), TrackSampler::new(&[], 1.0));
        assert_eq!(swept.coverage_at(0.0, 1.0, true), 1.0);
        assert_eq!(swept.coverage_at(0.0, 3.5, true), 0.0);
        assert_eq!(swept.coverage_at(14.0, 0.0, true), 1.0);
        assert_eq!(swept.coverage_at(16.0, 0.0, true), 0.0);
    }

    #[test]
    fn standing_nib_is_an_ellipse() {
        let swept = SweptNib::new(flat_nib(0.0), flat_nib(0.0), Texture::builtin_hairs(), TrackSampler::new(&[], 1.0));
        assert_eq!(swept.coverage_at(0.0, 0.0, true), 1.0);
        assert_eq!(swept.coverage_at(0.0, 3.0, false), 0.0);
        assert_eq!(swept.coverage_at(9.0, 0.0, false), 1.0);
    }
}
