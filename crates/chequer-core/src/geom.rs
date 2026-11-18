#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IPoint {
    pub x: i32,
    pub y: i32,
}

impl IPoint {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FPoint {
    pub x: f32,
    pub y: f32,
}

impl FPoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn floor(self) -> IPoint {
        IPoint::new(self.x.floor() as i32, self.y.floor() as i32)
    }

    pub fn distance(self, other: FPoint) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    pub fn lerp(self, other: FPoint, t: f32) -> FPoint {
        FPoint::new(self.x + (other.x - self.x) * t, self.y + (other.y - self.y) * t)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl FRect {
    pub const fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn from_corners(a: FPoint, b: FPoint) -> Self {
        Self::new(a.x, a.y, b.x, b.y).normalized()
    }

    pub fn normalized(self) -> Self {
        Self::new(self.x0.min(self.x1), self.y0.min(self.y1), self.x0.max(self.x1), self.y0.max(self.y1))
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    pub fn contains(&self, p: FPoint) -> bool {
        let frame = self.normalized();
        p.x >= frame.x0 && p.x < frame.x1 && p.y >= frame.y0 && p.y < frame.y1
    }

    pub fn outer(&self) -> IRect {
        let frame = self.normalized();
        let (x0, y0) = (frame.x0.floor() as i32, frame.y0.floor() as i32);
        IRect::new(x0, y0, (frame.x1.ceil() as i32).max(x0 + 1), (frame.y1.ceil() as i32).max(y0 + 1))
    }

    pub fn rounded(&self) -> IRect {
        let frame = self.normalized();
        let (x0, y0) = (frame.x0.round() as i32, frame.y0.round() as i32);
        IRect::new(x0, y0, (frame.x1.round() as i32).max(x0 + 1), (frame.y1.round() as i32).max(y0 + 1))
    }
}

impl From<IRect> for FRect {
    fn from(rect: IRect) -> Self {
        Self::new(rect.x0 as f32, rect.y0 as f32, rect.x1 as f32, rect.y1 as f32)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl IRect {
    pub const EMPTY: IRect = IRect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn from_size(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self::new(x, y, x + width as i32, y + height as i32)
    }

    pub fn from_corners(a: IPoint, b: IPoint) -> Self {
        Self::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x) + 1, a.y.max(b.y) + 1)
    }

    pub fn width(&self) -> i32 {
        (self.x1 - self.x0).max(0)
    }

    pub fn height(&self) -> i32 {
        (self.y1 - self.y0).max(0)
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn contains(&self, p: IPoint) -> bool {
        p.x >= self.x0 && p.x < self.x1 && p.y >= self.y0 && p.y < self.y1
    }

    pub fn intersect(&self, other: IRect) -> IRect {
        let r = IRect::new(
            self.x0.max(other.x0),
            self.y0.max(other.y0),
            self.x1.min(other.x1),
            self.y1.min(other.y1),
        );
        if r.is_empty() { IRect::EMPTY } else { r }
    }

    pub fn union(&self, other: IRect) -> IRect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return *self;
        }
        IRect::new(
            self.x0.min(other.x0),
            self.y0.min(other.y0),
            self.x1.max(other.x1),
            self.y1.max(other.y1),
        )
    }

    pub fn expand(&self, margin: i32) -> IRect {
        IRect::new(self.x0 - margin, self.y0 - margin, self.x1 + margin, self.y1 + margin)
    }

    pub fn translate(&self, dx: i32, dy: i32) -> IRect {
        IRect::new(self.x0 + dx, self.y0 + dy, self.x1 + dx, self.y1 + dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_of_disjoint_rects_is_empty() {
        let a = IRect::new(0, 0, 4, 4);
        let b = IRect::new(10, 10, 12, 12);
        assert!(a.intersect(b).is_empty());
    }

    #[test]
    fn union_ignores_empty() {
        let a = IRect::new(1, 2, 5, 6);
        assert_eq!(a.union(IRect::EMPTY), a);
        assert_eq!(IRect::EMPTY.union(a), a);
    }

    #[test]
    fn corners_are_inclusive_and_order_independent() {
        let r = IRect::from_corners(IPoint::new(5, 5), IPoint::new(2, 3));
        assert_eq!(r, IRect::new(2, 3, 6, 6));
    }
}
