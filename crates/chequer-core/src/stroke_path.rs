use crate::draw::Tilt;
use crate::geom::FPoint;

const KNOT_EXPONENT: f32 = 0.5;
const FLATTEN_STEP: f32 = 2.0;
const MAX_STEPS_PER_SEGMENT: usize = 256;
const MIN_SAMPLE_GAP: f32 = 0.01;
const SPEED_REFERENCE_PX_PER_MS: f32 = 3.0;
const SPEED_TIME_CONSTANT_MS: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathSample {
    pub point: FPoint,
    pub pressure: f32,
    pub tilt: Option<Tilt>,
    pub time_ms: Option<f64>,
}

impl PathSample {
    pub fn toward(self, other: PathSample, fraction: f32, point: FPoint) -> PathSample {
        PathSample {
            point,
            pressure: self.pressure + (other.pressure - self.pressure) * fraction,
            tilt: blend_tilt(self.tilt, other.tilt, fraction),
            time_ms: self.time_ms.zip(other.time_ms).map(|(from, to)| from + (to - from) * f64::from(fraction)),
        }
    }
}

fn blend_tilt(from: Option<Tilt>, to: Option<Tilt>, fraction: f32) -> Option<Tilt> {
    match (from, to) {
        (Some(from), Some(to)) => Some(Tilt { x: from.x + (to.x - from.x) * fraction, y: from.y + (to.y - from.y) * fraction }),
        (from, to) => to.or(from),
    }
}

pub struct PathInterpolator {
    before: Option<PathSample>,
    from: PathSample,
    to: Option<PathSample>,
}

impl PathInterpolator {
    pub fn new(start: PathSample) -> Self {
        Self { before: None, from: start, to: None }
    }

    pub fn push(&mut self, next: PathSample) -> Vec<PathSample> {
        let latest = self.to.unwrap_or(self.from);
        if next.point.distance(latest.point) < MIN_SAMPLE_GAP {
            return Vec::new();
        }
        let Some(to) = self.to.replace(next) else {
            return Vec::new();
        };
        let emitted = self.segment(to, next.point);
        self.before = Some(self.from);
        self.from = to;
        emitted
    }

    pub fn finish(&mut self) -> Vec<PathSample> {
        let Some(to) = self.to.take() else {
            return Vec::new();
        };
        let emitted = self.segment(to, reflected(to.point, self.from.point));
        self.before = Some(self.from);
        self.from = to;
        emitted
    }

    fn segment(&self, to: PathSample, after: FPoint) -> Vec<PathSample> {
        let before = self.before.map_or_else(|| reflected(self.from.point, to.point), |sample| sample.point);
        let corners = [before, self.from.point, to.point, after];
        let steps = ((self.from.point.distance(to.point) / FLATTEN_STEP).ceil() as usize).clamp(1, MAX_STEPS_PER_SEGMENT);
        (1..=steps)
            .map(|step| {
                let fraction = step as f32 / steps as f32;
                let point = if step == steps { to.point } else { centripetal_point(corners, fraction) };
                self.from.toward(to, fraction, point)
            })
            .collect()
    }
}

fn reflected(pivot: FPoint, over: FPoint) -> FPoint {
    FPoint::new(2.0 * pivot.x - over.x, 2.0 * pivot.y - over.y)
}

fn knot_step(a: FPoint, b: FPoint) -> f32 {
    a.distance(b).powf(KNOT_EXPONENT)
}

fn centripetal_point(corners: [FPoint; 4], fraction: f32) -> FPoint {
    let t0 = 0.0;
    let t1 = t0 + knot_step(corners[0], corners[1]);
    let t2 = t1 + knot_step(corners[1], corners[2]);
    let t3 = t2 + knot_step(corners[2], corners[3]);
    let t = t1 + (t2 - t1) * fraction;
    let mix = |a: FPoint, b: FPoint, start: f32, end: f32| a.lerp(b, (t - start) / (end - start));
    let a1 = mix(corners[0], corners[1], t0, t1);
    let a2 = mix(corners[1], corners[2], t1, t2);
    let a3 = mix(corners[2], corners[3], t2, t3);
    let b1 = mix(a1, a2, t0, t2);
    let b2 = mix(a2, a3, t1, t3);
    mix(b1, b2, t1, t2)
}

#[derive(Default)]
pub struct SpeedMeter {
    value: f32,
    clock_ms: Option<f64>,
    unmeasured_distance: f32,
}

impl SpeedMeter {
    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn advance(&mut self, distance: f32, now_ms: Option<f64>) {
        self.unmeasured_distance += distance;
        let Some(now) = now_ms else {
            return;
        };
        let Some(before) = self.clock_ms else {
            self.clock_ms = Some(now);
            self.unmeasured_distance = 0.0;
            return;
        };
        let elapsed = (now - before) as f32;
        if elapsed <= 0.0 {
            return;
        }
        let instant = (self.unmeasured_distance / elapsed / SPEED_REFERENCE_PX_PER_MS).clamp(0.0, 1.0);
        self.value += (instant - self.value) * (1.0 - (-elapsed / SPEED_TIME_CONSTANT_MS).exp());
        self.clock_ms = Some(now);
        self.unmeasured_distance = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(x: f32, y: f32, pressure: f32) -> PathSample {
        PathSample { point: FPoint::new(x, y), pressure, tilt: None, time_ms: None }
    }

    #[test]
    fn curve_passes_through_every_pushed_point_and_ends_on_the_last() {
        let corners = [(0.0, 0.0), (20.0, 10.0), (40.0, 0.0), (60.0, 10.0)];
        let mut path = PathInterpolator::new(sample(corners[0].0, corners[0].1, 1.0));
        let mut points: Vec<FPoint> = Vec::new();
        for &(x, y) in &corners[1..] {
            points.extend(path.push(sample(x, y, 1.0)).into_iter().map(|emitted| emitted.point));
        }
        points.extend(path.finish().into_iter().map(|emitted| emitted.point));
        for &(x, y) in &corners[1..] {
            assert!(points.iter().any(|point| point.distance(FPoint::new(x, y)) < 1e-3));
        }
        assert_eq!(points.last().copied(), Some(FPoint::new(60.0, 10.0)));
    }

    #[test]
    fn repeated_point_adds_nothing() {
        let mut path = PathInterpolator::new(sample(0.0, 0.0, 1.0));
        path.push(sample(10.0, 0.0, 1.0));
        path.push(sample(20.0, 0.0, 1.0));
        assert!(path.push(sample(20.0, 0.0, 0.5)).is_empty());
    }

    #[test]
    fn pressure_grows_smoothly_along_a_segment() {
        let mut path = PathInterpolator::new(sample(0.0, 0.0, 0.0));
        assert!(path.push(sample(20.0, 0.0, 1.0)).is_empty());
        let emitted = path.finish();
        assert!(emitted.windows(2).all(|pair| pair[0].pressure < pair[1].pressure));
        assert_eq!(emitted.last().map(|last| last.pressure), Some(1.0));
    }

    #[test]
    fn speed_does_not_depend_on_how_often_events_arrive() {
        let measure = |step_ms: f64| {
            let mut meter = SpeedMeter::default();
            let mut now = 0.0;
            while now < 400.0 {
                meter.advance(step_ms as f32, Some(now));
                now += step_ms;
            }
            meter.value()
        };
        assert!((measure(4.0) - measure(16.0)).abs() < 0.02);
    }
}
