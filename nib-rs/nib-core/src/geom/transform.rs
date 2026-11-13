use super::matrix::Matrix;
use super::point::Point;
use super::rect::Rect;

/// Page rotation, multiples of 90° clockwise, matching PDF `/Rotate`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Rotation {
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn from_degrees(degrees: i32) -> Result<Rotation, RotationError> {
        match degrees.rem_euclid(360) {
            0 => Ok(Rotation::R0),
            90 => Ok(Rotation::R90),
            180 => Ok(Rotation::R180),
            270 => Ok(Rotation::R270),
            d => Err(RotationError(d)),
        }
    }

    pub const fn degrees(self) -> i32 {
        match self {
            Rotation::R0 => 0,
            Rotation::R90 => 90,
            Rotation::R180 => 180,
            Rotation::R270 => 270,
        }
    }

    /// Whether the rotation swaps the page's width and height.
    pub const fn swaps_axes(self) -> bool {
        matches!(self, Rotation::R90 | Rotation::R270)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct RotationError(pub i32);

impl std::fmt::Display for RotationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid rotation: {} degrees", self.0)
    }
}

impl std::error::Error for RotationError {}

/// User-space ↔ device-space transform for one page at one zoom bucket.
/// One instance per (page, bucket); cheap to construct and copy.
#[derive(Debug, Copy, Clone)]
pub struct PageTransform {
    /// Crop box in user space (`/CropBox` ∩ `/MediaBox`).
    pub crop_box: Rect,
    pub rotate: Rotation,
    /// Device pixels per PDF point.
    pub scale: f64,
    device_from_user: Matrix,
    user_from_device: Matrix,
}

impl PageTransform {
    /// Builds the transform from the ARCHITECTURE §5.3 rotation table:
    ///
    /// | `/Rotate` | user→device for `(x,y)`         |
    /// |-----------|---------------------------------|
    /// | 0         | `(k(x−x0), k(y1−y))`            |
    /// | 90        | `(k(y−y0), k(x−x0))`            |
    /// | 180       | `(k(x1−x), k(y−y0))`            |
    /// | 270       | `(k(y1−y), k(x1−x))`            |
    pub fn new(crop_box: Rect, rotate: Rotation, scale: f64) -> PageTransform {
        let r = crop_box.normalize();
        let k = scale;
        let (x0, y0, x1, y1) = (r.x0, r.y0, r.x1, r.y1);
        let device_from_user = match rotate {
            Rotation::R0 => Matrix::new(k, 0.0, 0.0, -k, -k * x0, k * y1),
            Rotation::R90 => Matrix::new(0.0, k, k, 0.0, -k * y0, -k * x0),
            Rotation::R180 => Matrix::new(-k, 0.0, 0.0, k, k * x1, -k * y0),
            Rotation::R270 => Matrix::new(0.0, -k, -k, 0.0, k * y1, k * x1),
        };
        let user_from_device = device_from_user
            .invert()
            .expect("page transform is always invertible for scale != 0");
        PageTransform {
            crop_box: r,
            rotate,
            scale,
            device_from_user,
            user_from_device,
        }
    }

    /// `(width, height)` in device pixels; swapped for 90/270.
    pub fn device_size(&self) -> (f64, f64) {
        let w = self.crop_box.width() * self.scale;
        let h = self.crop_box.height() * self.scale;
        if self.rotate.swaps_axes() {
            (h, w)
        } else {
            (w, h)
        }
    }

    pub fn to_device(&self, p: Point) -> Point {
        self.device_from_user.apply(p)
    }

    pub fn to_user(&self, p: Point) -> Point {
        self.user_from_device.apply(p)
    }

    /// Maps a user-space rect through the transform; the result is
    /// re-normalized since rotation can swap corner ordering.
    pub fn rect_to_device(&self, r: Rect) -> Rect {
        self.map_rect(r, &self.device_from_user)
    }

    pub fn rect_to_user(&self, r: Rect) -> Rect {
        self.map_rect(r, &self.user_from_device)
    }

    fn map_rect(&self, r: Rect, m: &Matrix) -> Rect {
        let r = r.normalize();
        let corners = [
            Point::new(r.x0, r.y0),
            Point::new(r.x1, r.y0),
            Point::new(r.x1, r.y1),
            Point::new(r.x0, r.y1),
        ];
        let mut out = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for c in corners {
            let p = m.apply(c);
            out.x0 = out.x0.min(p.x);
            out.y0 = out.y0.min(p.y);
            out.x1 = out.x1.max(p.x);
            out.y1 = out.y1.max(p.y);
        }
        out
    }
}
