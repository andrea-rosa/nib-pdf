use super::point::Point;

/// An axis-aligned rectangle, normalized so `x0 <= x1` and `y0 <= y1`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub const fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// Returns a copy with `x0 <= x1` and `y0 <= y1`.
    pub fn normalize(&self) -> Self {
        Self {
            x0: self.x0.min(self.x1),
            y0: self.y0.min(self.y1),
            x1: self.x0.max(self.x1),
            y1: self.y0.max(self.y1),
        }
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// Smallest rect containing both `self` and `other`.
    pub fn union(&self, other: &Rect) -> Rect {
        let a = self.normalize();
        let b = other.normalize();
        Rect {
            x0: a.x0.min(b.x0),
            y0: a.y0.min(b.y0),
            x1: a.x1.max(b.x1),
            y1: a.y1.max(b.y1),
        }
    }

    /// Largest rect contained in both `self` and `other`, or `None` when they
    /// do not overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let a = self.normalize();
        let b = other.normalize();
        let r = Rect {
            x0: a.x0.max(b.x0),
            y0: a.y0.max(b.y0),
            x1: a.x1.min(b.x1),
            y1: a.y1.min(b.y1),
        };
        (r.x0 < r.x1 && r.y0 < r.y1).then_some(r)
    }

    pub fn contains(&self, p: Point) -> bool {
        let r = self.normalize();
        p.x >= r.x0 && p.x <= r.x1 && p.y >= r.y0 && p.y <= r.y1
    }
}
