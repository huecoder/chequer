use crate::geom::FPoint;

const FOLLOW_SOFTNESS: f32 = 4.0;
const FLATTEN_STEP: f32 = 2.0;

pub struct PathSmoother {
    filtered: FPoint,
    curve_start: FPoint,
    control: Option<FPoint>,
}

impl PathSmoother {
    pub fn new(at: FPoint) -> Self {
        Self { filtered: at, curve_start: at, control: None }
    }

    pub fn push(&mut self, raw: FPoint) -> Vec<FPoint> {
        let gap = raw.distance(self.filtered);
        let follow = gap / (gap + FOLLOW_SOFTNESS);
        self.filtered = lerp(self.filtered, raw, follow);
        let Some(control) = self.control.replace(self.filtered) else {
            return Vec::new();
        };
        let midpoint = lerp(control, self.filtered, 0.5);
        let points = flatten(self.curve_start, control, midpoint);
        self.curve_start = midpoint;
        points
    }

    pub fn finish(&mut self, end: FPoint) -> Vec<FPoint> {
        let control = self.control.take().unwrap_or(self.curve_start);
        flatten(self.curve_start, control, end)
    }
}

fn lerp(from: FPoint, to: FPoint, t: f32) -> FPoint {
    FPoint::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
}

fn flatten(start: FPoint, control: FPoint, end: FPoint) -> Vec<FPoint> {
    let length = start.distance(control) + control.distance(end);
    let steps = ((length / FLATTEN_STEP).ceil() as usize).max(1);
    (1..=steps)
        .map(|step| {
            let t = step as f32 / steps as f32;
            lerp(lerp(start, control, t), lerp(control, end, t), t)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_lands_on_requested_end() {
        let mut smoother = PathSmoother::new(FPoint::new(0.0, 0.0));
        smoother.push(FPoint::new(10.0, 0.0));
        smoother.push(FPoint::new(20.0, 5.0));
        let end = FPoint::new(30.0, 5.0);
        assert_eq!(smoother.finish(end).last(), Some(&end));
    }

    #[test]
    fn jitter_in_place_barely_moves_the_path() {
        let mut smoother = PathSmoother::new(FPoint::new(0.0, 0.0));
        let drift = [FPoint::new(1.0, 0.0), FPoint::new(0.0, 1.0), FPoint::new(-1.0, 0.0), FPoint::new(0.0, -1.0)];
        for raw in drift {
            for point in smoother.push(raw) {
                assert!(point.distance(FPoint::new(0.0, 0.0)) < 1.0);
            }
        }
    }
}
