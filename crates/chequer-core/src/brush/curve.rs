use serde::{Deserialize, Serialize};

pub const MAX_CURVE_POINTS: usize = 16;
const MIN_POINT_GAP: f32 = 0.01;
const ENDPOINT_TANGENT_LIMIT: f32 = 3.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Vec<(f32, f32)>", into = "Vec<(f32, f32)>")]
pub struct Curve {
    points: Vec<(f32, f32)>,
}

impl From<Vec<(f32, f32)>> for Curve {
    fn from(points: Vec<(f32, f32)>) -> Self {
        Self::new(points)
    }
}

impl From<Curve> for Vec<(f32, f32)> {
    fn from(curve: Curve) -> Self {
        curve.points
    }
}

impl Default for Curve {
    fn default() -> Self {
        Self::linear()
    }
}

impl Curve {
    pub fn linear() -> Self {
        Self { points: vec![(0.0, 0.0), (1.0, 1.0)] }
    }

    pub fn new(mut points: Vec<(f32, f32)>) -> Self {
        points.retain(|(x, y)| x.is_finite() && y.is_finite());
        for point in &mut points {
            *point = (point.0.clamp(0.0, 1.0), point.1.clamp(0.0, 1.0));
        }
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        points.dedup_by(|current, previous| current.0 - previous.0 < MIN_POINT_GAP);
        points.truncate(MAX_CURVE_POINTS);
        if points.len() < 2 { Self::linear() } else { Self { points } }
    }

    pub fn points(&self) -> &[(f32, f32)] {
        &self.points
    }

    pub fn is_linear(&self) -> bool {
        *self == Self::linear()
    }

    pub fn eval(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let last = self.points.len() - 1;
        if x <= self.points[0].0 {
            return self.points[0].1;
        }
        let Some(segment) = (0..last).find(|&k| x <= self.points[k + 1].0) else {
            return self.points[last].1;
        };
        let ((x0, y0), (x1, y1)) = (self.points[segment], self.points[segment + 1]);
        let width = x1 - x0;
        let t = (x - x0) / width;
        let (t2, t3) = (t * t, t * t * t);
        let value = (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * width * self.tangent(segment) + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * width * self.tangent(segment + 1);
        value.clamp(0.0, 1.0)
    }

    pub fn set_point(&mut self, index: usize, x: f32, y: f32) {
        let last = self.points.len() - 1;
        let y = y.clamp(0.0, 1.0);
        let x = if index == 0 || index == last {
            self.points[index].0
        } else {
            x.clamp(self.points[index - 1].0 + MIN_POINT_GAP, self.points[index + 1].0 - MIN_POINT_GAP)
        };
        self.points[index] = (x, y);
    }

    pub fn insert_point(&mut self, x: f32, y: f32) -> Option<usize> {
        let x = x.clamp(0.0, 1.0);
        let crowded = self.points.iter().any(|point| (point.0 - x).abs() < MIN_POINT_GAP);
        if crowded || self.points.len() >= MAX_CURVE_POINTS {
            return None;
        }
        let index = self.points.partition_point(|point| point.0 < x);
        self.points.insert(index, (x, y.clamp(0.0, 1.0)));
        Some(index)
    }

    pub fn remove_point(&mut self, index: usize) -> bool {
        let removable = index > 0 && index + 1 < self.points.len();
        if removable {
            self.points.remove(index);
        }
        removable
    }

    fn secant(&self, k: usize) -> f32 {
        let ((x0, y0), (x1, y1)) = (self.points[k], self.points[k + 1]);
        (y1 - y0) / (x1 - x0)
    }

    fn width(&self, k: usize) -> f32 {
        self.points[k + 1].0 - self.points[k].0
    }

    fn tangent(&self, k: usize) -> f32 {
        let last = self.points.len() - 1;
        if last == 1 {
            return self.secant(0);
        }
        if k == 0 {
            return edge_tangent(self.width(0), self.width(1), self.secant(0), self.secant(1));
        }
        if k == last {
            return edge_tangent(self.width(last - 1), self.width(last - 2), self.secant(last - 1), self.secant(last - 2));
        }
        let (before, after) = (self.secant(k - 1), self.secant(k));
        if before * after <= 0.0 {
            return 0.0;
        }
        let (width_before, width_after) = (self.width(k - 1), self.width(k));
        let (weight_before, weight_after) = (2.0 * width_after + width_before, width_after + 2.0 * width_before);
        (weight_before + weight_after) / (weight_before / before + weight_after / after)
    }
}

fn edge_tangent(near_width: f32, next_width: f32, near_secant: f32, next_secant: f32) -> f32 {
    let estimate = ((2.0 * near_width + next_width) * near_secant - near_width * next_secant) / (near_width + next_width);
    if estimate.signum() != near_secant.signum() {
        0.0
    } else if near_secant.signum() != next_secant.signum() && estimate.abs() > ENDPOINT_TANGENT_LIMIT * near_secant.abs() {
        ENDPOINT_TANGENT_LIMIT * near_secant
    } else {
        estimate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_curve_is_identity() {
        let curve = Curve::linear();
        for step in 0..=10 {
            let x = step as f32 / 10.0;
            assert!((curve.eval(x) - x).abs() < 1e-5);
        }
    }

    #[test]
    fn smooth_curve_passes_through_its_points_and_stays_monotone() {
        let curve = Curve::new(vec![(0.0, 0.0), (0.35, 0.1), (1.0, 1.0)]);
        assert!((curve.eval(0.35) - 0.1).abs() < 1e-5);
        let mut previous = 0.0;
        for step in 0..=100 {
            let value = curve.eval(step as f32 / 100.0);
            assert!(value >= previous - 1e-6);
            previous = value;
        }
    }

    #[test]
    fn broken_input_falls_back_to_linear_and_endpoints_stay_pinned() {
        assert!(Curve::new(vec![(0.5, 0.5)]).is_linear());
        let mut curve = Curve::linear();
        let middle = curve.insert_point(0.5, 0.2).unwrap();
        curve.set_point(middle, 2.0, 0.3);
        curve.set_point(0, 0.4, 0.5);
        assert_eq!(curve.points()[0], (0.0, 0.5));
        assert!(curve.points()[middle].0 < 1.0);
        assert!(!curve.remove_point(0));
        assert!(curve.remove_point(middle));
    }
}
