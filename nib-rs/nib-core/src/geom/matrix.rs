use super::point::Point;

/// 3×3 affine transform stored as `[a b c d e f]`, row-vector convention:
/// `[x y 1]·M` gives `(a·x + c·y + e, b·x + d·y + f)`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    /// `self · other`: apply `other` first, then `self`.
    pub fn mul(&self, other: &Matrix) -> Matrix {
        let (m, n) = (self, other);
        Matrix {
            a: m.a * n.a + m.b * n.c,
            b: m.a * n.b + m.b * n.d,
            c: m.c * n.a + m.d * n.c,
            d: m.c * n.b + m.d * n.d,
            e: m.e * n.a + m.f * n.c + n.e,
            f: m.e * n.b + m.f * n.d + n.f,
        }
    }

    pub fn apply(&self, p: Point) -> Point {
        Point {
            x: self.a * p.x + self.c * p.y + self.e,
            y: self.b * p.x + self.d * p.y + self.f,
        }
    }

    /// Inverse, or `None` when the matrix is singular.
    pub fn invert(&self) -> Option<Matrix> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < f64::EPSILON {
            return None;
        }
        let inv = 1.0 / det;
        Some(Matrix {
            a: self.d * inv,
            b: -self.b * inv,
            c: -self.c * inv,
            d: self.a * inv,
            e: (self.c * self.f - self.d * self.e) * inv,
            f: (self.b * self.e - self.a * self.f) * inv,
        })
    }
}
