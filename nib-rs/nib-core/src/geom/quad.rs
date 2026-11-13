use super::point::Point;
use super::rect::Rect;

/// PDF quad points: 8 numbers, per the PDF spec ordered
/// `(x1,y1)` upper-left, `(x2,y2)` upper-right, `(x3,y3)` lower-left,
/// `(x4,y4)` lower-right — in a y-up coordinate space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct QuadPoints(pub [f64; 8]);

impl QuadPoints {
    pub const fn new(points: [f64; 8]) -> Self {
        Self(points)
    }

    pub fn points(&self) -> [Point; 4] {
        let q = &self.0;
        [
            Point::new(q[0], q[1]),
            Point::new(q[2], q[3]),
            Point::new(q[4], q[5]),
            Point::new(q[6], q[7]),
        ]
    }

    /// Axis-aligned bounding box of the four points.
    pub fn bbox(&self) -> Rect {
        let pts = self.points();
        let mut r = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in pts {
            r.x0 = r.x0.min(p.x);
            r.y0 = r.y0.min(p.y);
            r.x1 = r.x1.max(p.x);
            r.y1 = r.y1.max(p.y);
        }
        r
    }

    /// Reorders the four points into spec order (upper-left, upper-right,
    /// lower-left, lower-right). Useful when quad points arrive unordered,
    /// e.g. from user drag selections.
    pub fn normalize(&self) -> QuadPoints {
        let b = self.bbox();
        let pts = self.points();
        let dist = |p: Point, t: Point| (p.x - t.x).powi(2) + (p.y - t.y).powi(2);
        let pick = |target: Point, taken: &[bool]| -> usize {
            pts.iter()
                .enumerate()
                .filter(|(i, _)| !taken[*i])
                .min_by(|(_, a), (_, b)| dist(**a, target).total_cmp(&dist(**b, target)))
                .map(|(i, _)| i)
                .unwrap()
        };
        let mut taken = [false; 4];
        let targets = [
            Point::new(b.x0, b.y1), // upper-left
            Point::new(b.x1, b.y1), // upper-right
            Point::new(b.x0, b.y0), // lower-left
            Point::new(b.x1, b.y0), // lower-right
        ];
        let mut out = [0.0; 8];
        for (i, t) in targets.iter().enumerate() {
            let idx = pick(*t, &taken);
            taken[idx] = true;
            out[2 * i] = pts[idx].x;
            out[2 * i + 1] = pts[idx].y;
        }
        QuadPoints(out)
    }
}
