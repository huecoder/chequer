use tiny_skia::{Path, PathBuilder};

use crate::geom::FPoint;

const CATMULL_ROM_DIVISOR: f32 = 6.0;
const SAMPLES_PER_SEGMENT: usize = 24;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineMode {
    #[default]
    Straight,
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cubic {
    pub from: FPoint,
    pub c1: FPoint,
    pub c2: FPoint,
    pub to: FPoint,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nearest {
    pub segment: usize,
    pub point: FPoint,
    pub distance: f32,
}

impl Cubic {
    pub fn straight(from: FPoint, to: FPoint) -> Self {
        Self { from, c1: from, c2: to, to }
    }

    pub fn at(&self, t: f32) -> FPoint {
        let u = 1.0 - t;
        let weights = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        let corners = [self.from, self.c1, self.c2, self.to];
        FPoint::new(
            corners.iter().zip(weights).map(|(p, w)| p.x * w).sum(),
            corners.iter().zip(weights).map(|(p, w)| p.y * w).sum(),
        )
    }
}

pub fn line_segments(points: &[FPoint], mode: LineMode) -> Vec<Cubic> {
    match (mode, points.first(), points.last()) {
        (LineMode::Straight, Some(&first), Some(&last)) => vec![Cubic::straight(first, last)],
        (LineMode::Smooth, _, _) => smooth_through(points),
        _ => Vec::new(),
    }
}

fn smooth_through(points: &[FPoint]) -> Vec<Cubic> {
    let last = points.len().saturating_sub(1);
    points
        .windows(2)
        .enumerate()
        .map(|(index, pair)| {
            let (from, to) = (pair[0], pair[1]);
            let before = points[index.saturating_sub(1)];
            let after = points[(index + 2).min(last)];
            Cubic {
                from,
                c1: FPoint::new(from.x + (to.x - before.x) / CATMULL_ROM_DIVISOR, from.y + (to.y - before.y) / CATMULL_ROM_DIVISOR),
                c2: FPoint::new(to.x - (after.x - from.x) / CATMULL_ROM_DIVISOR, to.y - (after.y - from.y) / CATMULL_ROM_DIVISOR),
                to,
            }
        })
        .collect()
}

pub fn segments_path(segments: &[Cubic]) -> Option<Path> {
    let first = segments.first()?;
    let mut builder = PathBuilder::new();
    builder.move_to(first.from.x, first.from.y);
    for segment in segments {
        builder.cubic_to(segment.c1.x, segment.c1.y, segment.c2.x, segment.c2.y, segment.to.x, segment.to.y);
    }
    builder.finish()
}

pub fn nearest_on(segments: &[Cubic], target: FPoint) -> Option<Nearest> {
    segments
        .iter()
        .enumerate()
        .flat_map(|(segment, cubic)| {
            (0..SAMPLES_PER_SEGMENT).map(move |step| {
                let a = cubic.at(step as f32 / SAMPLES_PER_SEGMENT as f32);
                let b = cubic.at((step + 1) as f32 / SAMPLES_PER_SEGMENT as f32);
                (segment, a, b)
            })
        })
        .map(|(segment, a, b)| {
            let point = closest_on_chord(a, b, target);
            Nearest { segment, point, distance: point.distance(target) }
        })
        .min_by(|left, right| left.distance.total_cmp(&right.distance))
}

fn closest_on_chord(a: FPoint, b: FPoint, target: FPoint) -> FPoint {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length_sq = dx * dx + dy * dy;
    if length_sq == 0.0 {
        return a;
    }
    let t = (((target.x - a.x) * dx + (target.y - a.y) * dy) / length_sq).clamp(0.0, 1.0);
    FPoint::new(a.x + dx * t, a.y + dy * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_curve_passes_through_every_point() {
        let points = [FPoint::new(0.0, 0.0), FPoint::new(10.0, 8.0), FPoint::new(20.0, 0.0)];
        let segments = line_segments(&points, LineMode::Smooth);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].at(1.0), points[1]);
        assert_eq!(segments[1].at(0.0), points[1]);
    }

    #[test]
    fn straight_mode_ignores_middle_points() {
        let points = [FPoint::new(0.0, 0.0), FPoint::new(10.0, 8.0), FPoint::new(20.0, 0.0)];
        let segments = line_segments(&points, LineMode::Straight);
        assert_eq!(segments, vec![Cubic::straight(points[0], points[2])]);
    }

    #[test]
    fn nearest_point_lies_on_the_curve() {
        let segments = line_segments(&[FPoint::new(0.0, 0.0), FPoint::new(10.0, 0.0)], LineMode::Smooth);
        let nearest = nearest_on(&segments, FPoint::new(5.0, 3.0)).expect("есть сегмент");
        assert!((nearest.point.x - 5.0).abs() < 0.1);
        assert!(nearest.point.y.abs() < 0.001);
        assert_eq!(nearest.segment, 0);
    }
}
