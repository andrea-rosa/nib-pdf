use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A point in PDF user space or device space. Serialises as `[x, y]`.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(into = "[f64; 2]", from = "[f64; 2]")]
#[schemars(with = "[f64; 2]")]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl From<Point> for [f64; 2] {
    fn from(p: Point) -> Self {
        [p.x, p.y]
    }
}

impl From<[f64; 2]> for Point {
    fn from(a: [f64; 2]) -> Self {
        Self::new(a[0], a[1])
    }
}
