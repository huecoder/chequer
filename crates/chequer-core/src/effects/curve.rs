const MAX_POINTS: usize = 12;
const CHANNEL_MAX: u8 = 255;
const LUT_SIZE: usize = CHANNEL_MAX as usize + 1;
const OVERSHOOT_LIMIT: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Curve {
    points: [(u8, u8); MAX_POINTS],
    len: usize,
}

impl Curve {
    pub fn identity() -> Self {
        let mut points = [(0, 0); MAX_POINTS];
        points[1] = (CHANNEL_MAX, CHANNEL_MAX);
        Self { points, len: 2 }
    }

    pub fn points(&self) -> &[(u8, u8)] {
        &self.points[..self.len]
    }

    pub fn is_full(&self) -> bool {
        self.len == MAX_POINTS
    }

    pub fn insert(&mut self, x: u8, y: u8) -> Option<usize> {
        if self.is_full() || self.points().iter().any(|&(px, _)| px == x) {
            return None;
        }
        let index = self.points().iter().position(|&(px, _)| px > x)?;
        self.points.copy_within(index..self.len, index + 1);
        self.points[index] = (x, y);
        self.len += 1;
        Some(index)
    }

    pub fn remove(&mut self, index: usize) -> bool {
        let inner = index > 0 && index + 1 < self.len;
        if inner {
            self.points.copy_within(index + 1..self.len, index);
            self.len -= 1;
        }
        inner
    }

    pub fn move_point(&mut self, index: usize, x: u8, y: u8) {
        let last = self.len - 1;
        let x = if index == 0 || index == last {
            self.points[index].0
        } else {
            x.clamp(self.points[index - 1].0 + 1, self.points[index + 1].0 - 1)
        };
        self.points[index] = (x, y);
    }

    pub fn lut(&self) -> [u8; LUT_SIZE] {
        let points = self.points();
        let xs: Vec<f32> = points.iter().map(|p| f32::from(p.0)).collect();
        let ys: Vec<f32> = points.iter().map(|p| f32::from(p.1)).collect();
        let slopes: Vec<f32> = (0..points.len() - 1).map(|k| (ys[k + 1] - ys[k]) / (xs[k + 1] - xs[k])).collect();
        let mut tangents: Vec<f32> = (0..points.len())
            .map(|k| match k {
                0 => slopes[0],
                k if k == points.len() - 1 => slopes[k - 1],
                k if slopes[k - 1] * slopes[k] <= 0.0 => 0.0,
                k => (slopes[k - 1] + slopes[k]) / 2.0,
            })
            .collect();
        for (k, &slope) in slopes.iter().enumerate() {
            if slope == 0.0 {
                tangents[k] = 0.0;
                tangents[k + 1] = 0.0;
                continue;
            }
            let (alpha, beta) = (tangents[k] / slope, tangents[k + 1] / slope);
            let length = alpha.hypot(beta);
            if length > OVERSHOOT_LIMIT {
                let scale = OVERSHOOT_LIMIT / length;
                tangents[k] = scale * alpha * slope;
                tangents[k + 1] = scale * beta * slope;
            }
        }
        let mut segment = 0;
        std::array::from_fn(|value| {
            let x = value as f32;
            while segment + 2 < points.len() && x > xs[segment + 1] {
                segment += 1;
            }
            let width = xs[segment + 1] - xs[segment];
            let t = ((x - xs[segment]) / width).clamp(0.0, 1.0);
            let (t2, t3) = (t * t, t * t * t);
            let y = (2.0 * t3 - 3.0 * t2 + 1.0) * ys[segment]
                + (t3 - 2.0 * t2 + t) * width * tangents[segment]
                + (-2.0 * t3 + 3.0 * t2) * ys[segment + 1]
                + (t3 - t2) * width * tangents[segment + 1];
            y.round().clamp(0.0, f32::from(CHANNEL_MAX)) as u8
        })
    }
}
